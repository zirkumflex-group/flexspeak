import SwiftUI

struct ServerView: View {
    @Environment(Voice.self) private var voice
    @State private var showingStatistics = false
    @State private var showingSettings = false
    @State private var showingShare = false
    @State private var joining: Channel?
    @State private var channelPassword = ""
    @State private var poking: Member?
    @State private var pokeMessage = ""

    var body: some View {
        @Bindable var voice = voice
        VStack(spacing: 0) {
            header
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    if voice.sharing.active { SharingBanner() }
                    if !voice.streams.isEmpty {
                        Text("Streams")
                            .font(.footnote.weight(.semibold))
                            .foregroundStyle(Palette.faint)
                            .padding(.horizontal, 8)
                            .padding(.bottom, 4)
                        ForEach(voice.streams) { streamRow($0) }
                    }
                    if let notice = voice.streamNotice, !notice.isEmpty {
                        Text(notice)
                            .font(.footnote)
                            .foregroundStyle(Palette.muted)
                            .padding(.horizontal, 8)
                            .padding(.bottom, 8)
                    }
                    Text("Channels")
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(Palette.faint)
                        .padding(.horizontal, 8)
                        .padding(.bottom, 4)
                    ForEach(voice.channels) { channel in
                        channelRow(channel)
                        ForEach(channel.members) { member in
                            memberRow(member, in: channel)
                        }
                    }
                }
                .padding(12)
            }
            if let warning = voice.warning {
                Text(warning)
                    .font(.footnote)
                    .foregroundStyle(Palette.poor)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 6)
                    .onTapGesture { voice.warning = nil }
            }
            controls
            footer
        }
        .background(Palette.canvas.ignoresSafeArea())
        .sheet(isPresented: $showingStatistics) { StatisticsView() }
        .sheet(isPresented: $showingSettings) { SettingsView() }
        .sheet(isPresented: $showingShare) { ShareView() }
        .fullScreenCover(isPresented: watchingBinding) { WatchView() }
        .alert(item: $voice.poke) { poke in
            Alert(title: Text("\(poke.from) poked you"), message: Text(poke.message))
        }
        .alert("Channel password", isPresented: joiningBinding) {
            SecureField("Password", text: $channelPassword)
            Button("Join") {
                if let joining { voice.join(joining, password: channelPassword) }
                channelPassword = ""
                joining = nil
            }
            Button("Cancel", role: .cancel) { joining = nil; channelPassword = "" }
        }
        .alert(pokeTitle, isPresented: pokingBinding) {
            TextField("Message", text: $pokeMessage)
            Button("Poke") {
                if let poking { voice.poke(poking, message: pokeMessage) }
                pokeMessage = ""
                poking = nil
            }
            Button("Cancel", role: .cancel) { poking = nil; pokeMessage = "" }
        }
        .onChange(of: voice.channelNeedingPassword) { _, id in
            guard let id else { return }
            joining = voice.channels.first { $0.id == id }
            voice.channelNeedingPassword = nil
        }
    }

    private var pokeTitle: String { "Poke \(poking?.name ?? "")" }
    private var watchingBinding: Binding<Bool> {
        Binding(get: { voice.watching != nil }, set: { if !$0 { voice.stopWatching() } })
    }
    private var joiningBinding: Binding<Bool> {
        Binding(get: { joining != nil }, set: { if !$0 { joining = nil } })
    }
    private var pokingBinding: Binding<Bool> {
        Binding(get: { poking != nil }, set: { if !$0 { poking = nil } })
    }

    private var header: some View {
        HStack(spacing: 10) {
            Text(String(voice.server.prefix(2)).uppercased())
                .font(.subheadline.bold())
                .foregroundStyle(.white)
                .frame(width: 36, height: 36)
                .background(Color(hex: 0xA5508F), in: RoundedRectangle(cornerRadius: 9))
            VStack(alignment: .leading, spacing: 1) {
                Text(voice.server).font(.headline).foregroundStyle(Palette.text).lineLimit(1)
                Text("\(onlineCount) online").font(.caption).foregroundStyle(Palette.muted)
            }
            Spacer()
            Button { showingStatistics = true } label: {
                Image(systemName: "chart.xyaxis.line")
                    .frame(width: 36, height: 36)
                    .foregroundStyle(Palette.muted)
            }
            .accessibilityLabel("Statistics")
            Button { showingSettings = true } label: {
                Image(systemName: "gearshape")
                    .frame(width: 36, height: 36)
                    .foregroundStyle(Palette.muted)
            }
            .accessibilityLabel("Settings")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Palette.chrome)
    }

    private var onlineCount: Int { voice.channels.reduce(0) { $0 + $1.members.count } }

    private func publisherName(_ stream: StreamInfo) -> String {
        voice.channels.lazy.flatMap(\.members).first { $0.id == stream.publisher }?.name ?? "Someone"
    }

    private func streamRow(_ stream: StreamInfo) -> some View {
        HStack(spacing: 10) {
            Circle().fill(Palette.poor).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 1) {
                Text(stream.name).fontWeight(.semibold).foregroundStyle(Palette.text).lineLimit(1)
                Text("\(publisherName(stream)) · \(stream.viewers) watching\(stream.hasAudio ? " · Audio" : "")")
                    .font(.caption)
                    .foregroundStyle(Palette.muted)
                    .lineLimit(1)
            }
            Spacer()
            Button("Watch") { voice.watch(stream) }
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(Palette.onAccent)
                .padding(.horizontal, 14)
                .padding(.vertical, 7)
                .background(Palette.accent, in: RoundedRectangle(cornerRadius: 8))
        }
        .padding(12)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 10))
        .padding(.bottom, 8)
    }

    private func channelRow(_ channel: Channel) -> some View {
        let current = channel.id == voice.currentChannel
        return Button {
            guard !current else { return }
            if channel.hasPassword { joining = channel } else { voice.join(channel) }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: channel.hasPassword ? "lock" : "speaker.wave.2")
                    .font(.footnote)
                    .frame(width: 20)
                Text(channel.name).fontWeight(.semibold).lineLimit(1)
                Spacer()
            }
            .foregroundStyle(current ? Palette.accent : channel.canJoin ? Palette.text : Palette.faint)
            .padding(.horizontal, 8)
            .frame(minHeight: 40)
            .background(current ? Palette.accent.opacity(0.18) : .clear, in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!channel.canJoin)
        .padding(.leading, CGFloat(channel.depth) * 16)
    }

    private func memberRow(_ member: Member, in channel: Channel) -> some View {
        let talking = voice.talking.contains(member.id)
        return HStack(spacing: 10) {
            Circle()
                .strokeBorder(talking ? Palette.good : Palette.faint, lineWidth: 2)
                .background(Circle().fill(talking ? Palette.good : .clear))
                .frame(width: 10, height: 10)
            Text(member.isSelf ? "\(member.name) (you)" : member.name)
                .foregroundStyle(Palette.text)
                .lineLimit(1)
            Spacer()
            if member.soundMuted {
                Image(systemName: "speaker.slash").font(.caption).foregroundStyle(Palette.faint)
            } else if member.micMuted {
                Image(systemName: "mic.slash").font(.caption).foregroundStyle(Palette.faint)
            }
        }
        .padding(.leading, CGFloat(channel.depth) * 16 + 36)
        .padding(.trailing, 8)
        .frame(minHeight: 34)
        .contentShape(Rectangle())
        .contextMenu {
            if !member.isSelf {
                if member.canPoke != false {
                    Button("Poke…") { poking = member }
                }
                if member.canMove != false {
                    Menu("Move to") {
                        ForEach(voice.channels.filter { $0.id != channel.id }) { target in
                            Button(target.name) { voice.move(member, to: target) }
                        }
                    }
                }
            }
        }
    }

    private var controls: some View {
        VStack(spacing: 10) {
            HStack(spacing: 10) {
                controlButton(voice.muted ? "mic.slash" : "mic", label: voice.muted ? "Unmute" : "Mute", active: voice.muted) {
                    voice.setMuted(!voice.muted)
                }
                controlButton(voice.deafened ? "speaker.slash" : "headphones", label: voice.deafened ? "Undeafen" : "Deafen", active: voice.deafened) {
                    voice.setDeafened(!voice.deafened)
                }
                controlButton("rectangle.on.rectangle", label: voice.sharing.active ? "Sharing" : "Share Screen", active: voice.sharing.active) {
                    showingShare = true
                }
                .disabled(voice.sharing.active)
                Button { voice.disconnect() } label: {
                    Image(systemName: "phone.down.fill")
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .foregroundStyle(Palette.onAccent)
                        .background(Palette.poor, in: RoundedRectangle(cornerRadius: 10))
                }
                .accessibilityLabel("Disconnect")
            }
            GeometryReader { box in
                ZStack(alignment: .leading) {
                    Capsule().fill(Palette.raised)
                    Capsule().fill(Palette.good)
                        .frame(width: box.size.width * min(1, voice.muted ? 0 : sqrt(voice.level)))
                }
            }
            .frame(height: 4)
            Text("Mic: \(voice.muted ? "Muted" : "Unmuted") · Sound \(voice.deafened ? "off" : "on")")
                .font(.footnote)
                .foregroundStyle(Palette.muted)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(14)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 14))
        .padding(.horizontal, 12)
        .padding(.bottom, 6)
    }

    private func controlButton(_ icon: String, label: String, active: Bool, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .frame(maxWidth: .infinity, minHeight: 44)
                .foregroundStyle(active ? Palette.poor : Palette.text)
                .background(Palette.raised, in: RoundedRectangle(cornerRadius: 10))
        }
        .accessibilityLabel(label)
    }

    private var footer: some View {
        HStack(spacing: 14) {
            Circle().fill(Palette.good).frame(width: 7, height: 7)
            metric("Ping", voice.ping.map { String(format: "%.0f ms", $0) })
            metric("In", voice.lossIn.map { String(format: "%.1f%%", $0) })
            metric("Out", voice.lossOut.map { String(format: "%.1f%%", $0) })
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .background(Palette.chrome)
    }

    private func metric(_ label: String, _ value: String?) -> some View {
        HStack(spacing: 4) {
            Text(label).foregroundStyle(Palette.muted)
            Text(value ?? "—").foregroundStyle(value == nil ? Palette.faint : Palette.good)
        }
        .font(.caption)
    }
}

