import Foundation
import Security

/// Listens for the broadcast extension while connected and forwards its pictures to the share.
final class ScreenInbox: @unchecked Sendable {
    private let lock = NSLock()
    private var listener: Int32 = -1
    private var client: Int32 = -1
    private var token = Data()
    private let started: @Sendable () -> Void
    private let ended: @Sendable () -> Void

    init(started: @escaping @Sendable () -> Void, ended: @escaping @Sendable () -> Void) {
        self.started = started
        self.ended = ended
    }

    /// Listens on a private local port and tells the extension how to reach it. False on failure.
    @discardableResult
    func open() -> Bool {
        close()
        let descriptor = socket(AF_INET, SOCK_STREAM, 0)
        guard descriptor >= 0 else { return false }
        var address = ScreenLink.address(port: 0)
        let bound = withUnsafePointer(to: &address) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                bind(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        var actual = sockaddr_in()
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let named = withUnsafeMutablePointer(to: &actual) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                getsockname(descriptor, $0, &length)
            }
        }
        var secret = Data(count: ScreenLink.tokenLength)
        let random = secret.withUnsafeMutableBytes {
            SecRandomCopyBytes(kSecRandomDefault, ScreenLink.tokenLength, $0.baseAddress!)
        }
        guard bound == 0, named == 0, random == errSecSuccess, listen(descriptor, 1) == 0,
              ScreenLink.publish(.init(port: UInt16(bigEndian: actual.sin_port), token: secret)) else {
            Darwin.close(descriptor)
            return false
        }
        lock.withLock {
            listener = descriptor
            token = secret
        }
        Thread.detachNewThread { [self] in accept(on: descriptor) }
        return true
    }

    func close() {
        lock.withLock {
            if listener >= 0 { shutdown(listener, SHUT_RDWR); Darwin.close(listener) }
            listener = -1
            if client >= 0 { shutdown(client, SHUT_RDWR) }
        }
        ScreenLink.withdraw()
    }

    /// Ends the current broadcast; the extension sees the socket close and stops.
    func endBroadcast() {
        lock.withLock {
            if client >= 0 { shutdown(client, SHUT_RDWR) }
        }
    }

    private func accept(on descriptor: Int32) {
        while true {
            let connection = Darwin.accept(descriptor, nil, nil)
            guard connection >= 0 else { return }
            var on: Int32 = 1
            setsockopt(connection, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
            guard authenticated(connection) else {
                Darwin.close(connection)
                continue
            }
            lock.withLock { client = connection }
            started()
            read(from: connection)
            lock.withLock { client = -1 }
            Darwin.close(connection)
            ended()
        }
    }

    /// Only the extension can read the token; anything else on this phone is turned away.
    private func authenticated(_ connection: Int32) -> Bool {
        var timeout = timeval(tv_sec: 3, tv_usec: 0)
        setsockopt(connection, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        var received = Data(count: ScreenLink.tokenLength)
        guard received.withUnsafeMutableBytes({ fill($0, from: connection) }) else { return false }
        timeout = timeval(tv_sec: 0, tv_usec: 0)
        setsockopt(connection, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout<timeval>.size))
        let expected = lock.withLock { token }
        guard expected.count == received.count else { return false }
        return zip(expected, received).reduce(UInt8(0)) { $0 | ($1.0 ^ $1.1) } == 0
    }

    private func read(from connection: Int32) {
        var y = [UInt8]()
        var uv = [UInt8]()
        while true {
            var header = ScreenLink.Header(width: 0, height: 0, yStride: 0, uvStride: 0, rotation: 0, yLength: 0, uvLength: 0)
            let received = withUnsafeMutableBytes(of: &header) { fill($0, from: connection) }
            guard received, header.magic == ScreenLink.magic,
                  header.yLength <= ScreenLink.maximumPlane, header.uvLength <= ScreenLink.maximumPlane else { return }
            if y.count != Int(header.yLength) { y = [UInt8](repeating: 0, count: Int(header.yLength)) }
            if uv.count != Int(header.uvLength) { uv = [UInt8](repeating: 0, count: Int(header.uvLength)) }
            guard y.withUnsafeMutableBytes({ fill($0, from: connection) }),
                  uv.withUnsafeMutableBytes({ fill($0, from: connection) }) else { return }
            y.withUnsafeBufferPointer { yBytes in
                uv.withUnsafeBufferPointer { uvBytes in
                    flexspeak_push_screen(
                        yBytes.baseAddress, UInt(yBytes.count), header.yStride,
                        uvBytes.baseAddress, UInt(uvBytes.count), header.uvStride,
                        header.width, header.height, header.rotation
                    )
                }
            }
        }
    }

    private func fill(_ target: UnsafeMutableRawBufferPointer, from connection: Int32) -> Bool {
        var offset = 0
        while offset < target.count {
            let received = recv(connection, target.baseAddress! + offset, target.count - offset, 0)
            if received <= 0 {
                if received < 0, errno == EINTR { continue }
                return false
            }
            offset += received
        }
        return true
    }
}
