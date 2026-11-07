#import <AVFoundation/AVFoundation.h>
#import <CoreVideo/CoreVideo.h>

using DeviceCallback = void (*)(void*, const char*, const char*);
using FrameCallback = void (*)(void*, const uint8_t*, uint32_t, uint32_t, uint32_t);

// Only the selected camera is opened, after the user starts sharing and grants access.
static NSArray<AVCaptureDevice*>* cameras() {
    return [AVCaptureDeviceDiscoverySession
        discoverySessionWithDeviceTypes:@[AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeExternal, AVCaptureDeviceTypeContinuityCamera]
        mediaType:AVMediaTypeVideo position:AVCaptureDevicePositionUnspecified].devices;
}

extern "C" void flex_camera_devices(void* context, DeviceCallback callback) {
    @autoreleasepool {
        for (AVCaptureDevice* device in cameras()) {
            callback(context, device.uniqueID.UTF8String, device.localizedName.UTF8String);
        }
    }
}

extern "C" void flex_camera_access(void* context, void (*callback)(void*, bool)) {
    [AVCaptureDevice requestAccessForMediaType:AVMediaTypeVideo completionHandler:^(BOOL granted) {
        callback(context, granted);
    }];
}

@interface FlexCameraCapture : NSObject<AVCaptureVideoDataOutputSampleBufferDelegate>
@property(nonatomic, strong) AVCaptureSession* session;
@property(nonatomic, strong) AVCaptureVideoDataOutput* output;
@property(nonatomic, strong) dispatch_queue_t queue;
@property(nonatomic) void* context;
@property(nonatomic) FrameCallback callback;
@end

@implementation FlexCameraCapture
- (void)captureOutput:(AVCaptureOutput*)output didOutputSampleBuffer:(CMSampleBufferRef)sample
       fromConnection:(AVCaptureConnection*)connection {
    CVPixelBufferRef pixels = CMSampleBufferGetImageBuffer(sample);
    if (!pixels || CVPixelBufferLockBaseAddress(pixels, kCVPixelBufferLock_ReadOnly) != kCVReturnSuccess) return;
    self.callback(self.context, static_cast<const uint8_t*>(CVPixelBufferGetBaseAddress(pixels)),
        (uint32_t)CVPixelBufferGetBytesPerRow(pixels),
        (uint32_t)CVPixelBufferGetWidth(pixels), (uint32_t)CVPixelBufferGetHeight(pixels));
    CVPixelBufferUnlockBaseAddress(pixels, kCVPixelBufferLock_ReadOnly);
}
@end

extern "C" void* flex_camera_start(const char* identifier, uint32_t width, uint32_t height,
                                   uint32_t fps, void* context, FrameCallback callback) {
    @autoreleasepool {
        if ([AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeVideo] != AVAuthorizationStatusAuthorized) return nullptr;
        AVCaptureDevice* device = [AVCaptureDevice deviceWithUniqueID:[NSString stringWithUTF8String:identifier]];
        if (!device) return nullptr;
        NSError* error = nil;
        AVCaptureDeviceInput* input = [AVCaptureDeviceInput deviceInputWithDevice:device error:&error];
        if (!input) return nullptr;
        FlexCameraCapture* capture = [FlexCameraCapture new];
        capture.context = context;
        capture.callback = callback;
        capture.session = [AVCaptureSession new];
        capture.output = [AVCaptureVideoDataOutput new];
        capture.output.videoSettings = @{(id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_32BGRA)};
        capture.output.alwaysDiscardsLateVideoFrames = YES;
        capture.queue = dispatch_queue_create("io.venator.flexspeak.camera", DISPATCH_QUEUE_SERIAL);
        [capture.session beginConfiguration];
        if (![capture.session canAddInput:input] || ![capture.session canAddOutput:capture.output]) return nullptr;
        [capture.session addInput:input];
        [capture.session addOutput:capture.output];
        // Choose a supported format nearest the requested ceiling. The Rust pipeline also
        // scales to fit and caps frame delivery, so unsupported settings never upscale.
        AVCaptureDeviceFormat* best = nil;
        double bestScore = HUGE_VAL;
        for (AVCaptureDeviceFormat* format in device.formats) {
            CMVideoDimensions size = CMVideoFormatDescriptionGetDimensions(format.formatDescription);
            if (size.width <= 0 || size.height <= 0 || size.width > 8192 || size.height > 8192) continue;
            double maxFps = 0;
            for (AVFrameRateRange* range in format.videoSupportedFrameRateRanges) maxFps = fmax(maxFps, range.maxFrameRate);
            double score = fabs(log((double)size.width / width)) + fabs(log((double)size.height / height))
                + fmax(0, (double)fps - maxFps) / fps;
            if (score < bestScore) { best = format; bestScore = score; }
        }
        if (best && [device lockForConfiguration:&error]) {
            device.activeFormat = best;
            AVFrameRateRange* closest = nil;
            double chosen = 0, distance = HUGE_VAL;
            for (AVFrameRateRange* range in best.videoSupportedFrameRateRanges) {
                double rate = fmin(range.maxFrameRate, fmax(range.minFrameRate, fps));
                if (fabs(rate - fps) < distance) { closest = range; chosen = rate; distance = fabs(rate - fps); }
            }
            if (closest && chosen > 0) {
                CMTime duration = CMTimeMake(1000000, (int32_t)llround(chosen * 1000000));
                device.activeVideoMinFrameDuration = duration;
                device.activeVideoMaxFrameDuration = duration;
            }
            [device unlockForConfiguration];
        }
        [capture.session commitConfiguration];
        [capture.output setSampleBufferDelegate:capture queue:capture.queue];
        [capture.session startRunning];
        if (!capture.session.running) {
            [capture.output setSampleBufferDelegate:nil queue:nil];
            dispatch_sync(capture.queue, ^{});
            return nullptr;
        }
        return (__bridge_retained void*)capture;
    }
}

extern "C" void flex_camera_stop(void* pointer) {
    @autoreleasepool {
        FlexCameraCapture* capture = (__bridge_transfer FlexCameraCapture*)pointer;
        [capture.session stopRunning];
        [capture.output setSampleBufferDelegate:nil queue:nil];
        // Finish any callback before Rust releases its context.
        dispatch_sync(capture.queue, ^{});
    }
}
