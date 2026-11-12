//! Flexspeak's own building blocks. Every surface shares one bevel: a hairline border that is
//! lighter on top, as if lit from above, over a soft two-layer shadow.

use rune::{Color, Drag, El, Icon, Phase, ScrollSpec, TextEdit, Tx, col, el, icon, row, text};
use rune_kit::{Kit, Select, TextInput};

use super::theme::{Theme, motion, radius, size, ty};
use super::{dispatch, icons, send};
use crate::{App, Message, usage::Health};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
    Danger,
    Ghost,
    /// A toggled-on control (a selected page).
    Active,
    /// Ending the call: a solid danger fill.
    Hangup,
}

struct Skin {
    fill: Color,
    top: Color,
    bottom: Color,
    ink: Color,
    hover: Color,
    press: Color,
}

fn skin(t: &Theme, v: Variant) -> Skin {
    match v {
        Variant::Primary => Skin {
            fill: t.accent,
            top: Color::WHITE.alpha(0.35),
            bottom: Color::BLACK.alpha(0.12),
            ink: t.on_accent,
            hover: Color::WHITE.alpha(0.10),
            press: Color::BLACK.alpha(0.10),
        },
        Variant::Secondary => Skin {
            fill: t.raised,
            top: t.bevel_top,
            bottom: t.bevel_bottom,
            ink: t.text,
            hover: t.wash(0.05),
            press: t.wash(0.09),
        },
        Variant::Danger => Skin {
            fill: t.poor.alpha(0.13),
            top: t.poor.alpha(0.40),
            bottom: t.poor.alpha(0.12),
            ink: t.poor,
            hover: t.poor.alpha(0.10),
            press: t.poor.alpha(0.18),
        },
        Variant::Ghost => Skin {
            fill: Color::TRANSPARENT,
            top: Color::TRANSPARENT,
            bottom: Color::TRANSPARENT,
            ink: t.muted,
            hover: t.wash(0.06),
            press: t.wash(0.10),
        },
        Variant::Hangup => Skin {
            fill: t.poor,
            top: Color::WHITE.alpha(0.25),
            bottom: Color::BLACK.alpha(0.15),
            ink: t.on_accent,
            hover: Color::WHITE.alpha(0.10),
            press: Color::BLACK.alpha(0.12),
        },
        Variant::Active => Skin {
            fill: t.accent.alpha(0.16),
            top: t.accent.alpha(0.45),
            bottom: t.accent.alpha(0.12),
            ink: t.accent,
            hover: t.accent.alpha(0.08),
            press: t.accent.alpha(0.16),
        },
    }
}

fn surface(t: &Theme, e: El, v: Variant, enabled: bool, on: Option<Message>) -> El {
    let s = skin(t, v);
    let mut e = e
        .fill(s.fill)
        .border_gradient(1.0, s.top, s.bottom)
        .motion(motion::QUICK);
    if v == Variant::Primary {
        // The faint top-down sheen of a default push button; no halo.
        e = e.linear_gradient(
            std::f32::consts::PI,
            [
                (0.0, Color::WHITE.alpha(0.10)),
                (1.0, Color::WHITE.alpha(0.0)),
            ],
        );
    }
    match on.filter(|_| enabled) {
        Some(message) => e
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(s.hover)
            .press(s.press)
            .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
            .on_press_tx(send(message)),
        None => e.disabled(true).dim(0.42),
    }
}

/// A labelled button, optionally led by an icon. `on` is `None` while it can't be used.
pub fn button(
    t: &Theme,
    key: &str,
    label: impl Into<String>,
    glyph: Option<&'static Icon>,
    v: Variant,
    on: Option<Message>,
) -> El {
    let label = label.into();
    let ink = skin(t, v).ink;
    let mut e = row()
        .key(key.to_owned())
        .role("button")
        .label(label.clone())
        .h(size::CONTROL)
        .px(12.0)
        .gap(6.0)
        .center()
        .radius(radius::CONTROL);
    if let Some(glyph) = glyph {
        e = e.child(icon(glyph, 14.0).color(ink).a11y_hidden());
    }
    let e = e.child(text(label).font(ty::LABEL).color(ink).a11y_hidden());
    surface(t, e, v, true, on)
}

