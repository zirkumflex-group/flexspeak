//! Settings, and the sections onboarding reuses: audio, permissions and shortcuts.

use rune::{Cx, El, TextEdit, col, el, row, text};
use rune_kit::Kit;

use super::theme::{Theme, size, ty};
use super::widgets::{
    Variant, button, field, form_row, group, group_note, group_title, labelled, meter,
    percent_slider, picker, scroll_page,
};
use super::{home, icons};
use crate::{App, Message, permissions, shortcuts};

/// A trailing control at the form's control width.
fn trailing(e: El) -> El {
    el().w(size::FORM_CONTROL).child(e)
}

/// Buttons that act on a whole group, aligned to its trailing edge.
fn actions(buttons: impl IntoIterator<Item = El>) -> El {
    row()
        .w_full()
        .px(size::CARD_PAD)
        .py(10.0)
        .gap(size::GAP)
        .justify_end()
        .flex_wrap()
        .children(buttons)
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let idle = s.session.is_none();
    let editable = |message: fn(String) -> Message| idle.then_some(message);
    let server = group(
        t,
        [
            form_row(
                t,
                "Server address",
                None,
                trailing(field(
                    k,
                    &s.ui.address,
                    |s| &mut s.ui.address,
                    "hostname:9987",
                    editable(Message::Address),
                    None,
                )),
            ),
            form_row(
                t,
                "Nickname",
                None,
                trailing(field(
                    k,
                    &s.ui.nickname,
                    |s| &mut s.ui.nickname,
                    "Nickname",
                    editable(Message::Nickname),
                    None,
                )),
            ),
            form_row(
                t,
                "Server password",
                Some("Optional. Kept for this session only."),
                trailing(field(
                    k,
                    &s.ui.password,
                    |s| &mut s.ui.password,
                    "None",
                    editable(Message::Password),
                    None,
                )),
            ),
        ],
    )
    .key("connection");
    let mut footer = row().w_full().gap(size::GAP).center_y().flex_wrap().pt(4.0);
    footer = footer.child(match &s.settings_path {
        Some(path) => text(path.display().to_string())
            .font(ty::CAPTION.mono())
            .color(t.faint)
            .selectable(true)
            .truncate_fade()
            .grow(1.0)
            .min_w(0.0),
        None => el().grow(1.0),
    });
    footer = footer
        .child(button(
            t,
            "copy-path",
            "Copy Path",
            None,
            Variant::Secondary,
            s.settings_path.as_ref().map(|_| Message::CopySettingsPath),
        ))
        .child(button(
            t,
            "reload",
            "Reload",
            None,
            Variant::Secondary,
            idle.then_some(Message::ReloadSettings),
        ))
        .child(button(
            t,
            "save",
            "Save",
            None,
            Variant::Primary,
            Some(Message::SaveSettings),
        ));
    let mut content = col()
        .w_full()
        .max_w(680.0)
        .self_center()
        .padding(size::PAGE)
        .gap(8.0)
        .child(group_title(t, "Server"))
        .child(server)
        .child(group_title(t, "Audio"))
        .child(audio(s, cx, t, k, layer).key("audio"))
        .child(group_note(
            t,
            "Disconnect to change devices; volume applies immediately. Use headphones for the microphone test.",
        ))
        .child(permissions(s, t).key("permissions"))
        .child(shortcut_settings(s, t, k).key("shortcuts"))
        .child(footer)
        .child(group_note(
            t,
            "Save writes your portable settings file. After editing it elsewhere, disconnect and Reload.",
        ));
    if let Some(message) = &s.settings_message {
        content = content.child(
            labelled(&icons::CHECK, message.clone(), t.good)
                .key("settings-message")
                .transition(rune::Transition::opacity()),
        );
    }
    scroll_page(k, "settings-scroll", content)
}

