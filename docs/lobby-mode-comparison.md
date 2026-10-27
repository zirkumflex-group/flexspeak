# Normal vs performance mode — 2026-10-27

On this Apple M4 Mac, performance mode reduced measured process CPU by approximately **75%**, both disconnected and connected quietly in Lobby. Median macOS physical footprint also fell substantially. RSS—the counter shown inside the application—was nearly unchanged.

## Measured results

Two 30-second samples per condition, each after a ten-second warmup. CPU is the duration-weighted mean across both samples; 100% represents one logical core. RSS is the median of the combined samples. Physical footprint is the median of the collected observations, with its full observed range shown separately.

| State | Mode | CPU, % of one core | Median RSS, MiB | Median footprint, MiB | Observed footprint range, MiB |
| --- | --- | ---: | ---: | ---: | ---: |
| Disconnected | Normal Voice | 0.395 | 100.8 | 156.7 | 145.9–156.7 |
| Disconnected | Performance | 0.099 | 100.4 | 35.7 | 35.6–126.3 |
| Connected to Lobby | Normal Voice | 4.032 | 108.4 | 158.0 | 147.9–159.1 |
| Connected to Lobby | Performance | 1.004 | 108.4 | 51.1 | 49.5–142.2 |

Per-run CPU ranges were 0.329–0.460% versus 0.099% disconnected, and 3.985–4.080% versus 0.988–1.020% connected. These are short local observations, not confidence intervals or guaranteed results on another machine.

The low footprint in performance mode is not a fixed ceiling: periodic redraws coincided with higher graphics-memory observations. A single snapshot can therefore show approximately 36 MiB or 126 MiB disconnected, or 51 MiB or 142 MiB connected. The remaining RSS does not track those graphics allocations in the same way.

## What was held constant

- One fresh optimized release process, Apple M4 with 10 logical CPUs, macOS 27.0.1 arm64, Retina scale 2. Executable hash and raw evidence are in the [measurement directory](measurements/2026-10-27-lobby-comparison/).
- Normal mode used the Voice tab at 900×790 logical points. Performance mode used its 440×320 text window with ten-second CPU/RAM refreshes. The results include both the smaller window and reduced update work; this does not isolate either contribution independently.
- Tracing was off. No builds, deliberate UI interaction or screenshots occurred during the timed samples. Other desktop apps remained running.
- Sequence for each connection state: normal, performance, performance, normal. The normal window size and view were restored for the repeat.
- Server `ts.zirkumflex.io`, server name Zirkumflex, **Lobby (#1) only**. Microphone muted, playback enabled throughout. MacBook Pro input at 48 kHz mono and speakers at 48 kHz stereo. No channel changes were requested.
- Audio packet counters remained **0 sent / 0 received** during the valid connected session. This measures connected idle/listening readiness, not speaking, active decoding, audible continuity, or latency under a live conversation.

## Measurement method and excluded run

The sampler reads this process's RSS and cumulative CPU time once per second. CPU is calculated from CPU-time deltas divided by elapsed wall time. Disconnected footprint evidence consists of two end-of-run `vmmap` readings plus five additional snapshots roughly 3.7 seconds apart, covering variation across a ten-second redraw interval.

The first connected attempt ended with an audio buffer-underrun/overrun error and disconnection. Its data is preserved as `lobby-normal-interrupted.json` with `valid_for_comparison=false` and excluded from every summary. The exact failure time relative to the deep `vmmap` inspection was not established. Inspection-induced scheduling disruption is a hypothesis, not a proven root cause.

For the valid connected runs, the sampler was changed to call `proc_pid_rusage(RUSAGE_INFO_V0)` and read `ri_phys_footprint` once per second, without invoking `vmmap`. The 96-byte structure and field offsets were checked against the installed macOS SDK; invalid-PID handling and a disconnected comparison with `vmmap` passed (158.360 MiB versus its rounded 158.4 MiB). Each connected condition therefore has 62 footprint observations. All four subsequent connected samples remained connected, and mute/channel/packet state was checked around the mode transitions.

[`measure_macos_process.py`](../scripts/measure_macos_process.py) now uses the lightweight counter by default. Deep mappings remain an optional `--vmmap` diagnostic; avoid it during real-time audio. No application or audio-processing code was changed for this comparison. The current client's response to an audio xrun remains a reliability concern; this run does not establish that it is resolved.

## Why the modes differ

| Behavior | Normal mode | Performance mode |
| --- | --- | --- |
| Interface | Full controls and tabs | Read-only connection/channel/mic/playback text |
| Process CPU/RAM readout sampling | Every 1 second | Every 10 seconds |
| History/graphs | Updated in the background | Paused; cached chart geometry released |
| Microphone-meter UI events while connected | Up to 10 per second | Paused |
| Optional server connection-info polling | Every 5 seconds | Paused |
| Audio processing and protocol keepalive | Active when connected | Active when connected |

The audio worker, native streams and network connection explain why connected performance mode still costs more CPU than disconnected performance mode. The codec settings and actual audio processing are unchanged by the switch.

## Windows: approximately 250 MiB in-app vs 150 MiB in Task Manager

The pinned `sysinfo` Windows implementation assigns `PROCESS_MEMORY_COUNTERS_EX.WorkingSetSize` to the app's resident-memory reading. That is the **total working set**, including private and shared resident pages. Task Manager's usual Memory column reports the **private working set**. If that is the column being viewed, approximately 250 MiB total and 150 MiB private imply approximately 100 MiB of shared resident pages; they are overlapping counters, not two amounts to add. The exact column and simultaneous values on Tribunal were not remotely inspected. ([Microsoft working-set definition](https://learn.microsoft.com/en-us/windows/win32/memory/working-set), [Microsoft's Task Manager counter explanation](https://blogs.windows.com/msedgedev/2021/01/13/investigate-microsoft-edge-memory-usage/))

For a matching counter, Task Manager → Details → Select columns → **Working set (memory)** is the relevant comparison with the app. Private and shared working-set columns can explain the difference. ([Microsoft instructions](https://learn.microsoft.com/en-us/archive/msdn-magazine/2012/windows-8-special-issue/javascript-managing-memory-in-windows-store-apps))

This explains the likely Windows display discrepancy, not the precise reason Windows retains more resident memory than the Mac. Both builds are optimized releases. Renderer/driver allocations, allocator retention, loaded libraries, window dimensions and display scaling can differ. Performance mode keeps the graphics backend initialized, and the OS may retain resident pages between redraws. Windows also exposes dedicated/shared **GPU memory** separately; those are different counters from private/shared working set. ([Microsoft GPU-memory accounting](https://devblogs.microsoft.com/directx/gpus-in-the-task-manager/))

The reported unchanged ~150 MiB Task Manager value in both modes does not establish whether GPU memory or CPU changed on Windows. No measured Windows allocation breakdown or mode benchmark was collected here, so the extra memory cannot honestly be assigned to a specific subsystem yet.

The Mac client was left connected to **Lobby**, microphone muted, playback enabled, in performance mode.
