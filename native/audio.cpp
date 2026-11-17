#include <cstdint>
#include <memory>
#include "api/audio/builtin_audio_processing_builder.h"
#include "api/environment/environment_factory.h"
#include "common_audio/vad/include/webrtc_vad.h"

namespace {
struct Processor {
    webrtc::scoped_refptr<webrtc::AudioProcessing> apm;
};
}

// Fixed 48 kHz, 10 ms PCM frames keep lengths and layouts at the Rust boundary.
extern "C" void* flex_apm_create(bool echo, bool gain, bool noise, int level) noexcept {
    if (level < 0 || level > 3) return nullptr;
    try {
        auto processor = std::make_unique<Processor>();
        processor->apm = webrtc::BuiltinAudioProcessingBuilder().Build(webrtc::CreateEnvironment());
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
        static_cast<Processor*>(processor)->apm->set_output_will_be_muted(muted);
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
};

extern "C" int flex_apm_stats(void* processor, FlexEchoStats* out) noexcept {
    if (!processor || !out) return -1;
    try {
        const auto stats = static_cast<Processor*>(processor)->apm->GetStatistics();
        *out = {};
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
