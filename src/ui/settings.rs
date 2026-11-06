//! Settings, and the sections onboarding reuses: audio, permissions and shortcuts.

use rune::{Color, Cx, El, TextEdit, col, el, icon, row, text};
use rune_kit::{Kit, Switch};

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Range, Variant, button, field, form_row, group, group_note, group_title, labelled, meter,
    percent_slider, picker, scroll_page, slider,
};
use super::{home, icons, send, updates};
use crate::settings::{Appearance, ColorScheme, ThemeName};
use crate::{App, Message, audio_processing, permissions, shortcuts, transmission};

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
        .child(appearance(s, t).key("appearance"))
        .child(group_title(t, "Server"))
        .child(server)
        .child(group_title(t, "Audio"))
        .child(audio(s, cx, t, k, layer).key("audio"))
        .child(permissions(s, t).key("permissions"))
        .child(shortcut_settings(s, t, k).key("shortcuts"))
        .child(group_title(t, "Updates"))
        .child(updates::settings(s, t, k).key("updates"))
        .child(updates::note(t))
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

/// The colour scheme, then a theme for light and one for dark. Choices apply and save at once.
pub fn appearance(s: &App, t: &Theme) -> El {
    let current = s.appearance;
    let schemes = row()
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .role("radiogroup")
        .label("Color scheme")
        .children(
            ColorScheme::ALL
                .into_iter()
                .map(|scheme| scheme_card(t, current, scheme)),
        );
    // An empty sixth cell keeps the last row's cards the width of the others.
    let themes = row()
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .children(
            ThemeName::ALL
                .into_iter()
                .map(|name| theme_card(t, current, name)),
        )
        .child(theme_cell().key("spacer").py(0.0).h(0.0).a11y_hidden());
    col()
        .w_full()
        .gap(8.0)
        .child(group_title(t, "Color Scheme"))
        .child(schemes)
        .child(group_title(t, "Themes"))
        .child(themes)
        .child(group_note(
            t,
            "Pick one theme for light and one for dark. System switches between them with your computer.",
        ))
}

fn scheme_card(t: &Theme, current: Appearance, scheme: ColorScheme) -> El {
    let selected = current.scheme == scheme;
    let light = Theme::new(current.light_theme, false);
    let dark = Theme::new(current.dark_theme, true);
    let preview = match scheme {
        ColorScheme::Light => mini_window(&light),
        ColorScheme::Dark => mini_window(&dark),
        ColorScheme::System => row()
            .w_full()
            .h_full()
            .child(mini_window(&light).w_frac(0.5))
            .child(mini_window(&dark).w_frac(0.5)),
    };
    let mut card = col()
        .key(format!("scheme-{}", scheme.title()))
        .role("radio")
        .label(scheme.title())
        .checked(selected)
        .grow(1.0)
        .w(0.0)
        .min_w(150.0)
        .padding(8.0)
        .gap(8.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, if selected { t.accent } else { t.hairline })
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.04))
        .press(t.wash(0.08))
        .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
        .motion(motion::QUICK)
        .on_press_tx(send(Message::Appearance(Appearance { scheme, ..current })))
        .child(
            el().w_full()
                .h(76.0)
                .radius(radius::CONTROL)
                .clip_rounded(radius::CONTROL)
                .border(1.0, t.hairline)
                .child(preview)
                .a11y_hidden(),
        )
        .child(
            text(scheme.title())
                .font(if selected { ty::BODY_STRONG } else { ty::BODY })
                .color(if selected { t.text } else { t.muted })
                .self_center(),
        );
    if selected {
        card = card.outline(1.0, 0.0, t.accent);
    }
    card
}

/// A window in miniature, painted with `p`: sidebar, a heading and two lines, and a composer.
fn mini_window(p: &Theme) -> El {
    let line = |w: f32, c: Color| el().w(w).h(5.0).radius(2.5).fill(c);
    let sidebar = col()
        .w_frac(0.26)
        .h_full()
        .padding(7.0)
        .gap(5.0)
        .fill(p.chrome)
        .child(el().w_full().h(7.0).radius(3.5).fill(p.raised))
        .child(el().w_full().h(5.0).radius(2.5).fill(p.accent.alpha(0.35)))
        .child(el().w_full().h(5.0).radius(2.5).fill(p.wash(0.08)))
        .child(el().w_frac(0.7).h(5.0).radius(2.5).fill(p.wash(0.08)));
    let main = col()
        .grow(1.0)
        .h_full()
        .padding(8.0)
        .gap(5.0)
        .fill(p.canvas)
        .child(line(46.0, p.wash(0.16)))
        .child(line(64.0, p.wash(0.08)))
        .child(line(38.0, p.wash(0.08)))
        .child(el().grow(1.0))
        .child(
            row()
                .w_full()
                .h(14.0)
                .px(4.0)
                .radius(7.0)
                .fill(p.surface)
                .border(1.0, p.hairline)
                .center_y()
                .child(el().grow(1.0))
                .child(el().size(7.0, 7.0).radius(3.5).fill(p.accent)),
        );
    row()
        .h_full()
        .grow(1.0)
        .w_full()
        .fill(p.canvas)
        .child(sidebar)
        .child(el().w(1.0).h_full().fill(p.hairline))
        .child(main)
}

