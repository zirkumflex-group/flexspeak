use std::{
    cell::Cell,
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::BufWriter,
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use cpu_time::{ProcessTime, ThreadTime};
use crossbeam_queue::ArrayQueue;
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};
use serde_json::json;

pub const MAX_DURATION: Duration = Duration::from_secs(30);
const CAPACITY: usize = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Category {
    Ui,
    Network,
    AudioProcessing,
    AudioCallbacks,
}

impl Category {
    pub const ALL: [Self; 4] = [
        Self::Ui,
        Self::Network,
        Self::AudioProcessing,
        Self::AudioCallbacks,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Ui => "UI handlers",
            Self::Network => "Network protocol",
            Self::AudioProcessing => "Audio processing",
            Self::AudioCallbacks => "Audio callbacks",
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TraceEvent {
    Span {
        generation: u64,
        at: Instant,
        duration: Duration,
        cpu: Duration,
        thread: u64,
        category: Category,
        name: &'static str,
    },
    Counters {
        generation: u64,
        at: Instant,
        rss: Option<u64>,
        heap: u64,
        audio: usize,
        upload: f64,
        download: f64,
    },
}

impl TraceEvent {
    fn generation(self) -> u64 {
        match self {
            Self::Span { generation, .. } | Self::Counters { generation, .. } => generation,
        }
    }
}

struct Recorder {
    active: AtomicU64,
    next: AtomicU64,
    dropped: AtomicU64,
    events: ArrayQueue<TraceEvent>,
}

static RECORDER: OnceLock<Recorder> = OnceLock::new();
static NEXT_THREAD: AtomicU64 = AtomicU64::new(1);
thread_local! { static THREAD: Cell<u64> = const { Cell::new(0) }; }

pub struct Recording {
    generation: u64,
    pub started: Instant,
    process_cpu: ProcessTime,
    dropped_at_start: u64,
}

impl Recording {
    pub fn start() -> Result<Self> {
        ThreadTime::try_now().context("Thread CPU clock is unavailable")?;
        let recorder = RECORDER.get_or_init(|| Recorder {
            active: AtomicU64::new(0),
            next: AtomicU64::new(1),
            dropped: AtomicU64::new(0),
            events: ArrayQueue::new(CAPACITY),
        });
        if recorder.active.load(Ordering::Acquire) != 0 {
            bail!("A trace is already recording");
        }
        while recorder.events.pop().is_some() {}
        let generation = recorder.next.fetch_add(1, Ordering::Relaxed);
        let process_cpu = ProcessTime::try_now().context("Process CPU clock is unavailable")?;
        let result = Self {
            generation,
            started: Instant::now(),
            process_cpu,
            dropped_at_start: recorder.dropped.load(Ordering::Relaxed),
        };
        recorder.active.store(generation, Ordering::Release);
        Ok(result)
    }

    pub fn finish(self) -> TraceReport {
        let recorder = RECORDER.get().expect("Recording has a recorder");
        recorder.active.store(0, Ordering::Release);
        let duration = self.started.elapsed();
        let process_cpu = self.process_cpu.try_elapsed().ok();
        let mut events = Vec::new();
        while let Some(event) = recorder.events.pop() {
            if event.generation() == self.generation {
                events.push(event);
            }
        }
        let mut categories = [Timing::default(); 4];
        let mut operations = BTreeMap::<&'static str, Timing>::new();
        for event in &events {
            if let TraceEvent::Span {
                duration,
                cpu,
                category,
                name,
                ..
            } = event
            {
                categories[*category as usize].add(*duration, *cpu);
                operations.entry(name).or_default().add(*duration, *cpu);
            }
        }
        let mut operations: Vec<_> = operations
            .into_iter()
            .map(|(name, timing)| Operation { name, timing })
            .collect();
        operations.sort_by(|a, b| b.timing.cpu.cmp(&a.timing.cpu));
        let summary = TraceSummary {
            duration,
            process_cpu,
            categories,
            operations,
            event_count: events.len(),
            dropped: recorder
                .dropped
                .load(Ordering::Relaxed)
                .saturating_sub(self.dropped_at_start),
        };
        TraceReport {
            started: self.started,
            events,
            summary,
        }
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if let Some(recorder) = RECORDER.get() {
            let _ = recorder.active.compare_exchange(
                self.generation,
                0,
                Ordering::AcqRel,
                Ordering::Relaxed,
            );
        }
    }
}

pub struct Scope {
    generation: u64,
    started: Instant,
    cpu: ThreadTime,
    thread: u64,
    category: Category,
    name: &'static str,
}

/// Guards stay on their starting thread and must never span an `.await`.
#[inline]
pub fn scope(category: Category, name: &'static str) -> Option<Scope> {
    let recorder = RECORDER.get()?;
    let generation = recorder.active.load(Ordering::Acquire);
    if generation == 0 {
        return None;
    }
    let thread = THREAD.with(|id| {
        if id.get() == 0 {
            id.set(NEXT_THREAD.fetch_add(1, Ordering::Relaxed));
        }
        id.get()
    });
    let started = Instant::now();
    let cpu = ThreadTime::try_now().ok()?;
    Some(Scope {
        generation,
        started,
        cpu,
        thread,
        category,
        name,
    })
}

impl Drop for Scope {
    fn drop(&mut self) {
        let Some(recorder) = RECORDER.get() else {
            return;
        };
        if recorder.active.load(Ordering::Acquire) != self.generation {
            return;
        }
        let Ok(cpu) = self.cpu.try_elapsed() else {
            return;
        };
        let event = TraceEvent::Span {
            generation: self.generation,
            at: self.started,
            duration: self.started.elapsed(),
            cpu,
            thread: self.thread,
            category: self.category,
            name: self.name,
        };
        if recorder.events.push(event).is_err() {
            recorder.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub fn record_counters(rss: Option<u64>, audio: usize, upload: f64, download: f64) {
    let Some(recorder) = RECORDER.get() else {
        return;
    };
    let generation = recorder.active.load(Ordering::Acquire);
    if generation == 0 {
        return;
    }
    let event = TraceEvent::Counters {
        generation,
        at: Instant::now(),
        rss,
        heap: crate::memory::snapshot().live,
        audio,
        upload,
        download,
    };
    if recorder.events.push(event).is_err() {
        recorder.dropped.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn buffer_bytes() -> usize {
    RECORDER.get().map_or(0, |r| {
        r.events.capacity() * std::mem::size_of::<TraceEvent>()
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Timing {
    pub calls: u64,
    pub cpu: Duration,
    pub wall: Duration,
    pub longest: Duration,
}

impl Timing {
    fn add(&mut self, wall: Duration, cpu: Duration) {
        self.calls += 1;
        self.cpu += cpu;
        self.wall += wall;
        self.longest = self.longest.max(wall);
    }
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub name: &'static str,
    pub timing: Timing,
}

#[derive(Debug, Clone, Copy)]
pub enum Ranking {
    TotalCpu,
    LongestCall,
}

#[derive(Debug, Clone)]
pub struct TraceSummary {
    pub duration: Duration,
    pub process_cpu: Option<Duration>,
    pub categories: [Timing; 4],
    pub operations: Vec<Operation>,
    pub event_count: usize,
    pub dropped: u64,
}

impl TraceSummary {
    pub fn captured_cpu(&self) -> Duration {
        self.categories.iter().map(|timing| timing.cpu).sum()
    }

    pub fn ranked_operations(&self, ranking: Ranking) -> Vec<&Operation> {
        let mut operations: Vec<_> = self
            .operations
            .iter()
            .filter(|operation| operation.timing.calls > 0)
            .collect();
        operations.sort_by(|a, b| {
            let value = |operation: &Operation| match ranking {
                Ranking::TotalCpu => operation.timing.cpu,
                Ranking::LongestCall => operation.timing.longest,
            };
            value(b).cmp(&value(a)).then_with(|| a.name.cmp(b.name))
        });
        operations
    }

    pub fn other_cpu(&self) -> Option<Duration> {
        self.process_cpu
            .map(|total| total.saturating_sub(self.captured_cpu()))
    }
}

pub struct TraceReport {
    started: Instant,
    events: Vec<TraceEvent>,
    pub summary: TraceSummary,
}

impl TraceReport {
    fn write_json(&self, writer: impl std::io::Write) -> Result<()> {
        serde_json::to_writer(writer, &TraceExport(self))?;
        Ok(())
    }

    pub fn save(&self) -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("io", "Venator", "TeamSpeakNative")
            .context("Cannot find app data directory")?;
        let folder = dirs.data_local_dir().join("traces");
        fs::create_dir_all(&folder)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis();
        let path = folder.join(format!("trace-{stamp}-{}.json", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        let mut writer = BufWriter::new(file);
        self.write_json(&mut writer)?;
        use std::io::Write;
        writer.flush()?;
        Ok(path)
    }
}

// Serialize one event at a time. Building a Value tree for the entire capture
// retained tens of megabytes of temporary maps and strings during export.
struct TraceExport<'a>(&'a TraceReport);
struct ExportEvents<'a>(&'a TraceReport);

impl Serialize for TraceExport<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut object = serializer.serialize_map(Some(3))?;
        object.serialize_entry("traceEvents", &ExportEvents(self.0))?;
        object.serialize_entry("displayTimeUnit", "ms")?;
        object.serialize_entry("metadata", &json!({
            "duration_seconds":self.0.summary.duration.as_secs_f64(), "dropped_events":self.0.summary.dropped,
            "scope":"Instrumented operations only. Timeline durations are elapsed time; thread_cpu_us is CPU time. No audio or message contents are recorded."
        }))?;
        object.end()
    }
}

impl Serialize for ExportEvents<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let pid = std::process::id();
        let mut events = serializer.serialize_seq(None)?;
        events.serialize_element(&json!({"name":"process_name","ph":"M","pid":pid,"tid":0,"args":{"name":"Native Voice"}}))?;
        for event in &self.0.events {
            match *event {
                TraceEvent::Span {
                    at,
                    duration,
                    cpu,
                    thread,
                    category,
                    name,
                    ..
                } => events.serialize_element(&json!({
                    "name":name,"cat":category.label(),"ph":"X","pid":pid,"tid":thread,
                    "ts":at.saturating_duration_since(self.0.started).as_secs_f64()*1e6,
                    "dur":duration.as_secs_f64()*1e6,"args":{"thread_cpu_us":cpu.as_secs_f64()*1e6}
                }))?,
                TraceEvent::Counters {
                    at,
                    rss,
                    heap,
                    audio,
                    upload,
                    download,
                    ..
                } => {
                    let ts = at.saturating_duration_since(self.0.started).as_secs_f64() * 1e6;
                    let mut memory =
                        json!({"rust_heap_bytes":heap,"audio_sample_capacity_bytes":audio});
                    if let Some(rss) = rss {
                        memory["resident_bytes"] = json!(rss);
                    }
                    events.serialize_element(
                        &json!({"name":"Memory","ph":"C","pid":pid,"tid":0,"ts":ts,"args":memory}),
                    )?;
                    events.serialize_element(&json!({"name":"Network","ph":"C","pid":pid,"tid":0,"ts":ts,"args":{"upload_bytes_per_second":upload,"download_bytes_per_second":download}}))?;
                }
            }
        }
        events.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_cost_and_longest_call_have_independent_rankings() {
        let mut categories = [Timing::default(); 4];
        categories[0].cpu = Duration::from_millis(20);
        let summary = TraceSummary {
            duration: Duration::from_secs(1),
            process_cpu: Some(Duration::from_millis(30)),
            categories,
            operations: vec![
                Operation {
                    name: "frequent",
                    timing: Timing {
                        calls: 100,
                        cpu: Duration::from_millis(18),
                        longest: Duration::from_millis(1),
                        wall: Duration::from_millis(100),
                    },
                },
                Operation {
                    name: "slow",
                    timing: Timing {
                        calls: 1,
                        cpu: Duration::from_millis(2),
                        longest: Duration::from_millis(80),
                        wall: Duration::from_millis(80),
                    },
                },
            ],
            event_count: 101,
            dropped: 0,
        };
        assert_eq!(
            summary.ranked_operations(Ranking::TotalCpu)[0].name,
            "frequent"
        );
        assert_eq!(
            summary.ranked_operations(Ranking::LongestCall)[0].name,
            "slow"
        );
        assert_eq!(summary.other_cpu(), Some(Duration::from_millis(10)));
        let empty = TraceSummary {
            operations: Vec::new(),
            process_cpu: None,
            ..summary
        };
        assert!(empty.ranked_operations(Ranking::TotalCpu).is_empty());
        assert_eq!(empty.other_cpu(), None);
    }

    #[test]
    fn trace_is_bounded_scoped_and_exports_cpu_and_memory_separately() -> Result<()> {
        assert!(scope(Category::Ui, "not_recording").is_none());
        let recording = Recording::start()?;
        {
            let _scope = scope(Category::AudioProcessing, "audio.test");
            std::thread::sleep(Duration::from_millis(20));
        }
        record_counters(Some(1234), 64, 12.0, 34.0);
        for _ in 0..CAPACITY {
            let _scope = scope(Category::Network, "network.test");
        }
        let report = recording.finish();
        assert_eq!(report.summary.event_count, CAPACITY);
        assert!(report.summary.dropped > 0);
        assert!(
            report.summary.categories[Category::AudioProcessing as usize].wall
                >= Duration::from_millis(20)
        );
        assert!(scope(Category::Ui, "after_stop").is_none());
        struct HeapWatchingSink {
            baseline: u64,
            largest_extra: u64,
            written: usize,
        }
        impl std::io::Write for HeapWatchingSink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.largest_extra = self
                    .largest_extra
                    .max(crate::memory::snapshot().live.saturating_sub(self.baseline));
                self.written += bytes.len();
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut sink = HeapWatchingSink {
            baseline: crate::memory::snapshot().live,
            largest_extra: 0,
            written: 0,
        };
        report.write_json(&mut sink)?;
        assert!(sink.written > 1000);
        assert!(
            sink.largest_extra < 4 * 1024 * 1024,
            "Export allocated {} extra bytes instead of streaming",
            sink.largest_extra
        );
        eprintln!(
            "Trace export: {} bytes written; {} bytes maximum temporary heap growth",
            sink.written, sink.largest_extra
        );
        let mut output = Vec::new();
        report.write_json(&mut output)?;
        let value: serde_json::Value = serde_json::from_slice(&output)?;
        let events = value["traceEvents"].as_array().unwrap();
        assert!(
            events
                .iter()
                .any(|e| e["ph"] == "X" && e["args"]["thread_cpu_us"].is_number())
        );
        assert!(
            events
                .iter()
                .any(|e| e["ph"] == "C" && e["args"]["resident_bytes"] == 1234)
        );
        let next = Recording::start()?.finish();
        assert_eq!(next.summary.event_count, 0);
        Ok(())
    }
}
