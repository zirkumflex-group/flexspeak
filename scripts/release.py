"""Build native release packages and generate the Homebrew cask."""

from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
from string import Template
import subprocess
import sys
import tarfile
import time
import tomllib
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "dist/releases"
# In its own file so CI can key the Opus cache on it alone.
OPUS = json.loads((ROOT / "scripts/opus.json").read_text(encoding="utf-8"))
OPUS_VERSION = OPUS["version"]
OPUS_SHA256 = OPUS["sha256"]
# The Arch packaging container, by digest so a new image can't change a release silently.
ARCH_IMAGE = "archlinux:base-devel@sha256:2de80105abb49f813a85393c6758a4334deb6bcec4a2411bd2f059c0bee6ee4c"
# Every platform Flexspeak builds for: its runner, the cache its checks save, and the packages
# a release carries for it. Which of them PRs check and releases build is the switch file.
PLATFORMS = {
    # First: the release's safety-net test runs on the first enabled platform, and Linux is
    # the quickest runner.
    "linux": {"os": "ubuntu-24.04", "platform": "linux-x64", "cache": "linux-checks",
              "packages": ["Flexspeak-linux-x64.tar.gz", "Flexspeak-linux-x64.pkg.tar.zst"]},
    "windows": {"os": "windows-2022", "platform": "windows-x64", "cache": "windows-checks",
                "packages": ["Flexspeak-windows-x64.exe", "Flexspeak-windows-x64.zip"]},
    # macOS 26 for its Xcode: the Liquid Glass icon only compiles there (Xcode 26 crashes on it
    # under 15), and a cache from another image holds that image's Xcode paths and won't link.
    "macos": {"os": "macos-26", "platform": "macos-arm64", "cache": "macos-26-checks",
              "packages": ["Flexspeak-macos-arm64.dmg"]},
}
SWITCHES = ROOT / ".github/platforms.json"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, cwd=ROOT, **kwargs)


def target():
    """The one target this runner builds. Fetching, license notices and the build all use
    it, so nothing needs another platform's crates."""
    if os.name == "nt":
        return "x86_64-pc-windows-msvc"
    if sys.platform == "darwin":
        return "aarch64-apple-darwin"
    return "x86_64-unknown-linux-gnu"


def fetch(env=None):
    """Downloads every crate and git dependency the build needs, before anything else runs
    Cargo. `build()` runs `cargo metadata` and `cargo build` side by side; if either had to
    fetch, it would hold Cargo's package lock while the other waits."""
    run("cargo", "fetch", "--locked", "--target", target(), env=env)


def sha256(path):
    with Path(path).open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def release_notes(message, earlier=()):
    """Release notes from a commit message: its `New:`, `Fixed:` and `Changed:` lines, or the
    subject under Changed when it has none. The app reads these sections. `earlier` holds
    the messages of older commits the same build carries; only their lines are added."""
    lines = message.splitlines()
    sections = {"New": [], "Fixed": [], "Changed": []}

    def collect(body):
        for line in body:
            match = re.fullmatch(r"\s*(?:[-*]\s*)?(New|Fixed|Changed):\s+(.+?)\s*", line, re.I)
            if match and match[2] not in sections[match[1].capitalize()]:
                sections[match[1].capitalize()].append(match[2])

    collect(lines[1:])
    if not any(sections.values()):
        sections["Changed"].append(re.sub(r"\s*\(#\d+\)$", "", lines[0]).strip())
    for other in earlier:
        collect(other.splitlines()[1:])
    return (
        "\n\n".join(
            f"## {name}\n\n" + "\n".join(f"- {item}" for item in items)
            for name, items in sections.items()
            if items
        )
        + "\n"
    )


def notes(commit, output):
    output.write_text(release_notes(commit.read_text(encoding="utf-8")), encoding="utf-8")


def git_output(*args):
    return run("git", *args, capture_output=True, text=True, encoding="utf-8").stdout


