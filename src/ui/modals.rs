//! The channel-password prompt and the error dialog. Both are Rune modals: the window behind
//! them is inert, Tab stays inside, Escape closes them and Return presses the default button.

use rune::{El, col, el, row, text};
use rune_kit::{Dialog, Kit};

use super::icons;
use super::theme::{Theme, size, ty};
use super::widgets::{Variant, button, error_text, faint, field, muted};
use crate::{App, Message, Poke};

/// Offers to copy a release that runs from Downloads or a disk image to where apps live.
fn install(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, offer: &crate::install::Offer) {
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(12.0)
        .child(
            text(
                "Flexspeak is running from a temporary place, so updates would be lost when it is \
                 moved or deleted. Install it to keep it and its updates. Your settings stay as they are.",
            )
            .font(ty::BODY)
            .color(t.text)
            .w_full()
            .wrap(),
        )
        .child(path_line(t, "From", &offer.from))
        .child(path_line(t, "To", &offer.to))
        .child(
            row()
                .w_full()
                .gap(size::GAP)
                .center_y()
                .child(button(
                    t,
                    "never-install",
                    "Don’t Ask Again",
                    None,
                    Variant::Ghost,
                    (!s.installing).then_some(Message::DeclineInstall(true)),
                ))
                .child(el().grow(1.0))
                .child(button(
                    t,
                    "not-now",
                    "Not Now",
                    None,
                    Variant::Secondary,
                    (!s.installing).then_some(Message::DeclineInstall(false)),
                ))
                .child(
                    button(
                        t,
                        "install",
                        if s.installing {
                            "Installing…"
                        } else {
                            "Install & Reopen"
                        },
                        Some(&icons::DOWNLOAD),
                        Variant::Primary,
                        (!s.installing).then_some(Message::Install),
                    )
                    .default_button(),
                ),
        );
    Dialog::new("install", "Install Flexspeak?")
        .open(true)
        .fit()
        .width(500.0)
        .on_close(|s: &mut App| {
            if !s.installing {
                s.install_prompt = false;
            }
        })
        .child(body)
        .build(k, layer);
}

fn path_line(t: &Theme, label: &str, path: &std::path::Path) -> El {
    row()
        .w_full()
        .gap(8.0)
        .child(muted(t, label.to_owned()).w(40.0))
        .child(
            text(path.display().to_string())
                .font(ty::NUMERIC)
                .color(t.muted)
                .grow(1.0)
                .min_w(0.0)
                .wrap()
                .selectable(true),
        )
}

/// Writing a poke: an optional short message, sent with Return.
fn poke(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, name: &str) {
    let length = s
        .poke
        .as_ref()
        .map_or(0, |draft| draft.message.chars().count());
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(12.0)
        .child(field(
            k,
            &s.ui.poke_message,
            |s| &mut s.ui.poke_message,
            "Message (optional)",
            Some(Message::PokeText),
            Some(Message::SubmitPoke),
        ))
        .child(
            row()
                .w_full()
                .gap(size::GAP)
                .center_y()
                .child(faint(t, format!("{length} / 100")).grow(1.0))
                .child(button(
                    t,
                    "cancel-poke",
                    "Cancel",
                    None,
                    Variant::Secondary,
                    Some(Message::CancelPoke),
                ))
                .child(
                    button(
                        t,
                        "send-poke",
                        "Poke",
                        Some(&icons::HAND),
                        Variant::Primary,
                        Some(Message::SubmitPoke),
                    )
                    .default_button(),
                ),
        );
    Dialog::new("poke", format!("Poke {name}"))
        .description("They get your message and a sound.")
        .open(true)
        .fit()
        .width(420.0)
        .on_close(|s: &mut App| {
            let _ = s.update(Message::CancelPoke);
            s.sync_fields();
        })
        .child(body)
        .build(k, layer);
}

