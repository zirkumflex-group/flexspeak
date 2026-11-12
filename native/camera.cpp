// Windows uses the camera module from the WebRTC SDK already shipped with the app.
#include <algorithm>
#include <cstdint>
#include <memory>
#include <vector>
#include <objbase.h>
#include "modules/video_capture/video_capture_factory.h"
#include "libyuv/convert_argb.h"

using DeviceCallback = void (*)(void*, const char*, const char*);
using FrameCallback = void (*)(void*, const uint8_t*, uint32_t, uint32_t, uint32_t);
// The picture as the camera module gives it: Y, U and V with their strides, then the size.
using PlanesCallback = void (*)(void*, const uint8_t*, uint32_t, const uint8_t*, uint32_t,
                               const uint8_t*, uint32_t, uint32_t, uint32_t);

namespace {
struct ComScope {
    HRESULT result = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    ~ComScope() { if (SUCCEEDED(result)) CoUninitialize(); }
};
struct Capture : webrtc::VideoSinkInterface<webrtc::VideoFrame> {
    ComScope com;
    webrtc::scoped_refptr<webrtc::VideoCaptureModule> module;
    void* context;
    FrameCallback callback = nullptr;
    PlanesCallback planes = nullptr;
    std::vector<uint8_t> pixels;
    void OnFrame(const webrtc::VideoFrame& frame) override {
        auto buffer = frame.video_frame_buffer()->ToI420();
        if (!buffer || buffer->width() <= 0 || buffer->height() <= 0 ||
            buffer->width() > 8192 || buffer->height() > 8192) return;
        // A share encodes I420: it takes the planes as they are, with nothing converted here.
        if (planes) {
            planes(context, buffer->DataY(), buffer->StrideY(), buffer->DataU(), buffer->StrideU(),
                   buffer->DataV(), buffer->StrideV(), buffer->width(), buffer->height());
            return;
        }
        uint32_t stride = buffer->width() * 4;
        pixels.resize(static_cast<size_t>(stride) * buffer->height());
        if (libyuv::I420ToARGB(buffer->DataY(), buffer->StrideY(), buffer->DataU(), buffer->StrideU(),
            buffer->DataV(), buffer->StrideV(), pixels.data(), stride, buffer->width(), buffer->height()) == 0) {
            callback(context, pixels.data(), stride, buffer->width(), buffer->height());
        }
    }
    ~Capture() {
        if (module) { module->StopCapture(); module->DeRegisterCaptureDataCallback(); }
    }
};
}

extern "C" void flex_camera_devices(void* context, DeviceCallback callback) noexcept {
    try {
        ComScope com;
        std::unique_ptr<webrtc::VideoCaptureModule::DeviceInfo> devices(webrtc::VideoCaptureFactory::CreateDeviceInfo());
        if (!devices) return;
        for (uint32_t i = 0; i < std::min(devices->NumberOfDevices(), 128u); ++i) {
            char name[1024] = {}, id[1024] = {};
            if (devices->GetDeviceName(i, name, sizeof(name), id, sizeof(id)) == 0) callback(context, id, name);
        }
    } catch (...) {}
}

extern "C" void flex_camera_access(void* context, void (*callback)(void*, bool)) noexcept {
    // Desktop-camera privacy is enforced when the OS opens the device.
    callback(context, true);
}

namespace {
void* start(const char* id, uint32_t width, uint32_t height, uint32_t fps, void* context,
            FrameCallback callback, PlanesCallback planes) noexcept {
    try {
        auto capture = std::make_unique<Capture>();
        capture->context = context;
        capture->callback = callback;
        capture->planes = planes;
        capture->module = webrtc::VideoCaptureFactory::Create(id);
        if (!capture->module) return nullptr;
        std::unique_ptr<webrtc::VideoCaptureModule::DeviceInfo> devices(webrtc::VideoCaptureFactory::CreateDeviceInfo());
        if (!devices) return nullptr;
        webrtc::VideoCaptureCapability wanted, chosen;
        wanted.width = width; wanted.height = height; wanted.maxFPS = fps;
        if (devices->GetBestMatchedCapability(id, wanted, chosen) < 0) return nullptr;
        capture->module->RegisterCaptureDataCallback(capture.get());
        if (capture->module->StartCapture(chosen) != 0) return nullptr;
        return capture.release();
    } catch (...) { return nullptr; }
}
}

// Pictures as BGRA, for a still.
extern "C" void* flex_camera_start(const char* id, uint32_t width, uint32_t height,
                                   uint32_t fps, void* context, FrameCallback callback) noexcept {
    return start(id, width, height, fps, context, callback, nullptr);
}
// Pictures as I420 planes, for a share.
extern "C" void* flex_camera_start_planes(const char* id, uint32_t width, uint32_t height,
                                          uint32_t fps, void* context, PlanesCallback callback) noexcept {
    return start(id, width, height, fps, context, nullptr, callback);
}
extern "C" void flex_camera_stop(void* pointer) noexcept { delete static_cast<Capture*>(pointer); }