/// A square icon button. `help` names it for tooltips and screen readers.
pub fn icon_button(
    t: &Theme,
    key: &str,
    glyph: &'static Icon,
    help: impl Into<String>,
    v: Variant,
    on: Option<Message>,
) -> El {
    let help = help.into();
    let ink = skin(t, v).ink;
    let e = el()
        .key(key.to_owned())
        .role("button")
        .label(help.clone())
        .help(help)
        .size(size::ICON_BUTTON, size::ICON_BUTTON)
        .center()
        .radius(radius::CONTROL)
        .child(icon(glyph, 16.0).color(ink).a11y_hidden());
    surface(t, e, v, true, on)
}

/// A grouped box for one task: flat, with a hairline edge. Shadows belong to things that float
/// above the window (menus, dialogs), not to content.
pub fn card(t: &Theme) -> El {
    col()
        .w_full()
        .padding(size::CARD_PAD)
        .gap(12.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, t.hairline)
}

/// A settings-style group: rows separated by inset hairlines, no padding of its own.
pub fn group(t: &Theme, rows: impl IntoIterator<Item = El>) -> El {
    let mut g = col()
        .w_full()
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, t.hairline)
        .clip_rounded(radius::CARD);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            g = g.child(el().h(1.0).w_full().px(size::CARD_PAD).child(divider(t)));
        }
        g = g.child(r);
    }
    g
}

/// A group's title, above it.
pub fn group_title(t: &Theme, label: impl Into<String>) -> El {
    text(label.into())
        .font(ty::HEADING)
        .color(t.text)
        .px(2.0)
        .pt(6.0)
}

/// A footnote under a group.
pub fn group_note(t: &Theme, label: impl Into<String>) -> El {
    faint(t, label).px(2.0)
}

/// One row of a group: the label (and an optional explanation) leads, the control trails.
pub fn form_row(t: &Theme, label: impl Into<String>, detail: Option<&str>, control: El) -> El {
    let mut lead = col()
        .grow(1.0)
        .min_w(size::FORM_LABEL)
        .gap(2.0)
        .child(text(label.into()).font(ty::BODY).color(t.text).wrap());
    if let Some(detail) = detail {
        lead = lead.child(faint(t, detail.to_owned()));
    }
    row()
        .w_full()
        .min_h(44.0)
        .px(size::CARD_PAD)
        .py(8.0)
        .gap(16.0)
        .center_y()
        .flex_wrap()
        .child(lead)
        .child(control)
}

/// A recessed well: charts, readings and code-like details sit in one.
pub fn well(t: &Theme) -> El {
    col()
        .w_full()
        .radius(radius::ROW)
        .fill(t.recess)
        .border(1.0, t.hairline)
}

pub fn heading(t: &Theme, label: impl Into<String>) -> El {
    text(label.into()).font(ty::HEADING).color(t.text)
}

pub fn title(t: &Theme, label: impl Into<String>) -> El {
    text(label.into()).font(ty::TITLE).color(t.text)
}

pub fn display(t: &Theme, label: impl Into<String>) -> El {
    text(label.into()).font(ty::DISPLAY).color(t.text)
}

pub fn body(t: &Theme, label: impl Into<String>) -> El {
    text(label.into()).font(ty::BODY).color(t.text).wrap()
}

pub fn muted(t: &Theme, label: impl Into<String>) -> El {
    text(label.into()).font(ty::SMALL).color(t.muted).wrap()
}

pub fn faint(t: &Theme, label: impl Into<String>) -> El {
    text(label.into()).font(ty::CAPTION).color(t.faint).wrap()
}

pub fn overline(t: &Theme, label: &str) -> El {
    text(label.to_owned()).font(ty::OVERLINE).color(t.faint)
}

pub fn error_text(t: &Theme, label: impl Into<String>) -> El {
    text(label.into())
        .font(ty::SMALL)
        .color(t.poor)
        .wrap()
        .live_region(rune::LiveRegion::Polite)
}

/// An icon beside a label.
pub fn labelled(glyph: &'static Icon, label: impl Into<String>, color: Color) -> El {
    row()
        .gap(8.0)
        .center_y()
        .child(icon(glyph, 15.0).color(color).a11y_hidden())
        .child(text(label.into()).font(ty::LABEL).color(color))
}

/// Something switched off, drawn as TeamSpeak does: the glyph stays neutral and only its mark
/// is red, so a tree full of muted people doesn't turn red. `glyph` and `mark` are the two
/// halves of one icon, such as `icons::MIC_OFF_GLYPH` and `icons::STRIKE`.
pub fn struck(t: &Theme, glyph: &'static Icon, mark: &'static Icon, size: f32) -> El {
    el().size(size, size)
        .a11y_hidden()
        .child(icon(glyph, size).color(t.muted).a11y_hidden())
        .child(
            icon(mark, size)
                .absolute(0.0, 0.0)
                .color(t.poor)
                .a11y_hidden(),
        )
}

