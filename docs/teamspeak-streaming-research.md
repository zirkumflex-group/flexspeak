# TeamSpeak streaming interoperability research — 2026-10-27

**Decision: conditional GO for a small native receiving proof; NO-GO for promising a complete streaming feature yet.** Flexspeak can plausibly use TeamSpeak 6's existing stream discovery and signaling and exchange WebRTC media with official clients. WebSpeak3 provides concrete Rust signaling code and browser send/receive implementations; a second project, EchoSixHIYA's WebSpeak, implements the same command family. This is substantially stronger evidence than an unimplemented roadmap item, but neither project establishes compatibility or performance for our native client. [WebSpeak3 roadmap][ws-roadmap] · [Rust connector][ws-connector] · [WebSpeak coordinator][echo-coordinator]

The useful reuse is **TeamSpeak signaling plus its existing server/channel membership**, not a ready-made native capture/codec engine. A separate application signaling server need not be the starting design. Media still needs a native WebRTC implementation, negotiated video decoding and rendering; publishing additionally needs OS capture and encoding. Native WebRTC APIs exist without a browser UI. No particular Rust wrapper or media dependency is selected by this research. [Native WebRTC APIs][native-webrtc]

Scope: primary-source browsing and read-only source inspection. No application code or dependencies changed; no build, app launch, TeamSpeak connection, capture, transmission, benchmark or runtime interoperability test was performed. The sole new artifact is this report. The version, permissions and streaming support of **`ts.zirkumflex.io` remain unknown**. Local [Cargo.toml](../Cargo.toml) pins ReSpeak/tsclientlib and tsproto-packets to `ee3bc6f45a7137db7793ba5593a321df400d53e5`; the Iced/CPAL/Opus application has no WebRTC/video engine. Its [Streaming tab documentation](streaming.md) correctly describes an inactive entry point.

**Evidence levels used here:** implementation evidence means inspected executable source, not executed source. Maintainer claims include README statements, comments and commit descriptions of live tests. Independent runtime verification by this investigation: **none**. Community bug reports are user observations, not TeamSpeak's confirmed diagnoses.

## Concrete reuse from WebSpeak3

Inspected WebSpeak3 revision: **`10ae24a99f4b397460f078798480a5527ea0f706`**, dated October 26, 2026. Its actual Git tree pins Moepchi/tsclientlib to **`2e7794928f18d5aef2da5b48f420f9c801b1112c`**; that fork pins Moepchi/tsdeclarations to **`c2992f940d7aad0102f156ece187ae8919e0aa59`**. The submodule branch is `webspeak3`. [WebSpeak3 tree][ws-tree] · [submodules][ws-submodules] · [fork comparison against our pin][fork-diff]

| Area | Inspected implementation | Native reuse assessment |
| --- | --- | --- |
| Incoming signaling | The fork adds `StreamItem::UnknownCommand { name, content }` instead of dropping undeclared commands. The connector filters `notifystream*` and both `*joinstreamrequest` notifications, parses TS escaping with `CommandParser`, and emits signaling events. | Essential extension point for the current dependency. The focused patch is identifiable; no browser is involved. [Fork patch][unknown-patch] · [connector handler][ws-incoming] |
| Discovery | Declarations add optional `client_is_streaming` state, including initial visibility and later client updates. Late joiners request stream details with `requeststreaminfo clid=…`; a client ID alone is not the stream ID. | Reuse the declaration/state changes and explicit discovery request. [Declarations diff][declarations-diff] · [request handler][ws-outgoing] |
| Outgoing commands | Rust builds ordinary C2S `PacketType::Command` packets with `OutCommand` and `write_arg`; setup, join response and signaling track command results. | Adapt these builders directly to the in-process Rust connection. The stdin/stdout connector process, Node gateway and browser WebSocket are deployment glue, not protocol requirements. [Connector][ws-outgoing] · [gateway adapter][ws-gateway] |
| Receive/publish state | `stream.ts` answers the publisher's offer and buffers early ICE; `publish.ts` owns a peer connection per admitted viewer and offers first. | Port the small state machine and wire shapes, replacing browser APIs with a native media engine. [Viewer][ws-viewer] · [publisher][ws-publisher] |
| Licensing | WebSpeak3 has MIT licensing; tsclientlib and tsdeclarations state MIT OR Apache-2.0. | These are practical source-reuse candidates with their license notices retained. This does not confer rights to redistribute TeamSpeak binaries or constitute official protocol support. [WebSpeak3 license][ws-license] · [fork license][fork-license] · [declarations license][decl-license] |

