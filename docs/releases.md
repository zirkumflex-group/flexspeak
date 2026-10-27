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

The cask installs **Native Voice.app**, the client's current application name. Apple Silicon and Intel use separate DMGs with verified SHA-256 checksums. macOS 14.2 or later is required. The token stays in your environment, not in the cask or download URL; use a token with read-only Contents access to `flexspeak` and `homebrew-flexspeak` if configuring a dedicated token.

To update, authenticate as above if your shell no longer has the token, then:

```sh
brew update
brew upgrade --cask zirkumflex-group/flexspeak/flexspeak
```

Close the app before upgrading. Saved settings and TeamSpeak identity remain in the existing application-data directory. The cask does not delete that directory on uninstall.

The macOS build is ad-hoc signed, not Developer ID signed or notarized. macOS may require explicit approval in Privacy & Security. Windows packages are not Authenticode-signed. Code signing requires separate signing credentials; the automation does not disable Gatekeeper or quarantine.

## Windows

Download `Flexspeak-windows-x64.zip` from the [latest private release](https://github.com/zirkumflex-group/flexspeak/releases/latest), extract it, and open `Flexspeak.exe`. A standalone `Flexspeak-windows-x64.exe` is also provided with a separate dependency-notices file. The ZIP includes settings documentation, a portable settings template, build metadata and dependency notices.

## Automation

- PRs run formatting, Clippy and tests once on Linux. Dependencies are cached, old runs of the same PR are cancelled, and documentation-only PRs skip checks. PR code receives a read-only token and no publishing secret. Fork PR workflow execution requires owner approval.
- Pushes to `main` run those checks, then parallel native Windows x64, macOS arm64 and macOS x64 release builds. Static Opus 1.6.1 is downloaded with a pinned checksum and built locally on each runner. There is no Homebrew runtime dependency.
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

`python scripts/release.py build` builds and packages on native macOS or Windows. It verifies the Opus download, smoke-tests `--help` without opening audio or connecting, verifies Mac ad-hoc signatures and DMG integrity, and rejects Homebrew-linked Mac executables. Hosted release builds are still not a substitute for a two-client voice test.

The private cask's download strategy uses Homebrew's `CurlDownloadStrategy` hooks to attach authentication only at download time. If Homebrew changes those hooks, update `scripts/flexspeak.rb.in` in the source repository; the next release regenerates the tap.
