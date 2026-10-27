# Experimental native TeamSpeak stream receiver

## Current scope

**Streaming**, beside **Voice**, discovers screen shares in your current channel and lets you request one with **Watch**. This first receiver proof is available on macOS. It is video-only: no screen, camera or microphone capture is started by the viewer, and the shared stream's audio is not played. Existing TeamSpeak voice uses the unchanged CPAL/Opus path.

The app uses the existing TS6 connection for discovery, viewer admission, SDP and ICE signaling. Media is handled by native `libwebrtc` 0.3.54, with no browser UI, custom signaling service or LiveKit server. Screen publishing and server/SFU streams are not implemented. Direct connectivity uses TeamSpeak's public STUN endpoints; no TURN credentials or relay service are configured.

## Use

1. Connect to a TS6 server and join the publisher's channel.
2. Start a peer-to-peer screen share from an official TS6 client. For the initial proof, use a harmless window and leave stream audio off.
3. Open **Streaming**, use **Refresh streams** if necessary, then **Watch**. The publisher may need to accept the request.
4. A transport connection alone does not count as successful video. Look for decoded frames, UI updates and a visibly changing picture.
5. **Stop watching**, changing channels, leaving Streaming, entering performance mode or disconnecting stops the viewer. An interrupted video connection requires clicking Watch again.

The player and its Stop button appear only while requesting or watching a stream. The watched stream is omitted from the available-stream list, so it appears once. After stopping, the player disappears and the single catalog entry offers **Watch** again. Failed requests leave an explanation above the list.

The UI reports the server version, negotiated video codec/parameters, decoder name, ICE candidate types (without IP addresses), RTP packet/byte counts, decoded frames/FPS, UI updates and the publisher's version when the server reports it. CPU and memory stay in the footer. Existing Statistics network counters describe the TeamSpeak transport; WebRTC media bytes are reported separately in Streaming.

## Resource and lifecycle bounds

- Only one receiver/peer connection is active. Native decoding runs on a separate worker so peer setup does not block the voice loop.
- The native frame callback queue and application display slot each keep the latest frame. Superseded frames are dropped, not queued for later display.
- The renderer explicitly uploads the next image before replacing the visible picture. It retains the displayed GPU allocation and permits only one upload in flight, including across a stop/restart. This avoids the blank frames caused by Iced's asynchronous upload of larger images; failed or late uploads cannot clear or revive the current picture.
- Display conversion is capped at 1280×720 and approximately 30 FPS. The incoming stream may be decoded at a larger resolution/rate; the diagnostics report its actual decoded size/FPS.
- Signaling commands, pending ICE candidates and control queues have explicit size/count bounds. Stream IDs and current-channel membership scope incoming messages and Watch requests. Old receiver generations cannot replace a newer picture.
- Only displayed metrics are copied from WebRTC statistics; raw SDP, ICE credentials, client addresses and video contents are not saved by the receiver.

## Protocol patch and builds

`vendor/tsclientlib` is based on ReSpeak revision `ee3bc6f45a7137db7793ba5593a321df400d53e5`. Its narrow extension forwards TS6 stream notifications and optional streaming-flag updates; protocol dependencies, handshake and crypto stay on the original pin. See its [patch notes](../vendor/tsclientlib/FLEXSPEAK-PATCH.md). The wire shapes follow the MIT-licensed [WebSpeak3 implementation](https://github.com/WebSpeak3/webspeak3/tree/10ae24a99f4b397460f078798480a5527ea0f706), not its browser/gateway deployment.

The macOS build downloads the pinned native WebRTC SDK through its build dependency. `build.rs` adds `-ObjC`, required for WebRTC's Objective-C categories. The app bundle includes native WebRTC and protocol-library license notices. Windows/Linux keep the voice client and discovery code; the native media backend is intentionally unavailable there for this first proof.

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

The earlier [interoperability research](teamspeak-streaming-research.md) records source evidence and the remaining protocol/version risks. Native screen capture and publishing are later work, after this receive proof succeeds.