The receiver sequence in actual code is:

1. Discover `client_is_streaming`, then `requeststreaminfo` → `notifystreaminfo` to obtain the stream ID.
2. Send `joinstreamrequest(id, clid=<publisher>, msg, is_remove=0)`.
3. Receive `notifyrespondjoinstreamrequest` with `decision=1` and `offer=<SDP>`; apply the remote offer and generate the answer.
4. Send `streamsignaling(id, clid, json={cmd:"answer", args:{answer:"<SDP>"}})`.
5. Exchange `{cmd:"iceCandidate", args:{sdp, mid, mLine}}`, buffering remote candidates until the remote description is set. Leave using `joinstreamrequest` with `is_remove=1`; handle stream stop/client-left notifications. [Rust join builder][ws-join] · [viewer implementation][ws-viewer]

For publishing, the connector sends `setupstream(name, type, bitrate, accessibility, mode, viewer_limit, audio)`, awaits the assigned ID in `notifystreamstarted`, and handles each `notifyjoinstreamrequest`. It accepts a viewer with `respondjoinstreamrequest(id, clid, msg, offer, decision=1)`; answer and ICE then use `streamsignaling`. `stopstream` terminates publication. No video frames travel in these command payloads. [Outbound Rust implementation][ws-outgoing] · [publisher][ws-publisher]

**Do not replace our dependency with the full fork merely to obtain these hooks.** The comparison is 14 commits ahead of our exact pin and includes TeaSpeak/GreenTeaSpeak handshake and crypto changes, resolver changes, file-transfer changes and gateway-oriented private-address blocking. The streaming-specific hooks and declaration updates are separable candidates for a later reviewed patch. Whether those alone suffice must be established by the proof, not assumed from this diff. [Full comparison][fork-diff] · [unknown-command patch][unknown-patch] · [live update declaration bump][live-update-patch]

**Maintainer runtime claims, not our verification:** the receive-fix commit reports audio/video RTP arriving from a real TS6 publisher after correcting `args.answer`; the publishing commit reports an official client rendering the project's video. Both say “TS6 6.0.0-beta12.1”. That label alone does not identify separate client/server versions; beta12.1 is also an official **server** release. Do not turn it into a verified current-client support matrix. The `ts6-re/findings/ts6-stream-protocol-static-analysis.md` referenced in comments is absent from the inspected public tree (fetch returned 404), so its underlying reverse-engineering evidence was unavailable. [Receive claim][receive-claim] · [publish claim][publish-claim] · [server beta12.1][server-beta12] · [tree][ws-tree]

## Media, permissions and connectivity limits

**Codecs:** WebSpeak3 delegates negotiation to browser `RTCPeerConnection`; its streaming modules do not pin a codec or implement a decoder. Capture uses `getDisplayMedia`; publication adjusts browser sender bitrate/degradation settings. Sending `setupstream bitrate=…` does not itself configure our encoder. An eventual native implementation must negotiate the actual offered codec/profile, handle media transport and decode/display frames; current CPAL+Opus voice support supplies none of the video path. [Viewer][ws-viewer] · [publisher][ws-publisher]

