Automatic build from `main`. The release tag identifies the build, and GitHub links the exact source commit.

This build adds:

- A clearer channel/client tree, click-to-join channel headings, sidebar audio controls and connection-health indicators.
- Fixed connection graphs beside CPU/RAM readings, including horizontal scrolling in narrow windows.
- Dismissible Iced error dialogs, grouped repeated errors, acknowledged audio-state updates and adaptive server-command pacing after flood responses.
- An experimental **macOS-only, video-only** receiver for peer-to-peer TeamSpeak 6 screen shares. AV1 1280×720 playback from an official Windows client was verified before the command-scheduler update. Screen publishing and shared-stream audio are not included.

**Known validation limit:** the latest local macOS 27 test stalled in CoreAudio while opening the microphone, before connecting to the server. The final app passed modal UI checks, 58 ordinary tests and a separate muted/deafened network-only test of paced command acknowledgements. Full voice/video runtime testing of this build remains incomplete. See [error-handling validation](https://github.com/zirkumflex-group/flexspeak/blob/main/docs/error-handling.md) and [streaming scope](https://github.com/zirkumflex-group/flexspeak/blob/main/docs/streaming.md).

- **Windows x64:** download `Flexspeak-windows-x64.zip`, extract, and open `Flexspeak.exe`. The standalone `.exe` is also available. Opus and the Microsoft C runtime are linked statically.
- **macOS (Apple Silicon):** download the `arm64` DMG, then drag **Native Voice.app** into Applications. Requires macOS 14.2 or later.
- **Linux x64 / Omarchy:** download `Flexspeak-linux-x64.pkg.tar.zst` and install with `sudo pacman -U ./Flexspeak-linux-x64.pkg.tar.zst`. A Linux tarball is also included. Native Wayland global shortcuts are currently unsupported; use the in-app controls.
- **Homebrew:** follow the [private tap installation instructions](https://github.com/zirkumflex-group/homebrew-flexspeak).

The macOS app is ad-hoc signed and not notarized; Windows builds are not Authenticode-signed. The OS may ask you to approve these internal builds. No installer disables OS security protections.

SHA-256 checksums and dependency license notices are included. These builds pass automated checks and a command-line launch check; they do not establish Windows audio or full voice-call runtime validation.
