use iced::{
    Color, Element, Length, Theme,
    widget::{column, container, progress_bar, row, text},
};

use crate::{
    memory::HeapUsage,
    profiler::{Category, Ranking, TraceSummary},
    usage,
};

const CYAN: Color = Color::from_rgb(0.19, 0.76, 0.84);
const PURPLE: Color = Color::from_rgb(0.65, 0.56, 0.96);
const AMBER: Color = Color::from_rgb(0.98, 0.73, 0.39);
const MUTED: Color = Color::from_rgb(0.46, 0.50, 0.61);

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

fn card<'a, Message: 'a>(title: &'static str, value: String, note: String) -> Element<'a, Message> {
    container(
        column![
            text(title).size(12),
            text(value).size(22),
            text(note).size(11)
        ]
        .spacing(5),
    )
    .padding(14)
    .width(Length::Fill)
    .style(container::rounded_box)
    .into()
}

fn bars<'a, Message: 'a>(
    title: &'static str,
    description: &'static str,
    entries: Vec<Bar>,
    unit: Unit,
) -> Element<'a, Message> {
    let scale = entries
        .iter()
        .map(|entry| entry.value)
        .fold(0.0_f64, f64::max);
    let mut content = column![
        text(title).size(19),
        text(description).size(12),
        text(format!("Shared scale: 0–{}", unit.format(scale)))
            .size(11)
            .color(MUTED),
    ]
    .spacing(12);
    for entry in entries {
        let fraction = if scale > 0.0 {
            (entry.value / scale).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let color = entry.color;
        content = content.push(
            column![
                row![
                    text(entry.label).size(14).width(Length::Fill),
                    text(unit.format(entry.value)).size(14)
                ]
                .spacing(16),
                progress_bar(0.0..=1.0, fraction)
                    .girth(10)
                    .style(move |theme: &Theme| progress_bar::Style {
                        background: theme.extended_palette().background.weak.color.into(),
                        bar: color.into(),
                        border: iced::Border::default().rounded(4),
                    }),
                text(entry.detail).size(11),
            ]
            .spacing(5),
        );
    }
    container(content)
        .padding(18)
        .width(Length::Fill)
        .style(container::bordered_box)
        .into()
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

pub fn trace<'a, Message: 'a>(summary: &TraceSummary) -> Element<'a, Message> {
    let cpu_rank = summary.ranked_operations(Ranking::TotalCpu);
    let elapsed_rank = summary.ranked_operations(Ranking::LongestCall);
    let mut content = column![
        row![
            card(
                "Recording",
                format!("{:.1} s", summary.duration.as_secs_f64()),
                format!("{} captured events", summary.event_count)
            ),
            card(
                "Process CPU",
                summary.process_cpu.map_or_else(
                    || "Not available".into(),
                    |cpu| duration_text(cpu.as_secs_f64())
                ),
                "CPU time across all threads".into()
            ),
            card(
                "Instrumented CPU",
                duration_text(summary.captured_cpu().as_secs_f64()),
                format!("{} events dropped", summary.dropped)
            ),
        ]
        .spacing(12)
    ]
    .spacing(18);
    if summary.dropped > 0 {
        content = content.push(text("The trace buffer filled. Rankings cover captured operations; missing work remains unattributed.").size(13).color(AMBER));
    }
    if let (Some(costliest), Some(longest)) = (cpu_rank.first(), elapsed_rank.first()) {
        content = content.push(
            container(
                column![
                    text(format!(
                        "Most recorded CPU: {} · {} across {} calls",
                        operation_label(costliest.name),
                        duration_text(costliest.timing.cpu.as_secs_f64()),
                        costliest.timing.calls
                    ))
                    .size(14),
                    text(format!(
                        "Longest single call: {} · {} elapsed",
                        operation_label(longest.name),
                        duration_text(longest.timing.longest.as_secs_f64())
                    ))
                    .size(14),
                ]
                .spacing(7),
            )
            .padding(14)
            .style(container::rounded_box),
        );
    } else {
        content = content.push(
            text("No completed operations were captured. Record again while using the app.")
                .size(13),
        );
    }

    let mut subsystem_bars: Vec<_> = Category::ALL
        .into_iter()
        .map(|category| {
            let timing = summary.categories[category as usize];
            Bar {
                label: category.label().into(),
                value: timing.cpu.as_secs_f64(),
                color: CYAN,
                detail: format!(
                    "{} captured calls · {} of one core averaged over the recording",
                    timing.calls,
                    crate::usage::format_cpu_percent(
                        timing.cpu.as_secs_f64() / summary.duration.as_secs_f64().max(0.001)
                            * 100.0
                    )
                ),
            }
        })
        .collect();
    if let Some(other) = summary.other_cpu() {
        subsystem_bars.push(Bar { label: "Other process work".into(), value: other.as_secs_f64(), color: MUTED,
            detail: "Runtime, renderer, tracing overhead and work outside captured spans. This is not a named bottleneck.".into() });
    }
    subsystem_bars.sort_by(|a, b| b.value.total_cmp(&a.value));
    content = content.push(bars("1. Where CPU time went", "Ranked by total CPU time. The gray bar shows what this trace cannot attribute to a subsystem.", subsystem_bars, Unit::Time));

    if !cpu_rank.is_empty() {
        let cpu_bars = cpu_rank
            .into_iter()
            .take(10)
            .map(|operation| Bar {
                label: operation_label(operation.name).into(),
                value: operation.timing.cpu.as_secs_f64(),
                color: CYAN,
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
        content = content.push(bars("2. Operations with the most CPU cost", "Summed CPU across all calls. Frequent small operations can cost more overall than one long call.", cpu_bars, Unit::Time));
        let elapsed_bars = elapsed_rank
            .into_iter()
            .take(10)
            .map(|operation| Bar {
                label: operation_label(operation.name).into(),
                value: operation.timing.longest.as_secs_f64(),
                color: AMBER,
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
        content = content.push(bars("3. Longest single calls", "The slowest captured occurrence of each operation. Elapsed time includes waiting and scheduling; it is different from CPU cost.", elapsed_bars, Unit::Time));
    }
    content.into()
}

pub fn memory<'a, Message: 'a>(
    resident: Option<u64>,
    heap: HeapUsage,
    audio_bytes: usize,
    history_bytes: usize,
    trace_bytes: usize,
) -> Element<'a, Message> {
    let mut process = Vec::new();
    if let Some(resident) = resident {
        process.push(Bar { label: "Resident RAM · whole process".into(), value: resident as f64, color: PURPLE,
            detail: "Physical memory mapped into the process, including native code, graphics, stacks and allocator-retained pages.".into() });
    }
    process.push(Bar { label: "Live Rust heap · now".into(), value: heap.live as f64, color: CYAN,
        detail: "Currently outstanding Rust allocation sizes. Excludes direct native-library allocations and allocator overhead.".into() });
    process.push(Bar {
        label: "Peak Rust heap · since launch".into(),
        value: heap.peak as f64,
        color: AMBER,
        detail: "Historical high-water mark. These allocations are not necessarily still live."
            .into(),
    });

    let mut buffers = vec![
        Bar {
            label: "Audio samples".into(),
            value: audio_bytes as f64,
            color: CYAN,
            detail: "Reserved sample queues and work buffers reported by the audio worker.".into(),
        },
        Bar {
            label: "Usage history".into(),
            value: history_bytes as f64,
            color: PURPLE,
            detail: "Reserved CPU/RAM sample storage; excludes renderer caches.".into(),
        },
        Bar {
            label: "Trace events".into(),
            value: trace_bytes as f64,
            color: AMBER,
            detail: "Reserved trace-event payload, retained for reuse; excludes queue bookkeeping."
                .into(),
        },
    ];
    buffers.sort_by(|a, b| b.value.total_cmp(&a.value));
    column![
        text("Memory · what is actually measured").size(22),
        text("Resident RAM can remain high while the live Rust heap is small. Native graphics/framework allocations, stacks and cached allocator pages are not fully attributed here. These bars overlap and must not be added together.").size(13),
        bars("Process footprint and heap", "Current resident memory compared with live and historical Rust allocations. Each is a separate measurement.", process, Unit::Bytes),
        bars("Known buffer capacity", "Smaller parts of the Rust heap, shown on their own scale. This is not a complete memory allocation breakdown.", buffers, Unit::Bytes),
    ].spacing(14).into()
}
