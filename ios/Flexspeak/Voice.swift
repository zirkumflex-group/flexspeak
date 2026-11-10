import AVFoundation
import Foundation
import Observation
import UIKit

struct Member: Decodable, Identifiable, Equatable {
    let id: Int
    let name: String
    let isSelf: Bool
    let micMuted: Bool
    let soundMuted: Bool
    let canPoke: Bool?
    let canMove: Bool?
}

struct Channel: Decodable, Identifiable, Equatable {
    let id: Int
    let name: String
    let depth: Int
    let hasPassword: Bool
    let canJoin: Bool
    let members: [Member]
}

struct StreamInfo: Decodable, Identifiable, Equatable {
    let id: String
    let publisher: Int
    let name: String
    let viewers: Int
    let hasAudio: Bool
}

/// The stream being watched, and how it is going.
struct Watch: Equatable {
    let slot: Int
    let stream: StreamInfo
    var phase: String
    var detail: String
    var summary = ""
}

struct ShareRequest: Decodable, Identifiable, Equatable {
    let id: Int
    let name: String
}

struct ShareViewer: Decodable, Identifiable, Equatable {
    let id: Int
    let name: String
    let ready: Bool
}

/// Your own screen share.
struct Sharing: Equatable {
    var active = false
    var live = false
    var detail = ""
    var requests: [ShareRequest] = []
    var viewers: [ShareViewer] = []
    var videoFrames = 0
}

struct Poke: Identifiable {
    let id = UUID()
    let from: String
    let message: String
}

/// One event from the Rust session. Each type fills in only its own fields.
private struct Wire: Decodable {
    let type: String
    var text: String?
    var error: String?
    var server: String?
    var serverVersion: String?
    var channels: [Channel]?
    var current: Int?
    var canTalk: Bool?
    var ids: [Int]?
    var value: Bool?
    var level: Double?
    var sent: Int?
    var received: Int?
    var gaps: Int?
    var callbackFrames: Int?
    var upload: Double?
    var download: Double?
    var uploaded: Int?
    var downloaded: Int?
    var ping: Double?
    var pingDeviation: Double?
    var lossIn: Double?
    var lossOut: Double?
    var muted: Bool?
    var deafened: Bool?
    var pending: Bool?
    var channel: Int?
    var from: String?
    var source: String?
    var lines: [String]?
    var streams: [StreamInfo]?
    var slot: Int?
    var stream: StreamInfo?
    var active: Bool?
    var phase: String?
    var detail: String?
    var codec: String?
    var decoder: String?
    var path: String?
    var fps: Double?
    var width: Int?
    var height: Int?
    var live: Bool?
    var requests: [ShareRequest]?
    var viewers: [ShareViewer]?
    var videoFrames: Int?
}

private let deliver: FlexspeakCallback = { json, context in
    guard let json, let context else { return }
    let data = Data(bytes: json, count: strlen(json))
    let voice = Unmanaged<Voice>.fromOpaque(context).takeUnretainedValue()
    DispatchQueue.main.async { voice.receive(data) }
}

/// The connection and everything the screens show about it.
@MainActor
@Observable
final class Voice {
    enum Phase { case idle, connecting, connected }

    var phase = Phase.idle
    var status = ""
    var failure: String?
    var warning: String?
    var server = ""
    var serverVersion = ""
    var channels: [Channel] = []
    var currentChannel: Int?
    var canTalk = false
    var talking: Set<Int> = []
    var transmitting = false
    var muted = true
    var deafened = false
    var level = 0.0
    var sentPackets = 0
    var receivedPackets = 0
    var playbackGaps = 0
    var callbackFrames = 0
    let routes = AudioRoutes()
    var upload = 0.0
    var download = 0.0
    var uploaded = 0
    var downloaded = 0
    var ping: Double?
    var pingDeviation: Double?
    var lossIn: Double?
    var lossOut: Double?
    var audioDescription = ""
    var poke: Poke?
    var channelNeedingPassword: Int?
    var activity: [String] = []
    var streams: [StreamInfo] = []
    var watching: Watch?
    var sharing = Sharing()
    var shareAccess = UserDefaults.standard.string(forKey: "shareAccess") ?? "private" {
        didSet { UserDefaults.standard.set(shareAccess, forKey: "shareAccess") }
    }
    var shareQuality = UserDefaults.standard.string(forKey: "shareQuality") ?? "balanced" {
        didSet { UserDefaults.standard.set(shareQuality, forKey: "shareQuality") }
    }
    var streamNotice: String?

