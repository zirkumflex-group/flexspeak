# Performance mode — 2026-10-27

For the current version with ten-second readouts, see the [normal/performance comparison, including connected Lobby tests](lobby-mode-comparison.md). The measurements below predate that readout.

The switch on Voice and Statistics opens a 440×320 logical-point window with read-only connection, server, channel, effective microphone state, and playback state. CPU and RAM (RSS) refresh every ten seconds, sampling only this process; CPU is averaged over the interval and initially shows “measuring…”. Errors remain visible and scrollable. Turning it off restores the selected tab and previous window size. It is a session setting; a fresh app launch starts with the full controls.

The memory row now places **Footprint (MiB)** beside RSS on macOS. It reads the current process's `ri_phys_footprint` through the SDK binding, without invoking `vmmap`. Windows shows **Private RAM** in the same position using `PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`; this is a different platform counter and is deliberately labeled separately. Unsupported/failed readings show `—`. Normal Statistics shows the same pair beside the RSS chart; chart history remains RSS-only. Refresh rates and renderer behavior are unchanged.

Validation of the added memory row: all 17 tests, Clippy with warnings denied, formatting and macOS/Windows release builds passed. Native macOS inspection confirmed both layouts and a sampled 39.2 MiB footprint matching the independent kernel query. Windows runtime validation remains pending.

The audio worker keeps running with the existing codec, capture/playback configuration, and mute/deafen state. The shared mode flag only gates optional meter/usage events and server connection-info requests. Protocol events, audio processing, reconnection, channel changes, errors, and shutdown continue normally. Switching modes does not mute an already unmuted microphone.

The UI replaces its one-second usage subscription with a ten-second process-only sample, drops the histories' cached geometry, and finishes/saves an active trace. Slow samples do not update history, heap accounting, trace counters or network statistics. Retained history expires normally when full sampling resumes, and gaps are drawn across paused periods. The first CPU value after switching modes is unavailable until a fresh sampling interval exists.

## Measured local effect

These measurements are from the initial 440×270 mode before the ten-second CPU/RAM readout was added. They are retained as historical evidence, not a benchmark of the updated mode.

One release process, Apple M4, macOS 27.0.1 arm64, Retina scale 2. GPU rendering with MSAA disabled. Disconnected, no audio streams, no trace recording. Five-second warmup and twenty-second sample per view; no UI interaction or builds during sampling.

| Measurement | Full Statistics, 900×790 | Compact text, 440×270 |
| --- | ---: | ---: |
| Median RSS | 103.375 MiB | 103.391 MiB |
| macOS physical footprint | 155.1 MiB | 36.5 MiB |
| Average process CPU, one core = 100% | 0.396% | 0.049% |

The observed physical-footprint reduction was 118.6 MiB (76.5%). In `vmmap`, owned-unmapped graphics residency fell from 93.8 MiB to about 3.7 MiB and IOSurface residency from 22.1 MiB to about 3.7 MiB. The application still owns a renderer; it does not guarantee all graphics memory will always be released. These observations do not independently attribute each resource to a specific optimization.

RSS includes resident mappings with different accounting from physical footprint, so its almost unchanged value is not a contradiction. Do not add the measurements. CPU timing uses cumulative process CPU deltas; the compact result represents only 0.01 seconds of measured CPU over the interval, so precision is limited. This single idle comparison is not a connected-call benchmark or a cross-platform guarantee.

The [raw samples, vmmap reports and environment metadata](measurements/2026-10-27-performance-mode/) were captured with [the process measurement script](../scripts/measure_macos_process.py).

## Validation and limits

- All 16 automated tests passed, including three new checks for paused sampling without stop/mute commands, effective microphone labels, and ignoring obsolete window-query responses. Existing codec, playback-queue and resampling tests also passed.
- Clippy with warnings denied, formatting, and optimized macOS bundle build passed.
- Native UI inspection verified compact mode, restoration to Statistics, a visible history gap, and restoration from an enlarged window.
- Entering the mode during recording saved a 7.2-second trace containing 22 events and zero dropped events. The result remained accessible on returning to Statistics.
- Live voice continuity, Windows/Linux behavior and full-screen transitions have not been runtime-tested for this change. The audio processing loop was not changed.
- Current window dimensions and maximized/full-screen state are saved. The older normal rectangle behind a maximized window is not separately retained, so a later OS unzoom may require manual resizing.

The application was left disconnected with performance mode enabled.

## Ten-second readout follow-up

The updated mode was checked in a separate Mac preview while the user's original connected session was left running. The initial CPU “measuring…” state changed to a sampled value, RAM was visible, and the refresh interval was labeled. All 16 tests, Clippy and formatting passed; the test for performance mode now verifies that slow samples update the process reading without adding history samples, and obsolete slow ticks are ignored after leaving the mode.

An attempted follow-up benchmark is saved as `interactive-preview-excluded.json` with an explicit exclusion flag. The preview's window/mode changed during the session, so that run is not evidence of compact idle overhead. No replacement idle-performance claim is made for the ten-second readout.
