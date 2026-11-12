//! In-app updates from GitHub Releases.
//!
//! [`Updater`] is the state machine: it takes [`Event`]s and answers with an [`Effect`] the app
//! runs on a worker thread ([`check`], [`download`], [`install`]). It does no I/O itself, so every
//! transition is testable without a network.
//!
//! The release workflow tags builds `v{Cargo version}-build.{run number}`, publishes them with
//! `--latest=false`, and marks one "latest" only after every package and the Homebrew tap are
//! out. That mark is the newest *complete* release and never moves backwards, so the app reads
//! `releases/latest` rather than guessing from the list. A release is an update when its
//! `(version, build)` is greater than the running one's and it carries this platform's package:
//! a platform can be left out of releases (`.github/platforms.json`), and its installs then stay
//! where they are instead of reporting an error.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const REPOSITORY: &str = "zirkumflex-group/flexspeak";
/// Checks after the first, while the app stays open.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// The first check waits for the window and any session to settle.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(5);
/// Progress reaches the window at most this often.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

// Asset names written by scripts/release.py (scripts/test_release.py keeps them in step).
pub const CHECKSUMS: &str = "SHA256SUMS.txt";
pub const WINDOWS_EXE: &str = "Flexspeak-windows-x64.exe";
/// What Windows installs update from: the executable deflates to well under half its size.
pub const WINDOWS_ZIP: &str = "Flexspeak-windows-x64.zip";
/// The executable's name inside [`WINDOWS_ZIP`].
const WINDOWS_ZIP_EXE: &str = "Flexspeak.exe";
pub const MACOS_DMG: &str = "Flexspeak-macos-arm64.dmg";
pub const LINUX_PACKAGE: &str = "Flexspeak-linux-x64.pkg.tar.zst";
pub const LINUX_TARBALL: &str = "Flexspeak-linux-x64.tar.gz";

const BUNDLE_ID: &str = "io.venator.flexspeak";
const PACMAN_EXECUTABLE: &str = "/usr/bin/flexspeak";

/// A release build: the Cargo version and the workflow run that built it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Build {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub number: u64,
}

impl Build {
    /// `v1.2.3-build.45`, the only tag shape the release workflow creates.
    pub fn parse_tag(tag: &str) -> Option<Build> {
        let (version, number) = tag.strip_prefix('v')?.split_once("-build.")?;
        Self::parse(version, number)
    }

    fn parse(version: &str, number: &str) -> Option<Build> {
        let mut parts = version.split('.');
        let mut part = || -> Option<u64> {
            let text = parts.next()?;
            // Plain decimal only: no signs, no leading `+`, nothing u64::from_str would forgive.
            (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
                .then(|| text.parse().ok())
                .flatten()
        };
        let (major, minor, patch) = (part()?, part()?, part()?);
        if parts.next().is_some()
            || number.is_empty()
            || !number.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        Some(Build {
            major,
            minor,
            patch,
            number: number.parse().ok()?,
        })
    }

    /// The running executable's build. Only release builds carry a build number, so a local
    /// `cargo run` never offers to replace itself.
    pub fn running() -> Option<Build> {
        Self::parse(
            env!("CARGO_PKG_VERSION"),
            option_env!("NATIVE_VOICE_BUILD_NUMBER")?,
        )
    }

    pub fn label(&self) -> String {
        format!(
            "{}.{}.{} · build {}",
            self.major, self.minor, self.patch, self.number
        )
    }
}

/// What this installation is, and so which asset replaces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Package {
    /// The standalone executable, updated from the Windows ZIP that holds it.
    WindowsExe,
    /// `Flexspeak.app`, from the DMG.
    MacApp,
    /// `/usr/bin/flexspeak`, owned by the Arch/Omarchy package.
    LinuxPacman,
    /// The executable from the Linux tarball, wherever it was unpacked.
    LinuxTarball,
}

impl Package {
    pub fn asset(self) -> &'static str {
        match self {
            Self::WindowsExe => WINDOWS_ZIP,
            Self::MacApp => MACOS_DMG,
            Self::LinuxPacman => LINUX_PACKAGE,
            Self::LinuxTarball => LINUX_TARBALL,
        }
    }

    /// This process's package, or why it can't update itself.
    pub fn detect() -> Result<Package, String> {
        let exe = std::env::current_exe()
            .map_err(|error| format!("Cannot locate the running executable: {error}"))?;
        Self::for_platform(std::env::consts::OS, std::env::consts::ARCH, &exe)
    }

    /// Chosen at run time rather than with `cfg`, so every installer compiles and is linted on
    /// every platform.
    pub fn for_platform(os: &str, arch: &str, exe: &Path) -> Result<Package, String> {
        match (os, arch) {
            ("windows", "x86_64") => Ok(Self::WindowsExe),
            ("macos", "aarch64") => app_bundle(exe).map(|_| Self::MacApp),
            ("linux", "x86_64") if exe == Path::new(PACMAN_EXECUTABLE) => Ok(Self::LinuxPacman),
            ("linux", "x86_64") => Ok(Self::LinuxTarball),
            _ => Err(format!(
                "No release packages are published for {os} on {arch}."
            )),
        }
    }
}

/// The `.app` that holds `exe`, if it runs from one macOS hasn't translocated.
fn app_bundle(exe: &Path) -> Result<PathBuf, String> {
    if exe
        .components()
        .any(|part| part.as_os_str() == "AppTranslocation")
    {
        return Err("macOS is running Flexspeak from a temporary copy. Move Flexspeak.app to Applications and open it from there to install updates.".into());
    }
    exe.ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            "Flexspeak isn't running from Flexspeak.app, so it can't replace itself.".into()
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// GitHub's own SHA-256 of the upload, lowercase hex, when the API reports one.
    pub digest: Option<String>,
}

/// A newer release with a package for this installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub build: Build,
    pub asset: Asset,
    pub checksums_url: String,
    /// Boxed because `Release` travels inside event enums.
    pub notes: Box<Notes>,
}

/// What a release changed, from the `## New`, `## Fixed` and `## Changed` sections of its notes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    pub new: Vec<String>,
    pub fixed: Vec<String>,
    pub changed: Vec<String>,
}

impl Notes {
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.fixed.is_empty() && self.changed.is_empty()
    }

    /// Adds a release's notes. Bodies without these sections add nothing.
    pub(crate) fn add(&mut self, body: &str) {
        let mut section = None;
        for line in body.lines().map(str::trim) {
            if let Some(heading) = line.strip_prefix('#') {
                let heading = heading.trim_start_matches('#').trim();
                section = match heading.to_ascii_lowercase().as_str() {
                    "new" => Some(&mut self.new),
                    "fixed" => Some(&mut self.fixed),
                    "changed" => Some(&mut self.changed),
                    _ => None,
                };
            } else if let (Some(list), Some(item)) = (
                section.as_deref_mut(),
                line.strip_prefix("- ").or(line.strip_prefix("* ")),
            ) {
                let item: String = item
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_NOTE_CHARS)
                    .collect();
                if list.len() < MAX_NOTES && !item.is_empty() && !list.contains(&item) {
                    list.push(item);
                }
            }
        }
    }
}

const MAX_NOTES: usize = 40;
const MAX_NOTE_CHARS: usize = 200;

/// Combines the notes of every published build after `running`, up to `target`, newest first.
fn notes_between(json: &str, running: Build, target: Build) -> Option<Notes> {
    let releases: Vec<GitHubRelease> = serde_json::from_str(json).ok()?;
    let mut notes = Notes::default();
    for release in &releases {
        if !release.draft
            && !release.prerelease
            && Build::parse_tag(&release.tag_name).is_some_and(|b| b > running && b <= target)
            && let Some(body) = &release.body
        {
            notes.add(body);
        }
    }
    Some(notes)
}

