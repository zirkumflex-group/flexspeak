# iOS app: handoff notes

Status as of 2026-11-10. Work-in-progress branch `feature/ios-app`, based on `main` at `1f30b80`.
Not reviewed, not merged, no PR yet.

## What this is

A native iPhone app built on the Flexspeak Rust core, with a SwiftUI front end.

- `crates/flexspeak-ios`: a static library that pulls desktop modules in with `#[path]`
  (audio, client, hub, streaming and so on) and exposes a small C ABI in `src/bridge.rs`.
  Commands and events cross it as JSON (`flexspeak_connect`, `flexspeak_command`,
  `flexspeak_disconnect`, `flexspeak_frame`, `flexspeak_push_screen`).
  This is the "prove it works" stage; the plan is to split a proper `flexspeak-core` crate later.
- `ios/`: the XcodeGen project (`project.yml`; the generated `.xcodeproj` and `build/` are gitignored).
  - `Flexspeak/`: the app (connect, channel tree, streams, statistics, settings, share sheet).
  - `FlexspeakBroadcast/`: the ReplayKit Broadcast Upload Extension for screen sharing.
  - `Shared/ScreenLink.swift`: the link between the extension and the app.
  - `build-rust.sh`: Xcode pre-build step that builds the Rust library.

## Building

```sh
cd ios
xcodegen generate
# Simulator
xcodebuild -project Flexspeak.xcodeproj -scheme Flexspeak -destination 'platform=iOS Simulator,name=<sim>' -derivedDataPath build build
# Phone (free Personal Team)
xcodebuild -project Flexspeak.xcodeproj -scheme Flexspeak -configuration Release \
  -destination 'generic/platform=iOS' -derivedDataPath build \
  DEVELOPMENT_TEAM=<team id> CODE_SIGN_STYLE=Automatic -allowProvisioningUpdates build
xcrun devicectl device install app --device <udid> build/Build/Products/Release-iphoneos/Flexspeak.app
xcrun devicectl device process launch --terminate-existing --device <udid> io.zirkumflex.flexspeak
```

Gotchas found along the way:

- `build-rust.sh` runs cargo under `env -i`, because Xcode's environment breaks `ring`
  (`TargetConditionals.h` not found).
- Opus needs `CMAKE_POLICY_VERSION_MINIMUM=3.5` with CMake 4.
- `sysinfo` doesn't compile for iOS, so `ProcessMonitor` in `src/usage.rs` is gated off there.
- A free Personal Team can't use App Groups. So the extension reaches the app over TCP on
  127.0.0.1, with a random 32-byte token and the port shared through a Keychain access group
  (`$(AppIdentifierPrefix)io.zirkumflex.flexspeak.shared`).
- Free signing expires after 7 days; reinstall after that.
- The first launch on a phone needs the developer trusted in Settings → General → VPN & Device Management.

## Desktop changes on this branch

All gated so desktop behaviour stays the same unless noted:

- `src/audio.rs`: 60 ms playback queue on iOS (20 ms on desktop); "Audio route changed" isn't
  treated as a fatal error on iOS; iOS-only playback gap and callback-size counters.
- `src/hub.rs`: audio hub thread gets user-interactive priority on iOS.
- `src/streaming/*`: watching and publishing enabled on iOS; new `broadcast.rs` hands pictures
  from the extension to the running share; iOS NV12 → I420 conversion in `publish_media.rs`.
- **Shared with desktop:** the multi-viewer fix below changes `publishing.rs` and `publish_media.rs`
  for every platform.

Desktop checks at the last commit: `cargo fmt --check` clean, `cargo test --locked`
245 passed / 0 failed / 4 ignored, `cargo clippy --all-targets -D warnings` clean.

## Screen sharing from the phone

The app is in the channel, the user taps the share button, picks access and quality, then starts
Apple's broadcast. The extension (about 50 MB memory cap) only shrinks NV12 frames with vImage
and forwards them; the app encodes VP8 at up to 15 fps (720p, or 480p in saver mode) and runs
the normal publishing path. Video only: no sound, no camera.

### Latest fix: share stopped when a second viewer joined

Cause, found from the code (no crash report on the phone): every viewer needs the phone's
network addresses (ICE candidates), and each one was a separate server command, paced at four
a second from a queue of 56. A phone has many addresses, so two viewers could overflow the queue
or make the server reply ClientIsFlooding, and either one stopped the whole share.

Fix:

- A full queue, a flooding reply or a failed negotiation now drops only that viewer
  (`Output::Rejected`, and the flood branch in `Publisher::result`).
- `CandidateFilter` sends only first-section UDP candidates, at most 8 host plus 4 others per
  viewer, so four viewers fit in the queue at once.

Tests: `a_burst_of_connection_details_never_ends_the_share`,
`a_viewer_who_cannot_be_offered_a_connection_leaves_alone`,
`the_server_limiting_viewer_signalling_keeps_the_share_running`,
`a_failed_negotiation_removes_only_that_viewer`,
`only_usable_connection_details_are_sent_and_reflexive_ones_are_never_crowded_out`.
The ignored real-WebRTC test `publisher_sends_generated_video_and_stereo_audio` still passes with
the filter (video and audio decoded).

**Not yet confirmed on a phone with two real viewers.** That's the first thing to test.

## Tested vs not tested

Tested in the simulator against `ts.zirkumflex.io`: connect and disconnect, channel tree,
statistics, deafen, Poke/Move menu (opens, nothing sent), settings, switching output and mic
while connected, share sheet.

Not tested yet:

- Real voice in and out (the mic stayed muted in every test) and joining channels.
- Watching a real stream on the phone.
- Voice in the background or on the lock screen; AirPods and real route changes.
- A full broadcast with several viewers, and broadcasting while the app is in the background.
- Whether official TeamSpeak clients can watch a phone share.

Waiting on feedback: whether playback is still choppy on the phone (Statistics shows
"Playback gaps" and "callback" size), and what exactly looked wrong with audio device discovery.

## Still missing compared with desktop

Sound with screen sharing, camera sharing, multiple servers, push-to-talk, voice activation, themes.

## Rules to keep

- Never push or merge to `main` without being asked: a push to `main` publishes a release.
- The repository is public: TeamSpeak WAV assets must never be committed.
- Don't poke, move or unmute on the real server without being asked.
