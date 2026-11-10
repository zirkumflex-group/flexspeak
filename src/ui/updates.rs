//! The update notice in the window's top-right corner and the Updates group in Settings.

use std::f32::consts::{FRAC_PI_2, TAU};

use rune::{Color, El, LineCap, Shape, canvas, col, el, icon, row, text};
use rune_kit::{Kit, Switch};

use super::theme::{Theme, motion, size, ty};
use super::widgets::{Variant, button, faint, form_row, group, pulse};
use super::{dispatch, icons};
use crate::changelog::{self, Entry};
use crate::updater::{Event, Notes, Phase, Progress, Retry};
use crate::usage::format_bytes;
use crate::{App, Message};

fn send(event: Event) -> Option<Message> {
    Some(Message::Updater(event))
}

/// A ring that fills clockwise from twelve o'clock, with the percentage inside.
pub fn ring(t: &Theme, key: &str, progress: &Progress, diameter: f32, label: bool) -> El {
    let stroke = if diameter >= 30.0 { 3.0 } else { 2.5 };
    let centre = diameter / 2.0;
    let r = centre - stroke / 2.0 - 0.5;
    let point = |angle: f32| [centre + r * angle.cos(), centre + r * angle.sin()];
    let track: Vec<_> = (0..72).map(|i| point(i as f32 / 72.0 * TAU)).collect();
    let mut shape =
        Shape::new().stroke_with(track, true, stroke, LineCap::Butt, Color::WHITE.alpha(0.10));
    let fraction = progress.fraction();
    // Without a known size, a quarter arc says "working" without claiming an amount.
    let sweep = fraction.unwrap_or(0.25);
    if sweep > 0.0 {
        let steps = ((72.0 * sweep).ceil() as usize).max(2);
        let arc: Vec<_> = (0..=steps)
            .map(|i| point(-FRAC_PI_2 + sweep * TAU * i as f32 / steps as f32))
            .collect();
        shape = shape.stroke(arc, stroke, t.accent);
    }
    let mut e = el()
        .key(key.to_owned())
        .role("progressbar")
        .label("Update download")
        .size(diameter, diameter)
        .center()
        .child(
            canvas(shape)
                .size(diameter, diameter)
                .absolute(0.0, 0.0)
                .a11y_hidden(),
        );
    if let Some(fraction) = fraction {
        e = e
            .range_value(f64::from(fraction), 0.0, 1.0)
            .a11y_value(format!("{}%", (fraction * 100.0).round()));
        if label {
            e = e.child(
                text(format!("{}", (fraction * 100.0).floor()))
                    .font(ty::NUMERIC)
                    .color(t.text)
                    .a11y_hidden(),
            );
        }
    }
    e
}

fn amount(progress: &Progress) -> String {
    match progress.total {
        Some(total) => format!(
            "{} of {}",
            format_bytes(progress.received as f64),
            format_bytes(total as f64)
        ),
        None => format_bytes(progress.received as f64),
    }
}

fn speed(progress: &Progress) -> String {
    match progress.bytes_per_second {
        Some(rate) => format!("{}/s", format_bytes(rate)),
        None => "Starting…".into(),
    }
}

fn shorten(detail: &str) -> String {
    const LIMIT: usize = 180;
    if detail.chars().count() <= LIMIT {
        detail.to_owned()
    } else {
        let cut: String = detail.chars().take(LIMIT).collect();
        format!("{}…", cut.trim_end())
    }
}

/// How many entries each section shows before summing up the rest.
const NOTES_SHOWN: usize = 5;

/// What a release brings, under New, Fixed and Changed, with at most `shown` entries in each.
fn whats_new(t: &Theme, notes: &Notes, shown: usize) -> El {
    let sections = [
        ("New", t.accent, &notes.new),
        ("Fixed", t.good, &notes.fixed),
        ("Changed", t.fair, &notes.changed),
    ];
    let mut list = col().key("update-notes").w_full().gap(10.0);
    for (name, tint, items) in sections {
        if items.is_empty() {
            continue;
        }
        let mut section = col()
            .key(name)
            .w_full()
            .gap(3.0)
            .child(text(name).font(ty::SMALL).color(tint));
        for item in items.iter().take(shown) {
            section = section.child(
                text(format!("• {item}"))
                    .font(ty::SMALL)
                    .color(t.text)
                    .wrap()
                    .w_full(),
            );
        }
        if items.len() > shown {
            section = section.child(faint(t, format!("and {} more", items.len() - shown)));
        }
        list = list.child(section);
    }
    list
}

