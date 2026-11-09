import copy
import re
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

import release


class ChangelogTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.path = Path(directory.name) / "CHANGELOG.md"
        self.path.write_text("<!-- note -->\n\n# v0.1.0-build.24 · 2026-11-08\n\n## New\n\n- Old\n", encoding="utf-8")

    def changelog(self, number, tags=()):
        commits = {"HEAD": "Latest thing\n\nFixed: Head fix\n", "v0.1.0-build.25": "A subject (#30)\n"}

        def git(*args):
            if args[0] == "tag":
                return "\n".join(tags) + "\n"
            return "2026-11-09\n" if "--format=%cs" in args else commits[args[-1]]

        with patch.object(release, "git_output", git):
            release.changelog("0.1.0", number, self.path)
        return self.path.read_text(encoding="utf-8")

    def test_this_build_and_a_missed_one_go_on_top_newest_first(self):
        text = self.changelog(26, ["v0.1.0-build.24", "v0.1.0-build.25"])
        self.assertLess(text.index("build.26 · 2026-11-09"), text.index("build.25 · 2026-11-09"))
        self.assertLess(text.index("build.25 · 2026-11-09"), text.index("build.24 · 2026-11-08"))
        self.assertIn("## Fixed\n\n- Head fix\n", text)
        self.assertIn("## Changed\n\n- A subject\n", text)
        self.assertTrue(text.startswith("<!-- note -->"))

    def test_a_build_already_in_the_file_is_left_alone(self):
        before = self.path.read_text(encoding="utf-8")
        self.assertEqual(self.changelog(24, ["v0.1.0-build.24"]), before)


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
        cask = self.output.read_text(encoding="utf-8")
        self.assertIn('version "0.1.0,12"', cask)
        for asset in self.data["assets"]:
            self.assertIn(f'releases/download/v0.1.0-build.12/{asset["name"]}"', cask)
            self.assertIn(asset["digest"].removeprefix("sha256:"), cask)
        self.assertNotIn("${", cask)
        self.assertIn('app "Flexspeak.app"', cask)
        self.assertIn("depends_on arch: :arm64", cask)

    def test_missing_duplicate_or_corrupt_assets_never_replace_cask(self):
        self.output.parent.mkdir()
        self.output.write_text("previous cask", encoding="utf-8")
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
                self.assertEqual(self.output.read_text(encoding="utf-8"), "previous cask")

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
        sums = (self.root / "SHA256SUMS.txt").read_text(encoding="utf-8").splitlines()
        self.assertEqual(len(sums), 6)
        for line in sums:
            digest, name = line.split("  ")
            self.assertEqual(digest, release.sha256(self.root / name))


class NotesTests(unittest.TestCase):
    def render(self, message):
        with tempfile.TemporaryDirectory() as directory:
            commit = Path(directory, "commit.txt")
            output = Path(directory, "notes.md")
            commit.write_text(message, encoding="utf-8")
            release.notes(commit, output)
            return output.read_text(encoding="utf-8")

    def test_commit_lines_become_sections_in_a_fixed_order(self):
        notes = self.render(
            "Subject (#9)\n\nFixed: A crash\nSome prose\nnew: Themes\n- Changed: Single keys\n"
        )
        self.assertEqual(
            notes,
            "## New\n\n- Themes\n\n## Fixed\n\n- A crash\n\n## Changed\n\n- Single keys\n",
        )

    def test_a_commit_without_lines_falls_back_to_its_subject(self):
        self.assertEqual(self.render("Tidy the footer (#12)\n\nProse.\n"), "## Changed\n\n- Tidy the footer\n")


class DownloadTests(unittest.TestCase):
    def test_a_reset_connection_is_retried_and_never_leaves_a_partial_file(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / "opus.tar.gz"
            calls = []

            def flaky(url, path):
                calls.append(url)
                Path(path).write_bytes(b"partial")
                if len(calls) < 3:
                    raise OSError("EOF occurred in violation of protocol")
                Path(path).write_bytes(b"complete")

            with patch.object(release.urllib.request, "urlretrieve", side_effect=flaky), \
                    patch.object(release.time, "sleep"):
                release.download("https://example.invalid/opus.tar.gz", target)
            self.assertEqual(len(calls), 3)
            self.assertEqual(target.read_bytes(), b"complete")
            self.assertEqual(sorted(p.name for p in Path(directory).iterdir()), ["opus.tar.gz"])

            target.unlink()
            with patch.object(release.urllib.request, "urlretrieve", side_effect=OSError("down")), \
                    patch.object(release.time, "sleep"), self.assertRaises(OSError):
                release.download("https://example.invalid/opus.tar.gz", target, attempts=2)
            self.assertEqual(list(Path(directory).iterdir()), [])


class OpusBuildTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.work = Path(self.directory.name)
        (self.work / f"opus-{release.OPUS_VERSION}.tar.gz").write_bytes(b"source")
        (self.work / f"opus-{release.OPUS_VERSION}").mkdir()

    def build(self):
        env = {"FLEXSPEAK_OPUS_DIR": str(self.work)}
        with patch.object(release, "sha256", return_value=release.OPUS_SHA256), \
                patch.object(release, "run") as run:
            run.side_effect = lambda *args, **kwargs: (self.work / "install").mkdir(exist_ok=True)
            release.build_opus(env)
        self.assertEqual(env["OPUS_LIB_DIR"], str(self.work / "install"))
        return run.call_count

    def test_cached_opus_build_is_reused_until_its_settings_change(self):
        self.assertEqual(self.build(), 3)
        self.assertEqual(self.build(), 0)
        stamp = self.work / "install/flexspeak-build.json"
        stamp.write_text(stamp.read_text(encoding="utf-8").replace(release.OPUS_VERSION, "0.0.0"), encoding="utf-8")
        self.assertEqual(self.build(), 3)


class InAppUpdaterContractTests(unittest.TestCase):
    """The app downloads release assets by name; renaming one here must not strand it."""

    def test_updater_asset_names_are_published_with_checksums(self):
        source = (release.ROOT / "src/updater.rs").read_text(encoding="utf-8")
        names = dict(re.findall(r'pub const ([A-Z_]+): &str = "(Flexspeak-[^"]+|SHA256SUMS\.txt)";', source))
        self.assertEqual(set(names), {"CHECKSUMS", "WINDOWS_EXE", "MACOS_DMG", "LINUX_PACKAGE", "LINUX_TARBALL"})
        self.assertEqual(names.pop("CHECKSUMS"), "SHA256SUMS.txt")
        for name in names.values():
            (self.root / name).write_bytes(name.encode())
        (self.root / "Flexspeak-windows-x64.zip").write_bytes(b"zip")
        (self.root / "THIRD-PARTY-LICENSES.txt").write_bytes(b"notices")
        with patch.object(release, "OUTPUT", self.root):
            release.checksums()
        listed = {line.split("  ", 1)[1] for line in (self.root / "SHA256SUMS.txt").read_text(encoding="utf-8").splitlines()}
        self.assertTrue(set(names.values()) <= listed)

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def test_updater_reads_the_tag_the_workflow_creates(self):
        workflow = (release.ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
        self.assertIn('tag="v${version}-build.${GITHUB_RUN_NUMBER}"', workflow)
        self.assertIn('gh release edit "$tag" --draft=false --latest="$latest"', workflow)
        source = (release.ROOT / "src/updater.rs").read_text(encoding="utf-8")
        self.assertIn('split_once("-build.")', source)
        self.assertIn("releases/latest", source)


if __name__ == "__main__":
    unittest.main()
