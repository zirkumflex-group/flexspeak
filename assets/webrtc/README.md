# Native WebRTC notices

`LICENSE.md` is copied unchanged from the macOS ARM64 native WebRTC SDK used by
`libwebrtc` 0.3.54 / `webrtc-sys` 0.3.51 / `webrtc-sys-build` 0.3.21.

Source artifact: <https://github.com/livekit/rust-sdks/releases/download/webrtc-a545de4/webrtc-mac-arm64-release.zip>.

Mac and Windows packages include these WebRTC and bundled dependency notices.
The same native artifact supplies streaming, microphone APM processing and VAD.
`native/audio.cpp` exposes suppression levels and VAD through a fixed-frame C
interface; its build uses the pinned helper above. Refresh these notices and
the matching build-helper pin when updating the SDK. Linux currently retains
the voice client without native WebRTC processing or streaming.
