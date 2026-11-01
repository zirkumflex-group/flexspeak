Automatic build from `main`. The release tag identifies the build, and GitHub links the exact source commit.

This build adds:

- Five-step first-run setup for username, server, permissions, audio devices and shortcuts. Existing valid settings skip setup; selected devices are saved.
- A saved-server home screen and a connected view with channels on the left and streaming on the right, plus a 760 × 560 minimum window size.
- Experimental native screen/window sharing and stream audio on Mac and Windows, with individual viewer approval and up to four viewers. Stream playback follows app volume and deafen.
- Recoverable audio-buffer glitches no longer end the voice session; device failures still report an error.
- A more desktop-like interface: compact 28 pt controls, flat grouped surfaces without glows or shadows, a neutral dark palette, and Settings as a grouped form with labels beside their controls.
- Opt-in WebRTC noise suppression, automatic microphone gain and experimental echo cancellation on Mac/Windows. Set them in Settings → Audio while disconnected, then save. Local headphone tests preview suppression/gain; echo cancellation runs during calls.
- Push-to-talk with a configurable release delay, speech-and-threshold voice activation, and four noise-suppression strengths. Continuous remains the default; choose a PTT key under Global keybinds.

**Validation:** 76 ordinary Rust tests, three packaging tests, formatting, macOS Clippy and a Windows x64 Clippy cross-check passed before this merge. Earlier generated-media tests and local Mac screen/system-audio capture passed. Official-client playback of published streams, Windows hardware capture/playback and the complete onboarding permission flow remain unverified. See [error-handling validation](https://github.com/zirkumflex-group/flexspeak/blob/main/docs/error-handling.md) and [streaming scope](https://github.com/zirkumflex-group/flexspeak/blob/main/docs/streaming.md).

- **Windows x64:** download `Flexspeak-windows-x64.zip`, extract, and open `Flexspeak.exe`. The standalone `.exe` is also available. Opus and the Microsoft C runtime are linked statically.
- **macOS (Apple Silicon):** download the `arm64` DMG, then drag **Flexspeak.app** into Applications. Requires macOS 14.2 or later.
- **Linux x64 / Omarchy:** download `Flexspeak-linux-x64.pkg.tar.zst` and install with `sudo pacman -U ./Flexspeak-linux-x64.pkg.tar.zst`. A Linux tarball is also included. Native Wayland global shortcuts are currently unsupported; use the in-app controls.
- **Homebrew:** follow the [private tap installation instructions](https://github.com/zirkumflex-group/homebrew-flexspeak).

The macOS app is ad-hoc signed and not notarized; Windows builds are not Authenticode-signed. The OS may ask you to approve these internal builds. No installer disables OS security protections.

SHA-256 checksums and dependency license notices are included. These builds pass automated checks and a command-line launch check; they do not establish Windows audio or full voice-call runtime validation.
