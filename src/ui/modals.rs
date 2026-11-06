//! The channel-password prompt and the error dialog. Both are Rune modals: the window behind
//! them is inert, Tab stays inside, Escape closes them and Return presses the default button.

use rune::{El, col, el, row, text};
use rune_kit::{Dialog, Kit};

use super::icons;
use super::theme::{Theme, size, ty};
use super::widgets::{Variant, button, error_text, faint, field, muted};
use crate::{App, Message};

pub fn build(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    if let Some(channel) = s
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
