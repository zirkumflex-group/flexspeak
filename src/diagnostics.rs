//! The app's own record of what it did, kept on disk so a problem can be looked into after it
//! happened, by a person or by an agent.
//!
//! Every run of the client writes one log in the logs folder (`folder`), one JSON object per
//! line: `ts`, `level`, `target`, `src`, `thread`, then the event's message as `msg` and its
//! fields under their own names. A run's log is gzipped once it is finished, so the folder holds
//! `flexspeak-<started>-<part>.jsonl` for the running app and `….jsonl.gz` for earlier runs.
//! The oldest go when the folder passes `KEEP_BYTES` or a file passes `KEEP_DAYS`.
//!
//! Normally only Flexspeak's own information and every crate's warnings are written. Debug
//! logging (Settings > Error Log) adds Flexspeak's debug events, what you click and choose, and
//! the voice and streaming libraries' information, until it is turned off again. `RUST_LOG`
//! overrides both when set.
//!
//! A panic, and on Windows a native crash, is written with its location before the app goes.
//! The first line of every run says what the app and the computer are (`session start`); the
//! last says it closed normally (`session end`), so a log without one ended in a crash or kill.

use std::{
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError},
    },
    time::{Duration, SystemTime},
};

use anyhow::{Context as _, Result};
use tracing::{Event, Level, Metadata, Subscriber, field::Field, subscriber::Interest};
use tracing_log::NormalizeEvent;
use tracing_subscriber::{
    EnvFilter, Layer,
    layer::{self, Context, Filter, SubscriberExt},
    registry::LookupSpan,
    util::SubscriberInitExt,
};

/// Raised with each change to the line format, so a reader can tell which one a file has.
const FORMAT: u32 = 1;
/// What the file gets normally.
const NORMAL: &str = "warn,flexspeak=info,ts_bookkeeping=error";
/// What the file gets with debug logging on. The GPU and font crates stay at warnings: their
/// information is per frame or per glyph and says nothing about a session.
const DEBUG: &str = "info,flexspeak=debug,tsclientlib=info,tsproto=info,\
                     wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn,\
                     cosmic_text=warn,parley=warn,fontique=warn,rune=info,\
                     ts_bookkeeping=error";
/// What a terminal gets, where there is one.
const TERMINAL: &str = "flexspeak=info,tsclientlib=warn,tsproto=warn";
/// A part this big is closed and gzipped, and the run goes on in the next.
const PART_BYTES: u64 = 32 * 1024 * 1024;
/// The logs folder is kept under this, oldest first out; the running log always stays.
const KEEP_BYTES: u64 = 256 * 1024 * 1024;
/// Older logs go regardless of size.
const KEEP_DAYS: u64 = 30;
/// A finished-looking log modified more recently than this may still be written by an app
/// that is closing, such as the copy an update replaces, so it is gzipped on a later start.
const SETTLED: Duration = Duration::from_secs(10 * 60);
/// Lines waiting for the writer. A burst beyond this is counted and dropped, never waited for:
/// the audio and UI threads log too.
const QUEUE: usize = 16_384;
/// A diagnostics bundle takes logs newest first up to this much.
const BUNDLE_BYTES: u64 = 64 * 1024 * 1024;

static DEBUG_ON: AtomicBool = AtomicBool::new(false);
static ENV_OVERRIDE: AtomicBool = AtomicBool::new(false);
static DROPPED: AtomicU64 = AtomicU64::new(0);
static SINK: OnceLock<SyncSender<Message>> = OnceLock::new();
static CURRENT: OnceLock<PathBuf> = OnceLock::new();

enum Message {
    Line(String),
    Flush(SyncSender<()>),
    /// Stops writing and removes this run's log.
    Discard(SyncSender<()>),
}

/// Where the logs go: beside settings.json, in `logs`. `NATIVE_VOICE_LOGS` moves them.
pub fn folder() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NATIVE_VOICE_LOGS") {
        return Some(PathBuf::from(path));
    }
    directories::ProjectDirs::from("io", "Venator", "TeamSpeakNative")
        .map(|dirs| dirs.data_local_dir().join("logs"))
}

