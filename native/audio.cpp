#include <cmath>
#include <cstdint>
#include <memory>
#include <string>
#include "api/audio/builtin_audio_processing_builder.h"
#include "api/audio/echo_canceller3_config.h"
#include "api/audio/echo_control.h"
#include "api/environment/environment_factory.h"
#include "api/field_trials_view.h"
#include "common_audio/vad/include/webrtc_vad.h"
#include "modules/audio_processing/aec3/echo_canceller3.h"
#include "modules/audio_processing/audio_buffer.h"

namespace {
// Echo cancellation's HMM transparent mode. The default (legacy) detector needs several
// seconds of playback before it decides that no echo reaches the microphone, as with a
// headset, and until then it suppresses the voice as possible echo, mostly above 4 kHz: the
// first words after starting sounded muffled with a music bot playing. The HMM detector
// decides within about a second and still leaves echo cancellation working on speakers.
class FieldTrials : public webrtc::FieldTrialsView {
public:
    std::string Lookup(absl::string_view key) const override {
        return key == "WebRTC-Aec3TransparentModeHmm" ? "Enabled" : "";
    }
    std::unique_ptr<webrtc::FieldTrialsView> CreateCopy() const override {
        return std::make_unique<FieldTrials>();
    }
};

// Energy going into and out of echo cancellation since it was last read, in the mic line's
// bands: below 1 kHz, 1-4 kHz and above 4 kHz. Only what is sent counts, as for the rest of the
// mic line. Written and read on the voice worker only.
struct EchoEnergy {
    double before[3] = {};
    double after[3] = {};
    // Set from `flex_apm_set_output_muted`. The processor rebuilds echo cancellation on its first
    // frame and doesn't tell the new one, so `SetCaptureOutputUsage` can't be relied on here.
    bool sent = true;
};

// A second-order low or high pass (RBJ cookbook, Q = 1/sqrt 2) at 16 kHz, the rate of the lowest
// of the three bands echo cancellation works in at 48 kHz.
struct Biquad {
    double b[3], a[2], z[2] = {};
    Biquad(bool high_pass, double frequency) {
        const double w = 6.283185307179586 * frequency / 16000.0;
        const double cos = std::cos(w), alpha = std::sin(w) * 0.7071067811865476;
        const double a0 = 1.0 + alpha, edge = (high_pass ? 1.0 + cos : 1.0 - cos) / 2.0;
        b[0] = edge / a0;
        b[1] = (high_pass ? -(1.0 + cos) : 1.0 - cos) / a0;
        b[2] = edge / a0;
        a[0] = -2.0 * cos / a0;
        a[1] = (1.0 - alpha) / a0;
    }
    double run(double x) {
        const double y = b[0] * x + z[0];
        z[0] = b[1] * x - a[0] * y + z[1];
        z[1] = b[2] * x - a[1] * y;
        return y;
    }
};

// Splits the lowest band (0-8 kHz) at 1 and 4 kHz with fourth-order filters, as the mic line
// does in src/voice_levels.rs; the bands above 8 kHz count to the treble whole.
class BandMeter {
public:
    void add(const webrtc::AudioBuffer& buffer, double* energy) {
        const float* const* bands = buffer.split_bands_const(0);
        for (size_t i = 0; i < buffer.num_frames_per_band(); ++i) {
            const double x = bands[0][i] / 32768.0;
            const double low = low_[1].run(low_[0].run(x));
            const double mid = mid_[3].run(mid_[2].run(mid_[1].run(mid_[0].run(x))));
            const double high = high_[1].run(high_[0].run(x));
            energy[0] += low * low;
            energy[1] += mid * mid;
            energy[2] += high * high;
            for (size_t band = 1; band < buffer.num_bands(); ++band) {
                const double y = bands[band][i] / 32768.0;
                energy[2] += y * y;
            }
        }
    }

private:
    Biquad low_[2] = {{false, 1000.0}, {false, 1000.0}};
    Biquad mid_[4] = {{true, 1000.0}, {true, 1000.0}, {false, 4000.0}, {false, 4000.0}};
    Biquad high_[2] = {{true, 4000.0}, {true, 4000.0}};
};

// Echo cancellation as WebRTC builds it, measured on the way through. Its own figures leave out
// what it does to the voice, and a headset voice it mistakes for echo loses its treble there.
class MeasuredEcho : public webrtc::EchoControl {
public:
    MeasuredEcho(std::unique_ptr<webrtc::EchoControl> inner, std::shared_ptr<EchoEnergy> energy)
        : inner_(std::move(inner)), energy_(std::move(energy)) {}
    void AnalyzeRender(webrtc::AudioBuffer* render) override { inner_->AnalyzeRender(render); }
    void AnalyzeCapture(webrtc::AudioBuffer* capture) override { inner_->AnalyzeCapture(capture); }
    void ProcessCapture(webrtc::AudioBuffer* capture, bool level_change) override {
        if (energy_->sent) before_.add(*capture, energy_->before);
        inner_->ProcessCapture(capture, level_change);
        if (energy_->sent) after_.add(*capture, energy_->after);
    }
    void ProcessCapture(webrtc::AudioBuffer* capture, webrtc::AudioBuffer* linear_output,
                        bool level_change) override {
        if (energy_->sent) before_.add(*capture, energy_->before);
        inner_->ProcessCapture(capture, linear_output, level_change);
        if (energy_->sent) after_.add(*capture, energy_->after);
    }
    Metrics GetMetrics() const override { return inner_->GetMetrics(); }
    void SetAudioBufferDelay(int delay_ms) override { inner_->SetAudioBufferDelay(delay_ms); }
    void SetCaptureOutputUsage(bool used) override { inner_->SetCaptureOutputUsage(used); }
    bool ActiveProcessing() const override { return inner_->ActiveProcessing(); }

private:
    std::unique_ptr<webrtc::EchoControl> inner_;
    std::shared_ptr<EchoEnergy> energy_;
    BandMeter before_, after_;
};

// The echo canceller the processor builds without a factory: the default configuration, for mono
// and stereo playback alike, adjusted to the field trials. Wrapped in `MeasuredEcho`.
class MeasuredEchoFactory : public webrtc::EchoControlFactory {
public:
    explicit MeasuredEchoFactory(std::shared_ptr<EchoEnergy> energy) : energy_(std::move(energy)) {}
    std::unique_ptr<webrtc::EchoControl> Create(const webrtc::Environment& env, int rate,
                                                int render_channels, int capture_channels) override {
        return Create(env, rate, render_channels, capture_channels, nullptr);
    }
    std::unique_ptr<webrtc::EchoControl> Create(
        const webrtc::Environment& env, int rate, int render_channels, int capture_channels,
        webrtc::NeuralResidualEchoEstimator* estimator) override {
        return std::make_unique<MeasuredEcho>(
            std::make_unique<webrtc::EchoCanceller3>(env, webrtc::EchoCanceller3Config(), std::nullopt,
                                                     estimator, rate, render_channels, capture_channels),
            energy_);
    }

private:
    std::shared_ptr<EchoEnergy> energy_;
};

struct Processor {
    webrtc::scoped_refptr<webrtc::AudioProcessing> apm;
    std::shared_ptr<EchoEnergy> echo = std::make_shared<EchoEnergy>();
};
}

