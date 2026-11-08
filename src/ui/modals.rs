//! The channel-password prompt and the error dialog. Both are Rune modals: the window behind
//! them is inert, Tab stays inside, Escape closes them and Return presses the default button.

use rune::{El, col, el, row, text};
use rune_kit::{Dialog, Kit, SegmentedControl};

use super::UserVolumeTarget;
use super::icons;
use super::theme::{Theme, size, ty};
use super::widgets::{Range, Variant, button, error_text, faint, field, muted, slider};
use crate::client::Moderation;
use crate::{App, Message, ModerationDraft, Poke};

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

/// The rail's +: every saved server you aren't already on, each with its own Connect.
fn server_picker(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let mut list = col().w_full().gap(4.0);
    for (index, server) in s.servers.iter().enumerate() {
        if s.session_at(&server.address).is_some() {
            continue;
        }
        let names = col()
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(
                text(server.title().to_owned())
                    .font(ty::BODY_STRONG)
                    .color(t.text)
                    .truncate_fade(),
            )
            .children((!server.name.trim().is_empty()).then(|| {
                text(server.address.clone())
                    .font(ty::SMALL)
                    .color(t.muted)
                    .truncate_fade()
            }));
        list = list.child(
            row()
                .key(format!("picker-{index}"))
                .w_full()
                .gap(12.0)
                .center_y()
                .child(names)
                .child(button(
                    t,
                    &format!("picker-connect-{index}"),
                    "Connect",
                    Some(&icons::CONNECT),
                    Variant::Secondary,
                    s.permissions
                        .pending
                        .is_none()
                        .then_some(Message::Connect(index)),
                )),
        );
    }
    let empty = s
        .servers
        .iter()
        .all(|server| s.session_at(&server.address).is_some());
    let mut body = col().w_full().px(16.0).pb(16.0).gap(14.0);
    if empty {
        body = body.child(muted(t, "You're connected to all your saved servers."));
    } else {
        body = body.child(list);
    }
    body = body.child(
        row()
            .w_full()
            .gap(size::GAP)
            .center_y()
            .child(button(
                t,
                "picker-add",
                "Add Server",
                Some(&icons::PLUS),
                Variant::Ghost,
                Some(Message::AddServer),
            ))
            .child(el().grow(1.0))
            .child(button(
                t,
                "picker-close",
                "Close",
                None,
                Variant::Secondary,
                Some(Message::CloseServerPicker),
            )),
    );
    Dialog::new("server-picker", "Connect to another server")
        .open(true)
        .fit()
        .width(440.0)
        .on_close(|s: &mut App| {
            let _ = s.update(Message::CloseServerPicker);
        })
        .child(body)
        .build(k, layer);
}

/// Writing a poke: an optional short message, sent with Return.
const USER_VOLUME: Range = Range {
    min: 0,
    max: crate::settings::UserAudio::MAX_PERCENT as i32,
    step: 1,
    key_step: 5,
};

/// One person's volume, for you alone. Their voice is amplified above 100%.
fn user_volume(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, target: &UserVolumeTarget) {
    let percent = s.user_audio(&target.uid).volume_percent;
    let body = col()
        .w_full()
        .px(16.0)
        .pb(16.0)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(el().grow(1.0).min_w(0.0).child(slider(
                    t,
                    "user-volume",
                    "Volume",
                    i32::from(percent),
                    format!("{percent}%"),
                    USER_VOLUME,
                    |s: &App| {
                        s.ui.user_volume.as_ref().map_or(100, |target| {
                            i32::from(s.user_audio(&target.uid).volume_percent)
                        })
                    },
                    |value| Message::UserVolume(value.clamp(0, USER_VOLUME.max) as u16),
                )))
                .child(text(format!("{percent}%")).font(ty::NUMERIC).color(t.text)),
        )
        .child(
            row()
                .w_full()
                .gap(size::GAP)
                .justify_end()
                .child(button(
                    t,
                    "reset-user-volume",
                    "Reset",
                    None,
                    Variant::Secondary,
                    (percent != 100).then_some(Message::UserVolume(100)),
                ))
                .child(
                    button(
                        t,
                        "close-user-volume",
                        "Done",
                        None,
                        Variant::Primary,
                        Some(Message::CloseUserVolume),
                    )
                    .default_button(),
                ),
        );
    Dialog::new("user-volume-dialog", format!("Volume for {}", target.name))
        .description("Only changes what you hear.")
        .open(true)
        .fit()
        .width(380.0)
        .on_close(|s: &mut App| {
            let _ = s.update(Message::CloseUserVolume);
            s.sync_fields();
        })
        .child(body)
        .build(k, layer);
}