/// The running app's log, when it writes one.
pub fn current() -> Option<&'static Path> {
    CURRENT.get().map(PathBuf::as_path)
}

/// Sets up logging for this process. `file` is false for previews and command-line tools,
/// which don't write a log; `debug` is the saved debug-logging choice; `terminal` prints to
/// stderr as well, for a tool run from a terminal.
pub fn init(file: bool, debug: bool, terminal: bool) {
    DEBUG_ON.store(debug, Ordering::Relaxed);
    let env = std::env::var("RUST_LOG")
        .ok()
        .filter(|v| !v.trim().is_empty());
    ENV_OVERRIDE.store(env.is_some(), Ordering::Relaxed);
    let parse = |fallback: &str| {
        env.as_deref()
            .and_then(|env| EnvFilter::try_new(env).ok())
            .unwrap_or_else(|| EnvFilter::new(fallback))
    };
    let sink = file.then(|| start_writer().map_err(|error| format!("{error:#}")));
    let file_layer = match &sink {
        Some(Ok(())) => Some(JsonLines.with_filter(Switchable {
            normal: parse(NORMAL),
            debug: parse(DEBUG),
        })),
        _ => None,
    };
    // A release on Windows has no console, so there is nobody to print to.
    let terminal = terminal || cfg!(debug_assertions) || !cfg!(windows) || env.is_some();
    let terminal_layer = terminal.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(io::stderr)
            .with_filter(parse(TERMINAL))
    });
    let _ = tracing_subscriber::registry()
        .with(file_layer)
        .with(terminal_layer)
        .try_init();
    install_crash_hooks();
    match sink {
        Some(Ok(())) => session_start(debug),
        Some(Err(error)) => tracing::warn!("The log file can't be written: {error}"),
        None => {}
    }
}

/// Turns debug logging on or off for the rest of the run.
pub fn set_debug(on: bool) {
    if DEBUG_ON.swap(on, Ordering::Relaxed) != on {
        tracing::info!(target: "flexspeak::session", debug = on, "debug logging changed");
    }
}

/// Whether `RUST_LOG` decides what is written, so the switch does nothing.
pub fn env_override() -> bool {
    ENV_OVERRIDE.load(Ordering::Relaxed)
}

/// Writes out what is waiting, waiting at most `limit`.
pub fn flush(limit: Duration) {
    let Some(sink) = SINK.get() else {
        return;
    };
    if std::thread::current().name() == Some("log-writer") {
        return;
    }
    let (done, wait) = mpsc::sync_channel(1);
    if sink.try_send(Message::Flush(done)).is_ok() {
        let _ = wait.recv_timeout(limit);
    }
}

/// Gzips earlier runs' logs and removes the oldest, on a thread of its own. Only the one
/// client that runs does this, never a second launch or `--probe` beside it.
pub fn tidy_later() {
    if SINK.get().is_some() {
        std::thread::Builder::new()
            .name("log-tidy".into())
            .spawn(tidy)
            .ok();
    }
}

/// Removes this run's log, for a launch that only brought the running window forward and has
/// nothing worth keeping.
pub fn discard() {
    let Some(sink) = SINK.get() else {
        return;
    };
    let (done, wait) = mpsc::sync_channel(1);
    if sink.send(Message::Discard(done)).is_ok() {
        let _ = wait.recv_timeout(Duration::from_secs(2));
    }
}

/// The last line of a run that closed normally.
pub fn session_end() {
    tracing::info!(target: "flexspeak::session", "session end");
    flush(Duration::from_secs(2));
}