/// One cell of the theme grid: three to a row, two or one as the window narrows.
fn theme_cell() -> El {
    col()
        .grow(1.0)
        .w(0.0)
        .min_w(170.0)
        .padding(size::CARD_PAD)
        .border(1.0, Color::TRANSPARENT)
}

fn theme_card(t: &Theme, current: Appearance, name: ThemeName) -> El {
    theme_cell()
        .key(format!("theme-{}", name.title()))
        .gap(12.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(1.0, t.hairline)
        .child(
            row()
                .w_full()
                .gap(18.0)
                .justify_center()
                .py(4.0)
                .child(orb(t, current, name, false))
                .child(orb(t, current, name, true)),
        )
        .child(text(name.title()).font(ty::BODY_STRONG).color(t.text))
}

/// A swatch of `name`'s light or dark palette. Pressing it uses that theme for that mode.
fn orb(t: &Theme, current: Appearance, name: ThemeName, dark: bool) -> El {
    const SIZE: f32 = 52.0;
    let p = Theme::new(name, dark);
    let selected = current.theme(dark) == name;
    let mode = if dark { "dark" } else { "light" };
    let help = format!("Use {} in {mode} mode", name.title());
    let next = if dark {
        Appearance {
            dark_theme: name,
            ..current
        }
    } else {
        Appearance {
            light_theme: name,
            ..current
        }
    };
    let mut e = el()
        .key(format!("{}-{mode}", name.title()))
        .role("radio")
        .label(help.clone())
        .help(help)
        .checked(selected)
        .size(SIZE, SIZE)
        .radius(SIZE / 2.0)
        .fill(p.surface)
        .radial_gradient(
            [0.62, 0.32],
            SIZE * 0.78,
            [
                (0.0, p.accent),
                (0.45, p.accent.alpha(0.55)),
                (1.0, p.accent.alpha(0.0)),
            ],
        )
        .border(1.0, p.hairline)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.08))
        .press(t.wash(0.14))
        .focus_ring(2.0, 3.0, t.accent.alpha(0.7))
        .motion(motion::QUICK)
        .on_press_tx(send(Message::Appearance(next)));
    if selected {
        e = e.outline(2.0, 3.0, t.accent).child(
            el().key("badge")
                .absolute(SIZE - 16.0, SIZE - 16.0)
                .size(20.0, 20.0)
                .radius(10.0)
                .center()
                .fill(t.surface)
                .border(1.0, t.hairline)
                .child(
                    icon(if dark { &icons::MOON } else { &icons::SUN }, 12.0)
                        .color(t.text)
                        .a11y_hidden(),
                )
                .a11y_hidden(),
        );
    }
    e
}

/// The audio section: devices, microphone processing and voice transmission.
pub fn audio(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(devices(s, cx, t, k, layer).key("devices"))
        .child(group_note(
            t,
            "Disconnect to change devices; volume applies immediately. Use headphones for the microphone test.",
        ))
        .child(group_title(t, "Microphone Processing"))
        .child(processing(s, t, k, layer).key("processing"))
        .child(group_title(t, "Voice Transmission"))
        .child(transmission_settings(s, t, k, layer).key("transmission"))
        .child(group_note(
            t,
            "Stop the microphone test or disconnect to change processing and transmission. Save to keep them.",
        ))
}

