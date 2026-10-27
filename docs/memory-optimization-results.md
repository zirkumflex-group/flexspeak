# Renderer memory measurements — 2026-10-27

**Kept change:** disable Iced's triangle MSAA with `.antialiasing(false)` and retain GPU rendering. The initial comparison reduced macOS physical footprint by **52.9 MiB at the default size** and **170.6 MiB when enlarged**. Resident memory (the app's RAM counter) changed by less than 1 MiB in that comparison. These are different memory measurements; the graphics saving must not be presented as a similar RSS reduction.

The native Voice screen, Statistics histories, text, and memory bars were inspected at Retina scale. Text remained smooth and charts readable. Canvas edges can look more jagged, especially on lower-density displays, which have not been checked. This change does not alter the audio pipeline or codec settings. Live voice latency and audio quality were not measured in this experiment.

## Results

Apple M4, 10 logical CPUs, macOS 27.0.1 arm64. All variants are optimized release builds with the same locked dependencies and thin LTO. Workload: disconnected Statistics tab, one-second chart updates, recording off, no audio streams. Default client area: 900×790 logical points at display scale 2. Enlarged/restored states use the same native window zoom action on the same display.

| Renderer / window state | Median RSS (MiB) | Physical footprint (MiB) | Average CPU (% of one core) |
| --- | ---: | ---: | ---: |
| GPU, MSAA×4, fresh default | 101.5 | 219.8 | 0.40 |
| GPU, MSAA off, fresh default | 100.9 | 166.9 | 0.54 |
| GPU, MSAA×4, enlarged | 105.4 | 371.8 | 0.40 |
| GPU, MSAA off, enlarged | 104.6 | 201.2 | 0.59 |
| GPU, MSAA×4, restored | 105.5 | 328.3 | 0.40 |
| GPU, MSAA off, restored | 104.6 | 170.6 | 0.49 |
| Software, fresh default | 136.9 | 69.6 | 2.82 |
| Software, enlarged | 570.3 | 321.1 | 4.94 |
| GPU, MSAA×4, fresh default repeat | 104.1 | 220.1 | 0.45 |
| GPU, MSAA off, fresh default repeat | 103.3 | 154.0 | 0.40 |

The fresh-process repeat saved **66.1 MiB (30.0%)** of physical footprint. Across the two default comparisons, the observed saving was **52.9–66.1 MiB (24.1–30.0%)**. The enlarged and restored sequences were each run once; their observed savings were 45.9% and 48.0%, respectively.

Software rendering used `ICED_BACKEND=tiny-skia` with the original binary. Its default-window footprint was lower, but CPU cost was higher. After enlarging, RSS varied from **389.0 to 680.6 MiB** during the short sample, and physical footprint was higher than the GPU candidate. This is not a stable long-run estimate or proof of a leak. It is sufficient reason to retain the GPU default pending a separate software-renderer investigation. The software backend remains compiled as a fallback; no dependency change is needed to test it.

The GPU CPU figures are all below 0.6% of one core. These short runs do not establish a CPU improvement or equivalence under active voice, scrolling, or resizing. The time spent performing resize actions was excluded from the timed idle samples.

## Why RSS barely changed

`vmmap` showed enlarged **owned-unmapped graphics residency** fall from 236.6 to 94.4 MiB. IOSurface residency remained 61.7 MiB in both enlarged cases. Rust live allocation sizes were around 2 MiB in the candidate UI. These observations locate the main improvement in native graphics ownership, outside the Rust allocation counter and much of the process's resident-memory counter.

Iced 0.14 allocates the triangle MSAA attachment/resolve targets for the physical viewport and retains their capacity when the window shrinks. Disabling this path explains the direction of the measured saving. The entire footprint difference is not individually attributed to those textures: driver resources and drawable counts also vary. See [Iced's implementation](https://github.com/iced-rs/iced/blob/0.14.0/wgpu/src/triangle/msaa.rs) and [Apple's explanation of Metal memory accounting](https://developer.apple.com/documentation/xcode/analyzing-the-memory-usage-of-your-metal-app).

RSS includes resident mappings that are not necessarily charged as private footprint; footprint includes some owned graphics resources without ordinary process mappings. Either number can be larger. Heap, RSS, and footprint overlap and must not be added together.

## Method and evidence

[Raw samples, vmmap reports, and build/environment metadata](measurements/2026-10-27-renderers/) are saved alongside this report. Temporary application bundles used distinct identifiers; only one benchmark process was running during each sample. No builds or UI interactions occurred during timed samples. Normal desktop applications were still running, so these are practical local comparisons, not isolated laboratory results.

[`scripts/measure_macos_process.py`](../scripts/measure_macos_process.py) reads this PID's RSS and cumulative CPU time once per second. CPU is the CPU-time delta divided by elapsed wall time, rather than `ps`'s lifetime-average CPU percentage. `vmmap -summary` runs after timed sampling and supplies a single current physical-footprint observation. The script leaves the application untouched.

Those historical runs used the earlier sampler. The current sampler reads the lightweight `proc_pid_rusage` footprint counter by default and makes deep mapping inspection opt-in with `--vmmap`, following an interrupted audio run described in the [Lobby comparison](lobby-mode-comparison.md).

- Initial default runs: five-second warmup, twenty-second sample.
- Enlarged runs: five-second warmup, fifteen-second sample.
- Restored runs: three-second warmup, ten-second sample.
- Fresh default repeat runs: ten-second warmup, twenty-second sample.

For a new comparison, launch the desired build, select the same screen and window size, find its PID, then run:

```sh
python3 scripts/measure_macos_process.py PID --label comparison \
  --warmup 10 --seconds 20 --output /tmp/nativevoice-comparison.json
```

Use `ICED_BACKEND=tiny-skia cargo run --release --locked` for an explicit software-renderer experiment. Confirm the selected renderer through its memory/resource report; an environment variable passed to `open` is not a reliable way to configure an already-running macOS application.

## Rust and dependency decisions

The [primary-source research report](memory-optimization-research.md) covers Iced/wgpu, Tokio, fonts/caches, CPAL, rubato, Opus, allocators, Cargo profiles, and feature unification.

- Keep release optimization and thin LTO. Size-oriented flags and symbol stripping primarily target executable size; they do not promise lower runtime memory. `opt-level="z"` also disables loop vectorization, a relevant CPU tradeoff for this audio client. [Cargo profile documentation](https://doc.rust-lang.org/cargo/reference/profiles.html)
- Keep the system allocator until native allocation profiling identifies a worthwhile heap target. Replacing Rust's allocator would not remove these live Metal allocations. More aggressive purging trades retained pages for future allocation work.
- Leave Tokio thread counts and stack sizes unchanged. The initial enlarged baseline had only 512 KiB resident stack pages despite 30.9 MiB reserved stack address space. A two-worker experiment remains possible, but substantial committed-RAM savings are unproven.
- Preserve bounded audio queues, reusable resampler buffers, five-minute metric history and trace coverage. Their known capacities are small relative to the measured graphics resources; reducing them would sacrifice useful behavior for little evidence-backed benefit.
- Keep GPU and software backends available. Removing a compiled fallback is not equivalent to removing a live renderer; only one is selected at runtime. Cross-platform graphics and voice-load validation remain separate follow-up work.

## Final validation

After keeping the renderer setting: all 13 tests passed, including Opus roundtrip, incoming playback-queue decoding and sample-rate conversion. Clippy with warnings denied, formatting, and the optimized release bundle build passed. The rebuilt `dist/Native Voice.app` opened successfully; Voice and Statistics were inspected. It was left disconnected on Statistics. These checks do not replace a live two-client audio test.
