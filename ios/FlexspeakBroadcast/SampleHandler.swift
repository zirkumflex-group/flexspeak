import Accelerate
import CoreMedia
import CoreVideo
import ReplayKit

/// Shrinks each screen picture and passes it to the app, which does the encoding. Encoding here
/// would not fit in the small amount of memory iOS gives broadcast extensions.
final class SampleHandler: RPBroadcastSampleHandler {
    private static let longestSide = 1280.0
    private static let shortestSide = 720.0
    private static let minimumInterval = 1.0 / 15

    private var socket: Int32 = -1
    private var lastPicture = 0.0
    private var yPlane = [UInt8]()
    private var uvPlane = [UInt8]()

    override func broadcastStarted(withSetupInfo setupInfo: [String: NSObject]?) {
        guard connect() else {
            finish("Open Flexspeak and connect to a server, then start the broadcast again.")
            return
        }
    }

    override func broadcastFinished() {
        disconnect()
    }

    override func processSampleBuffer(_ sampleBuffer: CMSampleBuffer, with sampleBufferType: RPSampleBufferType) {
        guard sampleBufferType == .video, socket >= 0 else { return }
        let now = CACurrentMediaTime()
        guard now - lastPicture >= Self.minimumInterval else { return }
        guard let pixels = CMSampleBufferGetImageBuffer(sampleBuffer) else { return }
        lastPicture = now
        let rotation = Self.rotation(of: sampleBuffer)
        CVPixelBufferLockBaseAddress(pixels, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pixels, .readOnly) }
        guard CVPixelBufferGetPlaneCount(pixels) == 2, send(pixels, rotation: rotation) else {
            finish("Flexspeak stopped sharing.")
            return
        }
    }

    private static func rotation(of sampleBuffer: CMSampleBuffer) -> UInt32 {
        let key = RPVideoSampleOrientationKey as CFString
        guard let value = CMGetAttachment(sampleBuffer, key: key, attachmentModeOut: nil) as? NSNumber,
              let orientation = CGImagePropertyOrientation(rawValue: value.uint32Value) else { return 0 }
        switch orientation {
        case .left, .leftMirrored: return 90
        case .right, .rightMirrored: return 270
        case .down, .downMirrored: return 180
        default: return 0
        }
    }

    private func send(_ pixels: CVPixelBuffer, rotation: UInt32) -> Bool {
        let sourceWidth = Double(CVPixelBufferGetWidth(pixels))
        let sourceHeight = Double(CVPixelBufferGetHeight(pixels))
        let (long, short) = sourceHeight > sourceWidth
            ? (Self.shortestSide, Self.longestSide)
            : (Self.longestSide, Self.shortestSide)
        let scale = min(long / sourceWidth, short / sourceHeight, 1)
        let width = max(Int(sourceWidth * scale) & ~1, 2)
        let height = max(Int(sourceHeight * scale) & ~1, 2)
        let yLength = width * height
        let uvLength = width * height / 2
        if yPlane.count != yLength { yPlane = [UInt8](repeating: 0, count: yLength) }
        if uvPlane.count != uvLength { uvPlane = [UInt8](repeating: 128, count: uvLength) }

        var yIn = Self.plane(pixels, 0)
        var uvIn = Self.plane(pixels, 1)
        let scaled: Bool = yPlane.withUnsafeMutableBytes { y in
            uvPlane.withUnsafeMutableBytes { uv in
                var yOut = vImage_Buffer(data: y.baseAddress, height: vImagePixelCount(height),
                                         width: vImagePixelCount(width), rowBytes: width)
                var uvOut = vImage_Buffer(data: uv.baseAddress, height: vImagePixelCount(height / 2),
                                          width: vImagePixelCount(width / 2), rowBytes: width)
                return vImageScale_Planar8(&yIn, &yOut, nil, vImage_Flags(kvImageNoFlags)) == kvImageNoError
                    && vImageScale_CbCr8(&uvIn, &uvOut, nil, vImage_Flags(kvImageNoFlags)) == kvImageNoError
            }
        }
        guard scaled else { return false }
        var header = ScreenLink.Header(
            width: UInt32(width), height: UInt32(height),
            yStride: UInt32(width), uvStride: UInt32(width), rotation: rotation,
            yLength: UInt32(yLength), uvLength: UInt32(uvLength)
        )
        return withUnsafeBytes(of: &header) { write($0) }
            && yPlane.withUnsafeBytes { write($0) }
            && uvPlane.withUnsafeBytes { write($0) }
    }

    private static func plane(_ pixels: CVPixelBuffer, _ index: Int) -> vImage_Buffer {
        vImage_Buffer(
            data: CVPixelBufferGetBaseAddressOfPlane(pixels, index),
            height: vImagePixelCount(CVPixelBufferGetHeightOfPlane(pixels, index)),
            width: vImagePixelCount(CVPixelBufferGetWidthOfPlane(pixels, index)),
            rowBytes: CVPixelBufferGetBytesPerRowOfPlane(pixels, index)
        )
    }

    private func write(_ bytes: UnsafeRawBufferPointer) -> Bool {
        var offset = 0
        while offset < bytes.count {
            let sent = Darwin.send(socket, bytes.baseAddress! + offset, bytes.count - offset, 0)
            if sent <= 0 {
                if sent < 0, errno == EINTR { continue }
                return false
            }
            offset += sent
        }
        return true
    }

    private func connect() -> Bool {
        guard let endpoint = ScreenLink.endpoint() else { return false }
        var address = ScreenLink.address(port: endpoint.port)
        let descriptor = Darwin.socket(AF_INET, SOCK_STREAM, 0)
        guard descriptor >= 0 else { return false }
        var on: Int32 = 1
        setsockopt(descriptor, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        var timeout = timeval(tv_sec: 2, tv_usec: 0)
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        let connected = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        guard connected == 0 else {
            close(descriptor)
            return false
        }
        socket = descriptor
        guard endpoint.token.withUnsafeBytes({ write($0) }) else {
            disconnect()
            return false
        }
        return true
    }

    private func disconnect() {
        if socket >= 0 { close(socket) }
        socket = -1
    }

    private func finish(_ message: String) {
        disconnect()
        finishBroadcastWithError(NSError(
            domain: "io.zirkumflex.flexspeak.broadcast", code: 1,
            userInfo: [NSLocalizedFailureReasonErrorKey: message, NSLocalizedDescriptionKey: message]
        ))
    }
}