/// A poke someone sent you, until you close it; later ones queue behind it.
fn poked(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, poke: &Poke) {
    let mut body = col().w_full().px(16.0).pb(16.0).gap(12.0);
    body = body.child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(
                el().size(36.0, 36.0)
                    .radius(18.0)
                    .center()
                    .fill(t.accent.alpha(0.16))
                    .child(rune::icon(&icons::HAND, 18.0).color(t.accent)),
            )
            .child(
                text(if poke.message.is_empty() {
                    "(no message)".to_owned()
                } else {
                    poke.message.clone()
                })
                .font(ty::BODY_STRONG)
                .color(if poke.message.is_empty() {
                    t.muted
                } else {
                    t.text
                })
                .grow(1.0)
                .min_w(0.0)
                .wrap()
                .selectable(true),
            ),
    );
    if s.pokes.len() > 1 {
        body = body.child(faint(
            t,
            format!("{} more pokes waiting", s.pokes.len() - 1),
        ));
    }
    body = body.child(
        row().w_full().child(el().grow(1.0)).child(
            button(
                t,
                "dismiss-poke",
                if s.pokes.len() > 1 { "Next" } else { "Close" },
                None,
                Variant::Primary,
                Some(Message::DismissPoke),
            )
            .default_button(),
        ),
    );
    Dialog::new("poked", format!("{} poked you", poke.from))
        .open(true)
        .fit()
        .width(420.0)
        .on_close(|s: &mut App| {
            let _ = s.update(Message::DismissPoke);
        })
        .child(body)
        .build(k, layer);
}

pub fn build(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    if s.install_prompt
        && let Some(offer) = &s.install_offer
        && s.alerts.current().is_none()
    {
        install(s, t, k, layer, offer);
    } else if let Some(draft) = &s.poke {
        poke(s, t, k, layer, &draft.name);
    } else if let Some(poke) = s.pokes.first() {
        poked(s, t, k, layer, poke);
    } else if let Some(channel) = s
        .password_channel
        .and_then(|id| s.state.channels.iter().find(|channel| channel.id == id))
    {
        let mut body = col().w_full().px(16.0).pb(16.0).gap(14.0).child(field(
            k,
            &s.ui.channel_password,
            |s| &mut s.ui.channel_password,
            "Channel password",
            Some(Message::ChannelPassword),
            Some(Message::SubmitChannelPassword),
        ));
        if let Some(error) = &s.password_error {
            body = body.child(error_text(t, error.clone()));
        }
        body = body.child(
            row()
                .w_full()
                .gap(size::GAP)
                .child(el().grow(1.0))
                .child(button(
                    t,
                    "cancel",
                    "Cancel",
                    None,
                    Variant::Secondary,
                    Some(Message::CancelChannelPassword),
                ))
                .child(
                    button(
                        t,
                        "join",
                        "Join",
                        Some(&icons::CHANNEL),
                        Variant::Primary,
                        Some(Message::SubmitChannelPassword),
                    )
                    .default_button(),
                ),
        );
        Dialog::new("channel-password", format!("Join {}", channel.name))
            .description("This channel is password protected.")
            .open(true)
            .fit()
            .width(400.0)
            .on_close(|s: &mut App| {
                let task = s.update(Message::CancelChannelPassword);
                drop(task);
                s.sync_fields();
            })
            .child(body)
            .build(k, layer);
    } else if let Some(incident) = s.alerts.current() {
        let mut body = col().w_full().px(16.0).pb(16.0).gap(12.0).child(
            // A column: a row viewport would stretch the text to its own height and never scroll.
            col()
                .w_full()
                .max_h(240.0)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    col().w_full().child(
                        text(incident.detail.clone())
                            .font(ty::BODY)
                            .color(t.text)
                            .w_full()
                            .wrap()
                            .selectable(true),
                    ),
                ),
        );
        if incident.reports > 1 {
            body = body.child(faint(
                t,
                format!("{} reports of this issue", incident.reports),
            ));
        }
        body = body.child(
            row()
                .w_full()
                .gap(size::GAP)
                .center_y()
                .child(muted(t, "Enter or Esc").grow(1.0))
                .child(
                    button(
                        t,
                        "dismiss",
                        "Close",
                        None,
                        Variant::Primary,
                        Some(Message::DismissError),
                    )
                    .default_button(),
                ),
        );
        Dialog::new("alert", incident.source.title())
            .open(true)
            .fit()
            .width(520.0)
            .on_close(|s: &mut App| s.alerts.dismiss())
            .child(body)
            .build(k, layer);
    }
}
