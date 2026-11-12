# Flexspeak update mirror

GitHub's release CDN serves each connection at a few hundred KiB/s. The Beelink serves the same
files over a 300 Mbit/s upload, behind Cloudflare, so the app downloads updates from here first and
from GitHub when this mirror can't help.

| What | Where |
| --- | --- |
| Public URL | `https://updates.zirkumflex.io/flexspeak/<tag>/<asset>` (Cloudflare-proxied, origin-locked) |
| Files | `/srv/flexspeak-updates/public/flexspeak/<tag>/`, owner `flexspeak-mirror`; the newest 3 releases |
| Sync | `flexspeak-mirror.timer` every 2 min → `flexspeak-mirror.service` (`/srv/flexspeak-updates/sync.py`, root-owned, runs as `flexspeak-mirror`) |
| State | `/var/lib/flexspeak-mirror/latest.etag` |
| Caddy | `/etc/caddy/sites-enabled/flexspeak-updates.caddy` |
| DNS | A `updates.zirkumflex.io` proxied, kept current by `beeroot-ddns` |
| Source | `mirror/` in `zirkumflex-group/flexspeak` |

## Why it can't ship a bad build

- The app gets the release, `SHA256SUMS.txt` and GitHub's per-file digest from GitHub, and only the
  bytes from here. A file that doesn't match is discarded and downloaded from GitHub instead
  (`src/updater.rs`, `download_into`). The worst a broken or compromised mirror can do is be slow.
- The sync publishes a release only after every file matches both `SHA256SUMS.txt` and GitHub's
  digest, and only with one rename of a complete directory, so a half-copied release is never served.
- No credentials anywhere: the sync reads public GitHub endpoints and nothing pushes to the box. CI,
  including the Windows runner VM, has no access to it.
- The service can write only `/srv/flexspeak-updates/public` and its state directory, and can't change
  its own code. Caddy serves only paths shaped like `/flexspeak/<build tag>/Flexspeak-*`, no listings.

## Everyday commands

```sh
systemctl list-timers flexspeak-mirror.timer
journalctl -u flexspeak-mirror.service -n 50
sudo systemctl start flexspeak-mirror.service       # sync now
ls /srv/flexspeak-updates/public/flexspeak/
curl -fsSI https://updates.zirkumflex.io/flexspeak/<tag>/Flexspeak-windows-x64.zip
```

A run with nothing new prints nothing. A new release logs `published <tag>`, `removed <old tag>`,
then `checked <url> (… s, cache …)` for each file fetched back through Cloudflare.

## Install or update

From a checkout of this directory on the Beelink:

```sh
python3 -B -m unittest -v test_sync
sudo ./install.sh
```

It runs the tests, keeps the previous files under `/srv/backups/host-updates/flexspeak-mirror-install-<UTC>`,
installs the user, files, units and Caddy site, validates Caddy (and restores the old site file if
that fails), reloads Caddy and enables the timer.

DNS, once: `~/codex/cloudflare-dns upsert` for `updates.zirkumflex.io` (A, proxied, the WAN IPv4),
then add the name to `records` in `/etc/beeroot-ddns.json` and in the beeroot-ddns `config.json`.

## Turning it off

- For everyone, from the app side: change `MIRROR` in `src/updater.rs`. Builds already out keep
  trying it, and fall back to GitHub within seconds once it's gone.
- On the box: `sudo systemctl disable --now flexspeak-mirror.timer`, remove the Caddy site file and
  reload Caddy. The app then downloads from GitHub.
- For one installation: `FLEXSPEAK_UPDATE_MIRROR=` (empty) in the environment.
