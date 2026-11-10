import SwiftUI

@main
struct FlexspeakApp: App {
    @State private var voice = Voice()

    var body: some Scene {
        WindowGroup {
            Group {
                if voice.phase == .connected {
                    ServerView()
                } else {
                    ConnectView()
                }
            }
            .environment(voice)
            .preferredColorScheme(.dark)
            .tint(Palette.accent)
        }
    }
}