/// Notes covering every build being skipped. Only worth a request when more than the latest
/// is missing, and a failure just leaves the latest build's own notes.
async fn missed_notes(url: &str, running: Build, target: Build) -> Option<Notes> {
    let response = client()
        .ok()?
        .get(format!("{}?per_page=30", url.strip_suffix("/latest")?))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    notes_between(&response.text().await.ok()?, running, target)
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

/// Reads a `releases/latest` response: `Ok(None)` when it's not newer than `running`, or has no
/// package for this platform at all.
pub fn select(json: &str, running: Build, asset_name: &str) -> Result<Option<Release>, String> {
    let release: GitHubRelease = serde_json::from_str(json)
        .map_err(|error| format!("GitHub sent an unexpected release description: {error}"))?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let Some(build) = Build::parse_tag(&release.tag_name) else {
        return Err(format!(
            "The latest release tag '{}' isn't a Flexspeak build tag.",
            release.tag_name
        ));
    };
    if build <= running {
        return Ok(None);
    }
    let uploaded = |name: &str| {
        let mut matches = release.assets.iter().filter(|asset| {
            asset.name == name
                && asset
                    .state
                    .as_deref()
                    .is_none_or(|state| state == "uploaded")
        });
        match (matches.next(), matches.next()) {
            (Some(asset), None) if asset.browser_download_url.starts_with("https://") => Ok(asset),
            _ => Err(format!(
                "Release {} has no single uploaded {name}.",
                release.tag_name
            )),
        }
    };
    // Not built for this platform: nothing to install, and nothing wrong either.
    if !release.assets.iter().any(|asset| asset.name == asset_name) {
        return Ok(None);
    }
    let asset = uploaded(asset_name)?;
    let checksums = uploaded(CHECKSUMS)?;
    let digest = match &asset.digest {
        None => None,
        Some(digest) => Some(
            digest
                .strip_prefix("sha256:")
                .filter(|hex| is_sha256(hex))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| format!("GitHub reports an unusable digest for {asset_name}."))?,
        ),
    };
    let mut notes = Notes::default();
    notes.add(release.body.as_deref().unwrap_or_default());
    Ok(Some(Release {
        tag: release.tag_name.clone(),
        build,
        asset: Asset {
            name: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            size: asset.size,
            digest,
        },
        checksums_url: checksums.browser_download_url.clone(),
        notes: Box::new(notes),
    }))
}

fn is_sha256(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The checksum `SHA256SUMS.txt` lists for `name` (`sha256sum` format).
pub fn checksum_for(sums: &str, name: &str) -> Result<String, String> {
    let mut found = sums.lines().filter_map(|line| {
        let (hex, file) = line.split_once(char::is_whitespace)?;
        let file = file.trim_start();
        let file = file.strip_prefix('*').unwrap_or(file);
        (file == name && is_sha256(hex)).then(|| hex.to_ascii_lowercase())
    });
    match (found.next(), found.next()) {
        (Some(hex), None) => Ok(hex),
        _ => Err(format!("{CHECKSUMS} has no single checksum for {name}.")),
    }
}

/// Downloaded bytes so far and a smoothed speed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Progress {
    pub received: u64,
    pub total: Option<u64>,
    pub bytes_per_second: Option<f64>,
}

impl Progress {
    pub fn fraction(&self) -> Option<f32> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (self.received as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }
}

/// A download's speed, smoothed so the reading doesn't flicker between samples.
pub struct SpeedMeter {
    last: Instant,
    last_received: u64,
    speed: Option<f64>,
}

impl SpeedMeter {
    pub fn new(now: Instant) -> Self {
        Self {
            last: now,
            last_received: 0,
            speed: None,
        }
    }