// Fixed 48 kHz, 10 ms PCM frames keep lengths and layouts at the Rust boundary.
extern "C" void* flex_apm_create(bool echo, bool gain, bool noise, int level) noexcept {
    if (level < 0 || level > 3) return nullptr;
    try {
        auto processor = std::make_unique<Processor>();
        webrtc::BuiltinAudioProcessingBuilder builder;
        // With a factory the processor always cancels echo, so it gets one only when asked to.
        if (echo) builder.SetEchoControlFactory(std::make_unique<MeasuredEchoFactory>(processor->echo));
        processor->apm = builder.Build(webrtc::CreateEnvironment(std::make_unique<FieldTrials>()));
        if (!processor->apm) return nullptr;
        webrtc::AudioProcessing::Config config;
        config.echo_canceller.enabled = echo;
        config.gain_controller2.enabled = gain;
        config.gain_controller2.adaptive_digital.enabled = gain;
        config.high_pass_filter.enabled = true;
        config.noise_suppression.enabled = noise;
        config.noise_suppression.level = static_cast<webrtc::AudioProcessing::Config::NoiseSuppression::Level>(level);
        processor->apm->ApplyConfig(config);
        if (processor->apm->Initialize() != 0) return nullptr;
        return processor.release();
    } catch (...) { return nullptr; }
}

