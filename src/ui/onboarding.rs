//! First-run setup: five steps in one centred card, with fixed Back / Continue controls.

use rune::{Cx, El, Transition, col, el, icon, row, text};
use rune_kit::Kit;

use super::icons;
use super::settings;
use super::theme::{Theme, motion, ty};
use super::widgets::{
    Variant, button, card, error_text, faint, field, muted, overline, scroll_page, title,
};
use crate::{App, Message, onboarding::Step};

/// The offer to fill in setup from TeamSpeak 6, then confirmation that it did.
fn ts6_offer(s: &App, t: &Theme) -> Option<El> {
    s.ts6_path.as_ref()?;
    let (headline, detail) = if s.ts6.is_some() {
        (
            "TeamSpeak 6 settings carried over",
            "The next steps show what came across. Change anything you like.",
        )
    } else {
        (
            "TeamSpeak 6 found on this computer",
            "Bring over your name, servers, audio devices and talk mode. Passwords and identities stay behind.",
        )
    };
    let mut line = row().w_full().gap(12.0).center_y().child(
        col()
            .grow(1.0)
            .min_w(0.0)
            .gap(2.0)
            .child(text(headline).font(ty::BODY_STRONG).color(t.text))
            .child(faint(t, detail).wrap()),
    );
    if s.ts6.is_none() {
        line = line.child(button(
            t,
            "import-ts6",
            "Use TeamSpeak 6 Settings",
            Some(&icons::DOWNLOAD),
            Variant::Primary,
            Some(Message::ImportTs6),
        ));
    }
    Some(card(t).key("ts6-offer").child(line))
}

/// One value TeamSpeak 6 had: marked when in use, with Use to go back to it after a change.
fn carried(t: &Theme, key: &str, value: String, in_use: bool, apply: Option<Message>) -> El {
    let mut line = row()
        .key(format!("ts6-{key}"))
        .w_full()
        .gap(8.0)
        .center_y()
        .child(icon(&icons::DOWNLOAD, 13.0).color(t.faint))
        .child(
            text(value)
                .font(ty::SMALL)
                .color(t.text)
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0),
        );
    line = if in_use {
        line.child(faint(t, "In use"))
    } else {
        line.child(button(
            t,
            &format!("use-ts6-{key}"),
            "Use",
            None,
            Variant::Secondary,
            apply,
        ))
    };
    line
}

fn carried_group(t: &Theme, rows: Vec<El>) -> Option<El> {
    if rows.is_empty() {
        return None;
    }
    let mut group = col()
        .key("ts6-carried")
        .w_full()
        .gap(6.0)
        .child(faint(t, "From TeamSpeak 6"));
    for row in rows {
        group = group.child(row);
    }
    Some(group)
}

/// The name TeamSpeak 6 showed for you.
fn ts6_name(s: &App, t: &Theme) -> Option<El> {
    let name = s.ts6.as_ref()?.nickname.as_ref()?;
    carried_group(
        t,
        vec![carried(
            t,
            "name",
            name.clone(),
            s.nickname.trim() == name,
            Some(Message::Nickname(name.clone())),
        )],
    )
}

/// The servers TeamSpeak 6 had. All are saved unless removed; Use picks the one to connect to.
fn ts6_servers(s: &App, t: &Theme) -> Option<El> {
    let imported = s.ts6.as_ref()?;
    let current = s.address.trim();
    let rows = imported
        .servers
        .iter()
        .enumerate()
        .map(|(index, address)| {
            let in_use = address == current;
            let mut line = carried(
                t,
                &format!("server-{index}"),
                address.clone(),
                in_use,
                Some(Message::Address(address.clone())),
            );
            if !in_use {
                line = line.child(super::widgets::icon_button(
                    t,
                    &format!("drop-ts6-server-{index}"),
                    &icons::X,
                    "Don't save this server",
                    Variant::Ghost,
                    Some(Message::DropTs6Server(address.clone())),
                ));
            }
            line
        })
        .collect();
    carried_group(t, rows)
}

/// The microphone, speakers and talk mode TeamSpeak 6 used, where this computer has them.
fn ts6_audio(s: &App, t: &Theme) -> Option<El> {
    let imported = s.ts6.as_ref()?;
    let free = s.idle();
    let mut rows = Vec::new();
    type Choose = fn(crate::audio::DeviceChoice) -> Message;
    let sides: [(&str, &str, _, _, _, Choose); 2] = [
        (
            "input",
            "Microphone",
            &imported.input,
            &s.devices.inputs,
            &s.input,
            Message::Input,
        ),
        (
            "output",
            "Speakers",
            &imported.output,
            &s.devices.outputs,
            &s.output,
            Message::Output,
        ),
    ];
    for (key, label, wanted, devices, current, choose) in sides {
        let Some(name) = wanted.as_deref() else {
            continue;
        };
        rows.push(match devices.iter().find(|device| device.name == name) {
            Some(device) => carried(
                t,
                key,
                format!("{label}: {name}"),
                current.as_ref() == Some(device),
                free.then(|| choose(device.clone())),
            ),
            None => carried(
                t,
                key,
                format!("{label}: {name} (not connected)"),
                false,
                None,
            ),
        });
    }
    if imported.push_to_talk {
        let settings = crate::transmission::Settings {
            mode: crate::transmission::Mode::PushToTalk,
            ptt_release_ms: imported.ptt_release_ms,
            ..s.transmission
        };
        rows.push(carried(
            t,
            "talk-mode",
            format!("Push to talk, {} ms release", imported.ptt_release_ms),
            s.transmission.mode == settings.mode
                && s.transmission.ptt_release_ms == settings.ptt_release_ms,
            free.then_some(Message::Transmission(settings)),
        ));
    }
    carried_group(t, rows)
}

