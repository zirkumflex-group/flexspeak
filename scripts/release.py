"""Build native release packages and generate the private Homebrew cask."""

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
import tomllib
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "dist/releases"
OPUS_VERSION = "1.6.1"
OPUS_SHA256 = "6ffcb593207be92584df15b32466ed64bbec99109f007c82205f0194572411a1"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, cwd=ROOT, **kwargs)


def sha256(path):
    with Path(path).open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def build_opus(env):
    work = ROOT / "target/release-opus"
    work.mkdir(parents=True, exist_ok=True)
    archive = work / f"opus-{OPUS_VERSION}.tar.gz"
    if not archive.exists():
        urllib.request.urlretrieve(
            f"https://downloads.xiph.org/releases/opus/{archive.name}", archive
        )
    if sha256(archive) != OPUS_SHA256:
        raise ValueError("Opus source checksum mismatch")
    source = work / f"opus-{OPUS_VERSION}"
    if not source.exists():
        with tarfile.open(archive) as file:
            file.extractall(work, filter="data")
    options = []
    if os.name == "nt":
        options = ["-G", "Visual Studio 17 2022", "-A", "x64"]
    elif sys.platform == "darwin":
        options = ["-DCMAKE_OSX_DEPLOYMENT_TARGET=14.2"]
    run("cmake", "-S", source, "-B", work / "build", *options,
        "-DCMAKE_BUILD_TYPE=Release", f"-DCMAKE_INSTALL_PREFIX={work / 'install'}",
        "-DBUILD_SHARED_LIBS=OFF", "-DOPUS_STATIC_RUNTIME=ON",
        "-DOPUS_BUILD_TESTING=OFF", "-DOPUS_BUILD_PROGRAMS=OFF", env=env)
    run("cmake", "--build", work / "build", "--config", "Release", "--parallel", "3", env=env)
    run("cmake", "--install", work / "build", "--config", "Release", env=env)
    env.update(OPUS_NO_PKG="1", OPUS_LIB_DIR=str(work / "install"))
    return source


def write_notices(destination, opus_source):
    metadata = json.loads(run("cargo", "metadata", "--locked", "--format-version=1",
                              capture_output=True, text=True).stdout)
    with destination.open("w", encoding="utf-8") as output:
        output.write("Dependency license notices\n")
        for package in sorted(metadata["packages"], key=lambda item: (item["name"], item["version"])):
            if package["source"] is None:
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
        output.write((opus_source / "COPYING").read_text())
        output.write("\n--- Lucide status icons ---\n")
        output.write((ROOT / "assets/lucide/LICENSE").read_text())