struct StatisticsView: View {
    @Environment(Voice.self) private var voice
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section("Connection") {
                    row("Ping", voice.ping.map { p in
                        String(format: "%.0f ms ± %.1f", p, voice.pingDeviation ?? 0)
                    })
                    row("Packet loss in", voice.lossIn.map { String(format: "%.1f%%", $0) })
                    row("Packet loss out", voice.lossOut.map { String(format: "%.1f%%", $0) })
                }
                Section("Traffic") {
                    row("Upload", rate(voice.upload))
                    row("Download", rate(voice.download))
                    row("Sent this session", bytes(voice.uploaded))
                    row("Received this session", bytes(voice.downloaded))
                    row("Voice packets sent", String(voice.sentPackets))
                    row("Voice packets received", String(voice.receivedPackets))
                    row("Playback gaps", String(voice.playbackGaps))
                    row("Playback callback", "\(voice.callbackFrames) frames")
                }
                Section("Server") {
                    row("Name", voice.server)
                    row("Version", voice.serverVersion)
                    row("Audio", voice.audioDescription)
                }
                if !voice.activity.isEmpty {
                    Section("Activity") {
                        ForEach(Array(voice.activity.reversed().enumerated()), id: \.offset) { _, line in
                            Text(line).font(.footnote)
                        }
                    }
                }
            }
            .navigationTitle("Statistics")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Done") { dismiss() } }
        }
    }

    private func row(_ label: String, _ value: String?) -> some View {
        LabeledContent(label, value: value ?? "—")
    }

    private func rate(_ value: Double) -> String { "\(bytes(Int(value)))/s" }

    private func bytes(_ value: Int) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(value), countStyle: .binary)
    }
}

struct WatchView: View {
    @Environment(Voice.self) private var voice
    @State private var picture: UIImage?
    private let tick = Timer.publish(every: 1.0 / 30, on: .main, in: .common).autoconnect()

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            if let picture {
                Image(uiImage: picture).resizable().scaledToFit()
            } else {
                VStack(spacing: 12) {
                    ProgressView().tint(Palette.muted)
                    Text(voice.watching?.phase ?? "").foregroundStyle(Palette.muted)
                }
            }
            VStack {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(voice.watching?.stream.name ?? "").font(.headline).foregroundStyle(.white)
                        Text(voice.watching?.summary ?? "").font(.caption).foregroundStyle(Palette.muted)
                    }
                    Spacer()
                    Button { voice.stopWatching() } label: {
                        Image(systemName: "xmark")
                            .font(.headline)
                            .frame(width: 40, height: 40)
                            .foregroundStyle(.white)
                            .background(.black.opacity(0.5), in: Circle())
                    }
                    .accessibilityLabel("Stop watching")
                }
                Spacer()
            }
            .padding()
        }
        .onReceive(tick) { _ in
            if let frame = voice.nextFrame() { picture = frame }
        }
    }
}
