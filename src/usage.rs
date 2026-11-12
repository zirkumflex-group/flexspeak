use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use rune::{Color, Shape, shape::LineCap};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tsclientlib::{ConnectionStats, PacketStat, data::ConnectionClientData};

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
pub const SERVER_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const SERVER_MAX_AGE: Duration = Duration::from_secs(15);
const HISTORY_WINDOW: Duration = Duration::from_secs(300);
const HISTORY_SAMPLES: usize = 300;

#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessUsage {
    pub memory_bytes: Option<u64>,
    /// macOS physical footprint or Windows private working set; see MEMORY_DETAIL_LABEL.
    pub memory_detail_bytes: Option<u64>,
    pub cpu_percent: Option<f32>,
}

#[cfg(windows)]
pub const MEMORY_DETAIL_LABEL: &str = "Private RAM";
#[cfg(not(windows))]
pub const MEMORY_DETAIL_LABEL: &str = "Footprint";

/// How heavy the app's own CPU use is: amber from 25 %, red from 50 %.
pub fn cpu_health(percent: Option<f32>) -> Health {
    match percent {
        None => Health::Unknown,
        Some(value) if value >= 50.0 => Health::Poor,
        Some(value) if value >= 25.0 => Health::Fair,
        Some(_) => Health::Good,
    }
}

/// How heavy the app's own memory use is: amber from 400 MiB, red from 700 MiB.
pub fn memory_health(bytes: Option<u64>) -> Health {
    const MIB: u64 = 1024 * 1024;
    match bytes {
        None => Health::Unknown,
        Some(value) if value >= 700 * MIB => Health::Poor,
        Some(value) if value >= 400 * MIB => Health::Fair,
        Some(_) => Health::Good,
    }
}

pub fn format_mib(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "—".into(),
        |bytes| format!("{:.1} MiB", bytes as f64 / 1_048_576.0),
    )
}

/// A CPU share to one decimal, which is as fine as a reading taken each second means anything.
pub fn format_cpu_percent(value: f64) -> String {
    if value > 0.0 && value < 0.05 {
        "<0.1%".into()
    } else {
        format!("{value:.1}%")
    }
}

/// `percent` of one core, and the same load as a share of the whole machine, as Task Manager
/// and Activity Monitor's CPU totals show it.
pub fn cpu_detail(percent: Option<f32>) -> String {
    static CORES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let cores = *CORES.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
    match percent {
        None => "CPU · measuring".into(),
        Some(value) => format!(
            "{} of one core · {} of all {cores} cores",
            format_cpu_percent(f64::from(value)),
            format_cpu_percent(f64::from(value) / cores as f64),
        ),
    }
}

#[cfg(target_os = "macos")]
fn memory_detail_bytes() -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v0>::zeroed();
    // SAFETY: V0 selects this exact SDK structure, and its storage is writable for the call.
    let result = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as libc::c_int,
            libc::RUSAGE_INFO_V0,
            info.as_mut_ptr().cast(),
        )
    };
    // SAFETY: A successful call filled the zero-initialized structure.
    (result == 0).then(|| unsafe { info.assume_init().ri_phys_footprint })
}

#[cfg(windows)]
fn memory_detail_bytes() -> Option<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX2},
        Threading::GetCurrentProcess,
    };
    // Sentinel detects older Windows versions that fill only the EX prefix.
    let mut info = PROCESS_MEMORY_COUNTERS_EX2 {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32,
        PrivateWorkingSetSize: usize::MAX,
        ..Default::default()
    };
    // SAFETY: GetCurrentProcess is a valid pseudo-handle; the buffer and size match EX2.
    let success = unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut info as *mut PROCESS_MEMORY_COUNTERS_EX2).cast(),
            info.cb,
        )
    };
    (success != 0 && info.PrivateWorkingSetSize != usize::MAX)
        .then_some(info.PrivateWorkingSetSize as u64)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn memory_detail_bytes() -> Option<u64> {
    None
}

pub struct ProcessMonitor {
    system: System,
    pid: Pid,
    primed: bool,
}