fn session_start(debug_logging: bool) {
    let (cpu, cores, memory_mb) = match sysinfo::System::new() {
        Ok(mut system) => {
            system.refresh_memory();
            system.refresh_cpu_list(sysinfo::CpuRefreshKind::nothing());
            let cpu = system
                .cpus()
                .first()
                .map(|cpu| cpu.brand().trim().to_owned())
                .unwrap_or_default();
            (
                cpu,
                system.cpus().len(),
                system.total_memory() / (1024 * 1024),
            )
        }
        Err(_) => (String::new(), 0, 0),
    };
    let build = option_env!("NATIVE_VOICE_BUILD_NUMBER").unwrap_or("development");
    let args: Vec<String> = std::env::args().skip(1).collect();
    tracing::info!(
        target: "flexspeak::session",
        format = FORMAT,
        version = env!("CARGO_PKG_VERSION"),
        build,
        os = sysinfo::System::long_os_version().unwrap_or_default(),
        kernel = sysinfo::System::kernel_long_version().unwrap_or_default(),
        arch = std::env::consts::ARCH,
        cpu,
        cores,
        memory_mb,
        exe = %std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default(),
        args = ?args,
        pid = std::process::id(),
        debug_logging,
        rust_log = std::env::var("RUST_LOG").unwrap_or_default(),
        "session start"
    );
}

/// Logs a panic, with its place and a backtrace, before the usual report; and on Windows a
/// native crash, which runs no Rust code at all otherwise.
fn install_crash_hooks() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(not text)".into());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_owned();
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(
            target: "flexspeak::crash",
            panic_thread = thread,
            location,
            backtrace = %backtrace,
            "panic: {message}"
        );
        flush(Duration::from_secs(1));
        previous(info);
    }));
    #[cfg(windows)]
    native::install();
}

#[cfg(windows)]
mod native {
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{HMODULE, MAX_PATH};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        EXCEPTION_POINTERS, SetUnhandledExceptionFilter,
    };
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleFileNameW, GetModuleHandleExW,
    };

    /// Lets Windows go on to its own crash handling.
    const CONTINUE_SEARCH: i32 = 0;

    pub fn install() {
        // SAFETY: `filter` is a plain function that lives as long as the process.
        unsafe {
            SetUnhandledExceptionFilter(Some(filter));
        }
    }

    unsafe extern "system" fn filter(pointers: *const EXCEPTION_POINTERS) -> i32 {
        // SAFETY: Windows passes valid exception pointers to an unhandled-exception filter.
        let (code, address) = unsafe {
            match pointers.as_ref().and_then(|p| p.ExceptionRecord.as_ref()) {
                Some(record) => (
                    record.ExceptionCode as u32,
                    record.ExceptionAddress as usize,
                ),
                None => (0, 0),
            }
        };
        let (module, offset) = module_of(address);
        tracing::error!(
            target: "flexspeak::crash",
            code = format!("{code:#010x}"),
            address = format!("{address:#x}"),
            module,
            offset = format!("{offset:#x}"),
            crash_thread = std::thread::current().name().unwrap_or("unnamed"),
            "native crash"
        );
        super::flush(Duration::from_secs(1));
        CONTINUE_SEARCH
    }

    /// The file the crashing code is from, and how far into it, which with the build number
    /// is enough to find the function.
    fn module_of(address: usize) -> (String, usize) {
        let mut module: HMODULE = std::ptr::null_mut();
        // SAFETY: the address is only looked up, never read; the out pointer is valid.
        let found = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address as *const u16,
                &mut module,
            )
        };
        if found == 0 {
            return (String::new(), 0);
        }
        let mut name = [0u16; MAX_PATH as usize];
        // SAFETY: the buffer's length is passed with it.
        let length = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) };
        let name = String::from_utf16_lossy(&name[..length as usize]);
        (name, address.wrapping_sub(module as usize))
    }
}

/// Normal or debug, switched at run time without rebuilding the subscriber.
struct Switchable {
    normal: EnvFilter,
    debug: EnvFilter,
}

impl Switchable {
    fn active(&self) -> &EnvFilter {
        if DEBUG_ON.load(Ordering::Relaxed) {
            &self.debug
        } else {
            &self.normal
        }
    }
}