pub fn divider(t: &Theme) -> El {
    el().h(1.0).w_full().fill(t.hairline).a11y_hidden()
}

pub fn vertical_divider(t: &Theme, height: f32) -> El {
    el().w(1.0).h(height).fill(t.hairline).a11y_hidden()
}

/// A lit dot in the health colour, with its explanation as the tooltip.
/// Connection health as a globe in the health colour; the tooltip carries the details.
pub fn health_dot(t: &Theme, health: Health, help: impl Into<String>) -> El {
    el().w(16.0).h(20.0).center().help(help).child(
        icon(&icons::GLOBE, 13.0)
            .color(t.health(health))
            .motion(motion::READING)
            .a11y_hidden(),
    )
}

/// A level from 0 to 1. Readings arrive about ten times a second, so the bar steps to each one
/// rather than springing: a spring would keep the window drawing for the whole call.
pub fn meter(t: &Theme, key: &str, level: f32, color: Color) -> El {
    let level = level.clamp(0.0, 1.0);
    el().key(key.to_owned())
        .role("progressbar")
        .range_value(f64::from(level), 0.0, 1.0)
        .w_full()
        .h(4.0)
        .radius(2.0)
        .fill(t.wash(0.08))
        .clip_rounded(2.0)
        .child(
            el().key("fill")
                .h_full()
                .w_frac(level)
                .radius(3.0)
                .fill(color)
                .a11y_hidden(),
        )
}

/// The values a [`slider`] covers. `step` snaps dragging; `key_step` is one arrow-key press.
#[derive(Clone, Copy)]
pub struct Range {
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub key_step: i32,
}

impl Range {
    pub const PERCENT: Range = Range {
        min: 0,
        max: 100,
        step: 1,
        key_step: 5,
    };

    fn fraction(self, value: i32) -> f32 {
        ((value - self.min) as f32 / (self.max - self.min).max(1) as f32).clamp(0.0, 1.0)
    }

    fn snap(self, value: f32) -> i32 {
        let steps = ((value - self.min as f32) / self.step as f32).round() as i32;
        (self.min + steps * self.step).clamp(self.min, self.max)
    }
}

/// A 0–100 slider whose value travels with the pointer and the arrow keys.
pub fn percent_slider(
    t: &Theme,
    key: &str,
    label: &str,
    value: u8,
    current: fn(&App) -> u8,
    on_change: fn(u8) -> Message,
) -> El {
    slider(
        t,
        key,
        label,
        i32::from(value),
        format!("{value}%"),
        Range::PERCENT,
        move |s| i32::from(current(s)),
        move |value| on_change(value.clamp(0, 100) as u8),
    )
}

/// A stepped slider whose value travels with the pointer and the arrow keys. `current` reads
/// the live value back from the app, so a drag or key repeat starts from what is applied.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    t: &Theme,
    key: &str,
    label: &str,
    value: i32,
    spoken: String,
    range: Range,
    current: impl Fn(&App) -> i32 + Copy + 'static,
    on_change: impl Fn(i32) -> Message + Copy + 'static,
) -> El {
    slider_with_home(
        t, key, label, value, spoken, range, None, current, on_change,
    )
}

