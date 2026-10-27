Automatic build from `main`. The release tag identifies the build, and GitHub links the exact source commit.

- **Windows x64:** download `Flexspeak-windows-x64.zip`, extract, and open `Flexspeak.exe`. The standalone `.exe` is also available. Opus and the Microsoft C runtime are linked statically.
- **macOS:** download the `arm64` DMG for Apple Silicon or `x64` DMG for Intel, then drag **Native Voice.app** into Applications. Requires macOS 14.2 or later.
- **Homebrew:** follow the [private tap installation instructions](https://github.com/zirkumflex-group/homebrew-flexspeak).

The macOS app is ad-hoc signed and not notarized; Windows builds are not Authenticode-signed. The OS may ask you to approve these internal builds. No installer disables OS security protections.

SHA-256 checksums and dependency license notices are included. These builds pass automated checks and a command-line launch check; they do not establish Windows audio or full voice-call runtime validation.
