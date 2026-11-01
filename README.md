# Native Voice

GitHub project: **Flexspeak**. [Download the latest private Windows/macOS/Linux release](https://github.com/zirkumflex-group/flexspeak/releases/latest), or follow the [Homebrew, Omarchy and automatic-release guide](docs/releases.md). Every successful push to `main` publishes updated packages; pull requests use a separate fast Linux check.

A minimal native Rust desktop client for TeamSpeak voice servers. The UI uses Iced; audio uses CPAL and native libopus; connectivity and incoming jitter buffering use a vendored [ReSpeak/tsclientlib](https://github.com/ReSpeak/tsclientlib) based on `ee3bc6f45a7137db7793ba5593a321df400d53e5`, with a narrow TS6 streaming-notification hook.

## Included

- Editable server address, nickname, and optional server password.
- Native microphone/playback device detection and selection.
- Local microphone-to-speaker test, disconnected from TeamSpeak.
- Opus microphone transmission and incoming voice playback.
- A channel/client tree with health dots, conditional mute/deafen icons, clickable channel headings and password prompts.
- Mute, deafen, microphone level, sent/received audio packet counters.
- Editable global mute/deafen shortcuts and an app-only playback volume slider.
- A Settings tab with portable JSON for server address, nickname, volume and keybinds.
- A Statistics tab with CPU/RAM usage, five-minute history graphs, and P50/P90/P99.
- Live upload/download rates, session traffic totals, packet loss in/out, ping variation, idle time, and server-reported file-transfer rates.
- Own connection health and compact line charts in the fixed footer; current-channel clients have collapsed health labels and expandable details in Voice (subject to server permissions).
- On-demand traces, CPU timing by subsystem, and a memory breakdown with explicit measurement boundaries.
- Persistent local TeamSpeak identity, bounded audio buffers, and device-rate conversion.
- Dismissible error modals, duplicate suppression, acknowledged audio controls and adaptive server-command pacing. See [error handling](docs/error-handling.md).
- Cancel/disconnect and visible network/audio errors. Temporary network interruptions use the protocol library's reconnect behavior.

The prefilled server address is `ts.zirkumflex.io`; it remains editable. The microphone starts muted on every connection. Unmuting transmits continuously until you mute, deafen, disconnect, or lose permission to speak. Use headphones: this prototype does not yet include echo cancellation, noise suppression, automatic gain control, voice activation, or push-to-talk.

## Run

Requires Rust 1.92 or newer, a C/C++ toolchain, CMake, and Git. `Cargo.lock` is checked in; use `--locked` for reproducible dependency resolution. The first build downloads the pinned protocol repository and its declarations submodule automatically.

### macOS

```sh
brew install cmake pkg-config opus
cargo run --locked
```

For predictable microphone permissions, use the application bundle:

```sh
sh scripts/macos-app.sh
open "dist/Native Voice.app"
```

Pass `debug` to the script for a faster development build. The bundle is locally ad-hoc signed and includes the microphone usage description. It is not notarized for public distribution. The CPAL version in this project requires macOS 14.2 or later.

Click **Allow** when macOS asks for microphone access. If denied, enable Native Voice in System Settings → Privacy & Security → Microphone, then restart the app. When running from a terminal, the permission may belong to that terminal instead.

### Linux

On Ubuntu/Debian:

```sh
sudo apt-get install build-essential cmake pkg-config libasound2-dev libopus-dev libxkbcommon-dev libwayland-dev
cargo run --locked
```

### Windows

The portable Windows x64 build is packaged as `dist/NativeVoice-Windows-x64.zip`. Extract it and open `NativeVoice.exe`. See [Windows build details and validation](docs/windows-build.md).

Install Rust's MSVC toolchain, Visual Studio C++ Build Tools, Git, and CMake. Then run:

```powershell
cargo run --locked
```

libopus links statically. The binding finds an installed libopus using pkg-config where available, or builds its bundled copy with CMake. To choose an installed build explicitly, set `OPUS_LIB_DIR` to the directory containing its `lib/` directory. Use a current libopus installation if codec-version consistency between machines matters; the bundled fallback is older. For CMake 4 with older bundled sources, set `CMAKE_POLICY_VERSION_MINIMUM=3.5` in the environment.

## First voice session

A missing settings file opens setup for your username, one saved server, permissions, devices and global shortcuts. Finish saves the profile; existing valid settings go straight to the saved-server card. Use its plug button to connect. While connected, the channel tree stays on the left and streaming occupies the right. Settings and Statistics are available from the header.


1. In **Settings → Audio**, select your microphone and headphones/speakers.
2. Choose **Test microphone locally**, approve microphone permission, speak, and check the activity bar below Microphone and local playback. Stop the test.
3. Enter the server address (hostname, `hostname:port`, or IP), nickname, and password if needed.
4. On the home screen, use the saved server’s **Connect** icon. Click a channel heading in the left sidebar to join; protected channels prompt for a password.
5. Click **Unmute microphone** to speak. The outgoing packet counter should increase; incoming packets indicate received voice. A second participant using TeamSpeak is needed to verify a real conversation.
6. **Deafen** stops playback and transmission. **Disconnect** releases the audio devices.

Devices are fixed for one session. Refresh/select and reconnect after unplugging devices or changing a Bluetooth audio profile. This version requires both an input and an output device. Microphone input is downmixed to mono; incoming Opus Music can play in stereo. Only Opus Voice/Music channels are supported.

If AirPods or other Bluetooth headphones are missing, connect them to this computer first and confirm they appear in the operating system's sound output list. Then **Disconnect → Settings → Audio → Refresh devices → choose the headphones under Playback → Voice → Connect**. The app keeps its selected output until you change it; changing the system default alone does not reroute an existing session. If incoming packet counts increase but playback is silent, check the selected device's system volume and mute setting, then use the local microphone test to check the output path.

## Controls and saved settings

The default global toggles are **Ctrl+Shift+M** for mute/unmute and **Ctrl+Shift+D** for deafen/undeafen. Change them in **Settings → Save settings**. The same JSON file stores the server address, nickname and playback volume. Use **Copy settings path** to locate it; edit/copy the file and use **Reload JSON** while disconnected, or restart the app. On native Wayland, global shortcuts are currently unavailable and the app reports that limitation.

The **Volume** slider in **Settings → Audio**, below Playback, changes only Native Voice's output (0–100%), including a live session or local audio test. Save in Settings to retain it. Setting volume to zero does not mute your microphone. See [controls, JSON format and validation](docs/controls-and-settings.md) and [the example JSON](settings.example.json).

## Streaming

The connected server view includes streaming controls that support experimental native watching and screen/window sharing on Mac and Windows. Join a TS6 channel, choose a capture source and optional system-output audio, then approve viewers individually. Stream playback uses the app's volume and deafen controls. Up to four viewers are supported. Generated-media tests and a local Mac capture/system-audio check passed; official-client viewer playback and Windows hardware still need runtime verification. See [scope, use and validation](docs/streaming.md).

## Usage and connection metrics

Open **Statistics** using the graph icon at the top right. Sampling and history continue while either tab is selected.

The fixed bottom bar keeps your connection-health indicator at the left, followed by a row of small loss-in, loss-out, ping and idle charts while connected in normal mode. CPU, RAM (RSS) and the platform memory detail remain at the right. The bar stays visible across Voice, Streaming, Statistics and Settings while the content scrolls.

### Performance mode

Use **Performance mode** at the top of either tab for a compact, read-only text window showing connection, server, channel, microphone, and playback status. Turn the same switch off to restore the previous tab and window size and use the controls again. Long names and errors remain scrollable.

Your current voice session, mute/deafen settings, codec, and audio processing continue unchanged. A microphone already unmuted stays unmuted; the text shows effective microphone state, including deafen and server talk restrictions. Connection changes and errors still update the window.

Read-only CPU and RAM (RSS) values refresh every ten seconds in performance mode. CPU is averaged over the sampling interval, with 100% representing one core; it shows “measuring…” until the first interval completes. Only this process is sampled. Graph updates, microphone meters, and optional connection-statistics polling remain paused. Existing history is retained within its five-minute limit, with a gap for the pause; CPU sampling starts a fresh interval when switching modes. Entering the mode finishes and saves an active trace. Charts release their own cached geometry, and the smaller window uses smaller display buffers. The graphics backend remains initialized, so this is not a promise that all previously allocated GPU/driver memory will be returned.

In an initial disconnected comparison before adding the ten-second CPU/RAM readout, physical footprint fell from 155.1 to 36.5 MiB and idle CPU from 0.40% to 0.05% of one core on the Apple M4 Mac. RSS stayed around 103.4 MiB. These historical measurements do not include the new periodic readout; active voice adds work too. See [performance-mode measurements and validation](docs/performance-mode.md).

The [updated normal/performance comparison](docs/lobby-mode-comparison.md) includes two samples per mode while disconnected and connected quietly in Lobby. Connected CPU averaged 4.03% versus 1.00% of one core, while RSS stayed around 108 MiB. Median physical footprint was 158 versus 51 MiB, with higher transient readings. These are Mac measurements; the report also explains Windows total/private working-set counters and the excluded audio-underrun run.

### Metrics

- **CPU** is this process's usage, with 100% representing one logical core. It can exceed 100% when multiple cores are busy. Normal and performance modes show up to three decimal places (for example `0.001%` or `0.02%`), with positive readings below `0.001%` shown as `<0.001%`. CPU percentiles and trace percentage labels use the same formatting. This changes display precision, not the sampling method or interval. The first CPU sample is left blank until a measurement interval exists.
- **RAM** is resident memory (RSS), shown in binary units such as MiB. It can differ from macOS Activity Monitor's memory-footprint figure.
- **Footprint** appears beside RSS on macOS, in MiB, using the lightweight `proc_pid_rusage` physical-footprint counter. On Windows the adjacent value is instead labeled **Private RAM** and uses the private working set, matching the usual Task Manager memory counter more closely. This Windows field needs Windows 10/11 22H2 with the September 2023 cumulative update or later; unavailable readings show `—`. These are overlapping measurements and must not be added together. Both values also appear side by side in performance mode, retaining its ten-second refresh interval. The RAM history and percentiles still describe RSS only.
- **History** retains at most 300 actual samples from the last five minutes. CPU and RAM show P50, P90, and P99 using the nearest-rank method; for example, P90 is the sampled value at or below which at least 90% of the measurements fall. Missing samples are omitted, gaps are shown across long pauses, and history is not stored on disk.
- **Upload/download** show bytes per second; **session sent/received** show cumulative bytes since connecting. They use TeamSpeak's UDP transport counters for voice, control, and keepalive traffic, including the library's UDP/IPv4 header estimates. They are not whole-machine bandwidth measurements and do not include DNS or separate TCP file transfers. Rates use actual elapsed time. Automatic reconnects preserve accumulated totals; a new session resets them. Disconnecting zeros rates and keeps the final totals visible.
- **Packet loss (in)** uses locally measured server-to-client loss. **Packet loss (out)** comes from the server's report for this client. Both appear as percentages.
- **Ping** appears as round-trip milliseconds plus/minus the measured deviation, for example `17 ms ± 2.1`.
- **Idle time** uses the server's report in `HH:MM:SS`, advances between reports, and resets when this client sends voice.
- **File transfer (in/out)** uses the server-reported per-client bandwidth in bytes per second. File sharing is not implemented in this prototype; absent server fields appear as `—`, not invented zeros.

Process and local connection metrics refresh once per second. Only this process's CPU/memory is sampled; history drawings are cached between updates. The client requests its own server-side connection information every five seconds. If the server denies that request, polling stops until reconnecting. Expanding another client in Voice requests that client's connection health while the details are visible; see [connection health and thresholds](docs/controls-and-settings.md#connection-health). Server measurements older than 15 seconds become unavailable, and disconnected/unsupported values show `—`.

### Traces and breakdowns

In **Statistics → Traces & CPU breakdown**, choose **Record 30 s trace**. Recording stops and saves automatically after about 30 seconds, or use **Stop & save trace** earlier. Closing the app discards an unfinished recording. The result includes CPU time and call counts for UI handlers, network polling/sending, audio processing, and audio callbacks, plus the longest elapsed duration of each captured operation. Uninstrumented CPU work—including framework, renderer, and monitoring overhead—remains in **Other process work**.

Trace results start with summary cards and the largest recorded CPU cost / longest call. Three ranked horizontal-bar panels show:

1. **Where CPU time went:** total CPU by subsystem, including a gray unattributed-work bar.
2. **Operations with the most CPU cost:** summed CPU across all calls, with call counts and average CPU per call.
3. **Longest single calls:** the maximum elapsed duration for each operation, ranked independently of CPU cost, with average elapsed duration for context.

Every row in a panel uses the same linear scale. CPU bars and elapsed-time bars are separate because an operation can wait a long time while consuming little CPU. Friendly labels retain the raw operation name for matching against the exported trace. Empty captures and dropped events are called out explicitly.

CPU timing uses operating-system thread clocks; it is not inferred from time spent waiting. Only synchronous work is timed: network futures are measured per poll rather than across an await. The exported Chrome Trace Event JSON uses elapsed durations for its timeline and includes CPU microseconds in each span's `thread_cpu_us` argument, plus memory/network counter tracks. It can be opened in [Perfetto](https://ui.perfetto.dev/) or a compatible Chrome trace viewer. **Copy trace path** copies the saved local filename. Files are stored in the app data directory's `traces/` subdirectory, alongside the identity directory reported by `--identity-path`.

The recorder retains at most 20,000 events and reports dropped events if the buffer fills. Completed spans that were not captured are included only in the process-wide CPU total. This is instrumented tracing, not a full call-stack sampling profiler or GPU profiler. It records static operation names and numeric resource counters; it does not record voice audio, chat contents, passwords, or server addresses. Captures remain local until you choose to share them.

The memory charts below the trace results show:

- Process resident RAM.
- Live and peak Rust allocation sizes, tracked by a thin wrapper around the standard system allocator. Peak is since app launch. This excludes direct C/native-library allocations and allocator bookkeeping/fragmentation.
- Reserved payload capacity of known audio sample queues/work buffers, history samples, and the trace event buffer. Audio decoder internals, resampler coefficients, queue metadata, renderer caches, and GPU allocations are not fully attributed.

The process-footprint chart compares resident RAM, live Rust heap and historical peak heap. A separate buffer chart uses a smaller scale for the known audio/history/trace capacities. These measurements overlap and must not be added together. A small live Rust heap does not explain the whole resident footprint: native graphics/framework allocations, stacks, fragmentation and allocator-retained pages still need native profiling for full attribution.

Recording adds timing and buffer overhead; compare normal usage with recording disabled. The trace buffer is allocated on first use and retained for reuse. JSON export now serializes one event at a time directly to a buffered file, avoiding a second in-memory tree for the complete capture. The 20,000-record regression test reduced temporary serialization heap growth from 27,244,195 bytes to 2,033 bytes; these figures exclude the already-built capture and the file writer buffer, and are not total process RAM measurements.

### Memory optimization

GPU rendering now disables Iced's full-viewport 4× triangle MSAA buffers, which otherwise retain their larger capacity after resizing. Text and standard widgets keep their own smoothing; canvas edges can look more jagged. Audio and codec settings are unchanged.

On the tested Apple M4 Mac, disconnected Statistics-tab comparisons reduced physical footprint from about 220 MiB to 154–167 MiB at the default window size, and from 372 MiB to 201 MiB when enlarged. RSS—the RAM number inside the app—stayed around 101–105 MiB because it does not include all owned GPU memory. These are optimized release measurements, not debug-build overhead. Idle CPU remained below 0.6% of one core in these samples.

Software rendering had a lower footprint at the default size but used more CPU and became substantially more memory-intensive when enlarged, so GPU rendering remains the default. See the [measurements, reproduction steps, and limitations](docs/memory-optimization-results.md) and the [Rust/dependency research and tradeoffs](docs/memory-optimization-research.md). Other operating systems, lower-density displays, and active voice workloads were not benchmarked in this comparison.

## Diagnostics and checks

```sh
cargo run --locked -- --list-devices
cargo run --locked -- --probe ts.zirkumflex.io
cargo run --locked -- --identity-path
cargo run --locked -- --settings-path
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
```

`--probe` connects muted/deafened, reports server state, then disconnects. It never opens a microphone or plays audio. Set `RUST_LOG=teamspeak_native=debug,tsclientlib=debug` for connection diagnostics; avoid distributing logs containing private server details.

## Data and scope

Chart/export validation on 2026-10-27: thirteen tests, Clippy with warnings denied, formatting and the optimized release build pass. The ranking regression distinguishes frequent CPU-heavy operations from individually slow calls. Native UI inspection confirmed the ranked CPU/elapsed bars and separate footprint/heap charts. A fresh disconnected 30.9-second capture saved 92 records with zero dropped events, and its chart values matched the JSON. Resident memory during that particular capture was approximately 104.4 MiB; this is a single observation, not a controlled comparison with earlier runs or window sizes. The streaming-export regression independently verifies the reduced temporary heap allocation.

Observability validation on 2026-10-27: the Statistics tab, rolling histories/percentiles, process memory, live traffic, packet loss in/out, ping variation, and idle time were inspected in the native macOS app. The server did not report per-client file-transfer rates, so those correctly displayed `—`. A 30.7-second trace captured 14,846 records with zero dropped events and saved successfully. Twelve automated tests, Clippy with warnings denied, formatting, and the optimized build pass. Tests cover allocator accounting, bounded tracing and JSON structure, history eviction/percentiles, traffic counter resets, and stale connection metrics.

Validation on 2026-10-26: the optimized Apple Silicon build, native window, audio device enumeration, five automated tests, Clippy, and formatting checks pass. After microphone permission was granted, the local audio test opened 48 kHz mono input and 48 kHz stereo output, showed a live microphone signal, and stopped cleanly. The native GUI connected to `ts.zirkumflex.io` and joined **Zirkumflex → Lobby**. The CoreAudio hardware-format fix passed this runtime recheck. A later silent-playback report showed incoming voice packets while the selected MacBook speakers were muted at 0% in macOS; AirPods were not yet connected to the Mac. After connecting AirPods, refreshing devices, and selecting them for playback, the user confirmed hearing their own voice in the local test. Audible local capture/playback is verified; a complete two-client voice conversation still needs confirmation. Windows/Linux CI is configured but has not run. Audio devices are initialized before the network connection so a permission prompt cannot stall the TeamSpeak event loop.

The app stores a TeamSpeak identity under the operating system's per-user application-data directory (`--identity-path` shows the location). It is a private key, saved with owner-only file permissions on Unix. Back it up to retain the same server identity. Windows uses the account's inherited application-data permissions. Passwords are held in memory and are not saved. Audio is neither recorded nor stored on disk.

This is a voice prototype. It does not implement myTeamSpeak, your future authentication/chat/file services, or full TS6 audio-processing parity. Server acceptance also depends on minimum client-version rules and permissions. The protocol library's compatibility must be tested against each target server.

## Structure

- `src/main.rs`: native window and user controls.
- `src/client.rs`: connection worker, channel state, identity storage, and muted network probe.
- `src/audio.rs`: device discovery, callback queues, Opus, resampling, and playback.
- `src/usage.rs`: per-process measurements, traffic rates, connection metrics, bounded histories, percentiles, and cached charts.
- `src/memory.rs`: allocation-size accounting while preserving the standard system allocator.
- `src/profiler.rs`: bounded trace capture, CPU attribution, and Chrome Trace Event JSON export.
- `src/statistics_charts.rs`: ranked trace bars and non-additive memory visualizations.

Audio callbacks only convert/copy samples through bounded queues. Encoding, decoding, and resampling run on the session worker. The UI receives session updates through a bounded event channel; a one-second timer keeps the process usage panel current even when disconnected.