impl<S> Filter<S> for Switchable {
    fn enabled(&self, meta: &Metadata<'_>, cx: &layer::Context<'_, S>) -> bool {
        <EnvFilter as Filter<S>>::enabled(self.active(), meta, cx)
    }

    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> Interest {
        let normal = <EnvFilter as Filter<S>>::callsite_enabled(&self.normal, meta);
        let debug = <EnvFilter as Filter<S>>::callsite_enabled(&self.debug, meta);
        if normal.is_never() && debug.is_never() {
            Interest::never()
        } else if normal.is_always() && debug.is_always() {
            Interest::always()
        } else {
            // Asked per event, so the switch takes effect straight away.
            Interest::sometimes()
        }
    }

    fn max_level_hint(&self) -> Option<tracing::level_filters::LevelFilter> {
        let normal = <EnvFilter as Filter<S>>::max_level_hint(&self.normal);
        let debug = <EnvFilter as Filter<S>>::max_level_hint(&self.debug);
        Some(normal?.max(debug?))
    }
}

/// Writes each event as one line of JSON for the writer thread.
struct JsonLines;

/// One event as a line of JSON, newline included.
fn format_line<S>(event: &Event<'_>, ctx: &Context<'_, S>) -> String
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let normalized = event.normalized_metadata();
    let meta = normalized.as_ref().unwrap_or_else(|| event.metadata());
    let mut line = String::with_capacity(256);
    line.push_str("{\"ts\":\"");
    let _ = write!(
        line,
        "{}",
        chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z")
    );
    line.push_str("\",\"level\":\"");
    line.push_str(level_name(*meta.level()));
    line.push_str("\",\"target\":");
    push_string(&mut line, meta.target());
    if let (Some(file), Some(number)) = (meta.file(), meta.line()) {
        line.push_str(",\"src\":");
        push_string(&mut line, &format!("{file}:{number}"));
    }
    line.push_str(",\"thread\":");
    let current = std::thread::current();
    match current.name() {
        Some(name) => push_string(&mut line, name),
        None => push_string(&mut line, &format!("{:?}", current.id())),
    }
    if let Some(scope) = ctx.event_scope(event) {
        let names: Vec<&str> = scope.from_root().map(|span| span.name()).collect();
        if !names.is_empty() {
            line.push_str(",\"spans\":");
            push_string(&mut line, &names.join(">"));
        }
    }
    event.record(&mut Fields(&mut line));
    line.push_str("}\n");
    line
}

impl<S> Layer<S> for JsonLines
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let Some(sink) = SINK.get() else {
            return;
        };
        let mut line = format_line(event, &ctx);
        let dropped = DROPPED.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            let note = format!(
                "{{\"ts\":\"{}\",\"level\":\"WARN\",\"target\":\"flexspeak::diagnostics\",\"msg\":\"lines dropped: the writer fell behind\",\"dropped\":{dropped}}}\n",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z")
            );
            line.insert_str(0, &note);
        }
        match sink.try_send(Message::Line(line)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                DROPPED.fetch_add(dropped + 1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

fn level_name(level: Level) -> &'static str {
    match level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

fn push_string(line: &mut String, value: &str) {
    match serde_json::to_string(value) {
        Ok(quoted) => line.push_str(&quoted),
        Err(_) => line.push_str("\"\""),
    }
}

/// The keys every line starts with. A field of the same name is written with a `_` before it.
const RESERVED: [&str; 7] = ["ts", "level", "target", "src", "thread", "spans", "msg"];

/// An event's fields as `,"name":value`. The message is `msg`; the `log` crate's own
/// bookkeeping fields are left out, as `target` and `src` already say it.
struct Fields<'a>(&'a mut String);

impl Fields<'_> {
    fn key(&mut self, field: &Field) -> bool {
        let name = field.name();
        if name.starts_with("log.") {
            return false;
        }
        self.0.push(',');
        if name == "message" {
            push_string(self.0, "msg");
        } else if RESERVED.contains(&name) {
            push_string(self.0, &format!("_{name}"));
        } else {
            push_string(self.0, name);
        }
        self.0.push(':');
        true
    }
}