fn stepper(t: &Theme, step: Step) -> El {
    let mut steps = row().w_full().gap(6.0);
    for item in Step::ALL {
        let done = item.index() < step.index();
        let current = item == step;
        let color = if current || done { t.accent } else { t.faint };
        steps = steps.child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(6.0)
                .label(format!("Step {} of 5: {}", item.index() + 1, item.title()))
                .child(
                    el().w_full()
                        .h(3.0)
                        .radius(1.5)
                        .fill(if current || done {
                            t.accent
                        } else {
                            t.wash(0.08)
                        })
                        .motion(motion::SETTLE)
                        .a11y_hidden(),
                )
                .child(
                    text(format!("{}. {}", item.index() + 1, item.title()))
                        .font(ty::CAPTION)
                        .color(color)
                        .truncate_fade()
                        .a11y_hidden(),
                ),
        );
    }
    steps
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>, step: Step) -> El {
    let busy = !s.idle() || s.permissions.pending.is_some();
    let fields = match step {
        Step::Username => col()
            .w_full()
            .gap(14.0)
            .children(ts6_offer(s, t))
            .child(super::widgets::body(t, "What should other people call you?"))
            .child(field(
                k,
                &s.ui.nickname,
                |s| &mut s.ui.nickname,
                "Username",
                Some(Message::Nickname),
                Some(Message::SetupNext),
            ))
            .children(ts6_name(s, t)),
        Step::Server => col()
            .w_full()
            .gap(14.0)
            .child(super::widgets::body(
                t,
                "Save the server you want to connect to. You can edit it later in Settings.",
            ))
            .child(field(
                k,
                &s.ui.address,
                |s| &mut s.ui.address,
                "voice.example.org:9987",
                Some(Message::Address),
                Some(Message::SetupNext),
            ))
            .child(faint(
                t,
                "Server password (optional · kept only for this session)",
            ))
            .child(field(
                k,
                &s.ui.password,
                |s| &mut s.ui.password,
                "Server password",
                Some(Message::Password),
                Some(Message::SetupNext),
            ))
            .children(ts6_servers(s, t)),
        Step::Permissions => settings::permissions(s, t),
        Step::Devices => col()
            .w_full()
            .gap(16.0)
            .child(super::widgets::body(
                t,
                "Choose your microphone and playback device, then test them locally. No server connection is made.",
            ))
            .children(ts6_audio(s, t))
            .child(settings::audio(s, cx, t, k, layer))
            .child(muted(
                t,
                if s.loopback {
                    s.server().status.as_str()
                } else {
                    "Stop the local test before continuing. You can also test devices later in Settings."
                },
            )),
        Step::Hotkeys => settings::shortcut_settings(s, t, k),
    };
    let mut content = col()
        .w_full()
        .h_full()
        .gap(16.0)
        .child(overline(t, "Welcome to Flexspeak"))
        .child(super::widgets::display(t, "Let’s get you talking"))
        .child(stepper(t, step))
        .child(
            title(t, step.title())
                .key(format!("step-title-{}", step.index()))
                .transition(Transition::opacity().combined(Transition::offset(12.0, 0.0))),
        )
        .child(scroll_page(
            k,
            "setup-scroll",
            col()
                .key(format!("step-{}", step.index()))
                .w_full()
                .pr(12.0)
                .transition(Transition::opacity().combined(Transition::offset(16.0, 0.0)))
                .child(fields),
        ));
    if let Some(error) = &s.onboarding_error {
        content = content.child(
            error_text(t, error.clone())
                .key("setup-error")
                .transition(Transition::opacity()),
        );
    }
    let last = step == Step::Hotkeys;
    content = content.child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(button(
                t,
                "back",
                "Back",
                None,
                Variant::Secondary,
                (!busy && step.previous().is_some()).then_some(Message::SetupBack),
            ))
            .child(el().grow(1.0))
            .children(s.redoing_setup.then(|| {
                button(
                    t,
                    "cancel-setup",
                    "Cancel",
                    None,
                    Variant::Ghost,
                    (!busy).then_some(Message::CancelSetup),
                )
                .help("Keep your saved settings as they were")
            }))
            .child(
                button(
                    t,
                    "continue",
                    if last { "Finish Setup" } else { "Continue" },
                    None,
                    Variant::Primary,
                    (!busy).then_some(Message::SetupNext),
                )
                .default_button(),
            ),
    );
    // A setup assistant: the steps sit on the window itself, not on a floating card.
    col()
        .w_full()
        .h_full()
        .padding(24.0)
        .pt(44.0)
        .center()
        .child(
            col()
                .key("setup")
                .w_full()
                .max_w(640.0)
                .h_full()
                .max_h(680.0)
                .child(content),
        )
}