    private var session: OpaquePointer?
    @ObservationIgnored private lazy var inbox = ScreenInbox(
        started: { [weak self] in DispatchQueue.main.async { self?.broadcastStarted() } },
        ended: { [weak self] in DispatchQueue.main.async { self?.broadcastEnded() } }
    )
    /// One 1280×720 RGBA picture, the most the receiver delivers.
    @ObservationIgnored private var frameBuffer = [UInt8](repeating: 0, count: 1280 * 720 * 4)

    func connect(address: String, nickname: String, password: String) async {
        guard phase == .idle else { return }
        phase = .connecting
        failure = nil
        status = "Waiting for microphone access…"
        guard await AVAudioApplication.requestRecordPermission() else {
            phase = .idle
            failure = "Flexspeak needs the microphone. Allow it in Settings, then connect again."
            return
        }
        do {
            try routes.activate()
        } catch {
            phase = .idle
            failure = "Cannot start audio: \(error.localizedDescription)"
            return
        }
        status = "Connecting…"
        let context = Unmanaged.passUnretained(self).toOpaque()
        let opened: UInt? = await Task.detached {
            let pointer = address.withCString { address in
                nickname.withCString { nickname in
                    password.withCString { password in
                        flexspeak_connect(address, nickname, password, deliver, context)
                    }
                }
            }
            return pointer.map { UInt(bitPattern: $0) }
        }.value
        session = opened.flatMap { OpaquePointer(bitPattern: $0) }
    }

    func disconnect() {
        guard let session else { return }
        self.session = nil
        status = "Disconnecting…"
        flexspeak_disconnect(session)
    }

    func setMuted(_ value: Bool) {
        muted = value
        send(["type": "mute", "value": value])
    }

    func setDeafened(_ value: Bool) {
        deafened = value
        send(["type": "deafen", "value": value])
    }

    func join(_ channel: Channel, password: String = "") {
        send(["type": "join", "channel": channel.id, "password": password])
    }

    func poke(_ member: Member, message: String) {
        send(["type": "poke", "client": member.id, "message": message])
    }

    func move(_ member: Member, to channel: Channel) {
        send(["type": "move", "client": member.id, "channel": channel.id])
    }

    private func broadcastStarted() {
        send(["type": "startSharing", "access": shareAccess, "quality": shareQuality])
    }

    private func broadcastEnded() {
        send(["type": "stopSharing"])
    }

    /// Stops the broadcast; the extension notices and ends, which stops the share.
    func stopSharing() {
        inbox.endBroadcast()
        send(["type": "stopSharing"])
    }

    func decide(_ request: ShareRequest, allow: Bool) {
        send(["type": "decideViewer", "client": request.id, "allow": allow])
    }

    func watch(_ stream: StreamInfo) {
        streamNotice = nil
        send(["type": "watch", "stream": stream.id])
    }

    func stopWatching() {
        guard let watching else { return }
        send(["type": "stopWatching", "stream": watching.stream.id])
        self.watching = nil
    }