impl tracing::field::Visit for Fields<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if self.key(field) {
            push_string(self.0, value);
        }
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        if self.key(field) {
            self.0.push_str(if value { "true" } else { "false" });
        }
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        if self.key(field) {
            let _ = write!(self.0, "{value}");
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if self.key(field) {
            let _ = write!(self.0, "{value}");
        }
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        if self.key(field) {
            if value.is_finite() {
                let _ = write!(self.0, "{value}");
            } else {
                push_string(self.0, &value.to_string());
            }
        }
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        if self.key(field) {
            push_string(self.0, &format!("{value:#}"));
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if self.key(field) {
            push_string(self.0, &format!("{value:?}"));
        }
    }
}

/// Opens this run's first part and starts the thread that writes to it.
fn start_writer() -> Result<()> {
    let folder = folder().context("Cannot locate the logs folder")?;
    fs::create_dir_all(&folder).with_context(|| format!("Cannot create {}", folder.display()))?;
    let started = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let (path, file) = open_part(&folder, &started, 1)?;
    let _ = CURRENT.set(path.clone());
    let (sink, lines) = mpsc::sync_channel(QUEUE);
    std::thread::Builder::new()
        .name("log-writer".into())
        .spawn(move || write_lines(lines, folder, started, path, file))
        .context("Cannot start the log writer")?;
    let _ = SINK.set(sink);
    Ok(())
}

fn open_part(folder: &Path, started: &str, part: u32) -> Result<(PathBuf, File)> {
    let mut path = folder.join(format!("flexspeak-{started}-{part:02}.jsonl"));
    let open = |path: &Path| OpenOptions::new().write(true).create_new(true).open(path);
    let file = match open(&path) {
        Ok(file) => file,
        // Two starts in one second: the process ID tells them apart.
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            path = folder.join(format!(
                "flexspeak-{started}-{part:02}-{}.jsonl",
                std::process::id()
            ));
            open(&path)?
        }
        Err(error) => return Err(error.into()),
    };
    Ok((path, file))
}

fn write_lines(
    lines: Receiver<Message>,
    folder: PathBuf,
    started: String,
    path: PathBuf,
    file: File,
) {
    let mut out = BufWriter::with_capacity(64 * 1024, file);
    let mut written = 0u64;
    let mut dirty = false;
    let mut part = 1;
    let mut path = path;
    loop {
        match lines.recv_timeout(Duration::from_secs(1)) {
            Ok(Message::Line(line)) => {
                if out.write_all(line.as_bytes()).is_ok() {
                    written += line.len() as u64;
                    dirty = true;
                }
                if written >= PART_BYTES {
                    let _ = out.flush();
                    part += 1;
                    match open_part(&folder, &started, part) {
                        Ok((next, file)) => {
                            let done = std::mem::replace(&mut path, next);
                            out = BufWriter::with_capacity(64 * 1024, file);
                            written = 0;
                            std::thread::spawn(move || compress(&done));
                        }
                        // Carries on in the full part rather than losing lines.
                        Err(_) => written = 0,
                    }
                }
            }
            Ok(Message::Flush(done)) => {
                let _ = out.flush();
                dirty = false;
                let _ = done.try_send(());
            }
            Ok(Message::Discard(done)) => {
                drop(out);
                let _ = fs::remove_file(&path);
                let _ = done.try_send(());
                return;
            }
            Err(RecvTimeoutError::Timeout) => {
                if dirty {
                    let _ = out.flush();
                    dirty = false;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = out.flush();
                return;
            }
        }
    }
}

/// Gzips a finished log beside itself and removes the original.
fn compress(path: &Path) -> Result<()> {
    let target = path.with_extension("jsonl.gz");
    let partial = path.with_extension("jsonl.gz.partial");
    {
        let mut input = File::open(path)?;
        let output = File::create(&partial)?;
        let mut encoder =
            flate2::write::GzEncoder::new(BufWriter::new(output), flate2::Compression::new(6));
        io::copy(&mut input, &mut encoder)?;
        encoder.finish()?.flush()?;
    }
    fs::rename(&partial, &target)?;
    fs::remove_file(path)?;
    Ok(())
}

/// Gzips earlier runs' logs and removes the oldest beyond the limits.
fn tidy() {
    let Some(folder) = folder() else {
        return;
    };
    let current = current().map(Path::to_path_buf);
    let now = SystemTime::now();
    for entry in logs(&folder) {
        let settled = now
            .duration_since(entry.modified)
            .is_ok_and(|age| age >= SETTLED);
        if entry.path.extension().is_some_and(|e| e == "jsonl")
            && Some(&entry.path) != current.as_ref()
            && settled
            && !in_use(&entry.path)
            && let Err(error) = compress(&entry.path)
        {
            tracing::debug!("Couldn't gzip {}: {error:#}", entry.path.display());
        }
        // Left behind by a gzip that didn't finish.
        if entry.path.extension().is_some_and(|e| e == "partial") && settled {
            let _ = fs::remove_file(&entry.path);
        }
    }
    let mut total = 0u64;
    for entry in logs(&folder).into_iter().rev() {
        if Some(&entry.path) == current.as_ref() {
            continue;
        }
        total += entry.size;
        let old = now
            .duration_since(entry.modified)
            .is_ok_and(|age| age > Duration::from_secs(KEEP_DAYS * 24 * 60 * 60));
        if total > KEEP_BYTES || old {
            let _ = fs::remove_file(&entry.path);
        }
    }
}

/// Whether another process still has `path` open, such as a `--probe` beside the client.
#[cfg(windows)]
fn in_use(path: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    // No sharing at all: it opens only when nobody else has it open.
    OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(path)
        .is_err()
}

#[cfg(not(windows))]
fn in_use(_: &Path) -> bool {
    false
}

struct LogFile {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

/// The logs in `folder`, oldest first: their names start with when the run started.
fn logs(folder: &Path) -> Vec<LogFile> {
    let mut found: Vec<LogFile> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("flexspeak-") && name.contains(".jsonl"))
        })
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            Some(LogFile {
                path: entry.path(),
                size: meta.len(),
                modified: meta.modified().ok()?,
            })
        })
        .collect();
    found.sort_by(|a, b| a.path.file_name().cmp(&b.path.file_name()));
    found
}

