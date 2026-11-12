//! Line charts drawn as Rune vector shapes, sized to the space layout gives them.

use rune::{Color, Cx, El, canvas, col, row, text};

use super::theme::{Theme, size, ty};
use super::widgets::{card, faint, well};
use crate::connection_charts::{ChartPaint, ConnectionHistory, Metric};
use crate::{App, usage};

fn color_key(color: Color) -> [u32; 4] {
    [
        color.r.to_bits(),
        color.g.to_bits(),
        color.b.to_bits(),
        color.a.to_bits(),
    ]
}

fn paint(t: &Theme) -> ChartPaint {
    ChartPaint {
        grid: t.wash(0.05),
        band: t.ram.alpha(0.22),
        health: [t.good, t.fair, t.poor, t.unknown],
    }
}

/// The width layout gave `key` last commit, or a guess before the first one.
fn width(cx: &mut Cx<App>, key: &str) -> f32 {
    cx.layout_size(key)
        .map_or(size::CHART_MIN_WIDTH, |[w, _]| w)
}

pub fn history(
    cx: &mut Cx<App>,
    t: &Theme,
    key: &str,
    history: &usage::MetricHistory,
    color: Color,
    height: f32,
) -> El {
    let w = width(cx, key);
    rune::memo(
        format!("chart-{key}"),
        (
            history.revision,
            w.to_bits(),
            height.to_bits(),
            color_key(color),
            color_key(t.wash(0.05)),
        ),
        || canvas(history.chart(w, height, color, t.wash(0.05))),
    )
    .key(key.to_owned())
    .w_full()
    .h(height)
    .a11y_hidden()
}

struct Reading {
    metric: Metric,
    label: &'static str,
    short: &'static str,
    value: String,
    health: usage::Health,
    status: Option<&'static str>,
    explanation: &'static str,
}

fn readings(quality: usage::ConnectionUsage) -> [Reading; 4] {
    let percent =
        |value: Option<f32>| value.map_or_else(|| "—".into(), |value| format!("{value:.1}%"));
    [
        Reading {
            metric: Metric::LossIn,
            label: "Packet loss in",
            short: "Packet loss in",
            value: percent(quality.packet_loss_in),
            health: usage::Health::loss(quality.packet_loss_in),
            status: None,
            explanation: "Server → this client. Good: below 1%; Fair: 1–5%; Poor: 5% or more.",
        },
        Reading {
            metric: Metric::LossOut,
            label: "Packet loss out",
            short: "Packet loss out",
            value: percent(quality.packet_loss_out),
            health: usage::Health::loss(quality.packet_loss_out),
            status: None,
            explanation: "This client → server. Good: below 1%; Fair: 1–5%; Poor: 5% or more.",
        },
        Reading {
            metric: Metric::Ping,
            label: "Ping",
            short: "Ping",
            value: quality.ping_text(),
            health: usage::Health::ping(quality.ping_ms, quality.ping_deviation_ms),
            status: None,
            explanation: "Round-trip latency ± variation. Fair: 100 ms or variation 20 ms; Poor: 200 ms or variation 50 ms.",
        },
        Reading {
            metric: Metric::Idle,
            label: "Idle time",
            short: "Idle",
            value: quality
                .idle_seconds
                .map_or_else(|| "—".into(), usage::format_idle),
            health: usage::Health::Unknown,
            status: quality
                .idle_seconds
                .map(|seconds| if seconds < 60 { "Active" } else { "Idle" }),
            explanation: "Time since voice activity. Being idle is not a connection problem.",
        },
    ]
}

/// Width of a status-bar sparkline.
const SPARK_WIDTH: f32 = 56.0;

fn help(reading: &Reading, status: &str) -> String {
    format!(
        "{} · {status}
{}
Last 60 s · auto-scaled · shaded band: ping variation",
        reading.label, reading.explanation
    )
}

/// The status bar's connection readings, one line each: label, value, then a small sparkline.
/// Idle time stays out; it says nothing about the connection.
pub fn inline(
    t: &Theme,
    scope: &str,
    quality: usage::ConnectionUsage,
    history: &ConnectionHistory,
) -> Vec<El> {
    let p = paint(t);
    readings(quality)
        .into_iter()
        .filter(|reading| !matches!(reading.metric, Metric::Idle))
        .map(|reading| {
            let label = match reading.metric {
                Metric::LossIn => "Loss in",
                Metric::LossOut => "Loss out",
                _ => reading.short,
            };
            row()
                .key(format!("{scope}-{}", reading.short))
                .gap(8.0)
                .center_y()
                .help(help(&reading, reading.health.label()))
                .label(format!("{} {}", reading.label, reading.value))
                .child(text(label).font(ty::SMALL).color(t.muted).a11y_hidden())
                .child(
                    text(reading.value.clone())
                        .font(ty::NUMERIC)
                        .color(t.reading(reading.health))
                        .a11y_hidden(),
                )
                .child(
                    rune::memo(
                        format!("spark-{scope}-{}", reading.short),
                        (
                            history.revision,
                            reading.metric as u8,
                            SPARK_WIDTH.to_bits(),
                            16.0_f32.to_bits(),
                            color_key(p.grid),
                            color_key(p.band),
                            p.health.map(color_key),
                        ),
                        || canvas(history.chart(reading.metric, SPARK_WIDTH, 16.0, &p)),
                    )
                    .w(SPARK_WIDTH)
                    .h(16.0)
                    .a11y_hidden(),
                )
        })
        .collect()
}

/// A peer's connection charts in Statistics; remote peers keep their idle chart.
pub fn connection(
    cx: &mut Cx<App>,
    t: &Theme,
    scope: &str,
    quality: usage::ConnectionUsage,
    history: &ConnectionHistory,
    show_idle: bool,
) -> El {
    let p = paint(t);
    let mut column = col().w_full().gap(8.0);
    for reading in readings(quality) {
        if !show_idle && matches!(reading.metric, Metric::Idle) {
            continue;
        }
        let status = reading.status.unwrap_or(reading.health.label());
        let color = t.health(reading.health);
        let key = format!("{scope}-{}", reading.short);
        let w = width(cx, &key);
        let chart = rune::memo(
            format!("chart-{key}"),
            (
                history.revision,
                reading.metric as u8,
                w.to_bits(),
                32.0_f32.to_bits(),
                color_key(p.grid),
                color_key(p.band),
                p.health.map(color_key),
            ),
            || canvas(history.chart(reading.metric, w, 32.0, &p)),
        )
        .key(key)
        .w_full()
        .h(32.0)
        .a11y_hidden();
        column = column.child(
            well(t)
                .padding(8.0)
                .gap(4.0)
                .help(help(&reading, status))
                .child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(text(reading.label).font(ty::LABEL).color(t.text).grow(1.0))
                        .child(text(status).font(ty::CAPTION).color(color))
                        .child(text(reading.value).font(ty::NUMERIC).color(t.text)),
                )
                .child(chart),
        );
    }
    card(t).padding(12.0).gap(8.0).child(column).child(faint(
        t,
        "Last 60 s → · auto-scaled · shaded band: ping variation",
    ))
}