fn poke(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, name: &str) {
    let length = s
        .server()
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

/// How long a ban lasts unless you pick something else.
pub const DEFAULT_BAN_SECONDS: u64 = 86_400;

const BAN_LENGTHS: [(&str, Option<u64>); 5] = [
    ("1 Hour", Some(3_600)),
    ("1 Day", Some(DEFAULT_BAN_SECONDS)),
    ("1 Week", Some(604_800)),
    ("30 Days", Some(2_592_000)),
    ("Forever", None),
];

/// Confirms a kick or ban and takes its reason. Return never sends it: this needs a real click.
fn moderation(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, draft: &ModerationDraft) {
    let who = s.people(&draft.ids);
    let (title, description, confirm) = match draft.action {
        Moderation::KickChannel => (
            format!("Kick {who} from the channel"),
            "They go to the server's default channel.",
            "Kick",
        ),
        Moderation::KickServer => (
            format!("Kick {who} from the server"),
            "They're disconnected, and can reconnect right away.",
            "Kick",
        ),
        Moderation::Ban(_) => (
            format!("Ban {who}"),
            "They're disconnected, and can't return until the ban ends.",
            "Ban",
        ),
    };
    let mut body = col().w_full().px(16.0).pb(16.0).gap(12.0).child(field(
        k,
        &s.ui.moderation_reason,
        |s| &mut s.ui.moderation_reason,
        "Reason (optional)",
        Some(Message::ModerationReason),
        None,
    ));
    if let Moderation::Ban(seconds) = draft.action {
        let labels = BAN_LENGTHS.map(|(label, _)| label);
        let selected = BAN_LENGTHS
            .iter()
            .position(|(_, length)| *length == seconds);
        body = body.child(
            SegmentedControl::new(&labels, selected)
                .label("Ban length")
                .fit()
                .on_pick(|s: &mut App, index, tx| {
                    super::dispatch(s, tx, Message::ModerationBan(BAN_LENGTHS[index].1));
                })
                .build(k),
        );
    }
    body = body.child(
        row()
            .w_full()
            .gap(size::GAP)
            .center_y()
            .child(faint(t, format!("{} / 80", draft.reason.chars().count())).grow(1.0))
            .child(button(
                t,
                "cancel-moderation",
                "Cancel",
                None,
                Variant::Secondary,
                Some(Message::CancelModeration),
            ))
            .child(button(
                t,
                "confirm-moderation",
                confirm,
                None,
                Variant::Danger,
                Some(Message::SubmitModeration),
            )),
    );
    Dialog::new("moderation", title)
        .description(description)
        .open(true)
        .fit()
        .width(420.0)
        .on_close(|s: &mut App| {
            let _ = s.update(Message::CancelModeration);
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
    if s.server().pokes.len() > 1 {
        body = body.child(faint(
            t,
            format!("{} more pokes waiting", s.server().pokes.len() - 1),
        ));
    }
    body = body.child(
        row().w_full().child(el().grow(1.0)).child(
            button(
                t,
                "dismiss-poke",
                if s.server().pokes.len() > 1 {
                    "Next"
                } else {
                    "Close"
                },
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
    if let Some(target) = &s.ui.user_volume
        && s.alerts.current().is_none()
    {
        user_volume(s, t, k, layer, target);
        return;
    }
    if s.install_prompt
        && let Some(offer) = &s.install_offer
        && s.alerts.current().is_none()
    {
        install(s, t, k, layer, offer);
    } else if s.ui.server_picker && s.alerts.current().is_none() {
        server_picker(s, t, k, layer);
    } else if let Some(draft) = &s.server().moderation {
        moderation(s, t, k, layer, draft);
    } else if let Some(draft) = &s.server().poke {
        poke(s, t, k, layer, &draft.name);
    } else if let Some(poke) = s.server().pokes.first() {
        poked(s, t, k, layer, poke);
    } else if let Some(channel) = s.server().password_channel.and_then(|id| {
        s.server()
            .state
            .channels
            .iter()
            .find(|channel| channel.id == id)
    }) {
        let mut body = col().w_full().px(16.0).pb(16.0).gap(14.0).child(field(
            k,
            &s.ui.channel_password,
            |s| &mut s.ui.channel_password,
            "Channel password",
            Some(Message::ChannelPassword),
            Some(Message::SubmitChannelPassword),
        ));
        if let Some(error) = &s.server().password_error {
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
        let title = match &incident.server {
            Some(server) => format!("{server}: {}", incident.source.title()),
            None => incident.source.title().to_owned(),
        };
        Dialog::new("alert", title)
            .open(true)
            .fit()
            .width(520.0)
            .on_close(|s: &mut App| s.alerts.dismiss())
            .child(body)
            .build(k, layer);
    }
}
