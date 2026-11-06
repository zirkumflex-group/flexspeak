//! The fixed footer: own connection health, the live connection charts and process resources.

use rune::{Color, Cx, El, col, el, icon, row, text};

use super::theme::{Theme, radius, ty};
use super::widgets::{divider, vertical_divider};
use super::{charts, icons};
use crate::{App, usage};

fn status_pill(s: &App, t: &Theme) -> El {
    let (health, label, detail) = s.footer_connection_status();
    let color = t.health(health);
    let lit = health != usage::Health::Unknown;
    row()
        .key("health-pill")
        .h(22.0)
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(radius::PILL)
        .fill(color.alpha(if lit { 0.12 } else { 0.06 }))
        .help(detail)
        .label(format!("Connection {label}"))
        .motion(super::theme::motion::READING)
        .child(el().size(6.0, 6.0).radius(3.0).fill(color).a11y_hidden())
        .child(text(label).font(ty::SMALL).color(color).a11y_hidden())
}

fn reading(t: &Theme, glyph: &'static rune::Icon, label: &str, value: String) -> El {
    // Plain tabular figures: readings change every second, and an animated roll would turn
    // each tick into half a second of full-window frames.
    row()
        .gap(6.0)
        .center_y()
        .label(format!("{label} {value}"))
        .child(icon(glyph, 14.0).color(t.faint).a11y_hidden())
        .child(
            text(label.to_owned())
                .font(ty::CAPTION)
                .color(t.faint)
                .a11y_hidden(),
        )
        .child(text(value).font(ty::NUMERIC).color(t.text).a11y_hidden())
}

fn readings(s: &App, t: &Theme, stacked: bool) -> El {
    let cpu = s.process_usage.cpu_percent.map_or_else(
        || "…".into(),
        |value| usage::format_cpu_percent(f64::from(value)),
    );
    let ram = usage::format_mib(s.process_usage.memory_bytes);
    let footprint = usage::format_mib(s.process_usage.memory_detail_bytes);
    let items = [
        reading(t, &icons::CPU, "CPU", cpu),
        reading(t, &icons::MEMORY, "RAM", ram),
        reading(t, &icons::ACTIVITY, usage::MEMORY_DETAIL_LABEL, footprint),
    ];
    if stacked {
        col().gap(3.0).w(160.0).children(items)
    } else {
        row().gap(20.0).center_y().flex_wrap().children(items)
    }
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, _compact: bool) -> El {
    let show_connection = s.connected && !s.loopback && !s.stopping && s.state.current.is_some();
    let content = if show_connection {
        row()
            .w_full()
            .gap(16.0)
            .center_y()
            .child(status_pill(s, t))
            .child(
                el().key("footer-charts")
                    .grow(1.0)
                    .min_w(0.0)
                    .h(44.0)
                    .scroll_x()
                    .scroll_edge_fade(12.0)
                    // The strip fills the band; below four 110 pt charts it scrolls sideways.
                    .child(
                        charts::connection(
                            cx,
                            t,
                            "footer",
                            s.connection_usage,
                            &s.connection_history,
                            true,
                        )
                        .h(44.0)
                        .center_y(),
                    ),
            )
            .child(vertical_divider(t, 36.0))
            .child(readings(s, t, true))
    } else {
        row()
            .w_full()
            .gap(20.0)
            .center_y()
            .child(status_pill(s, t))
            .child(el().grow(1.0))
            .child(readings(s, t, false))
    };
    col().key("footer").w_full().child(divider(t)).child(
        el().w_full()
            .px(12.0)
            .py(if show_connection { 6.0 } else { 8.0 })
            .fill(t.chrome)
            .child(content),
    )
}

/// Performance mode's footer: resources above the health indicator, no charts.
pub fn compact(s: &App, t: &Theme) -> El {
    col()
        .w_full()
        .child(divider(t))
        .child(
            col()
                .w_full()
                .padding(12.0)
                .gap(8.0)
                .fill(t.chrome)
                .child(readings(s, t, false))
                .child(status_pill(s, t).self_start()),
        )
        .fill(Color::TRANSPARENT)
}