def package_linux(stage, version, number):
    archive = OUTPUT / "Flexspeak-linux-x64.tar.gz"
    with tarfile.open(archive, "w:gz") as file:
        for path in sorted(stage.iterdir()):
            file.add(path, arcname=path.name)
    work = ROOT / "target/arch-package"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir()
    shutil.copy2(archive, work)
    recipe = (ROOT / "scripts/PKGBUILD.in").read_text()
    (work / "PKGBUILD").write_text(recipe.replace("@VERSION@", f"{version}.r{number}")
                                  .replace("@SHA256@", sha256(archive)))
    # makepkg packages the already-built binary; it does not compile a second copy.
    run("docker", "run", "--rm", "--platform", "linux/amd64",
        "-e", f"PACKAGER_UID={os.getuid()}", "-v", f"{work}:/work",
        "-w", "/work", "archlinux:base-devel", "bash", "-euc", """
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
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    number = int(env.get("GITHUB_RUN_NUMBER", "1"))
    env.update(MACOSX_DEPLOYMENT_TARGET="14.2", NATIVE_VOICE_APP_VERSION=version,
               NATIVE_VOICE_BUILD_NUMBER=str(number),
               CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-C target-feature=+crt-static")
    opus_source = build_opus(env)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    stage = ROOT / "target/release-package"
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir()
    shutil.copy2(ROOT / "settings.example.json", stage)
    shutil.copy2(ROOT / "README.md", stage)
    write_notices(stage / "THIRD-PARTY-LICENSES.txt", opus_source)
    (stage / "BUILD-INFO.json").write_text(json.dumps({
        "version": version, "build": number, "commit": env.get("GITHUB_SHA") or
        run("git", "rev-parse", "HEAD", capture_output=True, text=True).stdout.strip(),
        "platform": sys.platform, "architecture": platform.machine(), "opus": OPUS_VERSION,
    }, indent=2) + "\n")
    if os.name == "nt":
        run("cargo", "build", "--release", "--locked", "--target", "x86_64-pc-windows-msvc", env=env)
        executable = ROOT / "target/x86_64-pc-windows-msvc/release/teamspeak-native.exe"
        run(executable, "--help")
        shutil.copy2(executable, stage / "Flexspeak.exe")
        shutil.copy2(executable, OUTPUT / "Flexspeak-windows-x64.exe")
        with zipfile.ZipFile(OUTPUT / "Flexspeak-windows-x64.zip", "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(stage.iterdir()):
                archive.write(path, path.name)
        shutil.copy2(stage / "THIRD-PARTY-LICENSES.txt", OUTPUT)
    elif sys.platform == "darwin":
        run("sh", "scripts/macos-app.sh", "release", env=env)
        app = stage / "Native Voice.app"
        shutil.copytree(ROOT / "dist/Native Voice.app", app)
        resources = app / "Contents/Resources"
        resources.mkdir(exist_ok=True)
        shutil.copy2(stage / "THIRD-PARTY-LICENSES.txt", resources)
        run("codesign", "--force", "--sign", "-", app)
        run("codesign", "--verify", "--strict", app)
        binary = app / "Contents/MacOS/teamspeak-native"
        dependencies = run("otool", "-L", binary, capture_output=True, text=True).stdout
        if any(path in dependencies for path in ("/opt/homebrew/", "/usr/local/")):
            raise ValueError("App links to a Homebrew library instead of being standalone")
        run(binary, "--help")
        (stage / "Applications").symlink_to("/Applications")
        dmg = OUTPUT / "Flexspeak-macos-arm64.dmg"
        run("hdiutil", "create", "-ov", "-volname", "Flexspeak", "-srcfolder", stage,
            "-format", "UDZO", dmg)
        run("hdiutil", "verify", dmg)
    else:
        run("cargo", "build", "--release", "--locked", env=env)
        executable = ROOT / "target/release/teamspeak-native"
        run(executable, "--help")
        shutil.copy2(executable, stage / "flexspeak")
        shutil.copy2(ROOT / "scripts/flexspeak.desktop", stage)
        run("desktop-file-validate", stage / "flexspeak.desktop")
        package_linux(stage, version, number)


def cask(release, output, artifacts=OUTPUT):
    match = re.fullmatch(r"v(\d+\.\d+\.\d+)-build\.(\d+)", release["tag_name"])
    if not match:
        raise ValueError("Unexpected release tag")
    values = {"VERSION": f"{match[1]},{match[2]}"}
    name = "Flexspeak-macos-arm64.dmg"
    assets = [asset for asset in release["assets"] if asset["name"] == name]
    if len(assets) != 1 or type(assets[0]["id"]) is not int or assets[0]["id"] <= 0:
        raise ValueError(f"Expected one uploaded asset: {name}")
    digest = sha256(artifacts / name)
    if assets[0].get("digest") != f"sha256:{digest}":
        raise ValueError(f"Uploaded asset checksum does not match: {name}")
    values.update(ARM_ID=assets[0]["id"], ARM_SHA=digest)
    rendered = Template((ROOT / "scripts/flexspeak.rb.in").read_text()).substitute(values)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(rendered)


def checksums():
    required = {"Flexspeak-windows-x64.exe", "Flexspeak-windows-x64.zip",
                "Flexspeak-macos-arm64.dmg", "Flexspeak-linux-x64.tar.gz",
                "Flexspeak-linux-x64.pkg.tar.zst", "THIRD-PARTY-LICENSES.txt"}
    if not required.issubset(path.name for path in OUTPUT.iterdir()):
        raise ValueError("Missing release artifacts")
    (OUTPUT / "SHA256SUMS.txt").write_text("".join(
        f"{sha256(OUTPUT / name)}  {name}\n" for name in sorted(required)
    ))


if __name__ == "__main__":
    os.chdir(ROOT)
    if sys.argv[1:] == ["build"]:
        build()
    elif sys.argv[1:] == ["checksums"]:
        checksums()
    elif len(sys.argv) == 4 and sys.argv[1] == "cask":
        cask(json.loads(Path(sys.argv[2]).read_text()), Path(sys.argv[3]))
    else:
        sys.exit("Usage: release.py build | checksums | cask RELEASE_JSON OUTPUT_RB")
