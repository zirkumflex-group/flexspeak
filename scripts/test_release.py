import copy
import tempfile
from pathlib import Path
import unittest

import release


class HomebrewReleaseTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.output = self.root / "Casks/flexspeak.rb"
        self.data = {"tag_name": "v0.1.0-build.12", "assets": []}
        for asset_id, arch in enumerate(("arm64", "x64"), start=1):
            asset = self.root / f"Flexspeak-macos-{arch}.dmg"
            asset.write_bytes(arch.encode())
            self.data["assets"].append({
                "id": asset_id, "name": asset.name, "digest": f"sha256:{release.sha256(asset)}",
            })

    def test_architecture_urls_and_checksums_match_uploaded_assets(self):
        release.cask(self.data, self.output, self.root)
        cask = self.output.read_text()
        self.assertIn('version "0.1.0,12"', cask)
        for asset in self.data["assets"]:
            self.assertIn(f'releases/assets/{asset["id"]}"', cask)
            self.assertIn(asset["digest"].removeprefix("sha256:"), cask)
        self.assertNotIn("${", cask)
        self.assertIn('app "Native Voice.app"', cask)

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


if __name__ == "__main__":
    unittest.main()