pub fn audio(s: &App, _cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let busy = s.session.is_some();
    let volume = s.playback_volume.percent();
    let inputs: Vec<String> = s.devices.inputs.iter().map(|d| d.name.clone()).collect();
    let outputs: Vec<String> = s.devices.outputs.iter().map(|d| d.name.clone()).collect();
    let testing = busy && s.loopback;
    let input_status: String = if busy {
        s.microphone_status().to_string()
    } else {
        "Start a test to see input".into()
    };
    group(
        t,
        [
            form_row(
                t,
                "Microphone",
                None,
                picker(
                    k,
                    s,
                    "input-device",
                    inputs,
                    s.input.as_ref().map(|d| d.name.as_str()),
                    if s.devices.inputs.is_empty() {
                        "No microphone detected"
                    } else {
                        "Choose…"
                    },
                    !busy,
                    size::FORM_CONTROL,
                    {
                        let devices = s.devices.inputs.clone();
                        move |index| Message::Input(devices[index].clone())
                    },
                    layer,
                ),
            ),
            form_row(
                t,
                "Input level",
                Some(input_status.as_str()),
                trailing(meter(t, "input-meter", s.microphone_meter_level(), t.good)),
            ),
            form_row(
                t,
                "Output",
                None,
                picker(
                    k,
                    s,
                    "output-device",
                    outputs,
                    s.output.as_ref().map(|d| d.name.as_str()),
                    if s.devices.outputs.is_empty() {
                        "No playback device detected"
                    } else {
                        "Choose…"
                    },
                    !busy,
                    size::FORM_CONTROL,
                    {
                        let devices = s.devices.outputs.clone();
                        move |index| Message::Output(devices[index].clone())
                    },
                    layer,
                ),
            ),
            form_row(
                t,
                format!("Output volume · {volume}%"),
                None,
                trailing(percent_slider(
                    t,
                    "volume",
                    "Playback volume",
                    volume,
                    Message::PlaybackVolume,
                )),
            ),
            actions([
                button(
                    t,
                    "refresh-devices",
                    "Refresh Devices",
                    None,
                    Variant::Secondary,
                    (!busy).then_some(Message::Refresh),
                ),
                button(
                    t,
                    "mic-test",
                    if testing {
                        "Stop Test"
                    } else {
                        "Test Microphone"
                    },
                    None,
                    if testing {
                        Variant::Danger
                    } else {
                        Variant::Secondary
                    },
                    if s.stopping {
                        None
                    } else if testing {
                        Some(Message::Stop)
                    } else if !busy {
                        Some(Message::TestAudio)
                    } else {
                        None
                    },
                ),
            ]),
        ],
    )
}

pub fn permissions(s: &App, t: &Theme) -> El {
    let enabled = s.permissions.pending.is_none() && s.session.is_none();
    let mut rows = Vec::new();
    for kind in permissions::Kind::ALL {
        let request_label = if cfg!(windows) && kind == permissions::Kind::Microphone {
            "Open Privacy Settings"
        } else if cfg!(target_os = "macos") {
            "Request Access"
        } else {
            "Review Access"
        };
        let pending = s.permissions.pending == Some(kind);
        let detail = match &s.permissions.messages[kind.index()] {
            Some(message) => format!("{} {message}", kind.detail()),
            None => kind.detail().to_owned(),
        };
        let mut controls = row().gap(size::GAP).center_y();
        if pending {
            controls = controls.child(super::widgets::pulse(t.accent));
        }
        controls = controls.child(button(
            t,
            "request",
            request_label,
            None,
            Variant::Secondary,
            enabled.then_some(Message::RequestPermission(kind)),
        ));
        if cfg!(any(target_os = "macos", windows)) {
            controls = controls.child(super::widgets::icon_button(
                t,
                "os-settings",
                &icons::SETTINGS,
                "Open system settings",
                Variant::Ghost,
                enabled.then_some(Message::OpenPermissionSettings(kind)),
            ));
        }
        rows.push(
            form_row(t, kind.title(), Some(detail.as_str()), controls)
                .key(format!("permission-{}", kind.index())),
        );
    }
    let mut note = String::from("Your operating system controls access; you can change it later.");
    if cfg!(target_os = "macos") {
        note.push_str(" This build appears as Flexspeak in Privacy & Security.");
    }
    if s.session.is_some() {
        note.push_str(" Disconnect or stop the local test before requesting access.");
    }
    col()
        .w_full()
        .gap(8.0)
        .child(group_title(t, "Permissions"))
        .child(group(t, rows))
        .child(group_note(t, note))
}

fn key_field(
    k: &Kit,
    edit: &TextEdit,
    access: fn(&mut App) -> &mut TextEdit,
    placeholder: &str,
    change: fn(String) -> Message,
) -> El {
    trailing(field(k, edit, access, placeholder, Some(change), None))
}

pub fn shortcut_settings(s: &App, t: &Theme, k: &Kit) -> El {
    let detected = match s.last_shortcut {
        Some(shortcuts::Action::Mute) => "Detected mute / unmute",
        Some(shortcuts::Action::Deafen) => "Detected deafen / undeafen",
        None => "Press a shortcut after testing to check it",
    };
    col()
        .w_full()
        .gap(8.0)
        .child(group_title(t, "Keyboard Shortcuts"))
        .child(group(
            t,
            [
                form_row(
                    t,
                    "Toggle microphone",
                    None,
                    key_field(
                        k,
                        &s.ui.mute_key,
                        |s| &mut s.ui.mute_key,
                        "Ctrl+Shift+M",
                        Message::MuteKeybind,
                    ),
                ),
                form_row(
                    t,
                    "Toggle deafen",
                    None,
                    key_field(
                        k,
                        &s.ui.deafen_key,
                        |s| &mut s.ui.deafen_key,
                        "Ctrl+Shift+D",
                        Message::DeafenKeybind,
                    ),
                ),
                form_row(
                    t,
                    "Global shortcuts",
                    Some(detected),
                    button(
                        t,
                        "test-shortcuts",
                        "Test Shortcuts",
                        None,
                        Variant::Secondary,
                        Some(Message::TestHotkeys),
                    ),
                )
                .key("detected"),
            ],
        ))
        .child(home::shortcut_hint(s, t).px(2.0))
}
