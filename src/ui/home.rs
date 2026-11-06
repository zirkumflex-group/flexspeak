//! The window shell, the saved-server home, the connected sidebar and performance mode.

use rune::{Color, Cx, El, Transition, col, el, icon, row, text};
use rune_kit::{Kit, Switch};

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Variant, body, button, card, divider, faint, heading, health_dot, icon_button, labelled, meter,
    muted, overline, pulse, scroll_page, speaking_dot, vertical_divider,
};
use super::{footer, icons, send, settings, statistics, streams};
use crate::{App, Message, Tab, client, shortcuts, usage};

pub fn normal(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let page = match s.tab {
        Tab::Home => home(s, cx, t, k, layer),
        Tab::Statistics => statistics::view(s, cx, t, k),
        Tab::Settings => settings::view(s, cx, t, k, layer),
    };
    let key = match s.tab {
        Tab::Home => "page-home",
        Tab::Statistics => "page-statistics",
        Tab::Settings => "page-settings",
    };
    col()
        .w_full()
        .h_full()
        .child(header(s, cx, t, k))
        .child(
            el().key("pages").grow(1.0).min_h(0.0).w_full().child(
                col()
                    .key(key)
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h_full()
                    // Pages swap with a short cross-fade, as a window's panes do.
                    .transition(Transition::opacity())
                    .motion(motion::SETTLE)
                    .child(page),
            ),
        )
        .child(footer::view(s, cx, t, false))
}

fn header(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit) -> El {
    let mut bar = row()
        .key("header")
        .h(size::HEADER)
        .w_full()
        .pl(16.0 + cx.titlebar_inset())
        .pr(12.0)
        .gap(size::GAP)
        .center_y()
        .fill(t.chrome)
        .window_drag();
    if s.tab != Tab::Home {
        bar = bar.child(icon_button(
            t,
            "back",
            &icons::ARROW_LEFT,
            "Back to server · Esc",
            Variant::Ghost,
            Some(Message::ShowTab(Tab::Home)),
        ));
    }
    let title = match s.tab {
        Tab::Home if s.connected && !s.loopback => s.state.server.clone(),
        Tab::Home => "Flexspeak".into(),
        Tab::Statistics => "Usage & Statistics".into(),
        Tab::Settings => "Settings".into(),
    };
    bar = bar
        .child(
            text(title)
                .font(ty::HEADING)
                .color(t.text)
                .truncate()
                .ml(4.0),
        )
        .child(el().grow(1.0));
    bar = bar
        .child(performance_switch(k, false))
        .child(vertical_divider(t, 24.0));
    for (tab, glyph, label) in [
        (Tab::Statistics, &icons::CHART_LINE, "Statistics"),
        (Tab::Settings, &icons::SETTINGS, "Settings"),
    ] {
        let active = s.tab == tab;
        bar = bar.child(icon_button(
            t,
            label,
            glyph,
            label,
            if active {
                Variant::Active
            } else {
                Variant::Ghost
            },
            Some(Message::ShowTab(if active { Tab::Home } else { tab })),
        ));
    }
    col().w_full().child(bar).child(divider(t))
}

pub fn performance_switch(k: &Kit, enabled: bool) -> El {
    Switch::new(enabled)
        .label("Performance mode")
        .on_toggle_tx(move |s: &mut App, tx| {
            super::dispatch(s, tx, Message::PerformanceMode(!enabled))
        })
        .build(k)
        .help("A compact window that samples every 10 s and pauses graphs and streaming")
}

fn home(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    if s.connected && !s.loopback {
        return row()
            .w_full()
            .h_full()
            .child(sidebar(s, t, k))
            .child(el().w(1.0).h_full().fill(t.hairline))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .h_full()
                    .child(streams::view(s, cx, t, k, layer)),
            );
    }
    saved_server(s, t)
}

