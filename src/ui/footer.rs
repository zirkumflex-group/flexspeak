//! The fixed footer: own connection health, the live connection charts and process resources.

use rune::{Color, Cx, El, col, el, icon, row, text};

use super::theme::{Theme, radius, ty};
use super::widgets::{Variant, divider, icon_button, vertical_divider};
use super::{charts, icons};
use crate::{App, Message, usage};

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

/// Resource readings stay quiet until they run high, then take the warning colours.
fn load_color(t: &Theme, health: usage::Health) -> Option<Color> {
    matches!(health, usage::Health::Fair | usage::Health::Poor).then(|| t.health(health))
}

fn reading(
    t: &Theme,
    glyph: &'static rune::Icon,
    label: &str,
    value: String,
    health: usage::Health,
) -> El {
    // Plain tabular figures: readings change every second, and an animated roll would turn
    // each tick into half a second of full-window frames.
    let warning = load_color(t, health);
    row()
        .gap(6.0)
        .center_y()
        .label(format!("{label} {value}"))
        .child(
            icon(glyph, 14.0)
                .color(warning.unwrap_or(t.faint))
                .a11y_hidden(),
        )
        .child(
            text(label.to_owned())
                .font(ty::CAPTION)
                .color(t.faint)
                .a11y_hidden(),
        )
        .child(
            text(value)
                .font(ty::NUMERIC)
                .color(warning.unwrap_or(t.text))
                .a11y_hidden(),
        )
}

struct Resources {
    cpu: (String, usage::Health),
    ram: (String, usage::Health),
    footprint: (String, usage::Health),
}

fn resources(s: &App) -> Resources {
    let usage = &s.process_usage;
    Resources {
        cpu: (
            usage.cpu_percent.map_or_else(
                || "…".into(),
                |value| usage::format_cpu_percent(f64::from(value)),
            ),
            usage::cpu_health(usage.cpu_percent),
        ),
        ram: (
            usage::format_mib(usage.memory_bytes),
            usage::memory_health(usage.memory_bytes),
        ),
        footprint: (
            usage::format_mib(usage.memory_detail_bytes),
            usage::memory_health(usage.memory_detail_bytes),
        ),
    }
}

fn readings(s: &App, t: &Theme, stacked: bool) -> El {
    let r = resources(s);
    let items = [
        reading(t, &icons::CPU, "CPU", r.cpu.0, r.cpu.1),
        reading(t, &icons::MEMORY, "RAM", r.ram.0, r.ram.1),
        reading(
            t,
            &icons::ACTIVITY,
            usage::MEMORY_DETAIL_LABEL,
            r.footprint.0,
            r.footprint.1,
        ),
    ];
    if stacked {
        col().gap(3.0).w(160.0).children(items)
    } else {
        row().gap(20.0).center_y().flex_wrap().children(items)
    }
}

/// The hidden footer's resources: icons only, coloured by load, the figure in the tooltip.
fn resource_icons(s: &App, t: &Theme) -> El {
    let r = resources(s);
    let mut icons_row = row().key("resource-icons").gap(2.0).center_y();
    for (key, glyph, label, (value, health)) in [
        ("cpu", &icons::CPU, "CPU", r.cpu),
        (
            "footprint",
            &icons::ACTIVITY,
            usage::MEMORY_DETAIL_LABEL,
            r.footprint,
        ),
    ] {
        icons_row = icons_row.child(
            el().key(key)
                .size(22.0, 22.0)
                .center()
                .radius(6.0)
                .help(format!("{label} {value}"))
                .label(format!("{label} {value}"))
                .child(
                    icon(glyph, 14.0)
                        .color(t.health(health))
                        .motion(super::theme::motion::READING)
                        .a11y_hidden(),
                ),
        );
    }
    icons_row
}

fn toggle(t: &Theme, hidden: bool) -> El {
    icon_button(
        t,
        "footer-toggle",
        if hidden {
            &icons::CHEVRON_UP
        } else {
            &icons::CHEVRON_DOWN
        },
        if hidden {
            "Show connection charts and resources"
        } else {
            "Hide connection charts and resources"
        },
        Variant::Ghost,
        Some(Message::ToggleFooter),
    )
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme) -> El {
    let show_connection = s.connected && !s.loopback && !s.stopping && s.state.current.is_some();
    let (content, py) = if s.footer_hidden {
        (
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(status_pill(s, t))
                .child(resource_icons(s, t))
                .child(el().grow(1.0))
                .child(toggle(t, true)),
            4.0,
        )
    } else if show_connection {
        (
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
                .child(toggle(t, false)),
            6.0,
        )
    } else {
        (
            row()
                .w_full()
                .gap(20.0)
                .center_y()
                .child(status_pill(s, t))
                .child(el().grow(1.0))
                .child(readings(s, t, false))
                .child(toggle(t, false)),
            8.0,
        )
    };
    col()
        .key("footer")
        .w_full()
        .child(divider(t))
        .child(el().w_full().px(12.0).py(py).fill(t.chrome).child(content))
}
