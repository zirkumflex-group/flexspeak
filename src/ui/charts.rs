//! Line charts drawn as Rune vector shapes, sized to the space layout gives them.

use rune::{Color, Cx, El, canvas, col, row, text};

use super::theme::{Theme, size, ty};
use super::widgets::{card, faint, well};
use crate::connection_charts::{ChartPaint, ConnectionHistory, Metric};
use crate::{App, usage};

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
    canvas(history.chart(w, height, color, t.wash(0.05)))
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

/// The footer omits idle activity; remote peers retain their idle chart.
pub fn connection(
    cx: &mut Cx<App>,
    t: &Theme,
    scope: &str,
    quality: usage::ConnectionUsage,
    history: &ConnectionHistory,
    compact: bool,
    show_idle: bool,
) -> El {
    let p = paint(t);
    let mut strip = row().w_full().gap(size::CHART_GAP);
    let mut column = col().w_full().gap(8.0);
    for reading in readings(quality) {
        if !show_idle && matches!(reading.metric, Metric::Idle) {
            continue;
        }
        let status = reading.status.unwrap_or(reading.health.label());
        let color = t.health(reading.health);
        let key = format!("{scope}-{}", reading.short);
        let height = if compact { 20.0 } else { 32.0 };
        let w = width(cx, &key);
        let chart = canvas(history.chart(reading.metric, w, height, &p))
            .key(key)
            .w_full()
            .h(height)
            .a11y_hidden();
        let help = format!(
            "{} · {status}\n{}\nLast 60 s · auto-scaled · shaded band: ping variation",
            reading.label, reading.explanation
        );
        if compact {
            strip = strip.child(
                col()
                    .grow(1.0)
                    .min_w(size::CHART_MIN_WIDTH)
                    .gap(3.0)
                    .help(help)
                    .label(format!("{} {}", reading.label, reading.value))
                    .child(
                        row()
                            .w_full()
                            .gap(4.0)
                            .center_y()
                            .child(
                                text(reading.short)
                                    .font(ty::CAPTION)
                                    .color(t.faint)
                                    .grow(1.0),
                            )
                            .child(text(reading.value).font(ty::NUMERIC).color(color)),
                    )
                    .child(chart),
            );
        } else {
            column = column.child(
                well(t)
                    .padding(8.0)
                    .gap(4.0)
                    .help(help)
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
    }
    if compact {
        return strip;
    }
    card(t).padding(12.0).gap(8.0).child(column).child(faint(
        t,
        "Last 60 s → · auto-scaled · shaded band: ping variation",
    ))
}