/// The floating notice, while there's an update to act on and it hasn't been dismissed.
pub fn notice(s: &App, t: &Theme) -> Option<El> {
    let u = &s.updater;
    if s.onboarding.is_some() || !u.notice_visible() {
        return None;
    }
    let release = u.release.as_ref()?;
    let version = format!("Flexspeak {}", release.build.label());
    let has_notes = matches!(
        u.phase,
        Phase::Available | Phase::Downloading(_) | Phase::Ready
    ) && !release.notes.is_empty();
    let (glyph, tint, title, detail) = match &u.phase {
        Phase::Idle => return None,
        Phase::Available | Phase::Downloading(_) => {
            (&icons::DOWNLOAD, t.accent, "New update available", version)
        }
        Phase::Ready => (
            &icons::ROTATE_CW,
            t.good,
            "New update available",
            format!("{version} is downloaded and verified. Restart to install it."),
        ),
        Phase::Installing => (
            &icons::ROTATE_CW,
            t.accent,
            "Installing update…",
            "Flexspeak reopens in a moment.".into(),
        ),
        Phase::Failed { retry, detail } => (
            &icons::TRIANGLE_ALERT,
            t.poor,
            match retry {
                Retry::Download => "Update download failed",
                Retry::Install => "Update could not be installed",
            },
            shorten(detail),
        ),
    };
    let top = super::notices::top(
        t,
        glyph,
        tint,
        title,
        detail,
        (has_notes && !s.ui.update_hover).then(|| faint(t, "Hover to see what's new")),
        "dismiss-update",
        send(Event::Dismiss),
    );
    let later = || {
        button(
            t,
            "later",
            "Later",
            None,
            Variant::Ghost,
            send(Event::Dismiss),
        )
    };
    let actions = |primary: El| {
        row()
            .key("update-actions")
            .w_full()
            .gap(size::GAP)
            .justify_end()
            .child(later())
            .child(primary.default_button())
    };
    let footer = match &u.phase {
        Phase::Idle => return None,
        Phase::Available => actions(button(
            t,
            "download-update",
            "Download",
            Some(&icons::DOWNLOAD),
            Variant::Primary,
            send(Event::Download),
        )),
        Phase::Downloading(progress) => row()
            .key("update-progress")
            .w_full()
            .gap(12.0)
            .center_y()
            .child(ring(t, "update-ring", progress, 36.0, true))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(
                        text(amount(progress))
                            .font(ty::NUMERIC)
                            .color(t.text)
                            .truncate(),
                    )
                    .child(text(speed(progress)).font(ty::NUMERIC).color(t.faint)),
            ),
        Phase::Ready => actions(button(
            t,
            "restart-update",
            "Restart",
            Some(&icons::ROTATE_CW),
            Variant::Primary,
            send(Event::Restart),
        )),
        Phase::Installing => row()
            .w_full()
            .gap(10.0)
            .center_y()
            .justify_end()
            .child(pulse(t.accent))
            .child(text("Installing…").font(ty::SMALL).color(t.muted)),
        Phase::Failed { retry, .. } => actions(button(
            t,
            "retry-update",
            "Retry",
            Some(&icons::REFRESH),
            Variant::Primary,
            send(match retry {
                Retry::Download => Event::Download,
                Retry::Install => Event::Restart,
            }),
        )),
    };
    Some(
        super::notices::card(t, "update-notice", title)
            .on_hover_change(|s: &mut App, hovering, _| s.ui.update_hover = hovering)
            .child(top)
            .children(
                (has_notes && s.ui.update_hover).then(|| whats_new(t, &release.notes, NOTES_SHOWN)),
            )
            .child(footer),
    )
}

