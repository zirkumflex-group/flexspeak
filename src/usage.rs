use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use iced::{Color, Point, Rectangle, Renderer, Theme, mouse, widget::canvas};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tsclientlib::{ConnectionStats, PacketStat, data::ConnectionClientData};

pub const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
pub const COMPACT_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
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

pub fn format_mib(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "—".into(),
        |bytes| format!("{:.1} MiB", bytes as f64 / 1_048_576.0),
    )
}

pub fn format_cpu_percent(value: f64) -> String {
    if value > 0.0 && value < 0.001 {
        return "<0.001%".into();
    }
    let value = format!("{value:.3}");
    format!("{}%", value.trim_end_matches('0').trim_end_matches('.'))
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
    samples: VecDeque<(Instant, f64)>,
    now: Instant,
    pub percentiles: Option<Percentiles>,
    pub upper_bound: f64,
    cache: canvas::Cache,
}

impl MetricHistory {
    pub fn new(now: Instant) -> Self {
        Self {
            samples: VecDeque::new(),
            now,
            percentiles: None,
            upper_bound: 1.0,
            cache: canvas::Cache::new(),
        }
    }

    pub fn push(&mut self, now: Instant, value: Option<f64>) {
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
        self.cache.clear();
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn release_graphics(&mut self) {
        // clear() retains geometry for reuse; replacing the cache drops it.
        self.cache = canvas::Cache::new();
    }

    pub fn buffer_bytes(&self) -> usize {
        self.samples.capacity() * std::mem::size_of::<(Instant, f64)>()
    }
}

pub struct HistoryChart<'a> {
    pub history: &'a MetricHistory,
    pub color: Color,
}

impl<Message> canvas::Program<Message> for HistoryChart<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        vec![self.history.cache.draw(renderer, bounds.size(), |frame| {
            let width = (frame.width() - 4.0).max(1.0);
            let height = (frame.height() - 4.0).max(1.0);
            for fraction in [0.0, 0.5, 1.0] {
                let y = 2.0 + height * fraction;
                frame.stroke(
                    &canvas::Path::line(Point::new(2.0, y), Point::new(2.0 + width, y)),
                    canvas::Stroke::default().with_color(Color::from_rgba(0.65, 0.7, 0.85, 0.15)),
                );
            }
            let mut latest = None;
            let path = canvas::Path::new(|builder| {
                let mut previous_at = None;
                for (at, value) in &self.history.samples {
                    let age = self
                        .history
                        .now
                        .saturating_duration_since(*at)
                        .as_secs_f32();
                    let point = Point::new(
                        2.0 + width * (1.0 - age / HISTORY_WINDOW.as_secs_f32()).clamp(0.0, 1.0),
                        2.0 + height * (1.0 - (*value / self.history.upper_bound) as f32),
                    );
                    // Show missing measurements as gaps, rather than inventing
                    // a trend across a sleep or suspended UI interval.
                    if previous_at.is_some_and(|previous| {
                        at.saturating_duration_since(previous) <= Duration::from_secs(3)
                    }) {
                        builder.line_to(point);
                    } else {
                        builder.move_to(point);
                    }
                    previous_at = Some(*at);
                    latest = Some(point);
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_width(1.5)
                    .with_color(self.color),
            );
            if let Some(point) = latest {
                frame.fill(&canvas::Path::circle(point, 2.0), self.color);
            }
        })]
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
pub struct ServerUsage {
    received_at: Instant,
    packet_loss_out: Option<f32>,
    idle_seconds: u64,
    file_upload_per_second: Option<u64>,
    file_download_per_second: Option<u64>,
}

impl ServerUsage {
    pub fn new(data: &ConnectionClientData, now: Instant) -> Self {
        Self {
            received_at: now,
            packet_loss_out: loss_percent(data.client_to_server_packetloss_total),
            idle_seconds: data.idle_time.whole_seconds().max(0) as u64,
            file_upload_per_second: data.filetransfer_bandwidth_sent,
            file_download_per_second: data.filetransfer_bandwidth_received,
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
        ..ConnectionUsage::default()
    };
    if let Some(server) = server {
        let age = now.saturating_duration_since(server.received_at);
        if age <= SERVER_MAX_AGE {
            usage.packet_loss_out = server.packet_loss_out;
            usage.idle_seconds = Some(match last_voice.filter(|last| *last > server.received_at) {
                Some(last) => now.saturating_duration_since(last).as_secs(),
                None => server.idle_seconds.saturating_add(age.as_secs()),
            });
            usage.file_upload_per_second = server.file_upload_per_second;
            usage.file_download_per_second = server.file_download_per_second;
        }
    }
    usage
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
    fn cpu_display_preserves_small_readings_without_trailing_zeroes() {
        for (value, expected) in [
            (0.0, "0%"),
            (0.0001, "<0.001%"),
            (f64::from(0.001_f32), "0.001%"),
            (f64::from(0.02_f32), "0.02%"),
            (0.2, "0.2%"),
            (1.23456, "1.235%"),
            (100.0, "100%"),
            (201.125, "201.125%"),
        ] {
            assert_eq!(format_cpu_percent(value), expected);
        }
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
            packet_loss_out: loss_percent(0.025),
            idle_seconds: 3600,
            file_upload_per_second: Some(100),
            file_download_per_second: None,
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