fn saved_server(s: &App, t: &Theme) -> El {
    let busy = s.session.is_some();
    let mut content = col().w_full().gap(14.0).child(overline(t, "Saved server"));
    if let Some(saved) = &s.saved {
        let label = if s.stopping {
            "Stopping…"
        } else if busy && s.loopback {
            "Stop test"
        } else if busy {
            "Cancel"
        } else {
            "Connect"
        };
        let enabled = !s.stopping && s.permissions.pending.is_none();
        let action = if busy {
            Message::Stop
        } else {
            Message::Connect
        };
        let mut status = row().gap(8.0).center_y();
        if busy && !s.stopping {
            status = status.child(pulse(if s.loopback { t.live } else { t.accent }));
        }
        status = status.child(muted(t, s.status.clone()));
        content = content
            .child(
                row()
                    .w_full()
                    .gap(16.0)
                    .center_y()
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(4.0)
                            .child(
                                text(saved.server_address.clone())
                                    .font(ty::TITLE)
                                    .color(t.text)
                                    .truncate_fade(),
                            )
                            .child(labelled(
                                &icons::USER,
                                format!("Connect as {}", saved.nickname),
                                t.muted,
                            )),
                    )
                    .child(connect_button(t, busy, label, enabled.then_some(action))),
            )
            .child(divider(t))
            .child(status.key("status").transition(Transition::opacity()));
    } else {
        content = content
            .child(super::widgets::title(t, "No saved server yet"))
            .child(body(
                t,
                "Open Settings to configure your server and username.",
            ))
            .child(
                button(
                    t,
                    "open-settings",
                    "Open Settings",
                    Some(&icons::SETTINGS),
                    Variant::Primary,
                    Some(Message::ShowTab(Tab::Settings)),
                )
                .self_end(),
            );
    }
    let hero = card(t)
        .key("saved-server")
        .max_w(520.0)
        .padding(20.0)
        .child(content)
        .sequence(rune::Sequence::new(ENTRANCE).delay(0.05));
    col()
        .w_full()
        .h_full()
        .padding(size::PAGE)
        .center()
        .child(hero)
}

const ENTRANCE: &[rune::Beat] = &[rune::Beat::new(0.0).opacity(0.0), rune::Beat::new(0.25)];

/// Connect / Cancel on the home card: the window's default button, so Return connects.
fn connect_button(t: &Theme, busy: bool, label: &str, on: Option<Message>) -> El {
    let e = button(
        t,
        "connect",
        label,
        Some(if busy {
            &icons::DISCONNECT
        } else {
            &icons::CONNECT
        }),
        if busy {
            Variant::Secondary
        } else {
            Variant::Primary
        },
        on,
    )
    .help(label.to_owned());
    if busy { e } else { e.default_button() }
}

fn sidebar(s: &App, t: &Theme, k: &Kit) -> El {
    let channel = s
        .state
        .current
        .as_ref()
        .map_or("No channel", |channel| channel.name.as_str());
    let server = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(
            el().size(28.0, 28.0)
                .radius(radius::CONTROL)
                .center()
                .fill(t.raised)
                .child(icon(&icons::SERVER, 15.0).color(t.accent).a11y_hidden()),
        )
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(heading(t, s.state.server.clone()).truncate_fade())
                .child(labelled(&icons::CHANNEL, channel, t.muted)),
        );
    col()
        .key("sidebar")
        .w(size::SIDEBAR)
        .h_full()
        .fill(t.chrome)
        .padding(12.0)
        .gap(12.0)
        .child(server)
        .child(voice_controls(s, t))
        .child(divider(t))
        .child(overline(t, "Channels").ml(6.0))
        .child(scroll_page(
            k,
            "channel-scroll",
            channel_tree(s, t).pr(10.0).pb(8.0),
        ))
}

fn voice_controls(s: &App, t: &Theme) -> El {
    let enabled = s.connected && !s.loopback && !s.stopping;
    let mut controls = row().w_full().gap(size::GAP);
    for (key, glyph, label, message, active, on) in [
        (
            "mute",
            s.microphone_icon().glyph(),
            if s.muted {
                "Unmute microphone"
            } else {
                "Mute microphone"
            },
            Message::Mute,
            s.muted || s.deafened,
            enabled,
        ),
        (
            "deafen",
            s.playback_icon().glyph(),
            if s.deafened { "Undeafen" } else { "Deafen" },
            Message::Deafen,
            s.deafened,
            enabled,
        ),
    ] {
        controls = controls.child(icon_button(
            t,
            key,
            glyph,
            label,
            if active {
                Variant::Active
            } else {
                Variant::Secondary
            },
            on.then_some(message),
        ));
    }
    controls = controls.child(el().grow(1.0)).child(icon_button(
        t,
        "disconnect",
        &icons::DISCONNECT,
        "Disconnect",
        Variant::Danger,
        (!s.stopping).then_some(Message::Stop),
    ));
    let speaking = s.microphone_meter_level();
    // Lit while you are heard; it eases in and out only as that changes, not with every reading.
    let talking = speaking > crate::audio::SPEAKING_METER;
    let mut panel = col()
        .key("voice")
        .w_full()
        .gap(8.0)
        .padding(10.0)
        .radius(radius::CARD)
        .fill(t.surface)
        .border(
            1.0,
            if talking {
                t.good.alpha(0.5)
            } else {
                t.hairline
            },
        )
        .motion(motion::SETTLE)
        .child(controls)
        .child(meter(t, "mic-level", speaking, t.good))
        .child(faint(
            t,
            format!(
                "Mic: {} · {}",
                s.microphone_status(),
                if s.deafened { "Deafened" } else { "Sound on" }
            ),
        ));
    if s.controls_pending
        || s.server_controls
            .is_none_or(|state| state.muted != s.muted || state.deafened != s.deafened)
    {
        panel = panel.child(
            row()
                .key("pending")
                .gap(6.0)
                .center_y()
                .transition(Transition::opacity())
                .child(pulse(t.fair))
                .child(faint(
                    t,
                    if s.controls_pending {
                        "Server audio update pending"
                    } else {
                        "Server audio state unconfirmed"
                    },
                )),
        );
    }
    panel
}