impl ProcessMonitor {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            pid: Pid::from_u32(std::process::id()),
            primed: false,
        }
    }

    pub fn sample(&mut self) -> ProcessUsage {
        // Refresh only this process. Scanning all processes or Linux tasks would
        // make the usage display unnecessarily expensive.
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[self.pid]),
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .without_tasks(),
        );
        let Some(process) = self.system.process(self.pid) else {
            self.primed = false;
            return ProcessUsage::default();
        };
        let cpu = process.cpu_usage();
        let sample = ProcessUsage {
            memory_bytes: Some(process.memory()),
            memory_detail_bytes: memory_detail_bytes(),
            // The first CPU reading has no preceding measurement interval.
            cpu_percent: (self.primed && cpu.is_finite()).then_some(cpu.max(0.0)),
        };
        self.primed = true;
        sample
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Percentiles {
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
}

pub struct MetricHistory {
    pub revision: u64,
    samples: VecDeque<(Instant, f64)>,
    now: Instant,
    pub percentiles: Option<Percentiles>,
    pub upper_bound: f64,
}

impl MetricHistory {
    pub fn new(now: Instant) -> Self {
        Self {
            samples: VecDeque::new(),
            now,
            revision: crate::connection_charts::next_revision(),
            percentiles: None,
            upper_bound: 1.0,
        }
    }

    pub fn push(&mut self, now: Instant, value: Option<f64>) {
        self.revision = crate::connection_charts::next_revision();
        self.now = now;
        if let Some(value) = value.filter(|v| v.is_finite() && *v >= 0.0) {
            self.samples.push_back((now, value));
        }
        while self
            .samples
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > HISTORY_WINDOW)
            || self.samples.len() > HISTORY_SAMPLES
        {
            self.samples.pop_front();
        }
        // At most 300 numbers, sorted once per second, not on every UI frame.
        let mut sorted: Vec<_> = self.samples.iter().map(|(_, value)| *value).collect();
        sorted.sort_by(f64::total_cmp);
        self.percentiles = if sorted.is_empty() {
            None
        } else {
            let rank = |p: f64| sorted[(sorted.len() as f64 * p).ceil() as usize - 1];
            Some(Percentiles {
                p50: rank(0.50),
                p90: rank(0.90),
                p99: rank(0.99),
            })
        };
        self.upper_bound = sorted.last().copied().unwrap_or(0.0).max(1.0) * 1.1;
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn buffer_bytes(&self) -> usize {
        self.samples.capacity() * std::mem::size_of::<(Instant, f64)>()
    }
}

