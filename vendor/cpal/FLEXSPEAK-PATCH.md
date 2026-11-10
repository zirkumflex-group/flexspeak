# Own sound left out of loopback recording

This is the `cpal` crate 0.18.2 from crates.io. Its original Apache-2.0 notice is
included; repository-only files (CI, Docker, contributor guides) were dropped.

Local changes:

- `src/host/coreaudio/macos/loopback.rs`: the process tap behind macOS loopback
  recording excludes the recording process itself instead of nothing, so a shared
  screen's system audio no longer carries Flexspeak's own voice playback and sounds
  back to viewers. If the process has no Core Audio object yet (it never opened
  audio), the tap records everything, as upstream does.
- `src/lib.rs`: `dead_code` is allowed. As a path dependency the crate is built with
  the app's warnings, and an upstream helper is unused on Windows.

Windows does the same without this patch, through process loopback in
`src/streaming/process_audio.rs`.
