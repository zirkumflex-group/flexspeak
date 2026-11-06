# Flexspeak releases and Homebrew

Every push to `main` in [flexspeak](https://github.com/zirkumflex-group/flexspeak) queues a release. Releases and this Homebrew tap are private: your GitHub account needs read access to both repositories.

## Install on macOS

Install Homebrew and GitHub CLI (`brew install gh`), then authenticate Git and release downloads:

```sh
gh auth login
gh auth setup-git
export HOMEBREW_GITHUB_API_TOKEN="$(gh auth token)"
brew tap zirkumflex-group/flexspeak
brew install --cask zirkumflex-group/flexspeak/flexspeak
```

The cask installs **Flexspeak.app** on Apple Silicon Macs using a DMG with a verified SHA-256 checksum. macOS 14.2 or later is required. The token stays in your environment, not in the cask or download URL; use a token with read-only Contents access to `flexspeak` and `homebrew-flexspeak` if configuring a dedicated token.

To update, authenticate as above if your shell no longer has the token, then:

```sh
brew update
brew upgrade --cask zirkumflex-group/flexspeak/flexspeak
```

Close the app before upgrading. Saved settings and TeamSpeak identity remain in the existing application-data directory. The cask does not delete that directory on uninstall.

The macOS build is ad-hoc signed, not Developer ID signed or notarized. macOS may require explicit approval in Privacy & Security. Windows packages are not Authenticode-signed. Code signing requires separate signing credentials; the automation does not disable Gatekeeper or quarantine.

## Windows

Download `Flexspeak-windows-x64.zip` from the [latest private release](https://github.com/zirkumflex-group/flexspeak/releases/latest), extract it, and open `Flexspeak.exe`. A standalone `Flexspeak-windows-x64.exe` is also provided with a separate dependency-notices file. The ZIP includes settings documentation, a portable settings template, build metadata and dependency notices.

## Linux / Omarchy (x86_64)

Download `Flexspeak-linux-x64.pkg.tar.zst` from the latest private release and install it on Omarchy or Arch Linux:

```sh
sudo pacman -U ./Flexspeak-linux-x64.pkg.tar.zst
```

This installs `flexspeak` and an application-launcher entry, and lets pacman resolve the runtime dependencies. Install a newer downloaded package with the same command. Remove it with `sudo pacman -R flexspeak`; settings and identity in your home directory are retained. The package is not published to an Arch repository or AUR, so `pacman -Syu` alone does not fetch new Flexspeak releases.

`Flexspeak-linux-x64.tar.gz` contains the same executable for other x86_64 Linux distributions. It requires glibc 2.39 or newer plus ALSA, Wayland/X11 and xkbcommon runtime libraries; it is not a fully static or universally portable Linux binary. Opus is linked statically. A working system graphics driver is required for accelerated rendering.

Omarchy uses Wayland. Native Wayland global mute/deafen shortcuts remain unsupported by the app; use its buttons. The release pipeline installs the Arch package in a clean container and verifies `flexspeak --help`. GUI/audio behavior on an actual Omarchy desktop still needs runtime validation.

## In-app updates

Release builds check GitHub Releases five seconds after launch and every six hours. The newest build is the release GitHub marks **latest**. The workflow sets that mark only after every package and the Homebrew tap are published, and never moves it backwards. That release is an update when its `(version, build)` from `v{version}-build.{N}` is greater than the running build's, which comes from `CARGO_PKG_VERSION` and `NATIVE_VOICE_BUILD_NUMBER` at compile time. Local `cargo` builds have no build number and never update themselves.

**The app reads releases anonymously, so this only works once the repository (or at least its releases) is public.** While the repository is private, GitHub answers 404. The check fails quietly, and Settings → Updates shows why.

| Installation | Asset | Install on Restart |
| --- | --- | --- |
| Windows x64 (`Flexspeak.exe`, from the ZIP or the standalone exe) | `Flexspeak-windows-x64.exe` | The running `.exe` is renamed to `.exe.old`, the new one takes its name, and a hidden PowerShell waits for the old process to exit before starting it. The next launch removes `.old`. |
| macOS arm64 (`Flexspeak.app`) | `Flexspeak-macos-arm64.dmg` | At download time the DMG is mounted read-only, `Flexspeak.app` is copied out, and its signature (`codesign --verify --strict`) and bundle identifier are checked. On Restart it is copied beside the installed app, checked again, and swapped in with two renames on one volume (rolled back if the second fails). The app reopens with `open` after the old process exits. A translocated app (opened from a quarantined download) is asked to move to Applications first. |
| Arch/Omarchy package (`/usr/bin/flexspeak`) | `Flexspeak-linux-x64.pkg.tar.zst` | `pkexec pacman -U --noconfirm`, then relaunch. |
| Linux tarball (anywhere else) | `Flexspeak-linux-x64.tar.gz` | `flexspeak` is unpacked and renamed over the running executable, then relaunched. |

Every download is checked against `SHA256SUMS.txt` from the same release, and against GitHub's own asset digest when the API reports one. A mismatch, a short download or a disagreement between the two discards the file. Verified downloads wait in the cache directory (`…/TeamSpeakNative/updates/<tag>`, overridable with `FLEXSPEAK_UPDATES_DIR`) and are found again after a restart. Older ones are removed. This verifies integrity against what was published. It is not code signing: builds remain ad-hoc signed and not notarized.

With **Download updates automatically** on (the default), a found update downloads in the background and the notice offers **Restart**. With it off, the notice offers **Download**, shows a progress ring with size and speed while downloading, then offers **Restart**. Installing always waits for Restart. Before the update exits, Flexspeak writes `resume.json` beside `settings.json` (owner-only, deleted when read, ignored after ten minutes). The new build then returns to the same page, reconnects to the saved server with the session's server password, rejoins the channel (with its password if one was used) and restores mute and deafen.

After an in-place update, Homebrew's record still names the old cask version. `brew upgrade` later reinstalls the current build, which is harmless.

## Automation

- PRs run formatting, Clippy and tests once on Linux. Dependencies are cached, old runs of the same PR are cancelled, and documentation-only PRs skip checks. PR code receives a read-only token and no publishing secret. Fork PR workflow execution requires owner approval.
- Pushes to `main` run those checks, then one Windows x64, one macOS arm64 and one Linux x64 release build in parallel. The Linux binary is packaged as both a tarball and an Arch/Omarchy package without compiling it twice. Static Opus 1.6.1 is downloaded with a pinned checksum and built locally on each runner. There is no Homebrew runtime dependency.
- A completed build gets a unique `v0.1.0-build.N` tag (the base version comes from `Cargo.toml`). All packages upload to a draft before it is published. Packages include SHA-256 checksums. Failed builds leave the previous Homebrew version intact.
- The publishing job updates `zirkumflex-group/homebrew-flexspeak` using the repository-scoped `HOMEBREW_TAP_DEPLOY_KEY` secret. Its private key is only supplied to the main-branch publishing job. No generated commits are pushed into the source repository, so publishing cannot loop.
- Release runs queue instead of cancelling each other (GitHub permits up to 100 pending runs). A guard prevents rerunning an older build from downgrading the tap. Published assets are reused on rerun instead of overwritten.
- Homebrew versions include the build number, so every successful push is an upgrade even while the Cargo version stays unchanged. The tap updates before the release becomes “latest”. If tap publishing fails, rerun the failed job.

The `Release` workflow also supports manual dispatch from `main`. Artifact uploads are retained for seven days; GitHub Release assets remain available. Private hosted runners consume the organization's included Actions minutes and any configured paid allowance.

## Local validation

```sh
python3 -m unittest discover -s scripts -p 'test_*.py'
actionlint
```

`python scripts/release.py build` builds and packages on native macOS, Windows or Linux. Linux packaging also requires Docker and `desktop-file-validate`. It verifies the Opus download, smoke-tests `--help` without opening audio or connecting, verifies Mac ad-hoc signatures and DMG integrity, and rejects Homebrew-linked Mac executables. Hosted release builds are still not a substitute for a two-client voice test.

The private cask's download strategy uses Homebrew's `CurlDownloadStrategy` hooks to attach authentication only at download time. If Homebrew changes those hooks, update `scripts/flexspeak.rb.in` in the source repository; the next release regenerates the tap.
