import SwiftUI

struct ConnectView: View {
    @Environment(Voice.self) private var voice
    @AppStorage("address") private var address = "ts.zirkumflex.io"
    @AppStorage("nickname") private var nickname = ""
    @State private var password = ""
    @State private var showingSettings = false

    private var ready: Bool {
        voice.phase == .idle
            && !address.trimmingCharacters(in: .whitespaces).isEmpty
            && !nickname.trimmingCharacters(in: .whitespaces).isEmpty
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack {
                    Text("Flexspeak")
                        .font(.largeTitle.bold())
                        .foregroundStyle(Palette.text)
                    Spacer()
                    Button { showingSettings = true } label: {
                        Image(systemName: "gearshape")
                            .font(.title3)
                            .frame(width: 44, height: 44)
                            .foregroundStyle(Palette.muted)
                    }
                    .accessibilityLabel("Settings")
                }
                .padding(.top, 32)

                VStack(spacing: 0) {
                    field("Server", text: $address, keyboard: .URL)
                    Divider().overlay(Palette.raised)
                    field("Nickname", text: $nickname)
                    Divider().overlay(Palette.raised)
                    HStack {
                        Text("Password").foregroundStyle(Palette.muted)
                            .frame(width: 92, alignment: .leading)
                        SecureField("Optional", text: $password)
                            .foregroundStyle(Palette.text)
                    }
                    .padding(14)
                }
                .background(Palette.surface, in: RoundedRectangle(cornerRadius: 14))

                Button {
                    Task {
                        await voice.connect(
                            address: address.trimmingCharacters(in: .whitespaces),
                            nickname: nickname.trimmingCharacters(in: .whitespaces),
                            password: password
                        )
                    }
                } label: {
                    Group {
                        if voice.phase == .connecting {
                            ProgressView().tint(Palette.onAccent)
                        } else {
                            Text("Connect").fontWeight(.semibold)
                        }
                    }
                    .frame(maxWidth: .infinity, minHeight: 28)
                    .padding(.vertical, 10)
                    .foregroundStyle(Palette.onAccent)
                    .background(Palette.accent.opacity(ready || voice.phase == .connecting ? 1 : 0.4),
                                in: RoundedRectangle(cornerRadius: 12))
                }
                .disabled(!ready)
                .accessibilityIdentifier("connect")

                if voice.phase == .connecting {
                    Text(voice.status)
                        .font(.footnote)
                        .foregroundStyle(Palette.muted)
                }
                if let failure = voice.failure {
                    Text(failure)
                        .font(.footnote)
                        .foregroundStyle(Palette.poor)
                }
            }
            .padding(20)
            .frame(maxWidth: 520)
            .frame(maxWidth: .infinity)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(Palette.canvas.ignoresSafeArea())
        .sheet(isPresented: $showingSettings) { SettingsView() }
    }

    private func field(_ label: String, text: Binding<String>, keyboard: UIKeyboardType = .default) -> some View {
        HStack {
            Text(label).foregroundStyle(Palette.muted)
                .frame(width: 92, alignment: .leading)
            TextField(label, text: text)
                .keyboardType(keyboard)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .foregroundStyle(Palette.text)
                .accessibilityIdentifier(label.lowercased())
        }
        .padding(14)
    }
}