fn channel_tree(s: &App, t: &Theme) -> El {
    let mut tree = col().key("channels").w_full().gap(2.0);
    let mut hidden_below = None;
    for channel in &s.state.channels {
        if hidden_below.is_some_and(|depth| channel.depth > depth) {
            continue;
        }
        hidden_below = None;
        let collapsed = s.collapsed_channels.contains(&channel.id);
        if collapsed {
            hidden_below = Some(channel.depth);
        }
        let current = s
            .state
            .current
            .as_ref()
            .is_some_and(|value| value.id == channel.id);
        let hint = if current {
            "Current channel"
        } else {
            match channel.join_access {
                client::JoinAccess::Denied => "You do not have permission to join this channel",
                client::JoinAccess::UnsupportedCodec => {
                    "This channel uses an unsupported audio codec"
                }
                _ if channel.has_password => "Click to join · a password will be requested",
                _ => "Click to join this channel",
            }
        };
        let joinable = (!s.stopping && current) || s.can_request_join(channel.id);
        let ink = if current {
            t.accent
        } else if joinable {
            t.text
        } else {
            t.faint
        };
        let mut name = row()
            .key("name")
            .role("button")
            .label(channel.name.clone())
            .help(hint)
            .grow(1.0)
            .min_w(0.0)
            .h(size::ROW)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::ROW)
            .motion(motion::QUICK)
            .child(
                icon(
                    if channel.has_password {
                        &icons::LOCK
                    } else {
                        &icons::VOLUME
                    },
                    14.0,
                )
                .color(if current { t.accent } else { t.faint })
                .a11y_hidden(),
            )
            .child(
                text(channel.name.clone())
                    .font(ty::BODY_STRONG)
                    .color(ink)
                    .truncate_fade()
                    .a11y_hidden(),
            );
        if current {
            name = name.fill(t.accent.alpha(0.16));
        }
        if joinable {
            name = name
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(Color::WHITE.alpha(0.05))
                .press(Color::WHITE.alpha(0.09))
                .on_press_tx(send(Message::Join(channel.id)));
        }
        let disclosure = el()
            .key("disclosure")
            .role("button")
            .label(if collapsed {
                "Expand channel"
            } else {
                "Collapse channel"
            })
            .expanded(!collapsed)
            .size(18.0, size::ROW)
            .center()
            .radius(6.0)
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(Color::WHITE.alpha(0.06))
            .on_press_tx(send(Message::ToggleChannel(channel.id)))
            .child(
                icon(&icons::CHEVRON_RIGHT, 12.0)
                    .color(t.faint)
                    .rotate(if collapsed {
                        0.0
                    } else {
                        std::f32::consts::FRAC_PI_2
                    })
                    .motion(motion::QUICK)
                    .a11y_hidden(),
            );
        let mut node = col()
            .key(format!("channel-{}", channel.id))
            .w_full()
            .gap(2.0)
            .pl(channel.depth.min(8) as f32 * 14.0)
            .child(
                row()
                    .w_full()
                    .gap(2.0)
                    .center_y()
                    .child(disclosure)
                    .child(name),
            );
        if !collapsed {
            let mut members = col()
                .key("members")
                .w_full()
                .gap(1.0)
                .pl(20.0)
                .transition(Transition::opacity());
            for member in &channel.members {
                members = members.child(member_row(s, t, member, current));
            }
            if !channel.members_visible {
                members = members.child(faint(t, "Members not visible").pl(10.0).py(2.0));
            }
            node = node.child(members);
        }
        tree = tree.child(node);
    }
    tree
}