    /// The newest picture of the watched stream, if one arrived since the last call.
    func nextFrame() -> UIImage? {
        guard let session, let watching else { return nil }
        var width: UInt32 = 0
        var height: UInt32 = 0
        let capacity = frameBuffer.count
        let copied = frameBuffer.withUnsafeMutableBufferPointer { buffer in
            flexspeak_frame(session, UInt(watching.slot), buffer.baseAddress, UInt(capacity), &width, &height)
        }
        guard copied, width > 0, height > 0 else { return nil }
        let length = Int(width) * Int(height) * 4
        guard let provider = CGDataProvider(data: Data(frameBuffer[0..<length]) as CFData),
              let image = CGImage(
                  width: Int(width), height: Int(height),
                  bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: Int(width) * 4,
                  space: CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.noneSkipLast.rawValue),
                  provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
              ) else { return nil }
        return UIImage(cgImage: image)
    }

    private func send(_ command: [String: Any]) {
        guard let session,
              let data = try? JSONSerialization.data(withJSONObject: command),
              let json = String(data: data, encoding: .utf8) else { return }
        flexspeak_command(session, json)
    }

    fileprivate func receive(_ data: Data) {
        guard let event = try? JSONDecoder().decode(Wire.self, from: data) else { return }
        switch event.type {
        case "status":
            status = event.text ?? ""
        case "connected":
            audioDescription = event.text ?? ""
            muted = true
            deafened = false
            phase = .connected
            if !inbox.open() {
                warning = "Screen sharing is unavailable: the local link to the broadcast extension can't be opened."
            }
        case "state":
            server = event.server ?? ""
            serverVersion = event.serverVersion ?? ""
            channels = event.channels ?? []
            currentChannel = event.current
            canTalk = event.canTalk ?? false
        case "talking":
            talking = Set(event.ids ?? [])
        case "transmitting":
            transmitting = event.value ?? false
        case "meter":
            level = event.level ?? 0
            sentPackets = event.sent ?? 0
            receivedPackets = event.received ?? 0
            playbackGaps = event.gaps ?? 0
            callbackFrames = event.callbackFrames ?? 0
        case "usage":
            upload = event.upload ?? 0
            download = event.download ?? 0
            uploaded = event.uploaded ?? 0
            downloaded = event.downloaded ?? 0
            ping = event.ping
            pingDeviation = event.pingDeviation
            lossIn = event.lossIn
            lossOut = event.lossOut
        case "warning":
            warning = event.text
        case "recovered":
            warning = nil
        case "password":
            channelNeedingPassword = event.channel
        case "poked":
            poke = Poke(from: event.from ?? "Someone", message: event.text ?? "")
        case "activity":
            activity = Array((activity + (event.lines ?? [])).suffix(50))
        case "catalog":
            streams = event.streams ?? []
        case "notice":
            streamNotice = event.text
        case "sharing":
            sharing = Sharing(
                active: event.active ?? false, live: event.live ?? false, detail: event.detail ?? "",
                requests: event.requests ?? [], viewers: event.viewers ?? [],
                videoFrames: event.videoFrames ?? 0
            )
            if !sharing.active { inbox.endBroadcast() }
        case "watch":
            guard let stream = event.stream, let slot = event.slot else { break }
            if event.active == true {
                let summary = watching?.stream.id == stream.id ? watching?.summary ?? "" : ""
                watching = Watch(slot: slot, stream: stream, phase: event.phase ?? "",
                                 detail: event.detail ?? "", summary: summary)
            } else if watching?.stream.id == stream.id {
                watching = nil
                streamNotice = event.detail
            }
        case "streamStats":
            watching?.summary = [
                event.codec ?? "",
                event.width.flatMap { w in event.height.map { "\(w)×\($0)" } } ?? "",
                event.fps.map { String(format: "%.0f fps", $0) } ?? "",
                event.path ?? "",
            ].filter { !$0.isEmpty }.joined(separator: " · ")
        case "stopped":
            inbox.close()
            sharing = Sharing()
            watching = nil
            if let session {
                self.session = nil
                flexspeak_disconnect(session)
            }
            failure = event.error
            phase = .idle
            status = ""
            channels = []
            currentChannel = nil
            talking = []
            streams = []
            activity = []
            ping = nil
            routes.deactivate()
        default:
            break
        }
    }
}