    /// The speed once at least a progress interval has passed since the last reading; `None`
    /// means it's too soon to report.
    pub fn sample(&mut self, now: Instant, received: u64) -> Option<f64> {
        let elapsed = now.saturating_duration_since(self.last);
        if elapsed < PROGRESS_INTERVAL {
            return None;
        }
        let instant = received.saturating_sub(self.last_received) as f64 / elapsed.as_secs_f64();
        let speed = match self.speed {
            Some(previous) => previous * 0.6 + instant * 0.4,
            None => instant,
        };
        self.last = now;
        self.last_received = received;
        self.speed = Some(speed);
        Some(speed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    Download,
    Install,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// No newer build is known.
    Idle,
    /// A newer build exists and hasn't been downloaded.
    Available,
    Downloading(Progress),
    /// Downloaded and verified; Restart installs it.
    Ready,
    Installing,
    Failed {
        retry: Retry,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub release: Release,
    /// A verified download of this release left by an earlier run.
    pub staged: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Event {
    Check,
    Checked(Result<Option<Found>, String>),
    Download,
    Progress {
        generation: u64,
        progress: Progress,
    },
    Downloaded {
        generation: u64,
        result: Result<PathBuf, String>,
    },
    Restart,
    Installed(Result<(), String>),
    Dismiss,
    AutoDownload(bool),
}

/// Work an event asks the app to start.
#[derive(Debug)]
pub enum Effect {
    None,
    Check {
        running: Build,
        package: Package,
    },
    Download {
        generation: u64,
        release: Release,
        package: Package,
        cancel: Arc<AtomicBool>,
    },
    Install {
        staged: PathBuf,
        package: Package,
    },
    /// The new build is in place and will open once this process has exited.
    Exit,
}

pub struct Updater {
    /// `None` for development builds, which never update.
    pub running: Option<Build>,
    pub package: Result<Package, String>,
    pub auto_download: bool,
    pub phase: Phase,
    /// The newer release the phase refers to.
    pub release: Option<Release>,
    pub staged: Option<PathBuf>,
    pub checking: bool,
    /// The last check found nothing newer.
    pub up_to_date: bool,
    /// Why the last check failed. Shown only on the Settings page.
    pub check_error: Option<String>,
    /// The notice was closed; it returns for a newer release, a failure, or the next launch.
    pub dismissed: bool,
    pub next_check: Option<Instant>,
    generation: u64,
    cancel: Option<Arc<AtomicBool>>,
}

impl Updater {
    pub fn new(running: Option<Build>, package: Result<Package, String>, auto: bool) -> Self {
        Self {
            running,
            package,
            auto_download: auto,
            phase: Phase::Idle,
            release: None,
            staged: None,
            checking: false,
            up_to_date: false,
            check_error: None,
            dismissed: false,
            next_check: None,
            generation: 0,
            cancel: None,
        }
    }

    /// Why this copy can't update itself, if it can't.
    pub fn unsupported(&self) -> Option<&str> {
        match (&self.running, &self.package) {
            (None, _) => Some("This is a development build. Release builds update themselves."),
            (Some(_), Err(reason)) => Some(reason),
            (Some(_), Ok(_)) => None,
        }
    }

    /// Whether the top-right notice shows.
    pub fn notice_visible(&self) -> bool {
        self.release.is_some() && !self.dismissed && self.phase != Phase::Idle
    }

    pub fn handle(&mut self, event: Event, now: Instant) -> Effect {
        match event {
            Event::Check => {
                let (Some(running), Ok(package)) = (self.running, self.package.clone()) else {
                    self.next_check = None;
                    return Effect::None;
                };
                self.next_check = Some(now + CHECK_INTERVAL);
                if self.checking || self.phase == Phase::Installing {
                    return Effect::None;
                }
                self.checking = true;
                Effect::Check { running, package }
            }
            Event::Checked(result) => {
                self.checking = false;
                match result {
                    Err(detail) => {
                        self.check_error = Some(detail);
                        Effect::None
                    }
                    Ok(None) => {
                        self.check_error = None;
                        self.up_to_date = true;
                        if matches!(
                            self.phase,
                            Phase::Idle
                                | Phase::Available
                                | Phase::Failed {
                                    retry: Retry::Download,
                                    ..
                                }
                        ) {
                            self.release = None;
                            self.phase = Phase::Idle;
                        }
                        Effect::None
                    }
                    Ok(Some(found)) => {
                        self.check_error = None;
                        self.up_to_date = false;
                        self.found(found)
                    }
                }
            }
            Event::Download => match self.phase {
                Phase::Available
                | Phase::Failed {
                    retry: Retry::Download,
                    ..
                } => self.start_download(),
                _ => Effect::None,
            },
            Event::Progress {
                generation,
                progress,
            } => {
                if generation == self.generation && matches!(self.phase, Phase::Downloading(_)) {
                    self.phase = Phase::Downloading(progress);
                }
                Effect::None
            }
            Event::Downloaded { generation, result } => {
                if generation != self.generation || !matches!(self.phase, Phase::Downloading(_)) {
                    return Effect::None;
                }
                self.cancel = None;
                match result {
                    Ok(staged) => {
                        self.staged = Some(staged);
                        self.phase = Phase::Ready;
                    }
                    Err(detail) => {
                        self.phase = Phase::Failed {
                            retry: Retry::Download,
                            detail,
                        }
                    }
                }
                Effect::None
            }
            Event::Restart => {
                let ready = matches!(
                    self.phase,
                    Phase::Ready
                        | Phase::Failed {
                            retry: Retry::Install,
                            ..
                        }
                );
                match (ready, self.staged.clone(), self.package.clone()) {
                    (true, Some(staged), Ok(package)) => {
                        self.phase = Phase::Installing;
                        Effect::Install { staged, package }
                    }
                    _ => Effect::None,
                }
            }
            Event::Installed(result) => {
                if self.phase != Phase::Installing {
                    return Effect::None;
                }
                match result {
                    Ok(()) => Effect::Exit,
                    Err(detail) => {
                        // The user asked for this; show what went wrong even after a dismissal.
                        self.dismissed = false;
                        self.phase = Phase::Failed {
                            retry: Retry::Install,
                            detail,
                        };
                        Effect::None
                    }
                }
            }
            Event::Dismiss => {
                self.dismissed = true;
                Effect::None
            }
            Event::AutoDownload(on) => {
                self.auto_download = on;
                if on && self.phase == Phase::Available {
                    self.start_download()
                } else {
                    Effect::None
                }
            }
        }
    }

    fn found(&mut self, found: Found) -> Effect {
        let same = self
            .release
            .as_ref()
            .is_some_and(|release| release.tag == found.release.tag);
        if same
            && matches!(
                self.phase,
                Phase::Downloading(_)
                    | Phase::Ready
                    | Phase::Installing
                    | Phase::Failed {
                        retry: Retry::Install,
                        ..
                    }
            )
        {
            return Effect::None;
        }
        if !same {
            // A newer build supersedes whatever was under way for the previous one.
            self.dismissed = false;
            self.staged = None;
            if let Some(cancel) = self.cancel.take() {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        self.release = Some(found.release);
        if let Some(staged) = found.staged {
            self.staged = Some(staged);
            self.phase = Phase::Ready;
            Effect::None
        } else if self.auto_download && !matches!(self.phase, Phase::Failed { .. } if same) {
            self.start_download()
        } else if !same || self.phase == Phase::Idle {
            self.phase = Phase::Available;
            Effect::None
        } else {
            Effect::None
        }
    }

    fn start_download(&mut self) -> Effect {
        let (Some(release), Ok(package)) = (self.release.clone(), self.package.clone()) else {
            return Effect::None;
        };
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        self.generation += 1;
        self.staged = None;
        self.phase = Phase::Downloading(Progress {
            received: 0,
            total: Some(release.asset.size).filter(|size| *size > 0),
            bytes_per_second: None,
        });
        Effect::Download {
            generation: self.generation,
            release,
            package,
            cancel,
        }
    }
}

// ---- network ----

fn client() -> Result<reqwest::Client, String> {
    // reqwest is built without a default provider so TLS uses ring, which needs no C toolchain
    // beyond the one Rust already has. Installing twice is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(concat!("Flexspeak/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("Cannot start an HTTPS client: {error}"))
}

fn describe(error: reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

/// Asks GitHub for the latest release and whether this copy already downloaded it.
pub async fn check(running: Build, package: Package) -> Result<Option<Found>, String> {
    check_at(
        &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
        running,
        package,
        &updates_dir()?,
    )
    .await
}

async fn check_at(
    url: &str,
    running: Build,
    package: Package,
    cache: &Path,
) -> Result<Option<Found>, String> {
    let response = client()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(describe)?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "GitHub shows no public release for {REPOSITORY}. Releases of a private repository aren't visible to the app."
        ));
    }
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err(
            "GitHub is limiting update checks from this network. Flexspeak tries again later."
                .into(),
        );
    }
    if !status.is_success() {
        return Err(format!("GitHub answered {status}."));
    }
    let body = response.text().await.map_err(describe)?;
    let Some(mut release) = select(&body, running, package.asset())? else {
        return Ok(None);
    };
    if let Some(notes) = missed_notes(url, running, release.build)
        .await
        .filter(|notes| !notes.is_empty())
    {
        *release.notes = notes;
    }
    forget_other_downloads(cache, &release.tag);
    let staged = staged(&cache.join(&release.tag), &release, package);
    Ok(Some(Found { release, staged }))
}

/// Where downloads wait to be installed.
fn updates_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("FLEXSPEAK_UPDATES_DIR") {
        return Ok(PathBuf::from(path));
    }
    directories::ProjectDirs::from("io", "Venator", "TeamSpeakNative")
        .map(|dirs| dirs.cache_dir().join("updates"))
        .ok_or_else(|| "Cannot locate a cache directory for updates.".into())
}

/// Removes downloads of other releases; they'd never be installed.
fn forget_other_downloads(cache: &Path, keep: &str) {
    let Ok(entries) = fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name() != keep {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// The marker a finished download leaves: the checksum it was verified against.
const VERIFIED: &str = "verified";

/// A verified, prepared download of `release` in `dir`, if one is complete and unchanged.
fn staged(dir: &Path, release: &Release, package: Package) -> Option<PathBuf> {
    let marker = fs::read_to_string(dir.join(VERIFIED)).ok()?;
    let mut lines = marker.lines();
    let expected = lines.next()?;
    if release
        .asset
        .digest
        .as_ref()
        .is_some_and(|digest| digest != expected)
    {
        return None;
    }
    let payload = payload_path(dir, package);
    if !payload.exists() || sha256_file(&dir.join(&release.asset.name)).ok()? != expected {
        return None;
    }
    // An executable unpacked from the archive is checked as well.
    match lines.next() {
        Some(unpacked) if sha256_file(&payload).ok()? != unpacked => None,
        _ => Some(payload),
    }
}

/// What [`VERIFIED`] holds: the archive's digest, then an unpacked executable's.
fn verified_marker(expected: &str, payload: &Path, package: Package) -> Result<String, String> {
    match package {
        Package::WindowsExe | Package::LinuxTarball => {
            let unpacked = sha256_file(payload)
                .map_err(|error| format!("Cannot read the unpacked update: {error}"))?;
            Ok(format!("{expected}\n{unpacked}"))
        }
        Package::MacApp | Package::LinuxPacman => Ok(expected.to_owned()),
    }
}

fn payload_path(dir: &Path, package: Package) -> PathBuf {
    match package {
        Package::WindowsExe => dir.join(WINDOWS_EXE),
        Package::MacApp => dir.join("Flexspeak.app"),
        Package::LinuxPacman => dir.join(LINUX_PACKAGE),
        Package::LinuxTarball => dir.join("extracted").join("flexspeak"),
    }
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    use std::io::{ErrorKind, Read};

    // Streamed in chunks: the hash crates no longer implement `io::Write`, and a whole
    // installer shouldn't sit in memory just to be hashed.
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => hasher.update(&chunk[..read]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Downloads and verifies `release`, then unpacks what [`install`] needs. Returns that payload.
pub async fn download(
    release: &Release,
    package: Package,
    cancel: &AtomicBool,
    on_progress: impl Fn(Progress),
) -> Result<PathBuf, String> {
    let dir = updates_dir()?.join(&release.tag);
    download_into(&dir, release, package, Ranges::DEFAULT, cancel, on_progress).await
}

/// How a download is split into parallel range requests.
#[derive(Debug, Clone, Copy)]
struct Ranges {
    connections: usize,
    part: u64,
}

impl Ranges {
    /// GitHub's release CDN serves each connection at a few hundred KiB/s at best, far below a
    /// typical line, so the parts come over several connections at once. Parts are small enough
    /// that the last one doesn't trail alone for long, and a failed one is cheap to fetch again.
    const DEFAULT: Ranges = Ranges {
        connections: 8,
        part: 2 * 1024 * 1024,
    };
    const ATTEMPTS: u32 = 3;
}

enum Failure {
    /// The server sent the whole file instead of a range; fetch it in one piece instead.
    NoRanges,
    Error(String),
}

impl From<String> for Failure {
    fn from(error: String) -> Self {
        Self::Error(error)
    }
}

const REPLACED: &str = "The download was replaced by a newer release.";

fn write_error(error: std::io::Error) -> String {
    format!("Cannot write the download: {error}")
}

/// Fetches `total` bytes of `url` into `file` as parts over `ranges.connections` connections,
/// each from its own client so the parts don't share one connection. `on_progress` gets the
/// bytes received so far.
async fn fetch_ranges(
    url: &str,
    total: u64,
    ranges: Ranges,
    file: &fs::File,
    cancel: &AtomicBool,
    on_progress: &dyn Fn(u64),
) -> Result<(), Failure> {
    use std::cell::Cell;
    let next = &Cell::new(0u64);
    let received = &Cell::new(0u64);
    let workers = (0..ranges.connections).map(|_| async move {
        let client = client()?;
        loop {
            let start = next.get();
            if start >= total {
                return Ok(());
            }
            let end = (start + ranges.part).min(total);
            next.set(end);
            let mut attempt = 1;
            loop {
                let mut got = 0;
                let result = fetch_part(&client, url, start, end, file, cancel, &mut |bytes| {
                    got += bytes;
                    received.set(received.get() + bytes);
                    on_progress(received.get());
                })
                .await;
                match result {
                    Ok(()) => break,
                    Err(Failure::Error(_))
                        if attempt < Ranges::ATTEMPTS && !cancel.load(Ordering::Relaxed) =>
                    {
                        // Start the part over; what it already wrote is written again.
                        received.set(received.get() - got);
                        attempt += 1;
                    }
                    Err(failure) => return Err(failure),
                }
            }
        }
    });
    futures::future::try_join_all(workers).await.map(|_| ())
}

/// Fetches bytes `start..end` of `url` into the same place in `file`.
async fn fetch_part(
    client: &reqwest::Client,
    url: &str,
    start: u64,
    end: u64,
    file: &fs::File,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), Failure> {
    let mut response = client
        .get(url)
        .header(reqwest::header::RANGE, format!("bytes={start}-{}", end - 1))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(describe)?;
    let range = format!("bytes {start}-{}/", end - 1);
    let matches = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with(&range));
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT || !matches {
        return Err(Failure::NoRanges);
    }
    let mut offset = start;
    while let Some(chunk) = response.chunk().await.map_err(describe)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(Failure::Error(REPLACED.into()));
        }
        if offset + chunk.len() as u64 > end {
            return Err(Failure::Error(
                "The server sent more than was asked for.".into(),
            ));
        }
        write_at(file, &chunk, offset).map_err(write_error)?;
        offset += chunk.len() as u64;
        on_bytes(chunk.len() as u64);
    }
    if offset != end {
        return Err(Failure::Error(format!(
            "A part of the download ended after {} of {} bytes.",
            offset - start,
            end - start
        )));
    }
    Ok(())
}

/// Fetches all of `url` into `file` over one connection.
async fn fetch_whole(
    client: &reqwest::Client,
    url: &str,
    total: Option<u64>,
    file: &fs::File,
    cancel: &AtomicBool,
    on_progress: &dyn Fn(u64, Option<u64>),
) -> Result<(), String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(describe)?;
    let total = response.content_length().filter(|size| *size > 0).or(total);
    let mut received = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(describe)? {
        if cancel.load(Ordering::Relaxed) {
            return Err(REPLACED.into());
        }
        write_at(file, &chunk, received).map_err(write_error)?;
        received += chunk.len() as u64;
        on_progress(received, total);
    }
    if total.is_some_and(|total| total != received) {
        return Err(format!(
            "The download ended after {received} of {} bytes.",
            total.unwrap_or_default()
        ));
    }
    Ok(())
}

fn write_at(file: &fs::File, mut bytes: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !bytes.is_empty() {
        #[cfg(unix)]
        let written = std::os::unix::fs::FileExt::write_at(file, bytes, offset)?;
        #[cfg(windows)]
        let written = std::os::windows::fs::FileExt::seek_write(file, bytes, offset)?;
        if written == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        bytes = &bytes[written..];
        offset += written as u64;
    }
    Ok(())
}

async fn download_into(
    dir: &Path,
    release: &Release,
    package: Package,
    ranges: Ranges,
    cancel: &AtomicBool,
    on_progress: impl Fn(Progress),
) -> Result<PathBuf, String> {
    let _ = fs::remove_file(dir.join(VERIFIED));
    fs::create_dir_all(dir).map_err(|error| format!("Cannot create {}: {error}", dir.display()))?;
    let client = client()?;
    let sums = client
        .get(&release.checksums_url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(describe)?
        .text()
        .await
        .map_err(describe)?;
    let expected = checksum_for(&sums, &release.asset.name)?;
    if release
        .asset
        .digest
        .as_ref()
        .is_some_and(|digest| *digest != expected)
    {
        return Err(format!(
            "{CHECKSUMS} and GitHub disagree about {}; the release was not downloaded.",
            release.asset.name
        ));
    }

    let partial = dir.join(format!("{}.part", release.asset.name));
    let result = async {
        let file = fs::File::create(&partial)
            .map_err(|error| format!("Cannot write {}: {error}", partial.display()))?;
        let meter = std::cell::RefCell::new(SpeedMeter::new(Instant::now()));
        let total = Some(release.asset.size).filter(|size| *size > 0);
        let report = |received: u64, total: Option<u64>| {
            if let Some(speed) = meter.borrow_mut().sample(Instant::now(), received) {
                on_progress(Progress {
                    received,
                    total,
                    bytes_per_second: Some(speed),
                });
            }
        };
        let ranged = match total {
            Some(total) if total > ranges.part => {
                file.set_len(total)
                    .map_err(|error| format!("Cannot write the download: {error}"))?;
                match fetch_ranges(
                    &release.asset.url,
                    total,
                    ranges,
                    &file,
                    cancel,
                    &|received| report(received, Some(total)),
                )
                .await
                {
                    Ok(()) => true,
                    Err(Failure::NoRanges) => false,
                    Err(Failure::Error(error)) => return Err(error),
                }
            }
            _ => false,
        };
        if !ranged {
            file.set_len(0)
                .map_err(|error| format!("Cannot write the download: {error}"))?;
            fetch_whole(&client, &release.asset.url, total, &file, cancel, &report).await?;
        }
        file.sync_all()
            .map_err(|error| format!("Cannot write the download: {error}"))?;
        drop(file);
        let actual =
            sha256_file(&partial).map_err(|error| format!("Cannot read the download: {error}"))?;
        if actual != expected {
            return Err(format!(
                "{} failed verification (SHA-256 {actual}, expected {expected}) and was discarded.",
                release.asset.name
            ));
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    let archive = dir.join(&release.asset.name);
    fs::rename(&partial, &archive).map_err(|error| format!("Cannot keep the download: {error}"))?;
    let payload = prepare(dir, &archive, package)?;
    fs::write(
        dir.join(VERIFIED),
        verified_marker(&expected, &payload, package)?,
    )
    .map_err(|error| format!("Cannot record the verified download: {error}"))?;
    Ok(payload)
}

pub(crate) fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("{what}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    Err(format!("{what} ({}): {}", output.status, detail.trim()))
}

/// Unpacks the verified archive into what [`install`] puts in place.
fn prepare(dir: &Path, archive: &Path, package: Package) -> Result<PathBuf, String> {
    let payload = payload_path(dir, package);
    match package {
        Package::LinuxPacman => {}
        Package::WindowsExe => unzip_exe(archive, &payload)?,
        Package::MacApp => {
            let mount = dir.join("mount");
            let _ = fs::create_dir_all(&mount);
            run(
                Command::new("/usr/bin/hdiutil")
                    .args([
                        "attach",
                        "-nobrowse",
                        "-readonly",
                        "-noautoopen",
                        "-mountpoint",
                    ])
                    .arg(&mount)
                    .arg(archive),
                "Cannot open the disk image",
            )?;
            let copied = (|| {
                let _ = fs::remove_dir_all(&payload);
                run(
                    Command::new("/usr/bin/ditto")
                        .arg(mount.join("Flexspeak.app"))
                        .arg(&payload),
                    "Cannot copy Flexspeak.app from the disk image",
                )?;
                verify_app(&payload)
            })();
            if run(
                Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount),
                "detach",
            )
            .is_err()
            {
                let _ = run(
                    Command::new("/usr/bin/hdiutil")
                        .args(["detach", "-force"])
                        .arg(&mount),
                    "detach",
                );
            }
            copied?;
        }
        Package::LinuxTarball => {
            let extracted = dir.join("extracted");
            let _ = fs::remove_dir_all(&extracted);
            fs::create_dir_all(&extracted)
                .map_err(|error| format!("Cannot unpack the update: {error}"))?;
            run(
                Command::new("tar")
                    .arg("-xzf")
                    .arg(archive)
                    .arg("-C")
                    .arg(&extracted)
                    .arg("flexspeak"),
                "Cannot unpack the update",
            )?;
            if !payload.is_file() {
                return Err("The Linux archive has no flexspeak executable.".into());
            }
        }
    }
    Ok(payload)
}

/// Writes the executable from the Windows ZIP to `payload`.
fn unzip_exe(archive: &Path, payload: &Path) -> Result<(), String> {
    let failed = |error: &dyn std::fmt::Display| format!("Cannot unpack the update: {error}");
    let file = fs::File::open(archive).map_err(|error| failed(&error))?;
    let mut zip =
        zip::ZipArchive::new(std::io::BufReader::new(file)).map_err(|error| failed(&error))?;
    let mut entry = zip
        .by_name(WINDOWS_ZIP_EXE)
        .map_err(|_| format!("The Windows archive has no {WINDOWS_ZIP_EXE}."))?;
    let unpacking = sibling(payload, ".unpacking");
    let written = (|| {
        let mut out = fs::File::create(&unpacking)?;
        std::io::copy(&mut entry, &mut out)?;
        out.sync_all()?;
        fs::rename(&unpacking, payload)
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&unpacking);
        return Err(failed(&error));
    }
    Ok(())
}

/// The app is intact (signature) and is Flexspeak (bundle identifier).
pub(crate) fn verify_app(app: &Path) -> Result<(), String> {
    run(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict"])
            .arg(app),
        "The downloaded app's signature is invalid",
    )?;
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .map_err(|error| format!("Cannot read the downloaded app: {error}"))?;
    let identifier = String::from_utf8_lossy(&output.stdout);
    if identifier.trim() != BUNDLE_ID {
        return Err(format!(
            "The downloaded app is '{}', not Flexspeak.",
            identifier.trim()
        ));
    }
    Ok(())
}

// ---- installing ----

/// Puts the staged build in place of this one and starts a helper that opens it once this
/// process has exited. On failure the current installation is left as it was.
pub fn install(staged: &Path, package: Package) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("Cannot locate the running executable: {error}"))?;
    let pid = std::process::id();
    match package {
        Package::WindowsExe => {
            swap_file(staged, &exe)?;
            relaunch_windows(pid, &exe)
        }
        Package::MacApp => {
            let bundle = app_bundle(&exe)?;
            let old = swap_app(staged, &bundle, pid)?;
            relaunch_unix(
                "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; rm -rf \"$2\"; exec /usr/bin/open \"$3\"",
                &[
                    pid.to_string().as_ref(),
                    old.as_os_str(),
                    bundle.as_os_str(),
                ],
            )
        }
        Package::LinuxPacman => {
            // pacman owns /usr/bin; polkit asks for the administrator password.
            let status = Command::new("pkexec")
                .args(["pacman", "-U", "--noconfirm"])
                .arg(staged)
                .stdin(Stdio::null())
                .status()
                .map_err(|error| format!("Cannot run pkexec to install the package: {error}"))?;
            match status.code() {
                Some(0) => {}
                Some(126 | 127) => {
                    return Err(
                        "Installing needs administrator approval, which was not given.".into(),
                    );
                }
                _ => return Err(format!("pacman could not install the update ({status}).")),
            }
            relaunch_unix(
                "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$2\"",
                &[pid.to_string().as_ref(), PACMAN_EXECUTABLE.as_ref()],
            )
        }
        Package::LinuxTarball => {
            swap_file(staged, &exe)?;
            relaunch_unix(
                "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; exec \"$2\"",
                &[pid.to_string().as_ref(), exe.as_os_str()],
            )
        }
    }
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Replaces the running executable. Windows lets a running `.exe` be renamed but not
/// overwritten, so the old one steps aside to `.old` (removed by [`clean_up`] next launch);
/// on Linux the rename replaces the path while the running process keeps its inode.
fn swap_file(staged: &Path, exe: &Path) -> Result<(), String> {
    let incoming = sibling(exe, ".new");
    let old = sibling(exe, ".old");
    let failed = |error: std::io::Error| {
        format!(
            "Cannot replace {}: {error}. Check that you can write to {}.",
            exe.display(),
            exe.parent().unwrap_or(exe).display()
        )
    };
    fs::copy(staged, &incoming).map_err(failed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&incoming, fs::Permissions::from_mode(0o755)).map_err(failed)?;
    }
    if cfg!(windows) {
        let _ = fs::remove_file(&old);
        if let Err(error) = fs::rename(exe, &old) {
            let _ = fs::remove_file(&incoming);
            return Err(failed(error));
        }
    }
    if let Err(error) = fs::rename(&incoming, exe) {
        if cfg!(windows) {
            let _ = fs::rename(&old, exe);
        }
        let _ = fs::remove_file(&incoming);
        return Err(failed(error));
    }
    Ok(())
}

/// Moves the staged `.app` into `bundle`'s place and returns where the old one went. The copy
/// lands beside the target first, so the final swap is two renames on one volume.
fn swap_app(staged: &Path, bundle: &Path, pid: u32) -> Result<PathBuf, String> {
    let parent = bundle
        .parent()
        .ok_or("Flexspeak.app has no parent folder.")?;
    let incoming = parent.join(".Flexspeak-update.app");
    let old = parent.join(format!(".Flexspeak-replaced-{pid}.app"));
    let _ = fs::remove_dir_all(&incoming);
    let denied = |error: String| {
        format!(
            "Cannot update {}: {error}. Check that you can write to {}.",
            bundle.display(),
            parent.display()
        )
    };
    run(
        Command::new("/usr/bin/ditto").arg(staged).arg(&incoming),
        "copy failed",
    )
    .map_err(denied)?;
    if let Err(error) = verify_app(&incoming) {
        let _ = fs::remove_dir_all(&incoming);
        return Err(error);
    }
    if let Err(error) = fs::rename(bundle, &old) {
        let _ = fs::remove_dir_all(&incoming);
        return Err(denied(error.to_string()));
    }
    if let Err(error) = fs::rename(&incoming, bundle) {
        let _ = fs::rename(&old, bundle);
        let _ = fs::remove_dir_all(&incoming);
        return Err(denied(error.to_string()));
    }
    Ok(old)
}

/// Runs `script` under `sh` with `args` as `$1…`, detached from this process's group so it
/// outlives the window.
pub(crate) fn relaunch_unix(script: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(script)
        .arg("flexspeak-relaunch")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map(drop).map_err(|error| {
        format!("Installed, but Flexspeak could not be reopened: {error}. Open it again yourself.")
    })
}

pub(crate) fn relaunch_windows(pid: u32, exe: &Path) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!(
                "Wait-Process -Id {pid} -ErrorAction SilentlyContinue; Start-Process -FilePath $env:FLEXSPEAK_RELAUNCH"
            ),
        ])
        // The path travels in the environment, so no quoting can break the command.
        .env("FLEXSPEAK_RELAUNCH", exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.spawn().map(drop).map_err(|error| {
        format!("Installed, but Flexspeak could not be reopened: {error}. Open it again yourself.")
    })
}