fn member_row(s: &App, t: &Theme, member: &client::Member, current: bool) -> El {
    let mic_muted =
        member.mic_muted || (member.is_self && s.microphone_icon() == crate::Icon::MicOff);
    let sound_muted = member.sound_muted || (member.is_self && s.deafened);
    let (health, detail) = s.member_health(member, current);
    let label = if health == usage::Health::Unknown {
        "Unknown"
    } else {
        health.label()
    };
    let mut entry = row()
        .key(format!("member-{}", member.id))
        .w_full()
        .h(24.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(radius::ROW)
        .transition(Transition::opacity().combined(Transition::offset(-6.0, 0.0)))
        .child(health_dot(t, health, format!("{label}\n{detail}")))
        .child(speaking_dot(t, s.talking.contains(&member.id)))
        .child(
            text(if member.is_self {
                format!("{} (you)", member.name)
            } else {
                member.name.clone()
            })
            .font(ty::BODY)
            .color(t.text)
            .truncate_fade()
            .grow(1.0)
            .min_w(0.0),
        );
    for (muted, glyph, help) in [
        (mic_muted, &icons::MIC_OFF, "Microphone muted"),
        (sound_muted, &icons::HEADPHONE_OFF, "Sound muted"),
    ] {
        if muted {
            entry = entry.child(
                el().help(help)
                    .child(icon(glyph, 13.0).color(t.faint))
                    .label(help),
            );
        }
    }
    entry
}

pub fn performance(s: &App, t: &Theme, k: &Kit) -> El {
    let in_channel = s.connected && !s.loopback && !s.stopping;
    let connection = if s.stopping {
        "Disconnecting…"
    } else if s.connected {
        if s.loopback {
            "Local microphone test"
        } else {
            "Connected"
        }
    } else {
        &s.status
    };
    let channel = s
        .state
        .current
        .as_ref()
        .filter(|_| in_channel)
        .map_or("—", |channel| channel.name.as_str());
    let playback = if !s.connected || s.stopping {
        "Inactive"
    } else if s.deafened {
        "Deafened"
    } else {
        "On"
    };
    let line = |glyph: &'static rune::Icon, label: String| labelled(glyph, label, t.text);
    let info = col()
        .w_full()
        .gap(8.0)
        .child(line(&icons::ACTIVITY, format!("Connection: {connection}")))
        .child(line(
            &icons::SERVER,
            format!("Server: {}", if in_channel { &s.state.server } else { "—" }),
        ))
        .child(line(&icons::CHANNEL, format!("Channel: {channel}")))
        .child(line(
            s.microphone_icon().glyph(),
            format!("Microphone: {}", s.microphone_status()),
        ))
        .child(line(
            s.playback_icon().glyph(),
            format!("Playback: {playback}"),
        ));
    col()
        .w_full()
        .h_full()
        .child(
            col()
                .grow(1.0)
                .min_h(0.0)
                .w_full()
                .padding(16.0)
                // Room for the macOS traffic lights over the drag band.
                .pt(if cfg!(target_os = "macos") {
                    40.0
                } else {
                    16.0
                })
                .gap(10.0)
                .child(performance_switch(k, true))
                .child(scroll_page(k, "performance-info", info))
                .child(shortcut_hint(s, t))
                .child(faint(
                    t,
                    format!(
                        "Updates every {} s · CPU: 100% = one core",
                        usage::COMPACT_REFRESH_INTERVAL.as_secs()
                    ),
                )),
        )
        .child(footer::compact(s, t))
}

pub fn shortcut_hint(s: &App, t: &Theme) -> El {
    if let Some(error) = &s.shortcut_error {
        super::widgets::error_text(
            t,
            if shortcuts::supported() {
                "Global shortcuts unavailable. Configure them in Settings.".into()
            } else {
                format!("Global shortcuts unavailable: {error}")
            },
        )
    } else {
        faint(
            t,
            format!(
                "Global: {} · mute/unmute   |   {} · deafen/undeafen",
                s.keybinds.toggle_mute, s.keybinds.toggle_deafen
            ),
        )
    }
}