/// [`slider`] with a resting value, such as 100% on a volume: a tick marks it, a drag that
/// passes close to it settles on it, and a double-click goes back to it.
#[allow(clippy::too_many_arguments)]
pub fn slider_with_home(
    t: &Theme,
    key: &str,
    label: &str,
    value: i32,
    spoken: String,
    range: Range,
    home: Option<i32>,
    current: impl Fn(&App) -> i32 + Copy + 'static,
    on_change: impl Fn(i32) -> Message + Copy + 'static,
) -> El {
    let fraction = range.fraction(value);
    let knob = 16.0;
    let height = 22.0;
    // Within 3% of the whole range of the resting value, a drag lands on it.
    let pull = (range.max - range.min) as f32 * 0.03;
    let to_value = move |x: f32, w: f32| {
        let share = ((x - knob / 2.0) / (w - knob).max(1.0)).clamp(0.0, 1.0);
        let raw = range.min as f32 + share * (range.max - range.min) as f32;
        match home {
            Some(home) if (raw - home as f32).abs() <= pull => home,
            _ => range.snap(raw),
        }
    };
    let tick = home.map(|home| {
        let at = range.fraction(home);
        row()
            .key("home-lane")
            .absolute(0.0, 0.0)
            .w_full()
            .h(height)
            .child(el().w_frac(at).h(1.0))
            .child(
                el().w(knob).h(height).ml(-knob * at).center().child(
                    el().key("home")
                        .w(2.0)
                        .h(10.0)
                        .radius(1.0)
                        .fill(t.wash(0.28)),
                ),
            )
            .a11y_hidden()
    });
    el().key(key.to_owned())
        .role("slider")
        .label(label.to_owned())
        .focusable(true)
        .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
        .range_value(f64::from(value), f64::from(range.min), f64::from(range.max))
        .range_step(f64::from(range.step))
        .a11y_value(spoken)
        .w_full()
        .h(height)
        .cursor(rune::Cursor::Pointer)
        .on_step(move |s: &mut App, steps, tx| {
            let next = (current(s) + steps * range.key_step).clamp(range.min, range.max);
            dispatch(s, tx, on_change(next));
        })
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| {
            let next = to_value(d.x, d.w);
            match d.phase {
                // A double-click goes back to the resting value and ignores the jitter after.
                Phase::Down if d.clicks >= 2 && home.is_some() => {
                    d.data = f32::NAN;
                    dispatch(s, tx, on_change(home.unwrap_or(next)));
                }
                Phase::Down => {
                    d.data = current(s) as f32;
                    dispatch(s, tx, on_change(next));
                }
                Phase::Move if d.data.is_nan() => {}
                Phase::Move => {
                    tx.instant();
                    dispatch(s, tx, on_change(next));
                }
                Phase::Up => {
                    if d.cancelled() && !d.data.is_nan() {
                        dispatch(s, tx, on_change(d.data as i32));
                    }
                }
            }
        })
        .child(
            el().key("track")
                .absolute(0.0, (height - 4.0) / 2.0)
                .w_full()
                .h(4.0)
                .radius(2.0)
                .fill(t.wash(0.08))
                .a11y_hidden(),
        )
        .child(
            el().key("fill")
                .absolute(0.0, (height - 4.0) / 2.0)
                .w_frac(fraction)
                .h(4.0)
                .radius(2.0)
                .fill(t.accent)
                .motion(motion::QUICK)
                .a11y_hidden(),
        )
        .children(tick)
        .child(
            row()
                .key("knob-lane")
                .absolute(0.0, 0.0)
                .w_full()
                .h(height)
                // The spacer takes `fraction` of the lane and the knob pulls back by the same
                // share of its own width, so it travels from flush left to flush right.
                .child(el().w_frac(fraction).h(1.0).motion(motion::QUICK))
                .child(
                    el().key("knob")
                        .size(knob, knob)
                        .mt((height - knob) / 2.0)
                        .ml(-knob * fraction)
                        .radius(knob / 2.0)
                        .fill(t.knob)
                        .border_gradient(1.0, Color::WHITE, Color::BLACK.alpha(0.15))
                        .box_shadow(super::theme::shadow(0.0, 1.0, 4.0, Color::BLACK.alpha(0.5)))
                        .motion(motion::QUICK),
                )
                .a11y_hidden(),
        )
}

/// A vertical page that scrolls inside the space its parent gives it, with Rune's overlay
/// scroll bar and soft edges where content is hidden.
///
/// The viewport is a column: in a row (`el()`), the content stretches to the viewport's own
/// height, nothing overflows and the wheel does nothing.
pub fn scroll_page(k: &Kit, key: &str, content: El) -> El {
    k.scroll_bar(
        col()
            .key(key.to_owned())
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .scroll_edge_fade(18.0)
            .child(content),
    )
}

/// A single-line field bound to one of the editors in `UiState`.
#[allow(clippy::too_many_arguments)]
pub fn field(
    k: &Kit,
    edit: &TextEdit,
    access: fn(&mut App) -> &mut TextEdit,
    placeholder: &str,
    on_change: Option<fn(String) -> Message>,
    on_submit: Option<Message>,
) -> El {
    let mut input = TextInput::new(edit)
        .placeholder(placeholder)
        .label(placeholder)
        .width(None)
        .height(size::CONTROL)
        .on_edit(access);
    match on_change {
        Some(change) => {
            input = input.on_change(move |s: &mut App, tx: &mut Tx| {
                let value = access(s).text.clone();
                dispatch(s, tx, change(value));
            });
        }
        None => input = input.disabled(true),
    }
    if let Some(submit) = on_submit {
        input = input.on_submit(send(submit));
    }
    input.build(k)
}

