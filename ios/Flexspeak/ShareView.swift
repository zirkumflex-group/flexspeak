import ReplayKit
import SwiftUI

/// Apple's system button for starting a broadcast, kept out of sight behind our own button.
final class BroadcastPicker {
    let view: RPSystemBroadcastPickerView = {
        let picker = RPSystemBroadcastPickerView(frame: CGRect(x: 0, y: 0, width: 1, height: 1))
        picker.preferredExtension = "io.zirkumflex.flexspeak.broadcast"
        picker.showsMicrophoneButton = false
        return picker
    }()

    func present() {
        view.subviews.compactMap { $0 as? UIButton }.first?.sendActions(for: .touchUpInside)
    }
}

private struct PickerHost: UIViewRepresentable {
    let picker: BroadcastPicker
    func makeUIView(context: Context) -> RPSystemBroadcastPickerView { picker.view }
    func updateUIView(_ view: RPSystemBroadcastPickerView, context: Context) {}
}

struct ShareView: View {
    @Environment(Voice.self) private var voice
    @Environment(\.dismiss) private var dismiss
    @State private var picker = BroadcastPicker()

    var body: some View {
        @Bindable var voice = voice
        NavigationStack {
            List {
                Section {
                    Text("Your whole screen is shared, including notifications. iOS can't share one app alone. Sound isn't shared yet.")
                        .font(.footnote)
                        .foregroundStyle(Palette.muted)
                }
                Section("Who can watch") {
                    Picker("Access", selection: $voice.shareAccess) {
                        Text("Approve each viewer").tag("private")
                        Text("Anyone in the channel").tag("public")
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                }
                Section("Quality") {
                    Picker("Quality", selection: $voice.shareQuality) {
                        Text("Balanced · 720p").tag("balanced")
                        Text("Data saver · 480p").tag("saver")
                    }
                    .pickerStyle(.inline)
                    .labelsHidden()
                }
                Section {
                    Button { picker.present() } label: {
                        Text("Start Broadcast")
                            .fontWeight(.semibold)
                            .frame(maxWidth: .infinity, minHeight: 28)
                    }
                    .accessibilityIdentifier("startBroadcast")
                } footer: {
                    Text("Tap Start Broadcast in the system sheet that opens. You'll go live after its countdown.")
                }
            }
            .navigationTitle("Share Screen")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Done") { dismiss() } }
            .background(PickerHost(picker: picker).frame(width: 1, height: 1).opacity(0.02))
            .onChange(of: voice.sharing.active) { _, active in
                if active { dismiss() }
            }
        }
    }
}

/// While sharing: who is watching, who is asking, and how to stop.
struct SharingBanner: View {
    @Environment(Voice.self) private var voice

    var body: some View {
        let sharing = voice.sharing
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Circle().fill(sharing.live ? Palette.poor : Palette.faint).frame(width: 8, height: 8)
                VStack(alignment: .leading, spacing: 1) {
                    Text(sharing.live ? "You're live" : "Starting…")
                        .fontWeight(.semibold)
                        .foregroundStyle(Palette.text)
                    Text(sharing.live ? "\(sharing.viewers.count) watching" : sharing.detail)
                        .font(.caption)
                        .foregroundStyle(Palette.muted)
                        .lineLimit(2)
                }
                Spacer()
                Button("Stop") { voice.stopSharing() }
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Palette.onAccent)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 7)
                    .background(Palette.poor, in: RoundedRectangle(cornerRadius: 8))
            }
            ForEach(sharing.requests) { request in
                HStack {
                    Text("\(request.name) wants to watch")
                        .font(.footnote)
                        .foregroundStyle(Palette.text)
                        .lineLimit(1)
                    Spacer()
                    Button("Deny") { voice.decide(request, allow: false) }
                        .font(.footnote)
                        .foregroundStyle(Palette.muted)
                    Button("Allow") { voice.decide(request, allow: true) }
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(Palette.accent)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(12)
        .background(Palette.surface, in: RoundedRectangle(cornerRadius: 10))
        .padding(.bottom, 8)
    }
}
