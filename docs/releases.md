# Flexspeak releases and Homebrew

Every push to `main` in [flexspeak](https://github.com/zirkumflex-group/flexspeak) builds a release, unless it only changes documentation. The repository and its releases are public. This Homebrew tap is private: your GitHub account needs read access to it.

Not every platform is necessarily built: `.github/platforms.json` turns each of Windows, macOS and Linux on or off (see [Platforms](#platforms)). When a platform is off, new releases have no package for it, the Homebrew cask stays on the last macOS build, and installed copies on that platform see no update until it is back.

## Install on macOS

Install Homebrew and GitHub CLI (`brew install gh`), then let Git reach the private tap:

```sh
gh auth login
gh auth setup-git
brew tap zirkumflex-group/flexspeak
brew install --cask zirkumflex-group/flexspeak/flexspeak
```

The cask installs **Flexspeak.app** on Apple Silicon Macs using a DMG with a verified SHA-256 checksum. The same DMG opens in Finder with Flexspeak on the left, an arrow, and Applications on the right: drag the app into Applications. Both the classic Finder/Dock icon and the newer Icon Composer asset are bundled. macOS 14.2 or later is required. The DMG downloads from the public release; only the tap itself needs your GitHub login.

To update:

```sh
brew update
brew upgrade --cask zirkumflex-group/flexspeak/flexspeak
```

Close the app before upgrading. Saved settings and TeamSpeak identity remain in the existing application-data directory. The cask does not delete that directory on uninstall.

The macOS build is ad-hoc signed, not Developer ID signed or notarized. macOS may require explicit approval in Privacy & Security. Windows packages are not Authenticode-signed. Code signing requires separate signing credentials; the automation does not disable Gatekeeper or quarantine.

## Windows

Download `Flexspeak-windows-x64.zip` from the [latest release](https://github.com/zirkumflex-group/flexspeak/releases/latest), extract it, and open `Flexspeak.exe`. A standalone `Flexspeak-windows-x64.exe` is also provided with a separate dependency-notices file. The ZIP includes settings documentation, a portable settings template, build metadata and dependency notices.

## Linux / Omarchy (x86_64)

Download `Flexspeak-linux-x64.pkg.tar.zst` from the latest release and install it on Omarchy or Arch Linux:

```sh
sudo pacman -U ./Flexspeak-linux-x64.pkg.tar.zst
```

This installs `flexspeak` and an application-launcher entry, and lets pacman resolve the runtime dependencies. Install a newer downloaded package with the same command. Remove it with `sudo pacman -R flexspeak`; settings and identity in your home directory are retained. The package is not published to an Arch repository or AUR, so `pacman -Syu` alone does not fetch new Flexspeak releases.

`Flexspeak-linux-x64.tar.gz` contains the same executable for other x86_64 Linux distributions. It requires glibc 2.39 or newer plus ALSA, Wayland/X11 and xkbcommon runtime libraries; it is not a fully static or universally portable Linux binary. Opus is linked statically. A working system graphics driver is required for accelerated rendering.

Omarchy uses Wayland. Native Wayland global mute/deafen shortcuts remain unsupported by the app; use its buttons. The release pipeline installs the Arch package in a clean container and verifies `flexspeak --help`. GUI/audio behavior on an actual Omarchy desktop still needs runtime validation.

## Installing from a download

A release that runs from somewhere temporary offers to install itself at launch, so in-app updates replace a copy that stays put. **Install & Reopen** copies the running build, registers it and reopens it from there (returning to the same session, as an update does); **Not Now** asks again next launch, **Don't Ask Again** stops asking. **Settings → Updates → Installation** shows where Flexspeak runs from and installs it later. Settings and the TeamSpeak identity live in the per-user data directory either way.

| Platform | Installed copy | Also |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\Programs\Flexspeak\Flexspeak.exe` (no administrator rights) | Start menu shortcut and an entry in Installed apps, whose Uninstall runs `Flexspeak.exe --uninstall` (removes the program, keeps settings) |
| macOS | `/Applications/Flexspeak.app`, or `~/Applications` when `/Applications` isn't writable | The installed copy is verified with `codesign` and loses the download quarantine, as if dragged there, so it isn't translocated |
| Linux tarball | `~/.local/bin/flexspeak` | A launcher entry in `~/.local/share/applications` |

Homebrew and the Arch package already install the app and are never offered this. Development builds without a build number never offer it either.

## In-app updates

Release builds check GitHub Releases five seconds after launch and every six hours. The newest build is the release GitHub marks **latest**. The workflow sets that mark once every package is published, before it updates the Homebrew tap, and never moves it backwards. That release is an update when its `(version, build)` from `v{version}-build.{N}` is greater than the running build's, which comes from `CARGO_PKG_VERSION` and `NATIVE_VOICE_BUILD_NUMBER` at compile time. Local `cargo` builds have no build number and never update themselves.

The app reads releases anonymously, which works because the repository is public. If GitHub can't be reached, the check fails quietly and Settings → Updates shows why. A release without this platform's package (its platform is turned off) is not an update, and not an error either: the app stays on its build.

| Installation | Asset | Install on Restart |
| --- | --- | --- |
| Windows x64 (`Flexspeak.exe`, from the ZIP or the standalone exe) | `Flexspeak-windows-x64.exe` | The running `.exe` is renamed to `.exe.old`, the new one takes its name, and a hidden PowerShell waits for the old process to exit before starting it. The next launch removes `.old`. |
| macOS arm64 (`Flexspeak.app`) | `Flexspeak-macos-arm64.dmg` | At download time the DMG is mounted read-only, `Flexspeak.app` is copied out, and its signature (`codesign --verify --strict`) and bundle identifier are checked. On Restart it is copied beside the installed app, checked again, and swapped in with two renames on one volume (rolled back if the second fails). The app reopens with `open` after the old process exits. A translocated app (opened from a quarantined download) is asked to move to Applications first. |
| Arch/Omarchy package (`/usr/bin/flexspeak`) | `Flexspeak-linux-x64.pkg.tar.zst` | `pkexec pacman -U --noconfirm`, then relaunch. |
| Linux tarball (anywhere else) | `Flexspeak-linux-x64.tar.gz` | `flexspeak` is unpacked and renamed over the running executable, then relaunched. |

Every download is checked against `SHA256SUMS.txt` from the same release, and against GitHub's own asset digest when the API reports one. A mismatch, a short download or a disagreement between the two discards the file. Verified downloads wait in the cache directory (`…/TeamSpeakNative/updates/<tag>`, overridable with `FLEXSPEAK_UPDATES_DIR`) and are found again after a restart. Older ones are removed. This verifies integrity against what was published. It is not code signing: builds remain ad-hoc signed and not notarized.

With **Download updates automatically** on (the default), a found update downloads in the background and the notice offers **Restart**. With it off, the notice offers **Download**, shows a progress ring with size and speed while downloading, then offers **Restart**. Installing always waits for Restart. Before the update exits, Flexspeak writes `resume.json` beside `settings.json` (owner-only, deleted when read, ignored after ten minutes). The new build then returns to the same page, reconnects to the saved server with the session's server password, rejoins the channel (with its password if one was used) and restores mute and deafen.

After an in-place update, Homebrew's record still names the old cask version. `brew upgrade` later reinstalls the current build, which is harmless.

## Platforms

`.github/platforms.json` turns each platform on or off for both PR checks and releases:

```json
{
  "windows": true,
  "macos": true,
  "linux": false
}
```

Every platform must be listed, as `true` or `false`, and at least one must be on; a typo fails the workflows instead of silently dropping a platform. The runners, caches and packages of each platform are in `PLATFORMS` in `scripts/release.py`. A platform that is off is not checked on PRs, not built, and not required by `release.py checksums`; with macOS off the Homebrew tap is left alone. Its code still exists but is no longer compiled anywhere, so it can drift: before turning it back on, run the **Checks** workflow manually with **Also check the platforms .github/platforms.json turns off** ticked, and fix what that shows in the same PR that turns it on.

Turn a platform off only after a release whose app treats a missing package as "no update" has reached that platform's installs; older builds report an error on every update check instead.

## Automation

- PRs run Clippy, tests and the release-script tests on every platform that is on, because Windows and macOS compile code Linux never sees. Formatting is checked once, on a Linux runner that needs no system packages, whichever platforms are on. A final `Checks passed` job succeeds only if all of them did; it is the one status check `main` requires, whichever platforms are on, so documentation-only PRs run the checks too. Dependencies are fetched first through the git CLI with a time limit. The same checks run again on `main` after every merge. Those runs do not hold up the release; they save the build caches that PRs start from (PR runs never save one), and they show a merge that only broke together with another. Old runs of the same PR are cancelled. PR code receives a read-only token and no publishing secret. Fork PR workflow execution requires owner approval.
- Pushes to `main` build a Windows x64, a macOS arm64 and a Linux x64 release, each while it is on. Alongside the builds, the tests run once more on one platform (Linux while it is on, otherwise the first one that is), from the cache the checks saved, as a safety net for direct pushes and for two merges that each passed alone; the platforms are not checked a second time. Each build fetches its platform's dependencies first, then builds offline. The Linux binary is packaged as both a tarball and an Arch/Omarchy package without compiling it twice. Static Opus (version and checksum in `scripts/opus.json`) is downloaded with a pinned checksum and built locally on each runner. There is no Homebrew runtime dependency.
- A completed build gets a unique `v0.1.0-build.N` tag (the base version comes from `Cargo.toml`). All packages upload to a draft before it is published. Packages include SHA-256 checksums. Failed builds leave the previous Homebrew version intact.
- The publishing job runs only on `main`, in the `release` environment, which admits `main` alone. It updates `zirkumflex-group/homebrew-flexspeak` with the `HOMEBREW_TAP_DEPLOY_KEY` deploy key, stored as a secret of that environment, so no other branch or job can read it. No generated commits are pushed into the source repository, so publishing cannot loop.
- A push that only changes documentation releases nothing: `docs/` and Markdown files do not count, except the ones a release carries (`README.md` in every package, `CHANGELOG.md` in the app, `docs/releases.md` as the tap's README, `assets/webrtc/LICENSE.md` in the dependency notices). A Markdown file that starts to ship must be added to that list in `release.yml`; a test checks the ones `scripts/release.py` reads. A manual run always releases.
- Builds of several pushes run side by side; only publishing takes turns. Waiting publish jobs queue instead of cancelling each other (GitHub permits up to 100). A quick build can publish before an older, slower one, so the publish job never moves “latest” or the tap back to a lower build number, also when an old run is rerun. Published assets are reused on rerun instead of overwritten.
- A build may start before the push before it has been released. Its in-app changelog then lists that push's `New:`/`Fixed:`/`Changed:` lines under its own build, since it contains those changes; the same goes for a push whose build failed.
- Homebrew versions include the build number, so every successful push is an upgrade even while the Cargo version stays unchanged. The release becomes “latest” before the tap updates, so a tap failure never holds back in-app updates; rerun the failed job to bring Homebrew up to date.
- Build numbers are the `Release` workflow's run number. Renaming or recreating `.github/workflows/release.yml` restarts that count at 1, which would make every new build look older than the last and collide with existing tags, so keep the file in place (or carry the count over before such a change).

The `Release` workflow also supports manual dispatch. From `main` it releases; from any other branch it only builds the packages of the platforms that are on and keeps them as workflow artifacts for seven days, for testing. GitHub Release assets remain available.

## Local validation

```sh
python3 -m unittest discover -s scripts -p 'test_*.py'
actionlint
```

`python scripts/release.py build` builds and packages on native macOS, Windows or Linux. Mac packaging also needs dmgbuild in a virtual environment (`pip install --require-hashes -r scripts/dmg-requirements.txt`); set `FLEXSPEAK_DMGBUILD` to that environment’s `bin/dmgbuild`. CI installs it automatically. `scripts/dmg-settings.py` records the Finder layout without UI automation; regenerate its background with `swift scripts/dmg-background.swift`. The build fails if the app lacks `Flexspeak.icns`; after changing the Icon Composer icon, regenerate that fallback as described in `scripts/render-icns.swift`. Linux packaging also requires Docker (the Arch container is pinned by digest in `scripts/release.py`) and `desktop-file-validate`. It verifies the Opus download, smoke-tests `--help` without opening audio or connecting, verifies Mac ad-hoc signatures and DMG integrity, and rejects Homebrew-linked Mac executables. Hosted release builds are still not a substitute for a two-client voice test.

The cask (`scripts/flexspeak.rb.in`) downloads the DMG from the public release by its tag; each release regenerates it in the tap.
