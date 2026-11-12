//! Usage & Statistics: resource history, connection metrics, peers, traces and memory.

use rune::{Color, Cx, El, Transition, col, el, icon, row, text};
use rune_kit::Kit;

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Variant, button, divider, faint, heading, health_dot, muted, scroll_page, title, well,
};
use super::{charts, icons, send};
use crate::{
    App, Message,
    memory::HeapUsage,
    profiler::{self, Category, Ranking, TraceSummary},
    usage,
};

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit) -> El {
    let mut content = col()
        .w_full()
        .max_w(1100.0)
        .self_center()
        .padding(size::PAGE)
        .gap(12.0)
        .child(usage_view(s, cx, t));
    if s.server().connected && !s.loopback {
        content = content
            .child(muted(
                t,
                format!(
                    "Voice packets · sent {} · received {} · playback gaps {}",
                    s.server().sent,
                    s.server().received,
                    crate::audio::playback_gaps()
                ),
            ))
            .child(faint(t, s.server().detail.clone()))
            .child(client_connections(s, cx, t));
    }
    content = content
        .child(divider(t).my(8.0))
        .child(title(t, "Traces & CPU breakdown"))
        .child(muted(
            t,
            "Record up to 30 seconds of timings and resource counters. Audio, messages and passwords are excluded.",
        ));
    content = content.child(if let Some(recording) = &s.recording {
        row()
            .gap(12.0)
            .center_y()
            .child(super::widgets::pulse(t.live))
            .child(button(
                t,
                "stop-trace",
                format!(
                    "Stop & save trace · {} s",
                    recording.started.elapsed().as_secs()
                ),
                Some(&icons::STOP),
                Variant::Danger,
                Some(Message::StopTrace),
            ))
    } else {
        row().child(button(
            t,
            "record-trace",
            if s.trace_saving {
                "Saving trace…"
            } else {
                "Record 30 s Trace"
            },
            Some(&icons::RECORD),
            Variant::Primary,
            (!s.trace_saving).then_some(Message::RecordTrace),
        ))
    });
    if let Some(path) = &s.trace_path {
        content = content
            .child(
                row()
                    .gap(12.0)
                    .center_y()
                    .child(muted(
                        t,
                        format!(
                            "Saved {}",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                    ))
                    .child(button(
                        t,
                        "copy-trace",
                        "Copy Path",
                        Some(&icons::COPY),
                        Variant::Secondary,
                        Some(Message::CopyTracePath),
                    )),
            )
            .child(faint(
                t,
                "The JSON trace opens in Perfetto or a Chrome trace viewer.",
            ));
    }
    if let Some(summary) = &s.trace_summary {
        content = content.child(trace(t, summary));
    } else if s.recording.is_none() {
        content = content.child(muted(
            t,
            "Record a trace to measure CPU time in UI, networking, audio processing and audio callbacks.",
        ));
    }
    content = content.child(divider(t).my(8.0)).child(memory(
        t,
        s.process_usage.memory_bytes,
        s.heap_usage,
        s.audio_buffer_bytes,
        s.cpu_history.buffer_bytes()
            + s.ram_history.buffer_bytes()
            + s.server().connection_history.buffer_bytes()
            + s.server().client_history.buffer_bytes(),
        profiler::buffer_bytes(),
    ));
    scroll_page(k, "statistics-scroll", content)
}

/// Readings as a plain table under a heading: the label leads, the figure trails, and hairlines
/// part the rows, as a desktop app's inspector does.
fn readings(t: &Theme, key: &str, title: &'static str, rows: Vec<(&'static str, String)>) -> El {
    let mut table = col()
        .key(key.to_owned())
        .grow(1.0)
        .w(0.0)
        .min_w(260.0)
        .child(heading(t, title).mb(6.0));
    for (label, value) in rows {
        table = table.child(divider(t)).child(
            row()
                .w_full()
                .h(30.0)
                .gap(16.0)
                .center_y()
                .child(
                    text(label)
                        .font(ty::BODY)
                        .color(t.muted)
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(text(value).font(ty::NUMERIC).color(t.text)),
        );
    }
    table
}

fn usage_view(s: &App, cx: &mut Cx<App>, t: &Theme) -> El {
    let quality = s.server().connection_usage;
    let network = s.server().network_usage;
    let percent =
        |value: Option<f32>| value.map_or_else(|| "—".into(), |value| format!("{value:.1} %"));
    let rate = |value: f64| format!("{}/s", usage::format_bytes(value));
    let file_rate =
        |value: Option<u64>| value.map_or_else(|| "—".into(), |value| rate(value as f64));
    col()
        .w_full()
        .gap(20.0)
        .child(
            row()
                .w_full()
                .gap(40.0)
                .flex_wrap()
                .child(history_card(
                    cx,
                    t,
                    "cpu",
                    "App CPU",
                    s.process_usage.cpu_percent.map_or_else(
                        || "—".into(),
                        |value| usage::format_cpu_percent(f64::from(value)),
                    ),
                    &s.cpu_history,
                    true,
                    None,
                ))
                .child(history_card(
                    cx,
                    t,
                    "ram",
                    "RAM (RSS)",
                    usage::format_mib(s.process_usage.memory_bytes),
                    &s.ram_history,
                    false,
                    Some((
                        usage::MEMORY_DETAIL_LABEL,
                        usage::format_mib(s.process_usage.memory_detail_bytes),
                    )),
                )),
        )
        .child(
            row()
                .w_full()
                .gap(40.0)
                .flex_wrap()
                .child(readings(
                    t,
                    "connection-readings",
                    "Connection",
                    vec![
                        ("Upload", rate(network.upload_per_second)),
                        ("Download", rate(network.download_per_second)),
                        ("Packet loss in", percent(quality.packet_loss_in)),
                        ("Packet loss out", percent(quality.packet_loss_out)),
                        ("Ping", quality.ping_text()),
                        (
                            "Idle time",
                            quality
                                .idle_seconds
                                .map_or_else(|| "—".into(), usage::format_idle),
                        ),
                    ],
                ))
                .child(readings(
                    t,
                    "transfer-readings",
                    "Transfers",
                    vec![
                        (
                            "File transfer in",
                            file_rate(quality.file_download_per_second),
                        ),
                        (
                            "File transfer out",
                            file_rate(quality.file_upload_per_second),
                        ),
                        (
                            "Session sent",
                            usage::format_bytes(network.uploaded as f64),
                        ),
                        (
                            "Session received",
                            usage::format_bytes(network.downloaded as f64),
                        ),
                    ],
                )),
        )
        .child(faint(
            t,
            "CPU: 100% = one core · Memory counters overlap; do not add them · Traffic: TeamSpeak estimate · Local: 1 s / server: 5 s · — unavailable",
        ))
}

#[allow(clippy::too_many_arguments)]
fn history_card(
    cx: &mut Cx<App>,
    t: &Theme,
    key: &str,
    label: &'static str,
    current: String,
    history: &usage::MetricHistory,
    cpu: bool,
    memory_detail: Option<(&'static str, String)>,
) -> El {
    let format = |value: f64| {
        if cpu {
            usage::format_cpu_percent(value)
        } else {
            usage::format_bytes(value)
        }
    };
    let values = history.percentiles.map_or_else(
        || "P50 —   ·   P90 —   ·   P99 —".into(),
        |p| {
            format!(
                "P50 {}   ·   P90 {}   ·   P99 {}",
                format(p.p50),
                format(p.p90),
                format(p.p99)
            )
        },
    );
    let color = if cpu { t.cpu } else { t.ram };
    let mut heading_row = row()
        .w_full()
        .gap(10.0)
        .center_y()
        .child(
            icon(if cpu { &icons::CPU } else { &icons::MEMORY }, 16.0)
                .color(color)
                .a11y_hidden(),
        )
        .child(text(label).font(ty::LABEL).color(t.muted))
        .child(text(current).font(ty::BIG_VALUE).color(t.text));
    if let Some((detail_label, value)) = memory_detail {
        // Kept off the card's edge, and wrapped under the reading when the card is narrow.
        heading_row = heading_row
            .flex_wrap()
            .pr(12.0)
            .child(el().grow(1.0))
            .child(
                text(format!("{detail_label}  {value}"))
                    .font(ty::NUMERIC)
                    .color(t.muted),
            );
    }
    col()
        .grow(1.0)
        .min_w(320.0)
        .w(0.0)
        .gap(6.0)
        .child(heading_row)
        .child(faint(t, format!("Scale 0–{}", format(history.upper_bound))))
        .child(charts::history(
            cx,
            t,
            &format!("{key}-history"),
            history,
            color,
            86.0,
        ))
        .child(text(values).font(ty::NUMERIC).color(t.muted))
        .child(faint(
            t,
            format!(
                "{}Last 5 min · {} samples",
                if cpu { "" } else { "RSS history · " },
                history.len()
            ),
        ))
}

fn client_connections(s: &App, cx: &mut Cx<App>, t: &Theme) -> El {
    let mut clients = col()
        .w_full()
        .gap(8.0)
        .child(heading(t, "Channel connections"));
    let mut count = 0;
    if let Some(channel) = &s.server().state.current {
        for member in channel.members.iter().filter(|member| !member.is_self) {
            count += 1;
            let expanded = s.server().expanded_client == Some(member.id);
            let quality = s.client_quality(member.id, std::time::Instant::now());
            let (health, detail) = s.member_health(member, true);
            let mut header = row()
                .key(format!("peer-{}", member.id))
                .role("button")
                .label(member.name.clone())
                .expanded(expanded)
                .w_full()
                .h(30.0)
                .px(6.0)
                .gap(10.0)
                .center_y()
                .radius(radius::ROW)
                .child(health_dot(t, health, detail))
                .child(
                    text(member.name.clone())
                        .font(ty::BODY_STRONG)
                        .color(t.text)
                        .grow(1.0)
                        .a11y_hidden(),
                )
                .child(
                    icon(&icons::CHEVRON_RIGHT, 14.0)
                        .color(t.faint)
                        .rotate(if expanded {
                            std::f32::consts::FRAC_PI_2
                        } else {
                            0.0
                        })
                        .motion(motion::QUICK)
                        .a11y_hidden(),
                );
            if !s.server().stopping {
                header = header
                    .focusable(true)
                    .cursor(rune::Cursor::Pointer)
                    .hover(t.wash(0.04))
                    .press(t.wash(0.08))
                    .on_press_tx(send(Message::ToggleClient(member.id)));
            }
            clients = clients.child(header);
            if expanded {
                clients = clients.child(
                    col()
                        .key(format!("peer-detail-{}", member.id))
                        .w_full()
                        .gap(6.0)
                        .transition(Transition::opacity().combined(Transition::offset(0.0, -6.0)))
                        .child(charts::connection(
                            cx,
                            t,
                            "peer",
                            quality,
                            &s.server().client_history,
                            !member.is_self,
                        ))
                        .child(faint(
                            t,
                            match s.server().client_usage.get(&member.id) {
                                Some(Err(error)) => *error,
                                Some(Ok(_)) => {
                                    "Server snapshot · refreshes are staggered across your channel"
                                }
                                None => "Waiting for a connection-health sample…",
                            },
                        )),
                );
            }
        }
    }
    if count == 0 {
        clients = clients.child(muted(t, "No other clients in your channel."));
    }
    clients
}

const MUTED_BAR: fn(&Theme) -> Color = |t| t.faint;

struct Bar {
    label: String,
    detail: String,
    value: f64,
    color: Color,
}

#[derive(Clone, Copy)]
enum Unit {
    Time,
    Bytes,
}

impl Unit {
    fn format(self, value: f64) -> String {
        match self {
            Self::Time => duration_text(value),
            Self::Bytes => usage::format_bytes(value),
        }
    }
}

fn duration_text(seconds: f64) -> String {
    if seconds == 0.0 {
        "0 ms".into()
    } else if seconds < 0.001 {
        format!("{:.1} µs", seconds * 1e6)
    } else if seconds < 1.0 {
        format!("{:.2} ms", seconds * 1000.0)
    } else {
        format!("{seconds:.2} s")
    }
}

fn stat_card(t: &Theme, title: &'static str, value: String, note: String) -> El {
    col()
        .grow(1.0)
        .min_w(180.0)
        .w(0.0)
        .gap(4.0)
        .child(text(title).font(ty::CAPTION).color(t.faint))
        .child(text(value).font(ty::BIG_VALUE).color(t.text))
        .child(faint(t, note))
}

fn bars(
    t: &Theme,
    title: &'static str,
    description: &'static str,
    entries: Vec<Bar>,
    unit: Unit,
) -> El {
    let scale = entries
        .iter()
        .map(|entry| entry.value)
        .fold(0.0_f64, f64::max);
    let mut content = col()
        .w_full()
        .gap(12.0)
        .pt(8.0)
        .child(heading(t, title))
        .child(muted(t, description))
        .child(faint(t, format!("Shared scale: 0–{}", unit.format(scale))));
    for (index, entry) in entries.into_iter().enumerate() {
        let fraction = if scale > 0.0 {
            (entry.value / scale).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        content = content.child(
            col()
                .key(format!("bar-{index}"))
                .w_full()
                .gap(5.0)
                .child(
                    row()
                        .w_full()
                        .gap(16.0)
                        .child(text(entry.label).font(ty::BODY).color(t.text).grow(1.0))
                        .child(
                            text(unit.format(entry.value))
                                .font(ty::NUMERIC)
                                .color(t.text),
                        ),
                )
                .child(
                    el().w_full()
                        .h(6.0)
                        .radius(3.0)
                        .fill(t.wash(0.06))
                        .child(el().h_full().w_frac(fraction).radius(3.0).fill(entry.color)),
                )
                .child(faint(t, entry.detail)),
        );
    }
    content
}

fn operation_label(name: &str) -> &str {
    match name {
        "ui.update" => "UI updates",
        "ui.view" => "Build the interface",
        "network.poll" => "Handle network events",
        "network.send_voice" => "Send voice packets",
        "audio.process" => "Encode, mix and resample",
        "audio.receive" => "Queue incoming voice",
        "audio.capture_callback" => "Microphone callback",
        "audio.playback_callback" => "Speaker callback",
        other => other,
    }
}

fn trace(t: &Theme, summary: &TraceSummary) -> El {
    let cpu_rank = summary.ranked_operations(Ranking::TotalCpu);
    let elapsed_rank = summary.ranked_operations(Ranking::LongestCall);
    let mut content = col().w_full().gap(18.0).child(
        row()
            .w_full()
            .gap(12.0)
            .flex_wrap()
            .child(stat_card(
                t,
                "Recording",
                format!("{:.1} s", summary.duration.as_secs_f64()),
                format!("{} captured events", summary.event_count),
            ))
            .child(stat_card(
                t,
                "Process CPU",
                summary.process_cpu.map_or_else(
                    || "Not available".into(),
                    |cpu| duration_text(cpu.as_secs_f64()),
                ),
                "CPU time across all threads".into(),
            ))
            .child(stat_card(
                t,
                "Instrumented CPU",
                duration_text(summary.captured_cpu().as_secs_f64()),
                format!("{} events dropped", summary.dropped),
            )),
    );
    if summary.dropped > 0 {
        content = content.child(
            text("The trace buffer filled. Rankings cover captured operations; missing work remains unattributed.")
                .font(ty::SMALL)
                .color(t.fair)
                .wrap(),
        );
    }
    if let (Some(costliest), Some(longest)) = (cpu_rank.first(), elapsed_rank.first()) {
        content = content.child(
            well(t)
                .padding(14.0)
                .gap(7.0)
                .child(super::widgets::body(
                    t,
                    format!(
                        "Most recorded CPU: {} · {} across {} calls",
                        operation_label(costliest.name),
                        duration_text(costliest.timing.cpu.as_secs_f64()),
                        costliest.timing.calls
                    ),
                ))
                .child(super::widgets::body(
                    t,
                    format!(
                        "Longest single call: {} · {} elapsed",
                        operation_label(longest.name),
                        duration_text(longest.timing.longest.as_secs_f64())
                    ),
                )),
        );
    } else {
        content = content.child(muted(
            t,
            "No completed operations were captured. Record again while using the app.",
        ));
    }
    let mut subsystem_bars: Vec<_> = Category::ALL
        .into_iter()
        .map(|category| {
            let timing = summary.categories[category as usize];
            Bar {
                label: category.label().into(),
                value: timing.cpu.as_secs_f64(),
                color: t.cpu,
                detail: format!(
                    "{} captured calls · {} of one core averaged over the recording",
                    timing.calls,
                    usage::format_cpu_percent(
                        timing.cpu.as_secs_f64() / summary.duration.as_secs_f64().max(0.001)
                            * 100.0
                    )
                ),
            }
        })
        .collect();
    if let Some(other) = summary.other_cpu() {
        subsystem_bars.push(Bar {
            label: "Other process work".into(),
            value: other.as_secs_f64(),
            color: MUTED_BAR(t),
            detail: "Runtime, renderer, tracing overhead and work outside captured spans. This is not a named bottleneck.".into(),
        });
    }
    subsystem_bars.sort_by(|a, b| b.value.total_cmp(&a.value));
    content = content.child(bars(
        t,
        "1. Where CPU time went",
        "Ranked by total CPU time. The gray bar shows what this trace cannot attribute to a subsystem.",
        subsystem_bars,
        Unit::Time,
    ));
    if !cpu_rank.is_empty() {
        let cpu_bars = cpu_rank
            .into_iter()
            .take(10)
            .map(|operation| Bar {
                label: operation_label(operation.name).into(),
                value: operation.timing.cpu.as_secs_f64(),
                color: t.cpu,
                detail: format!(
                    "{} · {} calls · {} CPU per call on average",
                    operation.name,
                    operation.timing.calls,
                    duration_text(
                        operation.timing.cpu.as_secs_f64() / operation.timing.calls as f64
                    )
                ),
            })
            .collect();
        content = content.child(bars(
            t,
            "2. Operations with the most CPU cost",
            "Summed CPU across all calls. Frequent small operations can cost more overall than one long call.",
            cpu_bars,
            Unit::Time,
        ));
        let elapsed_bars = elapsed_rank
            .into_iter()
            .take(10)
            .map(|operation| Bar {
                label: operation_label(operation.name).into(),
                value: operation.timing.longest.as_secs_f64(),
                color: t.fair,
                detail: format!(
                    "{} · {} calls · {} elapsed per call on average",
                    operation.name,
                    operation.timing.calls,
                    duration_text(
                        operation.timing.wall.as_secs_f64() / operation.timing.calls as f64
                    )
                ),
            })
            .collect();
        content = content.child(bars(
            t,
            "3. Longest single calls",
            "The slowest captured occurrence of each operation. Elapsed time includes waiting and scheduling; it is different from CPU cost.",
            elapsed_bars,
            Unit::Time,
        ));
    }
    content
}

fn memory(
    t: &Theme,
    resident: Option<u64>,
    heap: HeapUsage,
    audio_bytes: usize,
    history_bytes: usize,
    trace_bytes: usize,
) -> El {
    let mut process = Vec::new();
    if let Some(resident) = resident {
        process.push(Bar {
            label: "Resident RAM · whole process".into(),
            value: resident as f64,
            color: t.ram,
            detail: "Physical memory mapped into the process, including native code, graphics, stacks and allocator-retained pages.".into(),
        });
    }
    process.push(Bar {
        label: "Live Rust heap · now".into(),
        value: heap.live as f64,
        color: t.cpu,
        detail: "Currently outstanding Rust allocation sizes. Excludes direct native-library allocations and allocator overhead.".into(),
    });
    process.push(Bar {
        label: "Peak Rust heap · since launch".into(),
        value: heap.peak as f64,
        color: t.fair,
        detail: "Historical high-water mark. These allocations are not necessarily still live."
            .into(),
    });
    let mut buffers = vec![
        Bar {
            label: "Audio samples".into(),
            value: audio_bytes as f64,
            color: t.cpu,
            detail: "Reserved sample queues and work buffers reported by the audio worker.".into(),
        },
        Bar {
            label: "Usage history".into(),
            value: history_bytes as f64,
            color: t.ram,
            detail: "Reserved CPU/RAM sample storage; excludes renderer caches.".into(),
        },
        Bar {
            label: "Trace events".into(),
            value: trace_bytes as f64,
            color: t.fair,
            detail: "Reserved trace-event payload, retained for reuse; excludes queue bookkeeping."
                .into(),
        },
    ];
    buffers.sort_by(|a, b| b.value.total_cmp(&a.value));
    col()
        .w_full()
        .gap(14.0)
        .child(title(t, "Memory · what is actually measured"))
        .child(muted(
            t,
            "Resident RAM can remain high while the live Rust heap is small. Native graphics/framework allocations, stacks and cached allocator pages are not fully attributed here. These bars overlap and must not be added together.",
        ))
        .child(bars(
            t,
            "Process footprint and heap",
            "Current resident memory compared with live and historical Rust allocations. Each is a separate measurement.",
            process,
            Unit::Bytes,
        ))
        .child(bars(
            t,
            "Known buffer capacity",
            "Smaller parts of the Rust heap, shown on their own scale. This is not a complete memory allocation breakdown.",
            buffers,
            Unit::Bytes,
        ))
}
