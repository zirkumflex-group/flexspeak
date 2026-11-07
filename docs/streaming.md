# Experimental native TeamSpeak stream receiver

## Current scope — development update, 2026-10-28

The native media backend is now enabled on **macOS and Windows** for watching and publishing peer-to-peer streams through the existing TS6 server connection. The earlier Windows build disabled Watch because the backend was compiled only on macOS. Watch still requires a TS6 server, a current-channel stream and no other active watch/share.

Incoming stream audio is decoded to stereo 48 kHz PCM and mixed into the existing CPAL playback path. App volume and deafen apply to both voice and stream audio. Deafen, stopping and receiver-generation changes clear buffered stream sound. WebRTC never opens a microphone or a playback device; TeamSpeak voice continues to use CPAL/Opus.

Publishing captures a selected screen, window or camera and optionally the sound on a selected output device. It uses native `libwebrtc` 0.3.54 with CPAL output loopback; there is no browser UI, LiveKit service or extra signaling server. SFU publishing and TURN relaying are not configured.

## Watching several streams — 2026-11-07

Up to four streams can be watched at once, each with its own receiver, picture and audio. One watched stream fills the right pane. With several, they share it as a grid; **Maximize** (or clicking a tile) puts one on the stage and lines the others up beneath it, and **Minimize** or double-clicking the stage returns to the grid. Every stream has its own volume slider and mute button on top of the app volume; the setting is kept per publisher for the session. Codec, resolution, frame rate, decoder and transport details are hidden behind the **info** button in the pane header. Watching still stops while you share.

## Start sharing

1. Connect and join the channel where you want to share. Press the **share** button beside Disconnect at the bottom of the sidebar; the dialog lists screens, windows and cameras.
2. On Windows, select a screen or window from the list. On Mac, select **Choose a screen or window…**; **Go Live** opens the system content picker. Cameras appear by device name on both platforms and request camera permission only when you start.
3. Choose **Data saver**, **Balanced**, **Sharp text** or **Smooth motion**, then adjust each control independently: resolution (480p–2160p), frame rate (5–60 fps), video bitrate (250–20,000 kbps) and system-audio bitrate (32–320 kbps). Changes show **Custom**; selecting a preset fills all four values. Resolution and FPS are ceilings, limited by the source; frames are never upscaled. Bitrate caps apply per viewer.
4. Leave **Share system audio** enabled and select the output carrying the sound, or disable it for silent video. **All sound on that output is shared**, including calls and notifications, even for a single-window share. This is not per-application audio. Microphone mute remains separate. Selecting a camera turns system audio off initially; you can enable it explicitly.
5. Press **Go Live** and grant the OS capture permissions if needed. The app announces the stream only after capture starts. A generic stream name is announced; the selected window's title is not sent as metadata.
6. Choose **Public** to admit current-channel viewers automatically, or **Private** (the default) to approve each viewer with **Allow**. **Deny** suppresses repeat requests for that share. Up to four can watch; public requests wait for a free slot and are then admitted automatically. The app enforces admission, channel membership and limits itself as well as advertising the selected access setting to the server.
7. A banner above the streams shows viewers and requests while you share. **Stop**, opening Statistics, changing channels or disconnecting stops capture. The Stop control cancels media directly, even when the voice-worker command queue is full. Server cleanup continues through the paced command scheduler. A timed-out, unconfirmed start may require reconnecting before another start.

Publishing uses VP8 video at the chosen quality (default 1280×720 / 30 FPS with a 2.5 Mbit/s cap per viewer), plus stereo Opus audio at the selected cap (128 kbit/s by default). Camera capture uses AVFoundation on Mac and the bundled WebRTC camera module on Windows; both stop their callback source before releasing capture state. Captured frames/audio blocks are capture counters, not proof of remote rendering. Each viewer has its own peer connection, so upload grows with the number of viewers. No publishing CPU/RAM or hardware-encoding performance claim is made.

On Mac, the current CPAL loopback path requires an **output-only audio endpoint**, such as the built-in speakers. A duplex endpoint that also exposes input is rejected explicitly so it cannot accidentally record the microphone; select another output or share without audio. Screen Recording and System Audio Recording permission may be required. On Windows, loopback captures the selected WASAPI render endpoint. Mono is duplicated to stereo; surround output is downmixed so center and other channels are retained.

## Development validation