/// What a diagnostics bundle carries besides the logs, gathered on the UI thread.
pub struct Report {
    /// The settings file's path, read and stripped of secrets in the bundle.
    pub settings: Option<PathBuf>,
    /// The app's state worth seeing at a glance: devices, servers, streams.
    pub state: serde_json::Value,
    /// The Error Log as Settings shows it.
    pub errors: String,
}

/// Zips the recent logs, the settings without secrets, the app's state and the Error Log into
/// the Downloads folder (or the logs folder without one), and returns where.
pub fn save_bundle(report: Report) -> Result<PathBuf> {
    flush(Duration::from_secs(2));
    let folder = folder().context("Cannot locate the logs folder")?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let place = directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| folder.clone());
    let path = place.join(format!("flexspeak-diagnostics-{stamp}.zip"));
    let file = File::create(&path).with_context(|| format!("Cannot create {}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mut taken = 0u64;
    for entry in logs(&folder).into_iter().rev() {
        if taken >= BUNDLE_BYTES {
            break;
        }
        taken += entry.size;
        let Some(name) = entry.path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let gz = name.ends_with(".gz");
        zip.start_file(format!("logs/{name}"), if gz { stored } else { deflated })?;
        // The running log is copied as it is now; it may end mid-line.
        io::copy(&mut File::open(&entry.path)?, &mut zip)?;
    }

    let settings = match report.settings.as_deref().map(fs::read_to_string) {
        Some(Ok(text)) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(mut value) => {
                redact(&mut value);
                serde_json::to_string_pretty(&value)?
            }
            Err(error) => format!("settings.json doesn't parse: {error}"),
        },
        Some(Err(error)) => format!("settings.json can't be read: {error}"),
        None => "No settings file.".into(),
    };
    zip.start_file("settings.json", deflated)?;
    zip.write_all(settings.as_bytes())?;

    let about = serde_json::json!({
        "format": FORMAT,
        "saved": chrono::Local::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        "build": option_env!("NATIVE_VOICE_BUILD_NUMBER"),
        "os": sysinfo::System::long_os_version().ok(),
        "arch": std::env::consts::ARCH,
        "debug_logging": DEBUG_ON.load(Ordering::Relaxed),
        "log": current(),
        "state": report.state,
    });
    zip.start_file("state.json", deflated)?;
    zip.write_all(serde_json::to_string_pretty(&about)?.as_bytes())?;

    zip.start_file("errors.txt", deflated)?;
    zip.write_all(report.errors.as_bytes())?;
    zip.finish()?;
    tracing::info!(path = %path.display(), "diagnostics saved");
    Ok(path)
}

