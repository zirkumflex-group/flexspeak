import hashlib
import tempfile
import unittest
from pathlib import Path

import sync

ZIP = "Flexspeak-windows-x64.zip"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def release(tag="v0.1.0-build.9", payload=b"zip bytes", **overrides):
    asset = {"name": ZIP, "state": "uploaded", "size": len(payload),
             "digest": f"sha256:{digest(payload)}",
             "browser_download_url": f"https://github.com/x/releases/download/{tag}/{ZIP}"}
    asset.update(overrides)
    return {"tag_name": tag, "draft": False, "prerelease": False, "assets": [
        asset,
        {"name": "Flexspeak-windows-x64.exe", "state": "uploaded", "size": 1, "digest": None,
         "browser_download_url": "https://github.com/x/exe"},
        {"name": sync.CHECKSUMS, "state": "uploaded", "size": 1,
         "browser_download_url": "https://github.com/x/sums"},
    ]}


class MirrorTest(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)

    def run_mirror(self, release_json, served=b"zip bytes", listed=b"zip bytes"):
        def fetch(url, size, path):
            path.write_bytes(served)
        return sync.mirror(release_json, self.root, fetch=fetch,
                           read=lambda url: f"{digest(listed)}  {ZIP}\n")

    def test_a_verified_release_is_published_whole_and_only_once(self):
        self.assertEqual(self.run_mirror(release()), "v0.1.0-build.9")
        self.assertEqual((self.root / "v0.1.0-build.9" / ZIP).read_bytes(), b"zip bytes")
        self.assertEqual([p.name for p in (self.root / "v0.1.0-build.9").iterdir()], [ZIP],
                         "only what the app downloads")
        self.assertIsNone(self.run_mirror(release()), "already mirrored")

    def test_bad_bytes_or_disagreeing_checksums_publish_nothing(self):
        with self.assertRaisesRegex(sync.SyncError, "failed verification"):
            self.run_mirror(release(), served=b"zip bytez")
        with self.assertRaisesRegex(sync.SyncError, "disagree"):
            self.run_mirror(release(), listed=b"other")
        self.assertEqual(list(self.root.iterdir()), [], "no release and no staging left")

    def test_only_published_build_tags_with_digests_are_mirrored(self):
        for bad in [release(tag="../etc"), release(tag="nightly"), {**release(), "draft": True},
                    {**release(), "prerelease": True}, release(digest=None), release(state="new"),
                    release(browser_download_url="https://example.com/x.zip")]:
            with self.assertRaises(sync.SyncError):
                sync.plan(bad)

    def test_the_newest_releases_are_kept(self):
        for number in [3, 10, 9, 1]:
            (self.root / f"v0.1.0-build.{number}").mkdir()
        (self.root / "unrelated").mkdir()
        self.assertEqual(sorted(sync.prune(self.root)), ["v0.1.0-build.1"])
        self.assertEqual(sorted(p.name for p in self.root.iterdir()),
                         ["unrelated", "v0.1.0-build.10", "v0.1.0-build.3", "v0.1.0-build.9"])

    def test_checksums_ignore_names_listed_twice(self):
        a, b = "a" * 64, "b" * 64
        self.assertEqual(sync.checksums(f"{a}  one\n{b} *two\n{a}  two\nnot a line\n"), {"one": a})


if __name__ == "__main__":
    unittest.main()