**ICE and relay:** WebSpeak3 defaults to `stun:turn.teamspeak.com:3478`, `stun:turn2.teamspeak.com:3478` and Google's STUN service. The `stun:` scheme matters: a hostname containing “turn” is not evidence of a TURN media relay. Current code additionally supports deployment-configured TURN URLs and credentials, supplied to both viewers and publishers. Thus its roadmap's “P2P only” means **no SFU**, not “no possible TURN relay”. A comment claims official clients accept a relay candidate supplied by WebSpeak3; that still needs native/runtime verification. [ICE defaults][ws-ice] · [TURN credential code][ws-turn] · [TURN wiring][ws-media-wiring]

**SFU:** `StreamMode.P2P=1` and `SFU=2` exist, but the SFU choice is disabled. Comments describe a separate access-info/mediasoup path that is not implemented. Setting `mode=2`, pointing at arbitrary mediasoup/LiveKit infrastructure, or owning a TS server does not establish interoperability. [Mode definitions][ws-publisher] · [disabled option][ws-sfu] · [implementation explanation][sfu-claim]

**Admission and permissions:** WebSpeak3 implements public auto-admission, pending approval, deny/block and a local viewer limit. Contacts-only falls back to manual approval because it lacks TS6 account contacts. Its maintainers report that the server forwarded requests despite private/limit settings, so it is unsafe to assume those advertised fields enforce access. The inspected Rust path does not introduce a special streaming entitlement or client-feature handshake. It is not evidence that every server/account configuration permits streaming. Historical TeamSpeak guidance said stream-specific moderation permissions were not available in August 2025; this research found no authoritative current permission matrix. Preserve command errors and verify the actual server/channel behavior later. [Admission code][ws-admission] · [maintainer admission test claim][admission-claim] · [official permission discussion][official-permissions]

