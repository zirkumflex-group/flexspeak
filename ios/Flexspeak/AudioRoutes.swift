import AVFoundation
import AVKit
import Observation
import SwiftUI

/// The phone's microphones and where sound is going, kept current as devices come and go.
@MainActor
@Observable
final class AudioRoutes {
    struct Microphone: Identifiable, Equatable {
        let id: String
        let name: String
    }

    var microphones: [Microphone] = []
    var currentMicrophone: String?
    var output = ""
    var sampleRate = 0.0
    var bufferMilliseconds = 0.0
    var problem: String?
    var usesSpeaker = UserDefaults.standard.object(forKey: "usesSpeaker") as? Bool ?? true

    @ObservationIgnored private var observer: NSObjectProtocol?

    init() {
        try? configure()
        refresh()
        observer = NotificationCenter.default.addObserver(
            forName: AVAudioSession.routeChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.refresh() }
        }
    }

    private func configure() throws {
        var options: AVAudioSession.CategoryOptions = [.allowBluetooth]
        if usesSpeaker { options.insert(.defaultToSpeaker) }
        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.playAndRecord, mode: .voiceChat, options: options)
        try session.setPreferredSampleRate(48_000)
        try session.setPreferredIOBufferDuration(0.02)
    }

    func activate() throws {
        try configure()
        try AVAudioSession.sharedInstance().setActive(true)
        refresh()
    }

    func deactivate() {
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
        refresh()
    }

    func setUsesSpeaker(_ value: Bool) {
        usesSpeaker = value
        UserDefaults.standard.set(value, forKey: "usesSpeaker")
        do {
            try configure()
            problem = nil
        } catch {
            problem = "Cannot change the output: \(error.localizedDescription)"
        }
        refresh()
    }

    func select(_ microphone: Microphone) {
        let session = AVAudioSession.sharedInstance()
        guard let port = session.availableInputs?.first(where: { $0.uid == microphone.id }) else { return }
        do {
            try session.setPreferredInput(port)
            problem = nil
        } catch {
            problem = "Cannot use \(microphone.name): \(error.localizedDescription)"
        }
        refresh()
    }

    func refresh() {
        let session = AVAudioSession.sharedInstance()
        microphones = (session.availableInputs ?? []).map { Microphone(id: $0.uid, name: $0.portName) }
        currentMicrophone = session.currentRoute.inputs.first?.uid
        output = session.currentRoute.outputs.map(\.portName).joined(separator: ", ")
        sampleRate = session.sampleRate
        bufferMilliseconds = session.ioBufferDuration * 1000
    }
}

/// Apple's picker for AirPods, Bluetooth and AirPlay outputs.
struct RoutePicker: UIViewRepresentable {
    func makeUIView(context: Context) -> AVRoutePickerView {
        let view = AVRoutePickerView()
        view.tintColor = UIColor(Palette.accent)
        view.activeTintColor = UIColor(Palette.accent)
        return view
    }

    func updateUIView(_ view: AVRoutePickerView, context: Context) {}
}

struct SettingsView: View {
    @Environment(Voice.self) private var voice
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let routes = voice.routes
        NavigationStack {
            List {
                Section {
                    LabeledContent("Playing through", value: routes.output.isEmpty ? "—" : routes.output)
                    Picker("Without headphones", selection: Binding(
                        get: { routes.usesSpeaker },
                        set: { routes.setUsesSpeaker($0) }
                    )) {
                        Text("Speaker").tag(true)
                        Text("Earpiece").tag(false)
                    }
                    HStack {
                        Text("AirPods and Bluetooth")
                        Spacer()
                        RoutePicker().frame(width: 36, height: 36)
                    }
                } header: {
                    Text("Output")
                }

                Section {
                    if routes.microphones.isEmpty {
                        Text("No microphones found").foregroundStyle(Palette.muted)
                    }
                    ForEach(routes.microphones) { microphone in
                        Button {
                            routes.select(microphone)
                        } label: {
                            HStack {
                                Text(microphone.name).foregroundStyle(Palette.text)
                                Spacer()
                                if microphone.id == routes.currentMicrophone {
                                    Image(systemName: "checkmark").foregroundStyle(Palette.accent)
                                }
                            }
                        }
                    }
                } header: {
                    Text("Microphone")
                }

                if let problem = routes.problem {
                    Section { Text(problem).foregroundStyle(Palette.poor) }
                }

                Section("Audio details") {
                    LabeledContent("Sample rate", value: String(format: "%.0f Hz", routes.sampleRate))
                    LabeledContent("Buffer", value: String(format: "%.1f ms", routes.bufferMilliseconds))
                }
            }
            .navigationTitle("Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { Button("Done") { dismiss() } }
            .onAppear { routes.refresh() }
        }
    }
}