def changelog(version, number, path=None):
    """Adds this build and any released build CHANGELOG.md doesn't have yet, newest first, so
    the app embeds its own notes. Rewrites the checkout's copy; nothing commits it.

    Builds of several pushes run side by side, so the push before this one may have no
    release yet (or never get one, if its build fails). This build carries those commits
    all the same, so their changelog lines go into its own entry."""
    path = path or ROOT / "CHANGELOG.md"
    text = path.read_bytes().decode("utf-8")
    newest = max(map(int, re.findall(r"^# v\S+-build\.(\d+) · ", text, re.M)), default=0)
    missing = [("HEAD", f"v{version}-build.{number}", number)] if number > newest else []
    released = {}
    for tag in git_output("tag", "--list", "v*-build.*").split():
        built = int(tag.rsplit(".", 1)[1])
        if built < number:
            released[built] = tag
        if newest < built < number:
            missing.append((tag, tag, built))
    if not missing:
        return
    entries = []
    for ref, tag, _ in sorted(missing, key=lambda entry: entry[2], reverse=True):
        date = git_output("log", "-1", "--format=%cs", ref).strip()
        earlier = ()
        if ref == "HEAD" and released:
            last = released[max(released)]
            unreleased = git_output("log", "--first-parent", "--skip=1", "--format=%B%x00", f"{last}..HEAD")
            earlier = [message.strip() for message in unreleased.split("\0") if message.strip()]
        entries.append(f"# {tag} · {date}\n\n{release_notes(git_output('log', '-1', '--format=%B', ref), earlier)}\n")
    match = re.search(r"^# v\S+-build\.", text, re.M)
    at = match.start() if match else len(text)
    path.write_bytes((text[:at] + "".join(entries) + text[at:]).encode("utf-8"))


def download(url, destination, attempts=5):
    """Fetch through a temporary name, so an interrupted download is never mistaken for
    the finished file, and retry the transient TLS resets hosted runners see."""
    partial = destination.with_name(destination.name + ".part")
    for attempt in range(1, attempts + 1):
        try:
            urllib.request.urlretrieve(url, partial)
            partial.replace(destination)
            return
        except OSError as error:
            partial.unlink(missing_ok=True)
            if attempt == attempts:
                raise
            print(f"Download failed ({error}); retrying {url}", file=sys.stderr)
            time.sleep(5 * attempt)


def build_opus(env):
    # CI points this outside target/ so it can be cached apart from Cargo's artifacts.
    work = Path(env.get("FLEXSPEAK_OPUS_DIR") or ROOT / "target/release-opus")
    work.mkdir(parents=True, exist_ok=True)
    archive = work / f"opus-{OPUS_VERSION}.tar.gz"
    if not archive.exists():
        download(f"https://downloads.xiph.org/releases/opus/{archive.name}", archive)
    if sha256(archive) != OPUS_SHA256:
        raise ValueError("Opus source checksum mismatch")
    source = work / f"opus-{OPUS_VERSION}"
    if not source.exists():
        with tarfile.open(archive) as file:
            file.extractall(work, filter="data")
    install = work / "install"
    options = []
    if os.name == "nt":
        options = ["-G", "Visual Studio 17 2022", "-A", "x64"]
    elif sys.platform == "darwin":
        options = ["-DCMAKE_OSX_DEPLOYMENT_TARGET=14.2"]
    options += ["-DCMAKE_BUILD_TYPE=Release", f"-DCMAKE_INSTALL_PREFIX={install}",
                "-DBUILD_SHARED_LIBS=OFF", "-DOPUS_STATIC_RUNTIME=ON",
                "-DOPUS_BUILD_TESTING=OFF", "-DOPUS_BUILD_PROGRAMS=OFF"]
    stamp = install / "flexspeak-build.json"
    settings = json.dumps({"version": OPUS_VERSION, "options": options})
    if not stamp.is_file() or stamp.read_text(encoding="utf-8") != settings:
        shutil.rmtree(install, ignore_errors=True)
        run("cmake", "-S", source, "-B", work / "build", *options, env=env)
        run("cmake", "--build", work / "build", "--config", "Release", "--parallel", "3", env=env)
        run("cmake", "--install", work / "build", "--config", "Release", env=env)
        stamp.write_text(settings, encoding="utf-8")
    env.update(OPUS_NO_PKG="1", OPUS_LIB_DIR=str(install))
    return source