impl MetricHistory {
    /// Five minutes of readings as a line over a translucent area, `width` × `height` points.
    /// Missing measurements stay gaps rather than inventing a trend across a sleep.
    pub fn chart(&self, width: f32, height: f32, color: Color, grid: Color) -> Shape {
        let inner_w = (width - 4.0).max(1.0);
        let inner_h = (height - 4.0).max(1.0);
        let mut shape = Shape::new();
        for fraction in [0.0, 0.5, 1.0] {
            let y = 2.0 + inner_h * fraction;
            shape = shape.line([2.0, y], [2.0 + inner_w, y], 1.0, LineCap::Butt, grid);
        }
        let baseline = 2.0 + inner_h;
        let mut runs: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut previous_at: Option<Instant> = None;
        for (at, value) in &self.samples {
            let age = self.now.saturating_duration_since(*at).as_secs_f32();
            let point = [
                2.0 + inner_w * (1.0 - age / HISTORY_WINDOW.as_secs_f32()).clamp(0.0, 1.0),
                2.0 + inner_h * (1.0 - (*value / self.upper_bound) as f32),
            ];
            let joined = previous_at.is_some_and(|previous| {
                at.saturating_duration_since(previous) <= Duration::from_secs(3)
            });
            match runs.last_mut() {
                Some(run) if joined => run.push(point),
                _ => runs.push(vec![point]),
            }
            previous_at = Some(*at);
        }
        for run in &runs {
            if run.len() > 1 {
                let mut area = Vec::with_capacity(run.len() + 2);
                area.push([run[0][0], baseline]);
                area.extend_from_slice(run);
                area.push([run[run.len() - 1][0], baseline]);
                shape = shape.fill(area, color.alpha(0.12));
                shape = shape.stroke(run.clone(), 1.6, color);
            }
        }
        if let Some(point) = runs.last().and_then(|run| run.last()) {
            shape = shape.line(*point, *point, 5.0, LineCap::Round, color);
        }
        shape
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NetworkUsage {
    pub upload_per_second: f64,
    pub download_per_second: f64,
    pub uploaded: u64,
    pub downloaded: u64,
}

impl NetworkUsage {
    pub fn stop(&mut self) {
        self.upload_per_second = 0.0;
        self.download_per_second = 0.0;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectionUsage {
    pub ping_ms: Option<f64>,
    pub ping_deviation_ms: Option<f64>,
    pub packet_loss_in: Option<f32>,
    pub packet_loss_out: Option<f32>,
    pub idle_seconds: Option<u64>,
    pub file_upload_per_second: Option<u64>,
    pub file_download_per_second: Option<u64>,
}

/// Only copy the metrics we display, not the address/private fields in the
/// server's connection-info response.
#[derive(Debug, Clone, Copy)]
pub struct ServerUsage {
    received_at: Instant,
    quality: ConnectionUsage,
}

impl ServerUsage {
    pub fn new(data: &ConnectionClientData, now: Instant) -> Self {
        Self {
            received_at: now,
            quality: ConnectionUsage {
                ping_ms: data
                    .ping
                    .filter(|v| !v.is_negative())
                    .map(|v| v.as_seconds_f64() * 1000.0),
                ping_deviation_ms: data
                    .ping_deviation
                    .filter(|v| !v.is_negative())
                    .map(|v| v.as_seconds_f64() * 1000.0),
                packet_loss_in: data
                    .server_to_client_packetloss_total
                    .and_then(loss_percent),
                packet_loss_out: loss_percent(data.client_to_server_packetloss_total),
                idle_seconds: Some(data.idle_time.whole_seconds().max(0) as u64),
                file_upload_per_second: data.filetransfer_bandwidth_sent,
                file_download_per_second: data.filetransfer_bandwidth_received,
            },
        }
    }

    /// A reading made up for previews.
    pub fn preview(quality: ConnectionUsage, now: Instant) -> Self {
        Self {
            received_at: now,
            quality,
        }
    }

    pub fn sample(&self, now: Instant) -> ConnectionUsage {
        if now.saturating_duration_since(self.received_at) > SERVER_MAX_AGE {
            ConnectionUsage::default()
        } else {
            // Remote idle time is a server snapshot: speech may resume between replies.
            self.quality
        }
    }
}

pub fn connection_usage(
    stats: &ConnectionStats,
    server: Option<&ServerUsage>,
    last_voice: Option<Instant>,
    now: Instant,
) -> ConnectionUsage {
    let mut usage = ConnectionUsage {
        ping_ms: (!stats.rtt.is_zero()).then_some(stats.rtt.as_secs_f64() * 1000.0),
        ping_deviation_ms: (!stats.rtt.is_zero()).then_some(stats.rtt_dev.as_secs_f64() * 1000.0),
        packet_loss_in: loss_percent(stats.get_packetloss_s2c_total()),
        ..server.map_or_else(ConnectionUsage::default, |server| server.sample(now))
    };
    if let Some(server) = server {
        let age = now.saturating_duration_since(server.received_at);
        if age <= SERVER_MAX_AGE {
            usage.idle_seconds = Some(match last_voice.filter(|last| *last > server.received_at) {
                Some(last) => now.saturating_duration_since(last).as_secs(),
                None => server
                    .quality
                    .idle_seconds
                    .unwrap_or(0)
                    .saturating_add(age.as_secs()),
            });
        }
    }
    usage
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Good,
    Fair,
    Poor,
    Unknown,
}

impl Health {
    pub fn label(self) -> &'static str {
        match self {
            Self::Good => "Good",
            Self::Fair => "Fair",
            Self::Poor => "Poor",
            Self::Unknown => "Unavailable",
        }
    }

    fn threshold(value: Option<f64>, fair: f64, poor: f64) -> Self {
        match value.filter(|v| v.is_finite() && *v >= 0.0) {
            Some(v) if v >= poor => Self::Poor,
            Some(v) if v >= fair => Self::Fair,
            Some(_) => Self::Good,
            None => Self::Unknown,
        }
    }

    pub fn loss(percent: Option<f32>) -> Self {
        Self::threshold(percent.filter(|v| *v <= 100.0).map(f64::from), 1.0, 5.0)
    }

    pub fn ping(ms: Option<f64>, deviation: Option<f64>) -> Self {
        let ping = Self::threshold(ms, 100.0, 200.0);
        match (ping, Self::threshold(deviation, 20.0, 50.0)) {
            (Self::Poor, _) | (_, Self::Poor) => Self::Poor,
            (Self::Fair, _) | (_, Self::Fair) => Self::Fair,
            (Self::Unknown, _) => Self::Unknown,
            _ => Self::Good,
        }
    }
}

/// How long a new health state must last before the globe shows it.
pub const HEALTH_HOLD: Duration = Duration::from_secs(4);

/// Connection health as shown. Readings arrive every second or so and wobble across the
/// thresholds while someone talks, and the first ones after connecting or joining are taken
/// over a handful of packets, where one lost packet already reads as heavy loss. So it starts
/// at Good, a new state only appears once it has lasted `HEALTH_HOLD`, and a reading back at
/// the shown state cancels the change.
#[derive(Debug, Clone, Copy)]
pub struct SteadyHealth {
    shown: Health,
    /// When readings started to differ from `shown`.
    since: Option<Instant>,
}

impl Default for SteadyHealth {
    fn default() -> Self {
        Self {
            shown: Health::Good,
            since: None,
        }
    }
}

impl SteadyHealth {
    pub fn update(&mut self, reading: Health, now: Instant) {
        // Missing data shows as it is (see `show`) and says nothing about the connection.
        if reading == Health::Unknown {
            return;
        }
        if reading == self.shown {
            self.since = None;
            return;
        }
        let since = *self.since.get_or_insert(now);
        if now.saturating_duration_since(since) >= HEALTH_HOLD {
            self.shown = reading;
            self.since = None;
        }
    }

    /// What to show while the latest reading is `reading`. Missing data shows at once.
    pub fn show(&self, reading: Health) -> Health {
        if reading == Health::Unknown {
            Health::Unknown
        } else {
            self.shown
        }
    }
}

impl ConnectionUsage {
    pub fn health_tooltip(self) -> String {
        let mut problems = Vec::new();
        let mut missing = Vec::new();
        for (label, value, health) in [
            (
                "Incoming packet loss",
                self.packet_loss_in.map(|v| format!("{v:.1}%")),
                Health::loss(self.packet_loss_in),
            ),
            (
                "Outgoing packet loss",
                self.packet_loss_out.map(|v| format!("{v:.1}%")),
                Health::loss(self.packet_loss_out),
            ),
            (
                "Latency",
                self.ping_ms.map(|v| format!("{v:.0} ms")),
                Health::threshold(self.ping_ms, 100.0, 200.0),
            ),
            (
                "Ping variation",
                self.ping_deviation_ms.map(|v| format!("{v:.1} ms")),
                Health::threshold(self.ping_deviation_ms, 20.0, 50.0),
            ),
        ] {
            match health {
                Health::Fair | Health::Poor => {
                    problems.push(format!("{label}: {}", value.unwrap()))
                }
                Health::Unknown => missing.push(label),
                Health::Good => {}
            }
        }
        if !problems.is_empty() {
            problems.join("\n")
        } else if !missing.is_empty() {
            format!("Unavailable: {}", missing.join(", "))
        } else {
            "No connection issues detected.".into()
        }
    }

    pub fn health(self) -> Health {
        let readings = [
            Health::loss(self.packet_loss_in),
            Health::loss(self.packet_loss_out),
            Health::ping(self.ping_ms, self.ping_deviation_ms),
        ];
        for health in [Health::Poor, Health::Fair, Health::Unknown] {
            if readings.contains(&health) {
                return health;
            }
        }
        Health::Good
    }

    pub fn ping_text(self) -> String {
        match (self.ping_ms, self.ping_deviation_ms) {
            (Some(ping), Some(deviation)) => format!("{ping:.0} ± {deviation:.1} ms"),
            (Some(ping), None) => format!("{ping:.0} ms"),
            _ => "—".into(),
        }
    }
}

fn loss_percent(ratio: f32) -> Option<f32> {
    (ratio.is_finite() && (0.0..=1.0).contains(&ratio)).then_some(ratio * 100.0)
}

pub fn format_idle(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

pub struct TrafficMeter {
    last_at: Instant,
    last_upload: u64,
    last_download: u64,
    usage: NetworkUsage,
}

impl TrafficMeter {
    pub fn new(now: Instant) -> Self {
        Self {
            last_at: now,
            last_upload: 0,
            last_download: 0,
            usage: NetworkUsage::default(),
        }
    }

    /// A reconnect replaces the library's transport counters. Keep the user's
    /// session totals, but start a fresh rate interval and raw-counter baseline.
    pub fn reset_transport(&mut self, now: Instant) -> NetworkUsage {
        self.last_at = now;
        self.last_upload = 0;
        self.last_download = 0;
        self.usage.stop();
        self.usage
    }

    pub fn sample(&mut self, counters: &[u64; 6], now: Instant) -> NetworkUsage {
        let elapsed = now.saturating_duration_since(self.last_at).as_secs_f64();
        if elapsed == 0.0 {
            return self.usage;
        }
        let uploaded = [
            PacketStat::OutControl,
            PacketStat::OutKeepalive,
            PacketStat::OutSpeech,
        ]
        .into_iter()
        .map(|kind| counters[kind as usize])
        .sum::<u64>();
        let downloaded = [
            PacketStat::InControl,
            PacketStat::InKeepalive,
            PacketStat::InSpeech,
        ]
        .into_iter()
        .map(|kind| counters[kind as usize])
        .sum::<u64>();
        let up = uploaded.checked_sub(self.last_upload).unwrap_or(uploaded);
        let down = downloaded
            .checked_sub(self.last_download)
            .unwrap_or(downloaded);
        self.usage.uploaded += up;
        self.usage.downloaded += down;
        self.usage.upload_per_second = up as f64 / elapsed;
        self.usage.download_per_second = down as f64 / elapsed;
        self.last_at = now;
        self.last_upload = uploaded;
        self.last_download = downloaded;
        self.usage
    }
}

pub fn format_bytes(mut value: f64) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut index = 0;
    while value >= 1024.0 && index < units.len() - 1 {
        value /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{value:.0} {}", units[index])
    } else {
        format!("{value:.1} {}", units[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn resource_load_turns_amber_then_red() {
        const MIB: u64 = 1024 * 1024;
        assert_eq!(cpu_health(None), Health::Unknown);
        assert_eq!(cpu_health(Some(24.9)), Health::Good);
        assert_eq!(cpu_health(Some(25.0)), Health::Fair);
        assert_eq!(cpu_health(Some(50.0)), Health::Poor);
        assert_eq!(memory_health(Some(399 * MIB)), Health::Good);
        assert_eq!(memory_health(Some(400 * MIB)), Health::Fair);
        assert_eq!(memory_health(Some(700 * MIB)), Health::Poor);
    }

    #[test]
    fn cpu_shows_one_decimal_and_marks_tiny_readings() {
        for (value, expected) in [
            (0.0, "0.0%"),
            (0.0001, "<0.1%"),
            (f64::from(0.02_f32), "<0.1%"),
            (0.2, "0.2%"),
            (1.23456, "1.2%"),
            (26.562, "26.6%"),
            (100.0, "100.0%"),
            (201.125, "201.1%"),
        ] {
            assert_eq!(format_cpu_percent(value), expected);
        }
    }

    #[test]
    fn steady_health_waits_before_changing() {
        let start = Instant::now();
        let at = |seconds: u64| start + Duration::from_secs(seconds);
        let mut steady = SteadyHealth::default();
        // The first readings after joining are noisy: a bad one is held back like any other.
        steady.update(Health::Poor, at(0));
        assert_eq!(steady.show(Health::Poor), Health::Good);
        // Missing data neither shows as a problem nor counts toward one.
        steady.update(Health::Unknown, at(0));
        steady.update(Health::Good, at(0));
        // One jittery reading, then back: never shown.
        steady.update(Health::Fair, at(1));
        assert_eq!(steady.show(Health::Fair), Health::Good);
        steady.update(Health::Good, at(2));
        steady.update(Health::Fair, at(3));
        steady.update(Health::Poor, at(5));
        assert_eq!(steady.show(Health::Poor), Health::Good);
        // Away from Good for the whole hold: the latest reading shows.
        steady.update(Health::Poor, at(7));
        assert_eq!(steady.show(Health::Poor), Health::Poor);
        // Missing data is never held back.
        assert_eq!(steady.show(Health::Unknown), Health::Unknown);
    }

    #[test]
    fn traffic_rates_use_all_packet_categories_and_actual_elapsed_time() {
        let start = Instant::now();
        let mut meter = TrafficMeter::new(start);
        let first = meter.sample(&[100, 20, 80, 30, 4, 6], start + Duration::from_secs(2));
        assert_eq!((first.uploaded, first.downloaded), (40, 200));
        assert_eq!(
            (first.upload_per_second, first.download_per_second),
            (20.0, 100.0)
        );
        let next = meter.sample(&[100, 30, 180, 60, 4, 16], start + Duration::from_secs(4));
        assert_eq!((next.uploaded, next.downloaded), (80, 310));
        assert_eq!(
            (next.upload_per_second, next.download_per_second),
            (20.0, 55.0)
        );
    }

    #[test]
    fn reconnect_preserves_totals_without_counter_underflow_or_rate_spikes() {
        let start = Instant::now();
        let mut meter = TrafficMeter::new(start);
        meter.sample(&[1000, 0, 0, 500, 0, 0], start + Duration::from_secs(1));
        let paused = meter.reset_transport(start + Duration::from_secs(5));
        assert_eq!((paused.uploaded, paused.downloaded), (500, 1000));
        assert_eq!(
            (paused.upload_per_second, paused.download_per_second),
            (0.0, 0.0)
        );
        let next = meter.sample(&[20, 0, 0, 10, 0, 0], start + Duration::from_secs(6));
        assert_eq!((next.uploaded, next.downloaded), (510, 1020));
        assert_eq!(
            (next.upload_per_second, next.download_per_second),
            (10.0, 20.0)
        );
        let unchanged = meter.sample(&[99; 6], start + Duration::from_secs(6));
        assert_eq!(unchanged.uploaded, next.uploaded);
    }

    #[test]
    fn process_monitor_reads_only_this_process_and_warms_up_cpu() {
        let mut monitor = ProcessMonitor::new();
        let first = monitor.sample();
        assert!(first.memory_bytes.is_some_and(|bytes| bytes > 0));
        #[cfg(target_os = "macos")]
        assert!(first.memory_detail_bytes.is_some_and(|bytes| bytes > 0));
        assert!(first.cpu_percent.is_none());
        assert_eq!(monitor.system.processes().len(), 1);
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(20));
        let second = monitor.sample();
        assert!(
            second
                .cpu_percent
                .is_some_and(|cpu| cpu.is_finite() && cpu >= 0.0)
        );
    }

    #[test]
    fn memory_details_use_mib_and_preserve_unavailable_values() {
        assert_eq!(format_mib(None), "—");
        assert_eq!(format_mib(Some(0)), "0.0 MiB");
        assert_eq!(format_mib(Some(157_286_400)), "150.0 MiB");
        #[cfg(target_os = "macos")]
        {
            assert_eq!(std::mem::size_of::<libc::rusage_info_v0>(), 96);
            assert_eq!(
                std::mem::offset_of!(libc::rusage_info_v0, ri_phys_footprint),
                72
            );
        }
    }

    #[test]
    fn connection_metrics_expire_and_voice_resets_idle_time() {
        let start = Instant::now();
        let mut stats = ConnectionStats::default();
        stats.rtt = Duration::from_millis(17);
        stats.rtt_dev = Duration::from_micros(2100);
        let server = ServerUsage {
            received_at: start,
            quality: ConnectionUsage {
                packet_loss_out: loss_percent(0.025),
                idle_seconds: Some(3600),
                file_upload_per_second: Some(100),
                ..Default::default()
            },
        };
        let sample = connection_usage(&stats, Some(&server), None, start + Duration::from_secs(3));
        assert_eq!(sample.ping_ms, Some(17.0));
        assert_eq!(sample.ping_deviation_ms, Some(2.1));
        assert_eq!(sample.packet_loss_out, Some(2.5));
        assert_eq!(sample.idle_seconds, Some(3603));
        assert_eq!(format_idle(3603), "01:00:03");
        assert_eq!(sample.file_upload_per_second, Some(100));
        assert_eq!(sample.file_download_per_second, None);
        let speaking = connection_usage(
            &stats,
            Some(&server),
            Some(start + Duration::from_secs(2)),
            start + Duration::from_secs(3),
        );
        assert_eq!(speaking.idle_seconds, Some(1));
        let stale = connection_usage(&stats, Some(&server), None, start + Duration::from_secs(16));
        assert_eq!(stale.packet_loss_out, None);
        assert_eq!(stale.idle_seconds, None);
        assert_eq!(stale.file_upload_per_second, None);
        assert_eq!(stale.ping_ms, Some(17.0));
        assert_eq!(loss_percent(f32::NAN), None);
        assert_eq!(loss_percent(-0.1), None);
    }

    #[test]
    fn health_flags_loss_latency_and_variation_without_treating_missing_data_as_good() {
        for (value, expected) in [
            (None, Health::Unknown),
            (Some(f32::NAN), Health::Unknown),
            (Some(-1.0), Health::Unknown),
            (Some(101.0), Health::Unknown),
            (Some(0.0), Health::Good),
            (Some(0.9), Health::Good),
            (Some(1.0), Health::Fair),
            (Some(5.0), Health::Poor),
        ] {
            assert_eq!(Health::loss(value), expected);
        }
        assert_eq!(Health::ping(Some(99.0), None), Health::Good);
        assert_eq!(Health::ping(Some(100.0), None), Health::Fair);
        assert_eq!(Health::ping(Some(200.0), None), Health::Poor);
        assert_eq!(Health::ping(Some(23.0), Some(20.0)), Health::Fair);
        assert_eq!(Health::ping(Some(23.0), Some(50.0)), Health::Poor);
        assert_eq!(Health::ping(None, Some(0.0)), Health::Unknown);
        assert_eq!(Health::ping(None, Some(50.0)), Health::Poor);
        let mut quality = ConnectionUsage {
            ping_ms: Some(23.0),
            packet_loss_in: Some(0.0),
            packet_loss_out: Some(0.0),
            ..Default::default()
        };
        assert_eq!(quality.health(), Health::Good);
        quality.packet_loss_out = None;
        assert_eq!(quality.health(), Health::Unknown);
        quality.packet_loss_in = Some(5.0);
        assert_eq!(quality.health(), Health::Poor);
    }

    #[test]
    fn health_tooltip_only_lists_problems_and_separates_missing_measurements() {
        let mut quality = ConnectionUsage {
            packet_loss_in: Some(0.0),
            packet_loss_out: Some(0.0),
            ping_ms: Some(23.0),
            ping_deviation_ms: Some(11.2),
            idle_seconds: Some(86_400),
            ..Default::default()
        };
        assert_eq!(quality.health_tooltip(), "No connection issues detected.");
        quality.packet_loss_out = Some(2.0);
        quality.ping_deviation_ms = Some(55.0);
        assert_eq!(
            quality.health_tooltip(),
            "Outgoing packet loss: 2.0%\nPing variation: 55.0 ms"
        );
        assert_eq!(quality.health(), Health::Poor);
        quality.packet_loss_out = None;
        quality.ping_deviation_ms = Some(0.0);
        assert_eq!(
            quality.health_tooltip(),
            "Unavailable: Outgoing packet loss"
        );
        assert_eq!(quality.health(), Health::Unknown);
        quality.ping_ms = Some(220.0);
        assert_eq!(quality.health_tooltip(), "Latency: 220 ms");
    }

    #[test]
    fn remote_samples_expire_and_do_not_invent_idle_activity_between_responses() {
        let now = Instant::now();
        let sample = ServerUsage {
            received_at: now,
            quality: ConnectionUsage {
                ping_ms: Some(23.0),
                ping_deviation_ms: Some(11.2),
                packet_loss_in: Some(0.0),
                packet_loss_out: Some(0.0),
                idle_seconds: Some(60),
                ..Default::default()
            },
        };
        assert_eq!(
            sample.sample(now + Duration::from_secs(5)).idle_seconds,
            Some(60)
        );
        assert_eq!(sample.sample(now).ping_text(), "23 ± 11.2 ms");
        let expired = sample.sample(now + Duration::from_secs(16));
        assert_eq!(expired.health(), Health::Unknown);
        assert!(
            expired.ping_ms.is_none()
                && expired.packet_loss_in.is_none()
                && expired.packet_loss_out.is_none()
                && expired.idle_seconds.is_none()
        );
    }

    #[test]
    fn server_metrics_keep_client_directions_units_and_missing_fields() {
        let data: ConnectionClientData = serde_json::from_value(serde_json::json!({
            "client_id": 7,
            "ping": [0, 23_000_000], "ping_deviation": [0, 11_200_000],
            "idle_time": [60, 0],
            "server_to_client_packetloss_total": 0.025,
            "client_to_server_packetloss_speech": 0.0,
            "client_to_server_packetloss_keepalive": 0.0,
            "client_to_server_packetloss_control": 0.0,
            "client_to_server_packetloss_total": 0.01
        }))
        .unwrap();
        let now = Instant::now();
        let quality = ServerUsage::new(&data, now).sample(now);
        assert_eq!(quality.ping_ms, Some(23.0));
        assert!((quality.ping_deviation_ms.unwrap() - 11.2).abs() < 0.0001);
        assert_eq!(quality.packet_loss_in, Some(2.5));
        assert_eq!(quality.packet_loss_out, Some(1.0));
        assert_eq!(quality.idle_seconds, Some(60));
        let mut missing = data;
        missing.ping = None;
        missing.server_to_client_packetloss_total = None;
        let quality = ServerUsage::new(&missing, now).sample(now);
        assert!(quality.ping_ms.is_none() && quality.packet_loss_in.is_none());
    }

    #[test]
    fn history_percentiles_use_real_samples_and_expire_after_five_minutes() {
        let start = Instant::now();
        let mut history = MetricHistory::new(start);
        history.push(start, None);
        history.push(start, Some(f64::NAN));
        assert!(history.percentiles.is_none());
        for n in 1..=100 {
            history.push(start + Duration::from_secs(n), Some(n as f64));
        }
        let percentiles = history.percentiles.unwrap();
        assert_eq!(
            (percentiles.p50, percentiles.p90, percentiles.p99),
            (50.0, 90.0, 99.0)
        );
        for n in 101..=350 {
            history.push(start + Duration::from_secs(n), Some(n as f64));
        }
        assert_eq!(history.len(), HISTORY_SAMPLES);
        assert_eq!(history.percentiles.unwrap().p50, 200.0);
        history.push(start + Duration::from_secs(651), None);
        assert_eq!(history.len(), 0);
        assert!(history.percentiles.is_none());
    }
}