/// The Updates group on the Settings page.
pub fn settings(s: &App, t: &Theme, k: &Kit) -> El {
    let u = &s.updater;
    let version = match &u.running {
        Some(build) => format!("Flexspeak {}", build.label()),
        None => format!("Flexspeak {} · development", env!("CARGO_PKG_VERSION")),
    };
    let next = u
        .release
        .as_ref()
        .map(|release| format!("Flexspeak {}", release.build.label()));
    let next = next.as_deref().unwrap_or("A newer build");
    let status = if let Some(reason) = u.unsupported() {
        reason.to_owned()
    } else {
        match &u.phase {
            Phase::Available => format!("{next} is available."),
            Phase::Downloading(progress) => {
                format!(
                    "Downloading {next} · {} · {}",
                    amount(progress),
                    speed(progress)
                )
            }
            Phase::Ready => format!("{next} is ready. Restart to install it."),
            Phase::Installing => format!("Installing {next}…"),
            Phase::Failed { detail, .. } => shorten(detail),
            Phase::Idle if u.checking => "Checking for updates…".into(),
            Phase::Idle => match &u.check_error {
                Some(error) => format!("Couldn't check for updates. {}", shorten(error)),
                None if u.up_to_date => "Up to date. Flexspeak checks every 6 hours.".into(),
                None => "Flexspeak checks for updates every 6 hours.".into(),
            },
        }
    };
    let mut controls = row().gap(size::GAP).center_y().flex_wrap();
    let action = match &u.phase {
        Phase::Available => Some(button(
            t,
            "settings-download",
            "Download",
            Some(&icons::DOWNLOAD),
            Variant::Primary,
            send(Event::Download),
        )),
        Phase::Downloading(progress) => Some(ring(t, "settings-ring", progress, 24.0, false)),
        Phase::Ready => Some(button(
            t,
            "settings-restart",
            "Restart",
            Some(&icons::ROTATE_CW),
            Variant::Primary,
            send(Event::Restart),
        )),
        Phase::Failed { retry, .. } => Some(button(
            t,
            "settings-retry",
            "Retry",
            Some(&icons::REFRESH),
            Variant::Secondary,
            send(match retry {
                Retry::Download => Event::Download,
                Retry::Install => Event::Restart,
            }),
        )),
        Phase::Idle | Phase::Installing => None,
    };
    if let Some(action) = action {
        controls = controls.child(action);
    }
    if u.checking {
        controls = controls.child(pulse(t.accent));
    }
    let can_check = u.unsupported().is_none() && !u.checking && u.phase != Phase::Installing;
    controls = controls.child(button(
        t,
        "check-updates",
        "Check Now",
        None,
        Variant::Secondary,
        can_check.then_some(Message::Updater(Event::Check)),
    ));
    let auto = u.auto_download;
    let switch = Switch::new(auto)
        .label("Download updates automatically")
        .hide_label()
        .on_toggle_tx(move |s: &mut App, tx| {
            dispatch(s, tx, Message::Updater(Event::AutoDownload(!auto)))
        })
        .build(k);
    group(
        t,
        [
            form_row(t, version, Some(status.as_str()), controls).key("update-status"),
            form_row(
                t,
                "Download updates automatically",
                Some("New builds download in the background. Installing waits until you choose Restart, and Flexspeak returns to your server and channel."),
                switch,
            )
            .key("auto-download"),
        ],
    )
}

/// One release in the changelog: its build, its date and everything it changed.
fn release(t: &Theme, entry: &Entry) -> El {
    col()
        .key(format!("build-{}", entry.build.number))
        .w_full()
        .padding(size::CARD_PAD)
        .gap(10.0)
        .child(
            row()
                .w_full()
                .gap(size::GAP)
                .center_y()
                .child(
                    text(entry.build.label())
                        .font(ty::BODY_STRONG)
                        .color(t.text)
                        .grow(1.0),
                )
                .child(faint(t, entry.date)),
        )
        .child(whats_new(t, &entry.notes, usize::MAX))
}

/// The row that shows or hides every release before the latest.
fn earlier_toggle(t: &Theme, count: usize, open: bool) -> El {
    row()
        .key("changelog-earlier")
        .role("button")
        .label("Earlier versions")
        .expanded(open)
        .w_full()
        .h(size::CONTROL + 12.0)
        .px(size::CARD_PAD)
        .gap(10.0)
        .center_y()
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.05))
        .press(t.wash(0.09))
        .focus_ring(2.0, 0.0, t.accent.alpha(0.7))
        .motion(motion::QUICK)
        .on_press_tx(|s: &mut App, _| s.ui.changelog_open = !s.ui.changelog_open)
        .child(
            text("Earlier versions")
                .font(ty::BODY)
                .color(t.text)
                .grow(1.0)
                .a11y_hidden(),
        )
        .child(faint(t, count.to_string()).a11y_hidden())
        .child(
            icon(&icons::CHEVRON_RIGHT, 14.0)
                .color(t.faint)
                .rotate(if open { FRAC_PI_2 } else { 0.0 })
                .motion(motion::QUICK)
                .a11y_hidden(),
        )
}

/// The latest release, then a row that opens the rest back to the first build.
pub fn changelog(s: &App, t: &Theme) -> Vec<El> {
    let Some((latest, earlier)) = changelog::entries().split_first() else {
        return Vec::new();
    };
    let mut rows = vec![release(t, latest)];
    if !earlier.is_empty() {
        rows.push(earlier_toggle(t, earlier.len(), s.ui.changelog_open));
    }
    if s.ui.changelog_open {
        rows.extend(earlier.iter().map(|entry| release(t, entry)));
    }
    rows
}

/// A footnote for the group.
pub fn note(t: &Theme) -> El {
    faint(
        t,
        "Builds come from GitHub Releases and are checked against the release's SHA-256 sums before they're installed. This choice saves immediately.",
    )
    .px(2.0)
}