69 ordinary tests passed, including manual admission, channel scoping, bounded requests, stale peer events, direct cancellation, delayed start cleanup, BGRA conversion and stream-audio deafen/reset behavior. A generated native sender-to-production-receiver test delivered **8 changing video frames and 41 audible stereo blocks**, with no capture devices, external STUN or TeamSpeak server. It also verified receiver cleanup. An unsafe empty-sender-track inspection in the native binding was identified during this test and avoided; receive-only behavior is checked from the generated SDP instead.

Both optimized builds and Mac/Windows cross-target Clippy passed. Native Mac UI inspection connected in Lobby with the microphone muted and zero voice packets, showed the publishing controls, and opened the system content picker. The capture attempt timed out without a source being selected; the picker was cancelled and the app was disconnected. No viewers were approved. A subsequent user-approved local test captured video and system audio in Lobby: counters reached 6,781 video frames / 27,958 audio blocks, and the input meter responded to a quiet generated 440 Hz tone played through MacBook Pro Speakers. The server accepted the stream announcement; zero viewers were approved. Stop sharing and disconnect both completed, with the microphone muted and zero voice packets throughout. The user selected T3 Code in the first successful run; the later selected source was not independently confirmed, so this is not evidence of a blank-document capture. No capture contents were saved. Official TS6 viewer playback and Windows hardware interoperability remain unverified. The earlier official-client receive proof below applies to the previous Mac video-only receiver; it must not be presented as validation of the new publishing/audio paths. The separate macOS CoreAudio startup stall remains documented in [error handling](error-handling.md).

## Use

1. Connect to a TS6 server and join the publisher's channel.
2. Start a peer-to-peer screen share from an official TS6 client. Use a harmless window for validation.
3. Open the server view, use **Refresh streams** if necessary, then **Watch**. The publisher may need to accept the request.
4. A transport connection alone does not count as successful video. Look for decoded frames, UI updates and a visibly changing picture.
5. **Stop watching** (the × on a stream, or **Stop All**), changing channels, opening Statistics or disconnecting stops viewing. An interrupted video connection requires clicking Watch again.

The player and its Stop button appear only while requesting or watching a stream. The watched stream is omitted from the available-stream list, so it appears once. After stopping, the player disappears and the single catalog entry offers **Watch** again. Failed requests leave an explanation above the list.

The receiving UI reports the server version, negotiated video codec/parameters, decoder name, ICE candidate types (without IP addresses), video RTP packet/byte counts, decoded frames/FPS, UI updates and the publisher's version when the server reports it. CPU and memory stay in the footer. Existing Statistics network counters describe the TeamSpeak transport; WebRTC media bytes are reported separately in Streaming.

## Resource and lifecycle bounds

- Only one receiver is active. Watching and publishing are mutually exclusive. Native decoding runs on a separate worker so peer setup does not block the voice loop.
- The native frame callback queue and application display slot each keep the latest frame. Superseded frames are dropped, not queued for later display.
- Decoded frames replace one `rune::Pixels` image from the decoder thread. Rune uploads only the newest image on the next drawn frame (same-size frames reuse their atlas slot), so the UI thread never copies pixels and the old picture stays until the new one is on the GPU. The view is woken only when the picture first appears or changes size or rotation. Ending a session releases the image; an old generation cannot publish into a new one.
- Display conversion is capped at 1280×720 and approximately 30 FPS. The incoming stream may be decoded at a larger resolution/rate; the diagnostics report its actual decoded size/FPS.
- Signaling commands, pending ICE candidates and control queues have explicit size/count bounds. Stream IDs and current-channel membership scope incoming messages and Watch requests. Old receiver generations cannot replace a newer picture.
- Only displayed metrics are copied from WebRTC statistics; raw SDP, ICE credentials, client addresses and video contents are not saved by the receiver.

## Protocol patch and builds