def write_notices(destination, opus_source):
    # Offline and for this platform only: `fetch()` has the crates, and the notices cover
    # what this package contains. Cargo's errors go to the log instead of being swallowed.
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline", "--format-version=1",
                              "--filter-platform", target(),
                              stdout=subprocess.PIPE, encoding="utf-8").stdout)
    with destination.open("w", encoding="utf-8") as output:
        output.write("Dependency license notices\n")
        for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
            if Path(package["manifest_path"]).resolve() == (ROOT / "Cargo.toml").resolve():
                continue
            directory = Path(package["manifest_path"]).parent
            files = {path for path in directory.rglob("*") if path.is_file()
                     and path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE"))}
            if package.get("license_file"):
                files.add(directory / package["license_file"])
            # Git workspace crates sometimes keep their license at the workspace root.
            if not files:
                for parent in list(directory.parents)[:3]:
                    files.update(path for path in parent.glob("*") if path.is_file()
                                 and path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE")))
                    if files:
                        break
            output.write(f"\n--- {package['name']} {package['version']} ---\n")
            output.write(f"License: {package.get('license')}\nSource: {package.get('repository')}\n")
            for path in sorted(files):
                output.write(f"\n{path.name}\n{path.read_text(encoding='utf-8', errors='replace')}\n")
        output.write(f"\n--- Native Opus {OPUS_VERSION} ---\n")
        # UTF-8 explicitly: Windows would read these as cp1252 and garble names and ©.
        output.write((opus_source / "COPYING").read_text(encoding="utf-8"))
        output.write("\n--- Lucide status icons ---\n")
        output.write((ROOT / "assets/lucide/LICENSE").read_text(encoding="utf-8"))
        output.write("\n--- Native WebRTC ---\n")
        output.write((ROOT / "assets/webrtc/LICENSE.md").read_text(encoding="utf-8"))
        output.write((ROOT / "assets/webrtc/NOTICE.txt").read_text(encoding="utf-8"))


def package_linux(stage, version, number):
    archive = OUTPUT / "Flexspeak-linux-x64.tar.gz"
    with tarfile.open(archive, "w:gz") as file:
        for path in sorted(stage.iterdir()):
            file.add(path, arcname=path.name)
    work = ROOT / "target/arch-package"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True)
    shutil.copy2(archive, work)
    recipe = (ROOT / "scripts/PKGBUILD.in").read_text(encoding="utf-8")
    (work / "PKGBUILD").write_text(recipe.replace("@VERSION@", f"{version}.r{number}")
                                  .replace("@SHA256@", sha256(archive)), encoding="utf-8")
    # makepkg packages the already-built binary; it does not compile a second copy.
    run("docker", "run", "--rm", "--platform", "linux/amd64",
        "-e", f"PACKAGER_UID={os.getuid()}", "-v", f"{work}:/work",
        "-w", "/work", ARCH_IMAGE, "bash", "-euc", """
pacman -Syu --noconfirm
useradd --uid "$PACKAGER_UID" --create-home packager
chown -R packager:packager /work
runuser -u packager -- makepkg --nodeps --noconfirm
pacman -U --noconfirm /work/flexspeak-*.pkg.tar.zst
cmp /work/src/flexspeak /usr/bin/flexspeak
flexspeak --help
""")
    packages = list(work.glob("*.pkg.tar.zst"))
    if len(packages) != 1:
        raise ValueError("Expected exactly one Arch package")
    shutil.copy2(packages[0], OUTPUT / "Flexspeak-linux-x64.pkg.tar.zst")


def build():
    if sys.platform not in ("darwin", "win32", "linux"):
        raise ValueError("Release packaging needs a native macOS, Windows or Linux runner")
    if sys.platform == "darwin" and platform.machine() != "arm64":
        raise ValueError("macOS releases require an Apple Silicon runner")
    if sys.platform == "linux" and platform.machine() != "x86_64":
        raise ValueError("Linux releases require an x86_64 runner")
    env = os.environ.copy()
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    number = int(env.get("GITHUB_RUN_NUMBER", "1"))
    if "GITHUB_RUN_NUMBER" in env:
        changelog(version, number)
    env.update(MACOSX_DEPLOYMENT_TARGET="14.2", NATIVE_VOICE_APP_VERSION=version,
               NATIVE_VOICE_BUILD_NUMBER=str(number),
               CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-C target-feature=+crt-static")
    opus_source = build_opus(env)
    cargo_target = ROOT / env.get("CARGO_TARGET_DIR", "target")
    OUTPUT.mkdir(parents=True, exist_ok=True)
    stage = ROOT / "target/release-package"
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir(parents=True)
    shutil.copy2(ROOT / "settings.example.json", stage)
    shutil.copy2(ROOT / "README.md", stage)
    def output(*args):
        return run(*args, capture_output=True, encoding="utf-8").stdout.strip()

    info = {
        "version": version, "build": number,
        "commit": env.get("GITHUB_SHA") or output("git", "rev-parse", "HEAD"),
        "platform": sys.platform, "architecture": platform.machine(), "opus": OPUS_VERSION,
        "rustc": output("rustc", "--version"),
    }
    if sys.platform == "darwin":
        # The runner image's default Xcode builds the app; record which one it was.
        info["xcode"] = output("xcodebuild", "-version").replace("\n", ", ")
    (stage / "BUILD-INFO.json").write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")
    fetch(env)
    # Collecting license files walks every dependency's source tree, which takes
    # most of a minute on Windows; do it while Cargo compiles. Both run offline now.
    with ThreadPoolExecutor(max_workers=1) as pool:
        notices = pool.submit(write_notices, stage / "THIRD-PARTY-LICENSES.txt", opus_source)
        if os.name == "nt":
            run("cargo", "build", "--release", "--locked", "--target", "x86_64-pc-windows-msvc", env=env)
        elif sys.platform == "darwin":
            run("sh", "scripts/macos-app.sh", "release", env=env)
        else:
            run("cargo", "build", "--release", "--locked", env=env)
        notices.result()
    if os.name == "nt":
        executable = cargo_target / "x86_64-pc-windows-msvc/release/flexspeak.exe"
        run(executable, "--help")
        shutil.copy2(executable, stage / "Flexspeak.exe")
        shutil.copy2(executable, OUTPUT / "Flexspeak-windows-x64.exe")
        with zipfile.ZipFile(OUTPUT / "Flexspeak-windows-x64.zip", "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(stage.iterdir()):
                archive.write(path, path.name)
        shutil.copy2(stage / "THIRD-PARTY-LICENSES.txt", OUTPUT)
    elif sys.platform == "darwin":
        app = stage / "Flexspeak.app"
        shutil.copytree(ROOT / "dist/Flexspeak.app", app)
        resources = app / "Contents/Resources"
        # Build 16 shipped without an icon: CI's actool exited 0 but wrote nothing.
        if not (resources / "Flexspeak.icns").is_file():
            raise ValueError("Flexspeak.app has no Flexspeak.icns; Finder would show a blank icon")
        if not (resources / "Assets.car").is_file():
            raise ValueError("Flexspeak.app has no Assets.car; macOS 26 would lose the Liquid Glass icon")
        for name in ("THIRD-PARTY-LICENSES.txt", "BUILD-INFO.json", "README.md", "settings.example.json"):
            shutil.copy2(stage / name, resources)
        run("codesign", "--force", "--sign", "-", app)
        run("codesign", "--verify", "--strict", app)
        binary = app / "Contents/MacOS/flexspeak"
        dependencies = run("otool", "-L", binary, capture_output=True, encoding="utf-8").stdout
        if any(path in dependencies for path in ("/opt/homebrew/", "/usr/local/")):
            raise ValueError("App links to a Homebrew library instead of being standalone")
        run(binary, "--help")
        dmg = OUTPUT / "Flexspeak-macos-arm64.dmg"
        # dmgbuild writes Finder's layout directly, including icon positions and the
        # background arrow, on CI as well as on Macs with a desktop session.
        run(os.environ.get("FLEXSPEAK_DMGBUILD", "dmgbuild"),
            "-s", ROOT / "scripts/dmg-settings.py", "-D", f"app={app}",
            "-D", f"background={ROOT / 'assets/installer/background.png'}", "Flexspeak", dmg)
        run("hdiutil", "verify", dmg)
    else:
        executable = cargo_target / "release/flexspeak"
        run(executable, "--help")
        shutil.copy2(executable, stage / "flexspeak")
        shutil.copy2(ROOT / "scripts/flexspeak.desktop", stage)
        shutil.copy2(ROOT / "assets/icon/flexspeak.svg", stage)
        run("desktop-file-validate", stage / "flexspeak.desktop")
        package_linux(stage, version, number)


def cask(release, output, artifacts=OUTPUT):
    match = re.fullmatch(r"v(\d+\.\d+\.\d+)-build\.(\d+)", release["tag_name"])
    if not match:
        raise ValueError("Unexpected release tag")
    values = {"VERSION": f"{match[1]},{match[2]}", "TAG": release["tag_name"]}
    name = "Flexspeak-macos-arm64.dmg"
    assets = [asset for asset in release["assets"] if asset["name"] == name]
    if len(assets) != 1 or type(assets[0]["id"]) is not int or assets[0]["id"] <= 0:
        raise ValueError(f"Expected one uploaded asset: {name}")
    digest = sha256(artifacts / name)
    if assets[0].get("digest") != f"sha256:{digest}":
        raise ValueError(f"Uploaded asset checksum does not match: {name}")
    values.update(ARM_ID=assets[0]["id"], ARM_SHA=digest)
    rendered = Template((ROOT / "scripts/flexspeak.rb.in").read_text(encoding="utf-8")).substitute(values)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(rendered, encoding="utf-8")


def enabled_platforms(everything=False):
    """The platforms the switch file turns on, in `PLATFORMS` order (all of them with
    `everything`). A typo there fails loudly rather than dropping a platform."""
    switches = json.loads(SWITCHES.read_text(encoding="utf-8"))
    if (not isinstance(switches, dict) or set(switches) != set(PLATFORMS)
            or not all(type(on) is bool for on in switches.values())):
        raise ValueError(f"{SWITCHES.name} must set each of {', '.join(PLATFORMS)} to true or false")
    names = [name for name in PLATFORMS if everything or switches[name]]
    if not names:
        raise ValueError(f"{SWITCHES.name} turns every platform off")
    return names


def platform_outputs(everything=False):
    """GitHub Actions job outputs: the matrix the checks and builds run, where the release's
    test runs, and whether Homebrew gets a cask."""
    names = enabled_platforms(everything)
    include = [{key: PLATFORMS[name][key] for key in ("os", "platform", "cache")} for name in names]
    return {
        "matrix": json.dumps({"include": include}, separators=(",", ":")),
        "test-os": include[0]["os"],
        "test-cache": include[0]["cache"],
        "macos": json.dumps("macos" in names),
    }


def checksums():
    required = {"THIRD-PARTY-LICENSES.txt"}
    for name in enabled_platforms():
        required.update(PLATFORMS[name]["packages"])
    if not required.issubset(path.name for path in OUTPUT.iterdir()):
        raise ValueError("Missing release artifacts")
    (OUTPUT / "SHA256SUMS.txt").write_text("".join(
        f"{sha256(OUTPUT / name)}  {name}\n" for name in sorted(required)
    ), encoding="utf-8")


if __name__ == "__main__":
    os.chdir(ROOT)
    if sys.argv[1:] == ["build"]:
        build()
    elif sys.argv[1:] == ["fetch"]:
        fetch()
    elif sys.argv[1:] == ["checksums"]:
        checksums()
    elif sys.argv[1:2] == ["platforms"] and sys.argv[2:] in ([], ["--all"]):
        for key, value in platform_outputs(everything=sys.argv[2:] == ["--all"]).items():
            print(f"{key}={value}")
    elif len(sys.argv) == 4 and sys.argv[1] == "notes":
        notes(Path(sys.argv[2]), Path(sys.argv[3]))
    elif len(sys.argv) == 4 and sys.argv[1] == "cask":
        cask(json.loads(Path(sys.argv[2]).read_text(encoding="utf-8")), Path(sys.argv[3]))
    else:
        sys.exit("Usage: release.py build | fetch | checksums | platforms [--all]"
                 " | notes COMMIT OUTPUT | cask RELEASE_JSON OUTPUT_RB")
