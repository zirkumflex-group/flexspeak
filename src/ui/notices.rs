//! Notices: cards stacked in the window's top corner, one per problem, all the same shape as
//! the update card: a tinted badge, a title, a sentence or two, a close button, and at most one
//! thing to do about it. They never cover the window or take the keyboard. A problem that is a
//! state (a missing device, a shortcut another app holds) closes itself once it recovers, a
//! one-off failure fades after a few seconds, and any card can be closed.

use rune::{Color, El, Icon, col, el, icon, row, text};

use super::icons;
use super::theme::{Theme, radius, shadow, size, ty};
use super::widgets::{Variant, button, icon_button};
use crate::alerts::{Incident, Severity, Source};
use crate::{App, Message, SettingsPage};

pub const WIDTH: f32 = 340.0;
/// Cards past this many wait behind a count, so the stack can't fill the window.
const SHOWN: usize = 3;
/// Longer details are cut here; the rest is in the log or the settings it points to.
const LIMIT: usize = 200;

/// Every notice, newest problems last, pinned under the title bar. While Settings is open it
/// sits in the bottom corner instead, clear of Settings' own header.
pub fn stack(s: &App, t: &Theme) -> Option<El> {
    let update = super::updates::notice(s, t);
    let message = super::chat::toast(s, t);
    let visible = s.alerts.visible();
    if update.is_none() && message.is_none() && visible.is_empty() {
        return None;
    }
    let hidden = visible.len().saturating_sub(SHOWN);
    let mut stack = col()
        .key("notices")
        .w(WIDTH)
        .gap(8.0)
        .children(update)
        .children(message)
        .children(
            visible
                .iter()
                .skip(hidden)
                .map(|incident| alert(s, t, incident)),
        );
    if hidden > 0 {
        stack = stack.child(
            text(format!(
                "{hidden} more {} behind these",
                if hidden == 1 { "problem" } else { "problems" }
            ))
            .font(ty::SMALL)
            .color(t.faint)
            .self_end()
            .mr(4.0),
        );
    }
    Some(if s.settings_page.is_some() {
        stack.pin(None, None, Some(16.0), Some(16.0))
    } else {
        stack.pin(None, Some(size::HEADER + 10.0), Some(16.0), None)
    })
}

/// The card every notice is drawn on.
pub fn card(t: &Theme, key: impl Into<String>, label: impl Into<String>) -> El {
    col()
        .key(key.into())
        .role("status")
        .label(label.into())
        .live_region(rune::LiveRegion::Polite)
        .w_full()
        .padding(size::CARD_PAD)
        .gap(12.0)
        .radius(radius::CARD)
        .fill(t.raised)
        .border_gradient(1.0, t.bevel_top, t.bevel_bottom)
        .box_shadow(shadow(0.0, 12.0, 32.0, Color::BLACK.alpha(0.45)))
        .transition(rune::Transition::opacity())
}

/// A card's top: badge, title and detail, and its close button.
#[allow(clippy::too_many_arguments)]
pub fn top(
    t: &Theme,
    glyph: &'static Icon,
    tint: Color,
    title: impl Into<String>,
    detail: impl Into<String>,
    extra: Option<El>,
    close_key: &str,
    close: Option<Message>,
) -> El {
    let badge = el()
        .size(32.0, 32.0)
        .radius(16.0)
        .center()
        .fill(tint.alpha(0.14))
        .border(1.0, tint.alpha(0.28))
        .child(icon(glyph, 16.0).color(tint).a11y_hidden())
        .a11y_hidden();
    row()
        .w_full()
        .gap(12.0)
        .items_start()
        .child(badge)
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .pt(1.0)
                .child(
                    text(title.into())
                        .font(ty::BODY_STRONG)
                        .color(t.text)
                        .wrap()
                        .w_full(),
                )
                .child(
                    text(shorten(&detail.into()))
                        .font(ty::SMALL)
                        .color(t.muted)
                        .wrap()
                        .w_full()
                        .selectable(true),
                )
                .children(extra),
        )
        .child(
            icon_button(t, close_key, &icons::CLOSE, "Close", Variant::Ghost, close)
                .mt(-4.0)
                .mr(-6.0),
        )
}

fn shorten(detail: &str) -> String {
    if detail.chars().count() <= LIMIT {
        detail.to_owned()
    } else {
        let cut: String = detail.chars().take(LIMIT).collect();
        format!("{}…", cut.trim_end())
    }
}

pub fn tint(t: &Theme, severity: Severity) -> Color {
    match severity {
        Severity::Notice => t.accent,
        Severity::Warning => t.fair,
        Severity::Error => t.poor,
    }
}

pub fn glyph(source: Source) -> &'static Icon {
    match source {
        Source::Connection | Source::Dropped => &icons::DISCONNECT,
        Source::Audio => &icons::HEADPHONE_OFF,
        Source::AudioFallback => &icons::HEADPHONES,
        Source::Shortcuts => &icons::KEYBOARD,
        Source::SettingsLoad | Source::SettingsSave => &icons::FILE_TEXT,
        Source::Flood => &icons::GAUGE,
        Source::StreamDiscovery
        | Source::StreamWatch
        | Source::StreamEnded
        | Source::StreamFrame
        | Source::StreamPublish => &icons::SCREEN_SHARE,
        Source::Install => &icons::DOWNLOAD,
        _ => &icons::TRIANGLE_ALERT,
    }
}

fn alert(s: &App, t: &Theme, incident: &Incident) -> El {
    let mut title = incident.heading();
    if incident.reports > 1 {
        title.push_str(&format!(" (×{})", incident.reports));
    }
    let mut card = card(t, format!("notice-{}", incident.id), incident.heading()).child(top(
        t,
        glyph(incident.source),
        tint(t, incident.source.severity()),
        title,
        incident.detail.as_str(),
        None,
        &format!("notice-close-{}", incident.id),
        Some(Message::DismissAlert(incident.id)),
    ));
    if let Some((label, glyph, message)) = action(s, incident) {
        card = card.child(row().w_full().justify_end().child(button(
            t,
            &format!("notice-action-{}", incident.id),
            label,
            Some(glyph),
            Variant::Primary,
            Some(message),
        )));
    }
    card
}

/// The one thing a card can offer to do about its problem, if any.
fn action(s: &App, incident: &Incident) -> Option<(&'static str, &'static Icon, Message)> {
    let settings = |label, page| {
        (s.settings_page != Some(page)).then_some((
            label,
            &icons::SETTINGS,
            Message::OpenSettings(page),
        ))
    };
    match incident.source {
        Source::Connection | Source::Dropped => {
            let address = incident.address.as_deref()?;
            if s.session_at(address).is_some() || s.loopback || s.onboarding.is_some() {
                return None;
            }
            let index = s
                .servers
                .iter()
                .position(|server| server.address.trim() == address)?;
            Some((
                if incident.source == Source::Dropped {
                    "Reconnect"
                } else {
                    "Try Again"
                },
                &icons::CONNECT,
                Message::Connect(index),
            ))
        }
        // Setup has no Settings window to open.
        _ if s.onboarding.is_some() => None,
        Source::Audio | Source::AudioFallback => settings("Audio Settings", SettingsPage::Audio),
        Source::Shortcuts => settings("Change Shortcut", SettingsPage::Shortcuts),
        Source::SettingsLoad | Source::SettingsSave => {
            settings("Open Settings File", SettingsPage::File)
        }
        _ => None,
    }
}