`vendor/tsclientlib` is based on ReSpeak revision `ee3bc6f45a7137db7793ba5593a321df400d53e5`. Its narrow extension forwards TS6 stream notifications and optional streaming-flag updates; protocol dependencies, handshake and crypto stay on the original pin. See its [patch notes](../vendor/tsclientlib/FLEXSPEAK-PATCH.md). The wire shapes follow the MIT-licensed [WebSpeak3 implementation](https://github.com/WebSpeak3/webspeak3/tree/10ae24a99f4b397460f078798480a5527ea0f706), not its browser/gateway deployment.

The Mac and Windows builds download the pinned native WebRTC SDK through its build dependency. `build.rs` adds `-ObjC`, required for WebRTC's Objective-C categories. The app bundle includes native WebRTC and protocol-library license notices. Linux keeps the voice client and discovery code; its native media backend remains unavailable. Windows now includes the same native receiving and publishing backend as Mac.

On this macOS 27 host, stripped release procedural-macro dylibs failed to load with `mis-aligned LINKEDIT string pool`. A direct `rustc --extern` probe reproduced the failure against a newly built plugin, while an older plugin loaded. Disabling stripping for release **build dependencies** made the same load check pass. The application still uses thin LTO and release debuginfo stripping.

## Validation so far

- The network-only probe joined Lobby (#1) with a fresh temporary identity, muted/deafened and without opening audio devices. The server reports **6.0.0-beta13 [Build: 1789645103]**, Linux, protocol 8.
- 48 ordinary tests passed; the explicit live-server and native-media tests are opt-in. Tests cover TS escaping, duplicate/bounded inputs, channel scoping, early ICE bounds, rejected/unrelated messages, stale receiver generations and frame replacement/cleanup. The upload regression failed before the fix and now checks that the displayed picture survives a pending/failed upload and intermediate frames cannot form a backlog.
- The native synthetic-media test passed: **VP8, 320×180, 84 RTP packets, 19 decoded frames, 18 delivered RGBA frames and 12 distinct red levels**, with a host-to-host UDP ICE path. It used generated pixels, no capture devices and no external STUN. Cancellation closed the output and cleared the display slot. This verifies native decoding/conversion/cleanup, not official TS6 interoperability.
- macOS Clippy and Windows cross-target Clippy passed. Packaging-script tests passed.
- An initial 30.33-second idle sample in Lobby measured **3.53% of one CPU core, 96.1 MiB median RSS and 151.9 MiB median physical footprint**. No video was being received; this is the baseline, not the cost of streaming. Raw readings and the exact binary/dependency versions are in [the measurement directory](measurements/2026-10-27-native-stream-receiver/).
- The user subsequently authorized testing in **Die Weiße Front**. The first official-client attempt negotiated **AV1, 1280×720, about 30 decoded FPS, dav1d, host-to-host UDP**, from **TeamSpeak 6.0.0-beta5 [Build: 1791901251] on Windows**. It decoded 533 frames, but the picture flickered because each new RGBA handle was shown before its asynchronous GPU upload completed. Playback was stopped and explicit upload completion was added.
- After the upload fix, successive native app observations showed a visible, changing game scene with no blank image between those observations. The first session reached **3,186 decoded frames / 1,766 UI updates**. **Stop watching** cleared the picture; a second Watch request produced fresh video and reached **897 decoded frames / 512 UI updates**. Leaving Streaming stopped that second session, and returning to the tab did not resume it. The server connection remained active and the microphone muted, with **zero sent and received voice packets**. This confirms connection coexistence, not a two-person voice conversation during video.

## Measured official-client receive proof

The fixed optimized build was measured on Apple M4 / macOS 27.0.1, in the user-authorized current channel. Each case has one roughly 30-second sample after five seconds of warm-up, using lightweight `ps` and `proc_pid_rusage` counters; no `vmmap` was used. The window remained at the same size. Playback showed moving game content, with video and diagnostics visible. These are short observations, not a sustained load or leak test.

| State | CPU (% of one core) | Median RSS | Median physical footprint |
| --- | ---: | ---: | ---: |
| Before watching | 3.59% | 95.8 MiB | 151.6 MiB |
| Receiving AV1 1280×720 | 67.73% | 142.0 MiB | 223.0 MiB |
| After Stop/rejoin/leaving Streaming | 3.92% | 125.4 MiB | 182.2 MiB |

Video decoding reported **30–31 FPS**, using software **dav1d** and **Host ↔ Host · UDP**. Display updates were fewer than decoded frames (about 55% in the first session); the decoder FPS must not be presented as the actual screen presentation rate. No hardware-decoding or zero-copy performance claim is made. CPU returned close to the idle baseline after stopping; RSS remained about 29.6 MiB above its initial level. This test does not establish the attribution or long-term behavior of that remaining memory.

See [raw measurements and trial observations](measurements/2026-10-27-native-stream-receiver/), including the exact binary SHA256, server/publisher versions, the initial flicker failure and the fixed retest. Video contents, raw SDP and ICE addresses were not saved. Discrete visual observations confirmed changing rendered content, and the user subsequently confirmed that the flickering was gone. No continuous flicker recording was taken. The official stream test covers this AV1 version/network combination only. Other codecs, remote NAT/TURN paths, publisher restart, video recovery, simultaneous active voice, and native Windows/Linux receiving remain unverified.

The earlier [interoperability research](teamspeak-streaming-research.md) records source evidence and the remaining protocol/version risks. The newer publishing path and its validation are described above.