extern "C" void flex_apm_destroy(void* processor) noexcept { delete static_cast<Processor*>(processor); }

extern "C" int flex_apm_capture(void* processor, int16_t* pcm, int delay_ms) noexcept {
    if (!processor || !pcm) return -1;
    try {
        auto& apm = static_cast<Processor*>(processor)->apm;
        if (delay_ms >= 0 && apm->set_stream_delay_ms(delay_ms) != 0) return -1;
        const webrtc::StreamConfig format(48000, 1);
        return apm->ProcessStream(pcm, format, format, pcm);
    } catch (...) { return -1; }
}

extern "C" int flex_apm_render(void* processor, int16_t* pcm) noexcept {
    if (!processor || !pcm) return -1;
    try {
        const webrtc::StreamConfig format(48000, 2);
        return static_cast<Processor*>(processor)->apm->ProcessReverseStream(pcm, format, format, pcm);
    } catch (...) { return -1; }
}

// Mute is applied after processing, as WebRTC's own send path does: capture keeps flowing so
// echo cancellation stays aligned with playback, and the processor is told its output is unused.
extern "C" int flex_apm_set_output_muted(void* processor, bool muted) noexcept {
    if (!processor) return -1;
    try {
        auto* self = static_cast<Processor*>(processor);
        self->apm->set_output_will_be_muted(muted);
        self->echo->sent = !muted;
        return 0;
    } catch (...) { return -1; }
}

struct FlexEchoStats {
    double erl;
    double erle;
    double divergent_filter_fraction;
    double residual_echo_likelihood;
    int32_t delay_ms;
    // Bit per field above, in order, set when WebRTC reported it.
    uint32_t present;
    // Energy into and out of echo cancellation since the last read, low/mid/high as in
    // `EchoEnergy`; zero while it is off or its output is not used.
    double echo_in[3];
    double echo_out[3];
};

extern "C" int flex_apm_stats(void* processor, FlexEchoStats* out) noexcept {
    if (!processor || !out) return -1;
    try {
        auto* self = static_cast<Processor*>(processor);
        const auto stats = self->apm->GetStatistics();
        *out = {};
        for (int band = 0; band < 3; ++band) {
            out->echo_in[band] = self->echo->before[band];
            out->echo_out[band] = self->echo->after[band];
        }
        *self->echo = {.sent = self->echo->sent};
        if (stats.echo_return_loss) { out->erl = *stats.echo_return_loss; out->present |= 1; }
        if (stats.echo_return_loss_enhancement) { out->erle = *stats.echo_return_loss_enhancement; out->present |= 2; }
        if (stats.divergent_filter_fraction) { out->divergent_filter_fraction = *stats.divergent_filter_fraction; out->present |= 4; }
        if (stats.residual_echo_likelihood) { out->residual_echo_likelihood = *stats.residual_echo_likelihood; out->present |= 8; }
        if (stats.delay_ms) { out->delay_ms = *stats.delay_ms; out->present |= 16; }
        return 0;
    } catch (...) { return -1; }
}

extern "C" void* flex_vad_create() noexcept {
    auto* vad = WebRtcVad_Create();
    if (!vad) return nullptr;
    if (WebRtcVad_Init(vad) != 0 || WebRtcVad_set_mode(vad, 2) != 0 ||
        WebRtcVad_ValidRateAndFrameLength(48000, 960) != 0) {
        WebRtcVad_Free(vad);
        return nullptr;
    }
    return vad;
}

extern "C" void flex_vad_destroy(void* vad) noexcept { WebRtcVad_Free(static_cast<VadInst*>(vad)); }
extern "C" int flex_vad_process(void* vad, const int16_t* pcm) noexcept {
    if (!vad || !pcm) return -1;
    return WebRtcVad_Process(static_cast<VadInst*>(vad), 48000, pcm, 960);
}