fn devices(s: &App, _cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
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
                    |s| s.playback_volume.percent(),
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

/// The OS's current answer as a tinted pill, so the row reads at a glance.
fn status_pill(t: &Theme, status: permissions::Status) -> El {
    use permissions::Status;
    let (glyph, color) = match status {
        Status::Granted => (&icons::CHECK, t.good),
        Status::Denied => (&icons::CLOSE, t.poor),
        Status::NotDetermined => (&icons::LOCK, t.fair),
        Status::NotRequired => (&icons::CHECK, t.muted),
        Status::Unknown => (&icons::LOCK, t.faint),
    };
    row()
        .h(22.0)
        .px(8.0)
        .gap(5.0)
        .center_y()
        .radius(11.0)
        .fill(color.alpha(0.14))
        .label(status.label())
        .child(rune::icon(glyph, 12.0).color(color).a11y_hidden())
        .child(
            text(status.label())
                .font(ty::LABEL)
                .color(color)
                .a11y_hidden(),
        )
}

/// A compact switch at a row's trailing edge; the row's label names it.
fn switch(
    key: &'static str,
    label: &str,
    on: bool,
    enabled: bool,
    k: &Kit,
    flip: fn(&App, bool) -> Message,
) -> El {
    Switch::new(on)
        .label(label)
        .hide_label()
        .disabled(!enabled)
        .on_toggle_tx(move |s: &mut App, tx| {
            let message = flip(s, !on);
            super::dispatch(s, tx, message);
        })
        .build(k)
        .key(key)
}

/// A processing change built from the applied options; `update` refuses it during a session.
fn processing_with(s: &App, change: impl FnOnce(&mut audio_processing::Settings)) -> Message {
    let mut next = s.audio_processing;
    change(&mut next);
    Message::AudioProcessing(next)
}

fn processing(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    if !audio_processing::AVAILABLE {
        return group(
            t,
            [form_row(
                t,
                "Noise suppression and echo cancellation",
                Some("Available on Mac and Windows. Saved effects are not applied here."),
                el(),
            )],
        );
    }
    let options = s.audio_processing;
    let editable = s.session.is_none();
    let levels: Vec<String> = audio_processing::SuppressionLevel::ALL
        .iter()
        .map(ToString::to_string)
        .collect();
    group(
        t,
        [
            form_row(
                t,
                "Noise suppression",
                Some("Reduces steady background noise such as fans and typing."),
                switch(
                    "noise-suppression-switch",
                    "Noise suppression",
                    options.noise_suppression,
                    editable,
                    k,
                    |s, on| processing_with(s, |o| o.noise_suppression = on),
                ),
            )
            .key("noise-suppression"),
            form_row(
                t,
                "Suppression level",
                Some("Higher levels remove more noise and some voice."),
                picker(
                    k,
                    s,
                    "suppression-level",
                    levels,
                    Some(options.suppression_level.to_string().as_str()),
                    "Choose…",
                    editable && options.noise_suppression,
                    size::FORM_CONTROL,
                    move |index| {
                        Message::AudioProcessing(audio_processing::Settings {
                            suppression_level: audio_processing::SuppressionLevel::ALL[index],
                            ..options
                        })
                    },
                    layer,
                ),
            )
            .key("suppression-level"),
            form_row(
                t,
                "Automatic gain",
                Some("Evens out your microphone level."),
                switch(
                    "automatic-gain-switch",
                    "Automatic gain",
                    options.automatic_gain,
                    editable,
                    k,
                    |s, on| processing_with(s, |o| o.automatic_gain = on),
                ),
            )
            .key("automatic-gain"),
            form_row(
                t,
                "Echo cancellation",
                Some("Experimental. Removes Flexspeak’s playback from your microphone in calls."),
                switch(
                    "echo-cancellation-switch",
                    "Echo cancellation",
                    options.echo_cancellation,
                    editable,
                    k,
                    |s, on| processing_with(s, |o| o.echo_cancellation = on),
                ),
            )
            .key("echo-cancellation"),
        ],
    )
}

fn transmission_settings(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let settings = s.transmission;
    let editable = s.session.is_none();
    let modes: Vec<String> = transmission::Mode::ALL
        .iter()
        .map(ToString::to_string)
        .collect();
    let ptt_key = s.keybinds.push_to_talk.trim();
    let detail: String = match settings.mode {
        transmission::Mode::Continuous => "Sends whenever you are unmuted.".into(),
        transmission::Mode::PushToTalk if !shortcuts::supported() => {
            "Global shortcuts are unavailable here. Choose another mode.".into()
        }
        transmission::Mode::PushToTalk if ptt_key.is_empty() => {
            "Set a push-to-talk key under Keyboard Shortcuts.".into()
        }
        transmission::Mode::PushToTalk => format!("Hold {ptt_key} to talk while unmuted."),
        transmission::Mode::VoiceActivation if !audio_processing::AVAILABLE => {
            "Speech detection needs Mac or Windows. Choose another mode.".into()
        }
        transmission::Mode::VoiceActivation => {
            "Sends when speech is detected above the threshold.".into()
        }
    };
    let mut rows = vec![
        form_row(
            t,
            "Transmission",
            Some(detail.as_str()),
            picker(
                k,
                s,
                "transmission-mode",
                modes,
                Some(settings.mode.to_string().as_str()),
                "Choose…",
                editable,
                size::FORM_CONTROL,
                move |index| {
                    Message::Transmission(transmission::Settings {
                        mode: transmission::Mode::ALL[index],
                        ..settings
                    })
                },
                layer,
            ),
        )
        .key("transmission-mode"),
    ];
    match settings.mode {
        transmission::Mode::Continuous => {}
        transmission::Mode::PushToTalk => rows.push(
            form_row(
                t,
                format!("Release delay · {} ms", settings.ptt_release_ms),
                Some("Keeps sending briefly after you let go."),
                trailing(locked(
                    editable,
                    slider(
                        t,
                        "ptt-release",
                        "Push-to-talk release delay",
                        i32::from(settings.ptt_release_ms),
                        format!("{} milliseconds", settings.ptt_release_ms),
                        Range {
                            min: 0,
                            max: 500,
                            step: 25,
                            key_step: 25,
                        },
                        |s| i32::from(s.transmission.ptt_release_ms),
                        move |value| {
                            Message::Transmission(transmission::Settings {
                                ptt_release_ms: value.clamp(0, 500) as u16,
                                ..settings
                            })
                        },
                    ),
                )),
            )
            .key("ptt-release"),
        ),
        transmission::Mode::VoiceActivation => rows.push(
            form_row(
                t,
                format!("Voice threshold · {} dBFS", settings.voice_threshold_db),
                Some("Move right to require a louder voice."),
                trailing(locked(
                    editable,
                    slider(
                        t,
                        "voice-threshold",
                        "Voice activation threshold",
                        i32::from(settings.voice_threshold_db),
                        format!("{} decibels", settings.voice_threshold_db),
                        Range {
                            min: -60,
                            max: 0,
                            step: 1,
                            key_step: 2,
                        },
                        |s| i32::from(s.transmission.voice_threshold_db),
                        move |value| {
                            Message::Transmission(transmission::Settings {
                                voice_threshold_db: value.clamp(-60, 0) as i16,
                                ..settings
                            })
                        },
                    ),
                )),
            )
            .key("voice-threshold"),
        ),
    }
    group(t, rows)
}

/// A control that `update` would refuse right now: shown at 42% and not interactive.
fn locked(editable: bool, control: El) -> El {
    if editable {
        control
    } else {
        control.dim(0.42).disabled(true)
    }
}

pub fn permissions(s: &App, t: &Theme) -> El {
    use permissions::Status;
    let enabled = s.permissions.pending.is_none() && s.session.is_none();
    let mut rows = Vec::new();
    for kind in permissions::Kind::ALL {
        let status = s.permissions.status[kind.index()];
        let pending = s.permissions.pending == Some(kind);
        let detail = match &s.permissions.messages[kind.index()] {
            Some(message) => format!("{} {message}", kind.detail()),
            None => kind.detail().to_owned(),
        };
        let mut controls = row().gap(size::GAP).center_y();
        if pending {
            controls = controls.child(super::widgets::pulse(t.accent));
        }
        controls = controls.child(status_pill(t, status));
        // Once denied, the OS won't ask again: the switch lives in its privacy settings.
        let give = match status {
            Status::NotDetermined | Status::Unknown => Some(Message::RequestPermission(kind)),
            Status::Denied => Some(Message::OpenPermissionSettings(kind)),
            Status::Granted | Status::NotRequired => None,
        };
        if let Some(give) = give {
            controls = controls.child(button(
                t,
                "request",
                "Give Access",
                None,
                Variant::Primary,
                enabled.then_some(give),
            ));
        }
        if cfg!(any(target_os = "macos", windows)) && status != Status::NotRequired {
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
        note.push_str(" This build appears as Flexspeak in Privacy & Security. macOS may ask you to reopen the app after allowing screen recording.");
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
        Some(shortcuts::Action::PushToTalk(true)) => "Detected push-to-talk pressed",
        Some(shortcuts::Action::PushToTalk(false)) => "Detected push-to-talk released",
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
                    "Push to talk",
                    Some("Held, not toggled. Used in push-to-talk mode."),
                    key_field(
                        k,
                        &s.ui.ptt_key,
                        |s| &mut s.ui.ptt_key,
                        "Ctrl+Shift+Space",
                        Message::PttKeybind,
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