/// Removes what a previous update left beside the executable.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(sibling(&exe, ".old"));
    }
}

/// Runs `future` on this worker thread.
pub fn block_on<T>(future: impl Future<Output = Result<T, String>>) -> Result<T, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Could not start a background task: {error}"))?
        .block_on(future)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn notes_are_sorted_into_sections_and_combined_across_skipped_builds() {
        let mut one = Notes::default();
        one.add("## New\n\n- Themes\n- Themes\n\n## Fixed\n* A crash\n\n## Other\n- Ignored");
        assert_eq!(one.new, ["Themes"]);
        assert_eq!(one.fixed, ["A crash"]);
        assert!(one.changed.is_empty());
        let mut old = Notes::default();
        old.add("Automatic build from main.\n\n- A plain list isn't a section");
        assert!(old.is_empty());

        let json = r###"[
            {"tag_name":"v0.1.0-build.23","body":"## New\n- Too new"},
            {"tag_name":"v0.1.0-build.22","body":"## New\n- Latest\n## Changed\n- Tweak"},
            {"tag_name":"v0.1.0-build.21","body":"## Fixed\n- Older fix"},
            {"tag_name":"v0.1.0-build.20","body":"## New\n- Already have it"}
        ]"###;
        let build = |n: &str| Build::parse_tag(&format!("v0.1.0-build.{n}")).unwrap();
        let notes = notes_between(json, build("20"), build("22")).unwrap();
        assert_eq!(notes.new, ["Latest"]);
        assert_eq!(notes.fixed, ["Older fix"]);
        assert_eq!(notes.changed, ["Tweak"]);
        assert!(notes_between("not json", build("20"), build("22")).is_none());
    }

    const RUNNING: Build = Build {
        major: 0,
        minor: 1,
        patch: 0,
        number: 6,
    };

    fn digest(bytes: &[u8]) -> String {
        hex(&Sha256::digest(bytes))
    }

    fn release_json(tag: &str, assets: &[(&str, Option<&str>)]) -> String {
        let assets: Vec<_> = assets
            .iter()
            .map(|(name, digest)| {
                serde_json::json!({
                    "name": name,
                    "size": 10,
                    "state": "uploaded",
                    "digest": digest,
                    "browser_download_url": format!("https://github.com/{REPOSITORY}/releases/download/{tag}/{name}"),
                })
            })
            .collect();
        serde_json::json!({ "tag_name": tag, "draft": false, "prerelease": false, "assets": assets })
            .to_string()
    }

    fn release(number: u64) -> Release {
        let tag = format!("v0.1.0-build.{number}");
        select(
            &release_json(&tag, &[(MACOS_DMG, None), (CHECKSUMS, None)]),
            RUNNING,
            MACOS_DMG,
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn tags_parse_strictly_and_compare_by_version_then_build() {
        assert_eq!(
            Build::parse_tag("v0.1.0-build.12"),
            Some(Build {
                number: 12,
                ..RUNNING
            })
        );
        for bad in [
            "0.1.0-build.1",
            "v0.1-build.1",
            "v0.1.0.1-build.1",
            "v0.1.0-build.",
            "v0.1.0-build.+1",
            "v0.1.0-build.1a",
            "v0.1.x-build.1",
            "v0.1.0",
        ] {
            assert_eq!(Build::parse_tag(bad), None, "{bad}");
        }
        let at = |tag| Build::parse_tag(tag).unwrap();
        assert!(
            at("v0.1.0-build.10") > at("v0.1.0-build.9"),
            "numeric, not lexical"
        );
        assert!(
            at("v0.2.0-build.1") > at("v0.1.9-build.99"),
            "version first"
        );
        assert!(at("v1.0.0-build.1") > at("v0.10.0-build.1"));
        assert_eq!(RUNNING.label(), "0.1.0 · build 6");
        // A plain `cargo build` has no build number and never updates itself.
        assert_eq!(
            Build::running().is_some(),
            option_env!("NATIVE_VOICE_BUILD_NUMBER").is_some()
        );
    }

    #[test]
    fn each_platform_takes_its_own_package() {
        let pick = |os, arch, exe: &str| Package::for_platform(os, arch, Path::new(exe));
        assert_eq!(
            pick("windows", "x86_64", r"C:\Flexspeak\Flexspeak.exe"),
            Ok(Package::WindowsExe)
        );
        assert_eq!(
            pick(
                "macos",
                "aarch64",
                "/Applications/Flexspeak.app/Contents/MacOS/flexspeak"
            ),
            Ok(Package::MacApp)
        );
        assert!(pick("macos", "aarch64", "/Users/me/target/release/flexspeak").is_err());
        assert!(
            pick(
                "macos",
                "aarch64",
                "/private/var/folders/x/AppTranslocation/1/d/Flexspeak.app/Contents/MacOS/flexspeak"
            )
            .unwrap_err()
            .contains("Move Flexspeak.app")
        );
        assert_eq!(
            pick("linux", "x86_64", "/usr/bin/flexspeak"),
            Ok(Package::LinuxPacman)
        );
        assert_eq!(
            pick("linux", "x86_64", "/home/me/bin/flexspeak"),
            Ok(Package::LinuxTarball)
        );
        assert!(
            pick(
                "macos",
                "x86_64",
                "/Applications/Flexspeak.app/Contents/MacOS/flexspeak"
            )
            .is_err()
        );
        assert!(pick("linux", "aarch64", "/usr/bin/flexspeak").is_err());
        assert_eq!(
            [
                Package::WindowsExe,
                Package::MacApp,
                Package::LinuxPacman,
                Package::LinuxTarball
            ]
            .map(Package::asset),
            [WINDOWS_ZIP, MACOS_DMG, LINUX_PACKAGE, LINUX_TARBALL]
        );
    }

    #[test]
    fn only_a_newer_complete_release_is_selected() {
        let hash = format!("sha256:{}", "AB".repeat(32));
        let newer = release_json(
            "v0.1.0-build.7",
            &[(LINUX_TARBALL, Some(&hash)), (CHECKSUMS, None)],
        );
        let found = select(&newer, RUNNING, LINUX_TARBALL).unwrap().unwrap();
        assert_eq!(found.build.number, 7);
        assert_eq!(found.asset.name, LINUX_TARBALL);
        assert_eq!(
            found.asset.digest.as_deref(),
            Some("ab".repeat(32).as_str())
        );
        assert!(found.checksums_url.ends_with("/SHA256SUMS.txt"));

        // The running build, and older ones, are not updates.
        for tag in ["v0.1.0-build.6", "v0.1.0-build.5", "v0.0.9-build.99"] {
            let json = release_json(tag, &[(LINUX_TARBALL, None), (CHECKSUMS, None)]);
            assert_eq!(select(&json, RUNNING, LINUX_TARBALL), Ok(None), "{tag}");
        }
        // Drafts and prereleases never count.
        for flag in ["draft", "prerelease"] {
            let mut json: serde_json::Value = serde_json::from_str(&newer).unwrap();
            json[flag] = true.into();
            assert_eq!(select(&json.to_string(), RUNNING, LINUX_TARBALL), Ok(None));
        }
        // A newer release without this platform's package was not built for it: no update.
        let other_platform =
            release_json("v0.1.0-build.7", &[(WINDOWS_EXE, None), (CHECKSUMS, None)]);
        assert_eq!(select(&other_platform, RUNNING, LINUX_TARBALL), Ok(None));
        // A newer release that can't be installed or verified is a (quiet) check failure.
        let unverifiable = release_json("v0.1.0-build.7", &[(LINUX_TARBALL, None)]);
        assert!(select(&unverifiable, RUNNING, LINUX_TARBALL).is_err());
        let doubled = release_json(
            "v0.1.0-build.7",
            &[
                (LINUX_TARBALL, None),
                (LINUX_TARBALL, None),
                (CHECKSUMS, None),
            ],
        );
        assert!(select(&doubled, RUNNING, LINUX_TARBALL).is_err());
        let bad_digest = release_json(
            "v0.1.0-build.7",
            &[(LINUX_TARBALL, Some("md5:00")), (CHECKSUMS, None)],
        );
        assert!(select(&bad_digest, RUNNING, LINUX_TARBALL).is_err());
        let mut uploading: serde_json::Value = serde_json::from_str(&newer).unwrap();
        uploading["assets"][0]["state"] = "starter".into();
        assert!(select(&uploading.to_string(), RUNNING, LINUX_TARBALL).is_err());
        assert!(select(&release_json("nightly", &[]), RUNNING, LINUX_TARBALL).is_err());
        assert!(select("<html>", RUNNING, LINUX_TARBALL).is_err());
    }

    #[test]
    fn checksums_file_names_exactly_one_digest_per_asset() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums = format!("{a}  {MACOS_DMG}\n{b} *{WINDOWS_EXE}\n{a}  {MACOS_DMG}.sig\n");
        assert_eq!(checksum_for(&sums, MACOS_DMG), Ok(a.clone()));
        assert_eq!(checksum_for(&sums, WINDOWS_EXE), Ok("b".repeat(64)));
        assert!(checksum_for(&sums, LINUX_TARBALL).is_err());
        assert!(checksum_for(&format!("{a}  {MACOS_DMG}\n{a}  {MACOS_DMG}\n"), MACOS_DMG).is_err());
        assert!(checksum_for(&format!("abc  {MACOS_DMG}\n"), MACOS_DMG).is_err());
    }

    #[test]
    fn speed_is_reported_a_few_times_a_second_and_smoothed() {
        let start = Instant::now();
        let mut meter = SpeedMeter::new(start);
        assert_eq!(
            meter.sample(start + Duration::from_millis(100), 1_000),
            None
        );
        assert_eq!(
            meter.sample(start + Duration::from_millis(500), 1_000_000),
            Some(2_000_000.0)
        );
        let next = meter
            .sample(start + Duration::from_millis(1_000), 1_500_000)
            .unwrap();
        assert!((next - 1_600_000.0).abs() < 1.0, "{next}");
        let progress = Progress {
            received: 50,
            total: Some(200),
            bytes_per_second: None,
        };
        assert_eq!(progress.fraction(), Some(0.25));
        assert_eq!(
            Progress {
                total: Some(0),
                ..progress
            }
            .fraction(),
            None
        );
    }

    fn updater(auto: bool) -> Updater {
        Updater::new(Some(RUNNING), Ok(Package::MacApp), auto)
    }

    fn found(number: u64) -> Event {
        Event::Checked(Ok(Some(Found {
            release: release(number),
            staged: None,
        })))
    }

    #[test]
    fn manual_download_shows_progress_then_restart_installs_and_exits() {
        let now = Instant::now();
        let mut u = updater(false);
        assert!(matches!(u.handle(Event::Check, now), Effect::Check { .. }));
        assert!(u.checking);
        assert_eq!(u.next_check, Some(now + CHECK_INTERVAL));
        assert!(
            matches!(u.handle(Event::Check, now), Effect::None),
            "one check at a time"
        );
        assert!(matches!(u.handle(found(7), now), Effect::None));
        assert_eq!(u.phase, Phase::Available);
        assert!(u.notice_visible());

        let Effect::Download {
            generation,
            release,
            ..
        } = u.handle(Event::Download, now)
        else {
            panic!("download")
        };
        assert_eq!(release.build.number, 7);
        let progress = Progress {
            received: 4,
            total: Some(10),
            bytes_per_second: Some(2.0),
        };
        u.handle(
            Event::Progress {
                generation,
                progress,
            },
            now,
        );
        assert_eq!(u.phase, Phase::Downloading(progress));
        // Reports from a superseded download are ignored.
        u.handle(
            Event::Progress {
                generation: generation + 1,
                progress: Progress::default(),
            },
            now,
        );
        assert_eq!(u.phase, Phase::Downloading(progress));
        u.handle(
            Event::Downloaded {
                generation,
                result: Ok("/tmp/Flexspeak.app".into()),
            },
            now,
        );
        assert_eq!(u.phase, Phase::Ready);

        let Effect::Install { staged, package } = u.handle(Event::Restart, now) else {
            panic!("install")
        };
        assert_eq!(
            (staged.as_path(), package),
            (Path::new("/tmp/Flexspeak.app"), Package::MacApp)
        );
        assert_eq!(u.phase, Phase::Installing);
        assert!(
            matches!(u.handle(Event::Check, now), Effect::None),
            "no checks mid-install"
        );
        assert!(matches!(
            u.handle(Event::Installed(Ok(())), now),
            Effect::Exit
        ));
    }

    #[test]
    fn automatic_download_starts_on_discovery_and_a_staged_build_is_ready_at_once() {
        let now = Instant::now();
        let mut u = updater(true);
        u.handle(Event::Check, now);
        assert!(matches!(u.handle(found(7), now), Effect::Download { .. }));
        assert!(matches!(u.phase, Phase::Downloading(_)));
        // Re-checking the same release doesn't restart the download.
        u.handle(Event::Check, now);
        assert!(matches!(u.handle(found(7), now), Effect::None));

        let mut later = updater(true);
        let staged = Found {
            release: release(7),
            staged: Some("/cache/Flexspeak.app".into()),
        };
        assert!(matches!(
            later.handle(Event::Checked(Ok(Some(staged))), now),
            Effect::None
        ));
        assert_eq!(later.phase, Phase::Ready);

        // Turning the setting on downloads what's already known.
        let mut off = updater(false);
        off.handle(found(7), now);
        assert!(matches!(
            off.handle(Event::AutoDownload(true), now),
            Effect::Download { .. }
        ));
        assert!(off.auto_download);
    }

    #[test]
    fn failures_offer_a_retry_and_failed_checks_stay_quiet() {
        let now = Instant::now();
        let mut u = updater(false);
        u.handle(Event::Check, now);
        u.handle(Event::Checked(Err("offline".into())), now);
        assert!(!u.checking);
        assert_eq!(u.check_error.as_deref(), Some("offline"));
        assert!(!u.notice_visible(), "a failed check never interrupts");

        u.handle(found(7), now);
        assert_eq!(u.check_error, None);
        let Effect::Download {
            generation, cancel, ..
        } = u.handle(Event::Download, now)
        else {
            panic!()
        };
        u.handle(
            Event::Downloaded {
                generation,
                result: Err("reset".into()),
            },
            now,
        );
        assert_eq!(
            u.phase,
            Phase::Failed {
                retry: Retry::Download,
                detail: "reset".into()
            }
        );
        assert!(u.notice_visible());
        assert!(!cancel.load(Ordering::Relaxed));
        // A failed check later keeps the known release and its retry.
        u.handle(Event::Checked(Err("offline".into())), now);
        assert!(matches!(u.phase, Phase::Failed { .. }));
        let Effect::Download { generation, .. } = u.handle(Event::Download, now) else {
            panic!("retry")
        };
        u.handle(
            Event::Downloaded {
                generation,
                result: Ok("/tmp/a".into()),
            },
            now,
        );

        u.handle(Event::Dismiss, now);
        assert!(!u.notice_visible());
        u.handle(Event::Restart, now);
        u.handle(Event::Installed(Err("read-only".into())), now);
        assert_eq!(
            u.phase,
            Phase::Failed {
                retry: Retry::Install,
                detail: "read-only".into()
            }
        );
        assert!(
            u.notice_visible(),
            "an install the user asked for reports its failure"
        );
        assert!(matches!(
            u.handle(Event::Restart, now),
            Effect::Install { .. }
        ));
    }

    #[test]
    fn a_newer_release_supersedes_the_old_one_and_reappears_after_dismissal() {
        let now = Instant::now();
        let mut u = updater(true);
        let Effect::Download {
            generation, cancel, ..
        } = u.handle(found(7), now)
        else {
            panic!("automatic download")
        };
        u.handle(Event::Dismiss, now);
        assert!(matches!(u.handle(found(8), now), Effect::Download { .. }));
        assert!(cancel.load(Ordering::Relaxed), "the build 7 download stops");
        assert!(u.notice_visible());
        u.handle(
            Event::Downloaded {
                generation,
                result: Ok("/tmp/7".into()),
            },
            now,
        );
        assert!(
            matches!(u.phase, Phase::Downloading(_)),
            "late build 7 result ignored"
        );

        // Nothing newer any more: an undownloaded offer disappears, a ready one stays.
        let mut offered = updater(false);
        offered.handle(found(7), now);
        offered.handle(Event::Checked(Ok(None)), now);
        assert_eq!(
            (offered.phase.clone(), offered.release.is_none()),
            (Phase::Idle, true)
        );
        assert!(offered.up_to_date);
    }

    #[test]
    fn development_builds_and_unsupported_platforms_never_check() {
        let now = Instant::now();
        let mut dev = Updater::new(None, Ok(Package::MacApp), true);
        dev.next_check = Some(now);
        assert!(matches!(dev.handle(Event::Check, now), Effect::None));
        assert_eq!(dev.next_check, None);
        assert!(dev.unsupported().unwrap().contains("development build"));
        let mut other = Updater::new(Some(RUNNING), Err("No packages".into()), true);
        assert!(matches!(other.handle(Event::Check, now), Effect::None));
        assert_eq!(other.unsupported(), Some("No packages"));
        assert_eq!(updater(true).unsupported(), None);
    }

    /// Serves `routes` over plain HTTP on localhost, ignoring `Range`.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> String {
        serve_with(routes, false, false).0
    }

    /// Serves `routes` over plain HTTP on localhost, one request per connection. With `ranges`
    /// it answers `Range` requests with 206 and counts them; with `flaky` the first ranged
    /// answer stops halfway.
    fn serve_with(
        routes: Vec<(&'static str, Vec<u8>)>,
        ranges: bool,
        flaky: bool,
    ) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let ranged = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = ranged.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
                let mut range = None;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header.trim().is_empty() {
                        break;
                    }
                    let lower = header.to_ascii_lowercase();
                    if let Some(value) = lower.strip_prefix("range: bytes=") {
                        let (start, end) = value.trim().split_once('-').unwrap();
                        range = Some((
                            start.parse::<usize>().unwrap(),
                            end.parse::<usize>().unwrap(),
                        ));
                    }
                }
                let body = routes
                    .iter()
                    .find(|(route, _)| *route == path)
                    .map(|(_, body)| body.clone());
                let (head, body) = match (body, range) {
                    (Some(body), Some((start, end))) if ranges => {
                        let n = counter.fetch_add(1, Ordering::Relaxed);
                        let part = body[start..=end].to_vec();
                        let head = format!(
                            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len(),
                            part.len()
                        );
                        let sent = if flaky && n == 0 {
                            part[..part.len() / 2].to_vec()
                        } else {
                            part
                        };
                        (head, sent)
                    }
                    (Some(body), _) => (
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ),
                        body,
                    ),
                    (None, _) => (
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .into(),
                        Vec::new(),
                    ),
                };
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        (address, ranged)
    }

    /// A Windows release ZIP holding `exe`, laid out as scripts/release.py writes it.
    fn windows_zip(exe: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("README.md", options).unwrap();
        zip.write_all(b"read me").unwrap();
        zip.start_file(WINDOWS_ZIP_EXE, options).unwrap();
        zip.write_all(exe).unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn windows_release(base: &str, archive: &[u8], digest: Option<String>) -> Release {
        Release {
            tag: "v0.1.0-build.7".into(),
            build: Build {
                number: 7,
                ..RUNNING
            },
            asset: Asset {
                name: WINDOWS_ZIP.into(),
                url: format!("{base}/zip"),
                size: archive.len() as u64,
                digest,
            },
            checksums_url: format!("{base}/sums"),
            notes: Box::default(),
        }
    }

    /// Small parts, so a test-sized file takes several.
    const TEST_RANGES: Ranges = Ranges {
        connections: 3,
        part: 4096,
    };

    #[test]
    fn downloads_come_in_parallel_parts_retry_a_dropped_one_and_fall_back_to_one_piece() {
        // Noise, so the ZIP doesn't deflate it below a few parts.
        let mut state = 0x2545_f491_u32;
        let exe: Vec<u8> = (0..40_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        let archive = windows_zip(&exe);
        assert!(archive.len() as u64 > TEST_RANGES.part * 4, "several parts");
        for (ranges, flaky) in [(true, false), (true, true), (false, false)] {
            let sums = format!("{}  {WINDOWS_ZIP}\n", digest(&archive));
            let (base, ranged) = serve_with(
                vec![("/sums", sums.into_bytes()), ("/zip", archive.clone())],
                ranges,
                flaky,
            );
            let release = windows_release(&base, &archive, Some(digest(&archive)));
            let dir = scratch("parts");
            let staged_path = block_on(download_into(
                &dir,
                &release,
                Package::WindowsExe,
                TEST_RANGES,
                &AtomicBool::new(false),
                |_| {},
            ))
            .unwrap_or_else(|error| panic!("ranges {ranges}, flaky {flaky}: {error}"));
            assert_eq!(
                fs::read(&staged_path).unwrap(),
                exe,
                "ranges {ranges}, flaky {flaky}"
            );
            let parts = (archive.len() as u64).div_ceil(TEST_RANGES.part) as usize;
            let expected = if !ranges {
                0
            } else {
                parts + usize::from(flaky)
            };
            assert_eq!(
                ranged.load(Ordering::Relaxed),
                expected,
                "ranges {ranges}, flaky {flaky}"
            );
            fs::remove_dir_all(dir).unwrap();
        }
    }

    /// A fresh directory per call. Only digits after the name: `{:?}` of an `Instant` has
    /// `:` and spaces, which Windows refuses in a directory name.
    fn scratch(name: &str) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "flexspeak-update-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // A reused process id may have left one behind.
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn downloads_are_verified_staged_and_found_again_after_a_restart() {
        let payload = b"#!/bin/sh\necho new flexspeak\n".repeat(4096);
        let archive = windows_zip(&payload);
        let sums = format!("{}  {WINDOWS_ZIP}\n", digest(&archive));
        let base = serve(vec![
            ("/sums", sums.into_bytes()),
            ("/zip", archive.clone()),
        ]);
        let release = windows_release(&base, &archive, Some(digest(&archive)));
        let cache = scratch("verified");
        let dir = cache.join(&release.tag);
        let reports = std::cell::RefCell::new(Vec::new());
        let staged_path = block_on(download_into(
            &dir,
            &release,
            Package::WindowsExe,
            Ranges::DEFAULT,
            &AtomicBool::new(false),
            |progress| reports.borrow_mut().push(progress),
        ))
        .unwrap();
        assert_eq!(staged_path, dir.join(WINDOWS_EXE));
        assert_eq!(fs::read(&staged_path).unwrap(), payload);
        assert!(
            reports
                .borrow()
                .iter()
                .all(|p| p.total == Some(archive.len() as u64))
        );
        assert_eq!(
            staged(&dir, &release, Package::WindowsExe),
            Some(staged_path.clone())
        );

        // Tampering after the download, or a different expected digest, voids it.
        let changed = Release {
            asset: Asset {
                digest: Some("0".repeat(64)),
                ..release.asset.clone()
            },
            ..release.clone()
        };
        assert_eq!(staged(&dir, &changed, Package::WindowsExe), None);
        fs::write(&staged_path, b"tampered").unwrap();
        assert_eq!(staged(&dir, &release, Package::WindowsExe), None);

        // Older downloads are cleared when a newer release is found.
        fs::create_dir_all(cache.join("v0.1.0-build.5")).unwrap();
        forget_other_downloads(&cache, &release.tag);
        assert!(!cache.join("v0.1.0-build.5").exists());
        assert!(dir.exists());
        fs::remove_dir_all(cache).unwrap();
    }

    #[test]
    fn a_corrupt_download_is_discarded() {
        let archive = windows_zip(b"the real build");
        let sums = format!("{}  {WINDOWS_ZIP}\n", digest(b"a different build"));
        let base = serve(vec![
            ("/sums", sums.into_bytes()),
            ("/zip", archive.clone()),
        ]);
        let release = windows_release(&base, &archive, None);
        let dir = scratch("corrupt");
        let error = block_on(download_into(
            &dir,
            &release,
            Package::WindowsExe,
            Ranges::DEFAULT,
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap_err();
        assert!(error.contains("failed verification"), "{error}");
        assert!(!dir.join(WINDOWS_ZIP).exists());
        assert!(!dir.join(WINDOWS_EXE).exists());
        assert!(!dir.join(format!("{WINDOWS_ZIP}.part")).exists());
        assert!(!dir.join(VERIFIED).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_release_explains_private_repositories() {
        let base = serve(vec![]);
        let error = block_on(check_at(
            &format!("{base}/latest"),
            RUNNING,
            Package::MacApp,
            &scratch("missing"),
        ))
        .unwrap_err();
        assert!(error.contains("private"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn swapping_a_file_replaces_it_executable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("swap");
        let (staged, exe) = (dir.join("staged"), dir.join("flexspeak"));
        fs::write(&staged, b"new").unwrap();
        fs::write(&exe, b"old").unwrap();
        swap_file(&staged, &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(!sibling(&exe, ".new").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    /// Downloads the latest real Windows update from GitHub and reports how long it took.
    #[test]
    #[ignore = "Downloads the latest release from GitHub"]
    fn the_latest_github_release_downloads_and_unpacks() {
        let cache = scratch("github");
        let oldest = Build {
            number: 0,
            ..RUNNING
        };
        let found = block_on(check_at(
            &format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
            oldest,
            Package::WindowsExe,
            &cache,
        ))
        .unwrap()
        .expect("a release");
        let started = Instant::now();
        let exe = block_on(download_into(
            &cache.join(&found.release.tag),
            &found.release,
            Package::WindowsExe,
            Ranges::DEFAULT,
            &AtomicBool::new(false),
            |_| {},
        ))
        .unwrap();
        let elapsed = started.elapsed().as_secs_f64();
        let size = found.release.asset.size as f64;
        println!(
            "{} ({:.1} MB) in {elapsed:.1} s, {:.0} KiB/s; unpacked {:.1} MB",
            found.release.asset.name,
            size / 1e6,
            size / 1024.0 / elapsed,
            fs::metadata(&exe).unwrap().len() as f64 / 1e6
        );
        fs::remove_dir_all(cache).unwrap();
    }

    /// Unpacks a real release DMG and swaps it over a stand-in installation (no relaunch).
    #[test]
    #[ignore = "Needs a release DMG: FLEXSPEAK_TEST_DMG=path/to/Flexspeak-macos-arm64.dmg"]
    fn macos_dmg_is_unpacked_verified_and_swapped() {
        let Some(dmg) = std::env::var_os("FLEXSPEAK_TEST_DMG") else {
            return;
        };
        let dir = scratch("dmg");
        let archive = dir.join(MACOS_DMG);
        fs::copy(&dmg, &archive).unwrap();
        let app = prepare(&dir, &archive, Package::MacApp).unwrap();
        assert!(app.join("Contents/MacOS/flexspeak").is_file());
        assert!(
            !dir.join("mount/Flexspeak.app").exists(),
            "the image is detached"
        );

        let installed = dir.join("Applications/Flexspeak.app");
        fs::create_dir_all(installed.join("Contents")).unwrap();
        fs::write(installed.join("Contents/old"), b"old build").unwrap();
        let old = swap_app(&app, &installed, 1).unwrap();
        assert!(installed.join("Contents/MacOS/flexspeak").is_file());
        assert!(
            old.join("Contents/old").is_file(),
            "kept until the helper removes it"
        );
        assert!(!dir.join("Applications/.Flexspeak-update.app").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
