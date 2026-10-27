# Windows x64 build — 2026-10-27

The latest controls/settings package is `dist/NativeVoice-Windows-x64-controls.zip`. It adds editable global keybinds, playback volume, a Settings tab, and `settings.example.json`. All 26 automated tests and Mac native settings checks pass; the user confirmed both global actions with physical keys on macOS. Windows global-hotkey/audio runtime checks remain pending. See [controls and portable settings](controls-and-settings.md).

An updated package, `dist/NativeVoice-Windows-x64-memory-stats.zip`, adds **Private RAM** beside **RAM (RSS)** in Statistics and performance mode. The former reads `PROCESS_MEMORY_COUNTERS_EX2.PrivateWorkingSetSize`; older unsupported Windows installations show `—`. This is not labeled macOS physical footprint. The added counter requires Windows 10/11 22H2 with the September 2023 update or later ([Microsoft requirements](https://learn.microsoft.com/en-us/windows/win32/api/psapi/ns-psapi-process_memory_counters_ex2)). The new Windows build and PE import checks passed; its private-memory reading has not yet been runtime-tested on Tribunal. The remainder below describes the initial package.

Built `dist/NativeVoice-Windows-x64.zip` with `NativeVoice.exe`, quick-start instructions, build metadata and dependency license notices. The executable is a Windows GUI application, so a release launch does not open a console window. For console diagnostics during development, use `cargo run --locked -- --list-devices` on Windows.

The archive includes the ten-second read-only CPU/RAM display in performance mode. It contains no TeamSpeak identity or saved password.

## Build configuration

- Rust 1.92.0, target `x86_64-pc-windows-msvc`, release profile, thin LTO.
- cargo-xwin 0.23.1, LLVM/LLD 23.1.4, CMake/Ninja on Apple Silicon macOS.
- Static Microsoft C runtime via `-C target-feature=+crt-static`.
- Static Opus 1.6.1, matching the installed Mac codec version. SSE4.1/AVX2 routines retain runtime CPU dispatch; they are not required globally.

The binding's bundled older Opus source failed to compile SSE4.1 routines with clang-cl. The delivered build instead uses the official [Opus 1.6.1 release](https://opus-codec.org/downloads/), verified against SHA-256 `6ffcb593207be92584df15b32466ed64bbec99109f007c82205f0194572411a1`. No registry source files or Cargo dependencies were patched.

The Windows SDK/CRT and generated CMake toolchain came from [cargo-xwin](https://github.com/rust-cross/cargo-xwin). Opus was configured under `target/windows-opus/build` with that toolchain, Release, Ninja, `BUILD_SHARED_LIBS=OFF`, `OPUS_STATIC_RUNTIME=ON`, `OPUS_BUILD_TESTING=OFF`, and `OPUS_BUILD_PROGRAMS=OFF`, then installed under `target/windows-opus/install`.

After preparing that native library, the successful Cargo command was:

```sh
env PATH="/opt/homebrew/opt/llvm/bin:/opt/homebrew/opt/lld/bin:$PATH" \
  CMAKE_GENERATOR=Ninja CMAKE_POLICY_VERSION_MINIMUM=3.5 \
  OPUS_NO_PKG=1 OPUS_LIB_DIR="$PWD/target/windows-opus/install" \
  XWIN_ARCH=x86_64 \
  CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS='-C target-feature=+crt-static' \
  uv tool run --from cargo-xwin==0.23.1 cargo-xwin build --release \
    --target x86_64-pc-windows-msvc --target-dir target/windows --locked
```

The executable is `target/windows/x86_64-pc-windows-msvc/release/teamspeak-native.exe` before packaging. `OPUS_LIB_DIR` points to the installation prefix containing `lib/opus.lib`, not to the `lib` directory itself.

## Validation and delivery

- Windows release compilation and link passed.
- PE inspection confirmed AMD64 and the Windows GUI subsystem. The import table contains Windows system DLLs; neither a separate Opus DLL nor Visual C++ redistributable DLL is imported. The default wgpu Fxc shader compiler uses system components.
- ZIP integrity was checked, and its embedded executable matched the built executable byte-for-byte.
- The updated Mac code passed all 16 tests, Clippy with warnings denied and formatting. Native preview inspection confirmed the CPU reading changes from “measuring…” to a value and that CPU/RAM carry the ten-second refresh label.
- Windows execution, device detection and live voice have not yet been tested on Tribunal. SSH was unavailable; the remote Downloads directory could not be inspected directly.

Taildrop confirmed successful transfer to Tribunal at 02:02:51 Europe/Berlin. Current Windows Tailscale versions place received files in the user's Downloads directory. See [Taildrop's Windows receiving behavior](https://tailscale.com/docs/features/taildrop?tab=windows) and the local [transfer acknowledgement](windows-transfer.json).

Archive: 8,395,907 bytes; SHA-256 `0f6b0a12a268ac092c486565c69ea070fe666eb7ef62311ec9d20063d61c31e9`.

Executable: 21,713,408 bytes; SHA-256 `28aa62812194706e900b75fa1e8664222737b142053fece0a8011ff34dd32463`.
