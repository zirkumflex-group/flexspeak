import copy
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

import release


class HomebrewReleaseTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.output = self.root / "Casks/flexspeak.rb"
        asset = self.root / "Flexspeak-macos-arm64.dmg"
        asset.write_bytes(b"arm64")
        self.data = {"tag_name": "v0.1.0-build.12", "assets": [{
            "id": 1, "name": asset.name, "digest": f"sha256:{release.sha256(asset)}",
        }]}

    def test_arm_only_release_generates_cask_with_matching_checksum(self):
        release.cask(self.data, self.output, self.root)
        cask = self.output.read_text()
        self.assertIn('version "0.1.0,12"', cask)
        for asset in self.data["assets"]:
            self.assertIn(f'releases/assets/{asset["id"]}"', cask)
            self.assertIn(asset["digest"].removeprefix("sha256:"), cask)
        self.assertNotIn("${", cask)
        self.assertIn('app "Native Voice.app"', cask)
        self.assertIn("depends_on arch: :arm64", cask)

    def test_missing_duplicate_or_corrupt_assets_never_replace_cask(self):
        self.output.parent.mkdir()
        self.output.write_text("previous cask")
        for invalid in ("missing", "duplicate", "checksum", "id", "tag"):
            with self.subTest(invalid=invalid):
                data = copy.deepcopy(self.data)
                if invalid == "missing":
                    data["assets"].pop()
                elif invalid == "duplicate":
                    data["assets"].append(data["assets"][0])
                elif invalid == "checksum":
                    data["assets"][0]["digest"] = "sha256:" + "0" * 64
                elif invalid == "id":
                    data["assets"][0]["id"] = "not-an-id"
                else:
                    data["tag_name"] = 'v0.1.0-build.12"; exit'
                with self.assertRaises(ValueError):
                    release.cask(data, self.output, self.root)
                self.assertEqual(self.output.read_text(), "previous cask")

    def test_release_requires_linux_packages_before_writing_checksums(self):
        names = ["Flexspeak-windows-x64.exe", "Flexspeak-windows-x64.zip",
                 "Flexspeak-macos-arm64.dmg", "THIRD-PARTY-LICENSES.txt"]
        for name in names:
            (self.root / name).write_bytes(b"test artifact")
        with patch.object(release, "OUTPUT", self.root):
            with self.assertRaises(ValueError):
                release.checksums()
            self.assertFalse((self.root / "SHA256SUMS.txt").exists())
            for name in ("Flexspeak-linux-x64.tar.gz", "Flexspeak-linux-x64.pkg.tar.zst"):
                (self.root / name).write_bytes(b"linux artifact")
            release.checksums()
        sums = (self.root / "SHA256SUMS.txt").read_text().splitlines()
        self.assertEqual(len(sums), 6)
        for line in sums:
            digest, name = line.split("  ")
            self.assertEqual(digest, release.sha256(self.root / name))


if __name__ == "__main__":
    unittest.main()