/// A choice of a few, side by side: each an icon and a word, the chosen one raised with its
/// icon in the accent colour. `label` names the choice for screen readers.
pub fn choice(
    t: &Theme,
    key: &str,
    label: &str,
    options: Vec<(&'static Icon, &'static str, Message)>,
    selected: Option<usize>,
) -> El {
    let options = options
        .into_iter()
        .map(|(glyph, name, message)| (glyph, name, "", message))
        .collect();
    choice_with_help(t, key, label, options, selected)
}

/// [`choice`] with a one-line tooltip on each option; an empty one shows none.
pub fn choice_with_help(
    t: &Theme,
    key: &str,
    label: &str,
    options: Vec<(&'static Icon, &'static str, &'static str, Message)>,
    selected: Option<usize>,
) -> El {
    let mut group = row()
        .key(key.to_owned())
        .role("radiogroup")
        .label(label.to_owned())
        .equal_columns(options.len())
        .h(size::CONTROL + 4.0)
        .padding(2.0)
        .gap(2.0)
        .radius(radius::CONTROL)
        .fill(t.recess)
        .border(1.0, t.hairline);
    for (index, (glyph, name, help, message)) in options.into_iter().enumerate() {
        let on = selected == Some(index);
        let mut option = row()
            .key(format!("{key}-{index}"))
            .role("radio")
            .label(name)
            .h_full()
            .px(12.0)
            .gap(6.0)
            .center()
            .radius(radius::CONTROL - 2.0)
            .motion(motion::QUICK)
            .child(
                icon(glyph, 14.0)
                    .color(if on { t.accent } else { t.muted })
                    .a11y_hidden(),
            )
            .child(
                text(name)
                    .font(ty::LABEL)
                    .color(if on { t.text } else { t.muted })
                    .a11y_hidden(),
            );
        option = if on {
            option
                .fill(t.raised)
                .border_gradient(1.0, t.bevel_top, t.bevel_bottom)
        } else {
            option
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(t.wash(0.06))
                .press(t.wash(0.10))
                .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
                .on_press_tx(send(message))
        };
        if !help.is_empty() {
            option = option.help(help);
        }
        group = group.child(option);
    }
    group
}

/// A device or source picker. Options are shown by name; choosing one sends its index.
#[allow(clippy::too_many_arguments)]
pub fn picker(
    k: &Kit,
    s: &App,
    key: &'static str,
    names: Vec<String>,
    current: Option<&str>,
    placeholder: &str,
    enabled: bool,
    width: f32,
    on_choose: impl Fn(usize) -> Message + Clone + 'static,
    layer: &mut Vec<El>,
) -> El {
    let lookup = names.clone();
    Select::new(key, &names)
        .current(current)
        .placeholder(placeholder)
        .label(placeholder)
        .width(width.max(160.0))
        .appearance(rune_kit::SelectAppearance::Field)
        .open(s.ui.open == Some(key))
        .disabled(!enabled || names.is_empty())
        .on_toggle(move |s: &mut App| {
            s.ui.open = if s.ui.open == Some(key) {
                None
            } else {
                Some(key)
            };
        })
        .on_close(|s: &mut App| s.ui.open = None)
        .on_choose(move |s: &mut App, name: &str, tx: &mut Tx| {
            s.ui.open = None;
            if let Some(index) = lookup.iter().position(|option| option == name) {
                dispatch(s, tx, on_choose(index));
            }
        })
        .build(k, layer)
}

/// A pulsing dot for short-lived work (connecting, pending, recording). The loop samples at
/// 12 fps, so the window wakes rarely while it shows and sleeps again once it's gone.
pub fn pulse(color: Color) -> El {
    lit_dot(color).cycle(rune::Cycle {
        period: 1.6,
        phase: 0.0,
        stops: &[(0.0, 1.0), (0.5, 0.35), (1.0, 1.0)],
        still: 1.0,
        fps: 12.0,
    })
}

/// A steady lit dot for long-lived states (a live stream, sharing).
pub fn lit_dot(color: Color) -> El {
    el().size(8.0, 8.0).radius(4.0).fill(color).a11y_hidden()
}
