//! The window shell, the saved-server home, the connected sidebar and performance mode.

use rune::{Cx, El, Transition, col, el, icon, row, text};
use rune_kit::{Kit, Switch};

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Variant, body, button, card, divider, faint, heading, health_dot, icon_button, labelled, meter,
    muted, overline, pulse, scroll_page, speaking_dot, vertical_divider,
};
use super::{footer, icons, send, statistics, streams};
use crate::{App, Message, Tab, client, shortcuts, usage};

pub fn normal(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let page = match s.tab {
        Tab::Home => home(s, cx, t, k, layer),
        Tab::Statistics => statistics::view(s, cx, t, k),
    };
    let key = match s.tab {
        Tab::Home => "page-home",
        Tab::Statistics => "page-statistics",
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
    // Connected, the sidebar already names the server.
    let title = match s.tab {
        Tab::Home if s.connected && !s.loopback => None,
        Tab::Home => Some("Flexspeak"),
        Tab::Statistics => Some("Usage & Statistics"),
    };
    if let Some(title) = title {
        bar = bar.child(
            text(title)
                .font(ty::HEADING)
                .color(t.text)
                .truncate()
                .ml(4.0),
        );
    }
    bar = bar.child(el().grow(1.0));
    bar = bar
        .child(performance_switch(k, false))
        .child(vertical_divider(t, 24.0));
    let statistics = s.tab == Tab::Statistics;
    bar = bar.child(icon_button(
        t,
        "Statistics",
        &icons::CHART_LINE,
        "Statistics",
        if statistics {
            Variant::Active
        } else {
            Variant::Ghost
        },
        Some(Message::ShowTab(if statistics {
            Tab::Home
        } else {
            Tab::Statistics
        })),
    ));
    let open = s.settings_page.is_some();
    bar = bar.child(icon_button(
        t,
        "Settings",
        &icons::SETTINGS,
        if cfg!(target_os = "macos") {
            "Settings · ⌘,"
        } else {
            "Settings · Ctrl+,"
        },
        if open {
            Variant::Active
        } else {
            Variant::Ghost
        },
        Some(if open {
            Message::CloseSettings
        } else {
            Message::OpenSettings(s.settings_recent)
        }),
    ));
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
    saved_servers(s, t, k)
}

/// The saved servers, each with its own Connect. One session runs at a time, so while one
/// connects or is connected the others wait.
fn saved_servers(s: &App, t: &Theme, k: &Kit) -> El {
    let busy = s.session.is_some();
    let mut content = col().w_full().gap(14.0);
    if let Some(saved) = s.saved.as_ref().filter(|_| !s.servers.is_empty()) {
        content = content.child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(overline(t, "Saved servers").grow(1.0))
                .child(labelled(
                    &icons::USER,
                    format!("Connect as {}", saved.nickname),
                    t.muted,
                ))
                .child(icon_button(
                    t,
                    "add-server",
                    &icons::PLUS,
                    "Add server",
                    Variant::Ghost,
                    Some(Message::AddServer),
                )),
        );
        let ready = !s.stopping && s.permissions.pending.is_none();
        let current = s.session_address();
        let mut list = col().key("server-list").w_full().gap(4.0);
        for (index, server) in s.servers.iter().enumerate() {
            let mine = current == Some(server.address.trim());
            list = list.child(server_row(t, s, index, server, mine, busy, ready));
        }
        content = content.child(
            k.scroll_bar(
                col()
                    .key("servers-scroll")
                    .w_full()
                    .max_h(300.0)
                    .scroll(rune::ScrollSpec {
                        stick_bottom: false,
                    })
                    .scroll_edge_fade(12.0)
                    .child(list),
            ),
        );
        let mut status = row().w_full().gap(8.0).center_y();
        if busy && !s.stopping {
            status = status.child(pulse(if s.loopback { t.live } else { t.accent }));
        }
        status = status.child(muted(t, s.status.clone()).grow(1.0));
        if busy && s.loopback {
            status = status.child(button(
                t,
                "stop-test",
                if s.stopping {
                    "Stopping…"
                } else {
                    "Stop test"
                },
                Some(&icons::STOP),
                Variant::Secondary,
                (!s.stopping).then_some(Message::Stop),
            ));
        }
        content = content
            .child(divider(t))
            .child(status.key("status").transition(Transition::opacity()));
    } else {
        content = content
            .child(overline(t, "Saved servers"))
            .child(super::widgets::title(t, "No saved server yet"))
            .child(body(t, "Add a server and choose your nickname to connect."))
            .child(
                button(
                    t,
                    "open-settings",
                    "Add Server",
                    Some(&icons::PLUS),
                    Variant::Primary,
                    Some(Message::AddServer),
                )
                .self_end(),
            );
    }
    let hero = card(t)
        .key("saved-server")
        .max_w(560.0)
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

/// One saved server: its name and address, and Connect — or Cancel while it connects.
fn server_row(
    t: &Theme,
    s: &App,
    index: usize,
    server: &crate::settings::Server,
    mine: bool,
    busy: bool,
    ready: bool,
) -> El {
    let named = !server.name.trim().is_empty();
    let mut names = col().grow(1.0).min_w(0.0).gap(2.0).child(
        text(server.title().to_owned())
            .font(ty::BODY_STRONG)
            .color(t.text)
            .truncate_fade(),
    );
    if named {
        names = names.child(
            text(server.address.clone())
                .font(ty::SMALL)
                .color(t.muted)
                .truncate_fade(),
        );
    }
    let (label, action) = if mine {
        (
            if s.stopping { "Stopping…" } else { "Cancel" },
            (!s.stopping).then_some(Message::Stop),
        )
    } else {
        (
            "Connect",
            (!busy && ready).then_some(Message::Connect(index)),
        )
    };
    // The first server is the window's default button, so Return connects to it.
    let default = index == 0 && !busy;
    let mut connect = button(
        t,
        &format!("connect-{index}"),
        label,
        Some(if mine {
            &icons::DISCONNECT
        } else {
            &icons::CONNECT
        }),
        if default {
            Variant::Primary
        } else {
            Variant::Secondary
        },
        action,
    )
    .help(if mine {
        format!("Cancel connecting to {}", server.title())
    } else if busy {
        "Disconnect first: one server at a time".to_owned()
    } else {
        format!("Connect to {}", server.title())
    });
    if default {
        connect = connect.default_button();
    }
    row()
        .key(format!("server-{index}"))
        .w_full()
        .min_h(52.0)
        .px(10.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(radius::ROW)
        .fill(if mine {
            t.accent.alpha(0.10)
        } else {
            t.wash(0.03)
        })
        .border(
            1.0,
            if mine {
                t.accent.alpha(0.35)
            } else {
                t.hairline
            },
        )
        .motion(motion::QUICK)
        .child(
            el().size(32.0, 32.0)
                .radius(radius::CONTROL)
                .center()
                .fill(t.raised)
                .child(icon(&icons::SERVER, 16.0).color(t.accent).a11y_hidden())
                .a11y_hidden(),
        )
        .child(names)
        .child(connect)
}

const ENTRANCE: &[rune::Beat] = &[rune::Beat::new(0.0).opacity(0.0), rune::Beat::new(0.25)];

fn sidebar(s: &App, t: &Theme, k: &Kit) -> El {
    let all = s.subscribe_all.load(std::sync::atomic::Ordering::Relaxed);
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
            heading(t, s.state.server.clone())
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(icon_button(
            t,
            "subscribe-all",
            if all { &icons::EYE } else { &icons::EYE_OFF },
            if all {
                "Seeing every channel · show only your channel"
            } else {
                "Seeing only your channel · show every channel"
            },
            if all { Variant::Active } else { Variant::Ghost },
            Some(Message::ToggleSubscribeAll),
        ));
    col()
        .key("sidebar")
        .w(size::SIDEBAR)
        .h_full()
        .fill(t.chrome)
        .padding(12.0)
        .gap(12.0)
        .child(server)
        .child(divider(t))
        .child(overline(t, "Channels").ml(6.0))
        // The channel list takes the free height, so the voice controls sit at the bottom.
        .child(scroll_page(
            k,
            "channel-scroll",
            channel_tree(s, t).pr(10.0).pb(8.0),
        ))
        .child(voice_controls(s, t))
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
    // Push-to-talk and voice activation keep it dark while their gate is closed.
    let talking = speaking > crate::audio::SPEAKING_METER && s.transmitting;
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
                    .grow(1.0)
                    .min_w(0.0)
                    .a11y_hidden(),
            );
        if !channel.members_visible {
            name = name.child(
                el().help("You can't see who is in this channel")
                    .label("Members not visible")
                    .child(icon(&icons::EYE_OFF, 13.0).color(t.faint)),
            );
        }
        if current {
            name = name.fill(t.accent.alpha(0.16));
        }
        if joinable {
            name = name
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(t.wash(0.05))
                .press(t.wash(0.09))
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
            .hover(t.wash(0.06))
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
    if s.is_streaming(member) {
        entry = entry.child(
            el().help("Sharing a stream")
                .label("Sharing a stream")
                .child(icon(&icons::SCREEN_SHARE, 13.0).color(t.live)),
        );
    }
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
        let mut hint = format!(
            "Global: {} · mute/unmute   |   {} · deafen/undeafen",
            s.keybinds.toggle_mute, s.keybinds.toggle_deafen
        );
        if !s.keybinds.push_to_talk.trim().is_empty() {
            hint.push_str(&format!(
                "   |   {} · hold to talk",
                s.keybinds.push_to_talk
            ));
        }
        faint(t, hint)
    }
}