**Costs:** in the implemented P2P topology each viewer has a distinct peer connection and media sender. For `N` viewers receiving approximately bitrate `R`, publisher media upload is approximately `N × R`, plus transport overhead; this is topology arithmetic, not a benchmark. Per-viewer transport state also grows. Encoder sharing, hardware acceleration, copying and buffering determine CPU/RAM; do not assume CPU is exactly linear or that native code automatically makes streaming cheap. Restrictive NAT/firewall combinations can defeat direct ICE even while server voice works. TURN can offer another media path, at relay bandwidth cost, without becoming an SFU. [Per-viewer connections][ws-per-viewer] · [ICE configuration][ws-ice] · [official issue #116][issue116]

## Other relevant attempts

| Project | What the primary sources establish | Relevance to official TS6 interoperability |
| --- | --- | --- |
| **EchoSixHIYA/WebSpeak-client-for-TeamSpeak**, inspected at `37f5b86b52884266d3fd1dfe576dc48c7f4cc181` (October 25, 2026) | A second codebase implements `setupstream`, `requeststreaminfo`, `joinstreamrequest`, `respondjoinstreamrequest` and `streamsignaling`, plus browser WebRTC peers. The author announced bidirectional native-TS6 interoperability in v0.2.4 on September 22. | **Directly relevant implementation evidence and author claim**, not independent runtime proof. Its coordinator tests use fake TS clients and synthetic SDP/notifications; they do not demonstrate a live official-client session. AGPL-3.0-only, so use deliberately as a reference rather than treating its source as interchangeable with MIT-licensed WebSpeak3. [Coordinator][echo-coordinator] · [tests][echo-tests] · [author announcement][echo-announcement] · [license][echo-license] |
| **mfreiholz/ts3video**, inspected at `a6740a1a664280fe990410ca74e3dc4bd272f0db` | TS3 plugin plus its own video client/server. The video server's configuration defines a separate TCP/UDP media port and an optional ServerQuery bridge to TeamSpeak. | A real video extension, but **not TS6 Stream/Call interoperability**. Official TS3 users need its plugin/video stack. [Plugin source][ts3-plugin] · [video server configuration][ts3-config] |
| **TeaSpeak / TeaWeb**, inspected TeaWeb at `597375676e20cb4b1515d248005603801da7d05f` | TeaSpeak's changelog introduced camera/screen RTC video in 1.5.0. TeaWeb implements its own `notifyrtcsessiondescription`, `rtcsessiondescribe`, `notifyrtcicecandidate` and `notifybroadcastvideo` handling. | An alternate client/server platform with video, **not evidence of speaking official TS6's streaming commands**. [TeaSpeak changelog][tea-changelog] · [RTC source][tea-rtc] · [video source][tea-video] |
| **ReSpeak/Qint and tsclientlib examples** | Qint is a Tauri/web alternative voice/chat client; its README disclaims active development/support. ReSpeak's audio example connects microphone/speakers to channel audio. Targeted Qint searches found no TS6 setup/join/peer-connection implementation. | Useful voice/protocol foundations, **no streaming interoperability evidence found**. “Stream” in Rust/audio APIs is not screen sharing. [Qint README][qint] · [pinned audio example][respeak-audio] |
| **Metihttp/Teamspeak_Live_stream** | Its README describes a Windows TS3 plugin uploading H.264/AAC segments through channel file transfer, with plugin commands notifying viewers. It explicitly reserves copying/modification rights rather than granting an open-source license. | Uses TeamSpeak infrastructure in a different sense: **file-transfer video**, requiring the plugin, not official TS6 screen-share negotiation. It is not the requested native interoperable layer. [Project description][live-plugin] |

One useful cross-check from EchoSixHIYA: its browser publisher explicitly prefers VP8 before H.264/AV1 for native viewers. Comments attribute this to observed rejection of a browser H.264 profile by a TS receiver. The preference is actual code; the decoder diagnosis is a maintainer claim. It is a reason to inspect negotiated SDP and codec parameters, **not** proof that every TS6 client requires VP8 or that H.264 never works. [Codec preference implementation][echo-codecs]

## Official TeamSpeak prerequisites and status, with dates

- **Server-channel streaming needs TS6 server signaling.** In the supplied forum thread, December 30, 2025 guidance requires a TS6 server; April 28, 2026 replies distinguish this from private/group-chat streaming. They also describe compatibility among streaming-capable TS5/6 clients. This does not add streaming support to an official TS3 client or a TS3 server. Flexspeak currently implements neither TeamSpeak's global chat/account path nor the newer direct-call feature. [Prerequisites thread][official-prerequisites]
- **SFU statements are dated plans.** On December 30, 2025, repository collaborator HuppiN said current streaming was P2P and server streaming was being developed. On June 29, 2026, SYOX described a tentative SFU release after Foundation III. Neither statement is a release artifact or an API contract. [Issue #62 collaborator response][issue62] · [June SFU update][official-sfu]
- **Issue #116 is an unresolved user report, not proof of one root cause.** Opened August 14, 2026 against server beta12.1; still open when fetched, with comments through October 9. Users report streams stuck connecting; comments speculate about firewalls, codec negotiation and VPN workarounds. No maintainer diagnosis appears in the fetched comments. [Issue and discussion][issue116]
- **Newer evidence changes how to read those reports.** The October 13, 2026 official client beta5/Foundation III announcement adds configurable ICE UDP ports and reports fixes for P2P connection loss, large offers after ICE restarts/codec changes, and a stalled viewer freezing a shared encoder. It also introduces direct calls. These are TeamSpeak release claims, not our rerun of #116, and do not prove WebSpeak compatibility with that release. [Foundation III announcement][foundation3]
- **No released self-hosted SFU was confirmed in the checked sources as of October 27.** The latest listed official server release is beta13.1, September 22; its and beta13's notes do not announce an SFU. Foundation III's announcement also does not announce one. A visible “SFU Endpoint” setting was explicitly described as internal-test functionality in earlier official guidance. The practical decision is to target P2P and not depend on SFU availability, while avoiding a timeless claim that SFU can never ship. [Server releases][server-releases] · [SFU setting explanation][official-sfu-setting] · [Foundation III][foundation3]

## Smallest useful next proof — proposed only

1. **Establish the environment first:** obtain the exact server version/configuration for `ts.zirkumflex.io` and an exact official client version. Confirm an official-to-official stream works on the intended network. If the server is TS3, stop the server-channel proof there; do not silently change the server or substitute another streaming service.
2. **Build only a native receiver experiment after research approval:** expose TS6 notifications and streaming discovery on the existing Rust protocol stack; join one consenting official publisher in one channel. Use one native WebRTC peer, answer its real SDP, exchange ICE, decode its negotiated video, and display frames in a minimal native surface. No capture, publishing, account integration, SFU, or full settings UI is necessary for this first result.
3. **Make success observable:** preserve exact client/server/build revisions and redacted signaling traces; record negotiated codec/profile, ICE candidate-pair type, received RTP, decoded-frame count and actual moving rendered output. A successful command or `connected` status alone is not success. Verify leave/stop releases media resources and that existing voice remains usable. Repeat once with a newly started stream and once joining an already active stream.
4. **Then assess resource and network viability:** compare idle versus receiving CPU and process memory, decoded resolution/FPS, buffering and stop/rejoin behavior. Try the intended separate networks; introduce an explicitly configured TURN test only if direct connectivity needs it. No CPU/RAM savings or universal NAT success is established here.
5. **Only after receive interoperability works**, scope native capture plus hardware-capable encoding and one official viewer. Evaluate codec/profile compatibility and viewer admission before multi-viewer scaling. Decide whether publishing is worth its measured resource cost.

The proof's decision gate is concrete: **an official TS6 publisher produces moving video in a browser-free native receiver using the existing TS6 signaling server, with known versions and measured cost**. Until then, leave the current Streaming tab inactive and describe interoperability as experimental research.

[ws-roadmap]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/ROADMAP.md
[ws-connector]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/connector/src/main.rs
[ws-tree]: https://github.com/WebSpeak3/webspeak3/tree/10ae24a99f4b397460f078798480a5527ea0f706
[ws-submodules]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/.gitmodules
[fork-diff]: https://github.com/Moepchi/tsclientlib/compare/ee3bc6f45a7137db7793ba5593a321df400d53e5...2e7794928f18d5aef2da5b48f420f9c801b1112c
[unknown-patch]: https://github.com/Moepchi/tsclientlib/commit/e9dd82e8035bb6a5248b2a8dac01cfa2e8726cef
[live-update-patch]: https://github.com/Moepchi/tsclientlib/commit/6df8ba1bcea3a115f61b0cfd01c29f052805cee5
[declarations-diff]: https://github.com/Moepchi/tsdeclarations/compare/83cb8a94b65021bb8ec4beb339e5bfbdd93df60f...c2992f940d7aad0102f156ece187ae8919e0aa59
[ws-incoming]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/connector/src/main.rs#L2829-L2846
[ws-outgoing]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/connector/src/main.rs#L1666-L1809
[ws-join]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/connector/src/main.rs#L804-L852
[ws-gateway]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/gateway/src/ts3/connection.ts
[ws-viewer]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/web/src/stream.ts
[ws-publisher]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/web/src/publish.ts
[ws-license]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/LICENSE
[fork-license]: https://github.com/Moepchi/tsclientlib/blob/2e7794928f18d5aef2da5b48f420f9c801b1112c/README.md#license
[decl-license]: https://github.com/Moepchi/tsdeclarations/blob/c2992f940d7aad0102f156ece187ae8919e0aa59/README.md#license
[receive-claim]: https://github.com/WebSpeak3/webspeak3/commit/b57d32bc18621a01c9c90bb6ef10e3d281084705
[publish-claim]: https://github.com/WebSpeak3/webspeak3/commit/15e215d89297b17c9864ab2c43b250d266b97c22
[admission-claim]: https://github.com/WebSpeak3/webspeak3/commit/13d3f56c89c159f0ae08cf23caef9d0e0fef7b84
[sfu-claim]: https://github.com/WebSpeak3/webspeak3/commit/115cd7150e240961582d86d7fd48c9f51b25640c
[ws-ice]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/web/src/stream.ts#L48-L69
[ws-turn]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/gateway/src/index.ts#L129-L143
[ws-media-wiring]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/web/src/App.tsx#L11902-L11967
[ws-sfu]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/web/src/App.tsx#L2763-L2775
[ws-admission]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/web/src/publish.ts#L194-L257
[ws-per-viewer]: https://github.com/WebSpeak3/webspeak3/blob/10ae24a99f4b397460f078798480a5527ea0f706/web/src/publish.ts#L270-L307
[echo-coordinator]: https://github.com/EchoSixHIYA/WebSpeak-client-for-TeamSpeak/blob/37f5b86b52884266d3fd1dfe576dc48c7f4cc181/src/server/screen-share-coordinator.ts
[echo-tests]: https://github.com/EchoSixHIYA/WebSpeak-client-for-TeamSpeak/blob/37f5b86b52884266d3fd1dfe576dc48c7f4cc181/src/server/screen-share-coordinator.test.ts
[echo-codecs]: https://github.com/EchoSixHIYA/WebSpeak-client-for-TeamSpeak/blob/37f5b86b52884266d3fd1dfe576dc48c7f4cc181/web/src/voice/screen-share.ts#L322-L478
[echo-announcement]: https://community.teamspeak.com/t/webspeak-self-hosted-teamspeak-web-client-browser-gateway/65302/4
[echo-license]: https://github.com/EchoSixHIYA/WebSpeak-client-for-TeamSpeak/blob/37f5b86b52884266d3fd1dfe576dc48c7f4cc181/README.md
[ts3-plugin]: https://github.com/mfreiholz/ts3video/blob/a6740a1a664280fe990410ca74e3dc4bd272f0db/projects/ts3plugin/impl.cpp
[ts3-config]: https://github.com/mfreiholz/ts3video/blob/a6740a1a664280fe990410ca74e3dc4bd272f0db/projects/videoserver/res/default.ini
[tea-changelog]: https://github.com/TeaSpeak/TeaSpeak/blob/master/ChangeLog.md
[tea-rtc]: https://github.com/TeaSpeak/TeaWeb/blob/597375676e20cb4b1515d248005603801da7d05f/shared/js/connection/rtc/Connection.ts
[tea-video]: https://github.com/TeaSpeak/TeaWeb/blob/597375676e20cb4b1515d248005603801da7d05f/shared/js/connection/rtc/video/Connection.ts
[qint]: https://github.com/ReSpeak/Qint/blob/a507f9a5016e35fc43d7f432289f05241c27cd1a/README.md
[respeak-audio]: https://github.com/ReSpeak/tsclientlib/blob/ee3bc6f45a7137db7793ba5593a321df400d53e5/tsclientlib/examples/audio.rs
[live-plugin]: https://github.com/Metihttp/Teamspeak_Live_stream/blob/main/README.md
[official-prerequisites]: https://community.teamspeak.com/t/where-is-screen-share-available-ts3-server-does-not-show-start-stream-button/58890
[official-permissions]: https://community.teamspeak.com/t/restrict-the-ability-to-start-a-stream-with-permissions/61697/4
[issue62]: https://github.com/teamspeak/teamspeak6-server/issues/62#issuecomment-3700096706
[issue116]: https://github.com/teamspeak/teamspeak6-server/issues/116
[official-sfu]: https://community.teamspeak.com/t/only-p2p-available-on-server-not-implemented-yet/55034/72
[official-sfu-setting]: https://community.teamspeak.com/t/since-when-is-the-sfu-video-feature-in-teamspeak-6-which-self-hosted-sfu-do-you-use/62715
[server-beta12]: https://github.com/teamspeak/teamspeak6-server/releases/tag/v6.0.0-beta12.1
[server-releases]: https://github.com/teamspeak/teamspeak6-server/releases
[foundation3]: https://community.teamspeak.com/t/teamspeak-6-0-0-beta5-foundation-iii-update/65513
[native-webrtc]: https://webrtc.github.io/webrtc-org/native-code/native-apis/
