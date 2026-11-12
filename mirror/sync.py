#!/usr/bin/env python3
"""Copies the latest Flexspeak release from GitHub to the update mirror.

Runs from flexspeak-mirror.timer. Each run asks GitHub for the release marked latest. A
release the mirror doesn't have yet is downloaded, checked against both SHA256SUMS.txt and
GitHub's own digest of every file, and only then published as <root>/<tag>/ with one rename.
Older releases beyond the newest few are removed. Finally each published file is fetched once
through the public address, which warms Cloudflare's cache and checks the whole path.

The app checks the files against GitHub again, so this mirror can fail an update but never
change one. Standard library only.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

REPOSITORY = "zirkumflex-group/flexspeak"
LATEST = f"https://api.github.com/repos/{REPOSITORY}/releases/latest"
# What the app downloads (src/updater.rs). Nothing else is mirrored.
ASSETS = {
    "Flexspeak-windows-x64.zip",
    "Flexspeak-macos-arm64.dmg",
    "Flexspeak-linux-x64.pkg.tar.zst",
    "Flexspeak-linux-x64.tar.gz",
}
CHECKSUMS = "SHA256SUMS.txt"
TAG = re.compile(r"v(\d+)\.(\d+)\.(\d+)-build\.(\d+)")
KEEP = 3
PART = 4 * 1024 * 1024
CONNECTIONS = 8
USER_AGENT = "flexspeak-update-mirror/1"


class SyncError(RuntimeError):
    pass


def log(message: str) -> None:
    print(message, flush=True)


def request(url: str, headers: dict[str, str] | None = None, timeout: float = 60):
    return urllib.request.urlopen(
        urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})}),
        timeout=timeout,
    )


def tag_key(tag: str) -> tuple[int, ...] | None:
    match = TAG.fullmatch(tag)
    return tuple(int(part) for part in match.groups()) if match else None


def checksums(text: str) -> dict[str, str]:
    """`<sha256>  <name>` lines; a name listed twice is unusable."""
    sums: dict[str, str] = {}
    seen: set[str] = set()
    for line in text.splitlines():
        parts = line.strip().split(maxsplit=1)
        if len(parts) != 2:
            continue
        digest, name = parts[0].lower(), parts[1].lstrip("*")
        if name in seen:
            sums.pop(name, None)
            continue
        seen.add(name)
        if re.fullmatch(r"[0-9a-f]{64}", digest):
            sums[name] = digest
    return sums


def plan(release: dict) -> tuple[str, str, list[dict]]:
    """The tag, the checksums URL and the assets to mirror, with their expected digests."""
    tag = release.get("tag_name", "")
    if release.get("draft") or release.get("prerelease") or tag_key(tag) is None:
        raise SyncError(f"The latest release {tag!r} is not a published build tag.")
    assets = release.get("assets") or []
    sums = [asset for asset in assets if asset.get("name") == CHECKSUMS]
    if len(sums) != 1:
        raise SyncError(f"{tag} has no single {CHECKSUMS}.")
    wanted = []
    for asset in assets:
        if asset.get("name") not in ASSETS:
            continue
        if asset.get("state") != "uploaded":
            raise SyncError(f"{tag}: {asset['name']} is still uploading.")
        digest = str(asset.get("digest") or "")
        if not re.fullmatch(r"sha256:[0-9a-fA-F]{64}", digest):
            raise SyncError(f"{tag}: GitHub reports no SHA-256 for {asset['name']}.")
        url = str(asset.get("browser_download_url", ""))
        if not url.startswith("https://github.com/"):
            raise SyncError(f"{tag}: unexpected download address for {asset['name']}.")
        wanted.append({"name": asset["name"], "url": url, "size": int(asset["size"]),
                       "digest": digest.split(":", 1)[1].lower()})
    if len({asset["name"] for asset in wanted}) != len(wanted):
        raise SyncError(f"{tag} lists a package twice.")
    if not wanted:
        raise SyncError(f"{tag} has no packages the app downloads.")
    return tag, sums[0]["browser_download_url"], wanted


def fetch_part(url: str, path: Path, start: int, end: int) -> None:
    for attempt in range(3):
        try:
            with request(url, {"Range": f"bytes={start}-{end - 1}"}) as response:
                if response.status != 206 or not str(response.headers.get("Content-Range", "")).startswith(
                        f"bytes {start}-{end - 1}/"):
                    raise SyncError(f"{url} ignored a range request.")
                data = response.read(end - start + 1)
            if len(data) != end - start:
                raise SyncError(f"A part of {url} was {len(data)} of {end - start} bytes.")
            with path.open("r+b") as file:
                file.seek(start)
                file.write(data)
            return
        except (OSError, urllib.error.URLError) as error:
            if attempt == 2:
                raise SyncError(f"Cannot download {url}: {error}") from error
            time.sleep(2 * (attempt + 1))


def download(url: str, size: int, path: Path) -> None:
    """Fetches `size` bytes of `url` into `path` in parallel parts."""
    with path.open("wb") as file:
        file.truncate(size)
    parts = [(start, min(start + PART, size)) for start in range(0, size, PART)]
    with concurrent.futures.ThreadPoolExecutor(CONNECTIONS) as pool:
        for future in [pool.submit(fetch_part, url, path, start, end) for start, end in parts]:
            future.result()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def mirror(release: dict, root: Path, fetch=download, read=None) -> str | None:
    """Publishes `release` under `root`; returns its tag when something new was published."""
    tag, sums_url, assets = plan(release)
    target = root / tag
    if target.is_dir():
        return None
    if read is None:
        def read(url: str) -> str:
            with request(url) as response:
                return response.read(1024 * 1024).decode()
    sums = checksums(read(sums_url))
    for asset in assets:
        listed = sums.get(asset["name"])
        if listed != asset["digest"]:
            raise SyncError(f"{tag}: {CHECKSUMS} and GitHub disagree about {asset['name']}; nothing was published.")
    staging = Path(tempfile.mkdtemp(prefix=f".{tag}.", dir=root))
    try:
        for asset in assets:
            path = staging / asset["name"]
            fetch(asset["url"], asset["size"], path)
            if path.stat().st_size != asset["size"]:
                raise SyncError(f"{tag}: {asset['name']} has the wrong size.")
            actual = sha256(path)
            if actual != asset["digest"]:
                raise SyncError(f"{tag}: {asset['name']} failed verification (SHA-256 {actual}).")
            path.chmod(0o644)
        staging.chmod(0o755)
        os.rename(staging, target)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    return tag


def prune(root: Path, keep: int = KEEP) -> list[str]:
    """Removes all but the newest `keep` releases, and staging left by an interrupted run."""
    removed = []
    for entry in root.iterdir():
        if entry.name.startswith(".") and entry.is_dir() and time.time() - entry.stat().st_mtime > 3600:
            shutil.rmtree(entry, ignore_errors=True)
    tags = sorted((entry.name for entry in root.iterdir() if entry.is_dir() and tag_key(entry.name)),
                  key=tag_key, reverse=True)
    for tag in tags[keep:]:
        shutil.rmtree(root / tag)
        removed.append(tag)
    return removed


def warm(public: str, root: Path, tag: str) -> None:
    """Fetches each published file through the public address and checks it."""
    for path in sorted((root / tag).iterdir()):
        url = f"{public}/{tag}/{path.name}"
        digest = hashlib.sha256()
        started = time.monotonic()
        with request(url, timeout=120) as response:
            for chunk in iter(lambda: response.read(1024 * 1024), b""):
                digest.update(chunk)
            cache = response.headers.get("CF-Cache-Status", "-")
        if digest.hexdigest() != sha256(path):
            raise SyncError(f"{url} does not serve the published file.")
        log(f"checked {url} ({time.monotonic() - started:.1f} s, cache {cache})")


def latest(state: Path) -> dict | None:
    """The latest release, or None when it hasn't changed since the last run (ETag)."""
    etag_path = state / "latest.etag"
    headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28"}
    if etag_path.exists():
        headers["If-None-Match"] = etag_path.read_text().strip()
    try:
        with request(LATEST, headers) as response:
            body = response.read()
            etag = response.headers.get("ETag")
    except urllib.error.HTTPError as error:
        if error.code == 304:
            return None
        raise SyncError(f"GitHub answered {error.code} for the latest release.") from error
    release = json.loads(body)
    release["_etag"] = etag
    return release


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path("/srv/flexspeak-updates/public/flexspeak"))
    parser.add_argument("--state", type=Path, default=Path(os.environ.get("STATE_DIRECTORY", "/var/lib/flexspeak-mirror")))
    parser.add_argument("--public", default="https://updates.zirkumflex.io/flexspeak")
    args = parser.parse_args()
    args.root.mkdir(parents=True, exist_ok=True)
    try:
        release = latest(args.state)
        if release is None:
            return 0
        published = mirror(release, args.root)
        if published:
            log(f"published {published}")
        for tag in prune(args.root):
            log(f"removed {tag}")
        # Checked until it passes once: the ETag is remembered only after that, so a failed
        # run (sync or check) tries again.
        warm(args.public, args.root, plan(release)[0])
        if release.get("_etag"):
            (args.state / "latest.etag").write_text(release["_etag"])
    except (SyncError, OSError, urllib.error.URLError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