/// Blanks every value that would let someone in: passwords, sessions, tokens and keys.
fn redact(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                let key = key.to_ascii_lowercase();
                let secret = [
                    "password", "session", "token", "secret", "identity", "private",
                ]
                .iter()
                .any(|word| key.contains(word));
                let filled = match value {
                    serde_json::Value::String(text) => !text.is_empty(),
                    serde_json::Value::Null => false,
                    _ => true,
                };
                if secret && filled {
                    *value = serde_json::Value::String("(removed)".into());
                } else {
                    redact(value);
                }
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_blanks_secrets_and_keeps_the_rest() {
        let mut value = serde_json::json!({
            "nickname": "Jan",
            "monster_session": "sealed",
            "servers": [{"address": "ts.example", "password": "sealed", "name": "A"},
                        {"address": "b", "password": ""}],
            "keybinds": {"toggle_mute": "F1"},
            "settings_sync_version": 4
        });
        redact(&mut value);
        assert_eq!(value["nickname"], "Jan");
        assert_eq!(value["monster_session"], "(removed)");
        assert_eq!(value["servers"][0]["password"], "(removed)");
        assert_eq!(value["servers"][0]["address"], "ts.example");
        assert_eq!(value["servers"][1]["password"], "");
        assert_eq!(value["keybinds"]["toggle_mute"], "F1");
        assert_eq!(value["settings_sync_version"], 4);
    }

    #[test]
    fn events_are_written_as_json_lines() {
        use std::sync::{Arc, Mutex};
        struct Capture(Arc<Mutex<Vec<String>>>);
        impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
            fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
                self.0.lock().unwrap().push(format_line(event, &ctx));
            }
        }
        let lines = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(Capture(lines.clone()));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("connect");
            let _entered = span.enter();
            tracing::warn!(count = 3, ratio = 0.5, ok = true, who = ?"Jan", level = 7, "hello \"there\"");
        });
        let lines = lines.lock().unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with('\n'));
        let value: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(value["level"], "WARN");
        assert_eq!(value["msg"], "hello \"there\"");
        assert_eq!(value["count"], 3);
        assert_eq!(value["ratio"], 0.5);
        assert_eq!(value["ok"], true);
        assert_eq!(value["who"], "\"Jan\"");
        assert_eq!(value["spans"], "connect");
        assert_eq!(value["_level"], 7);
        assert!(value["src"].as_str().unwrap().contains("diagnostics.rs"));
        assert!(lines[0].starts_with("{\"ts\":\""));
    }

    #[test]
    fn compress_replaces_the_log_with_a_gzip_of_it() {
        let dir = std::env::temp_dir().join(format!("flexspeak-log-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("flexspeak-20260101-000000-01.jsonl");
        fs::write(&path, "{\"msg\":\"a\"}\n".repeat(1000)).unwrap();
        compress(&path).unwrap();
        assert!(!path.exists());
        let gz = dir.join("flexspeak-20260101-000000-01.jsonl.gz");
        let mut text = String::new();
        io::Read::read_to_string(
            &mut flate2::read::GzDecoder::new(File::open(&gz).unwrap()),
            &mut text,
        )
        .unwrap();
        assert_eq!(text.lines().count(), 1000);
        assert!(fs::metadata(&gz).unwrap().len() < 200);
        let listed = logs(&dir);
        assert_eq!(listed.len(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
