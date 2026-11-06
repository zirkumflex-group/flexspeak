//! Named states for headless previews: `teamspeak-native --snap out.png --scene connected`.
//! They change only what the view shows; no session, device or file is touched.

use std::time::{Duration, Instant};

use crate::permissions::{Kind, Status};
use crate::{App, Tab, alerts::Source, client, onboarding, settings, usage};

type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("setup", setup),
    ("devices", devices),
    ("permissions-not-asked", permissions_not_asked),
    ("permissions-allowed", permissions_allowed),
    ("permissions-denied", permissions_denied),
    ("permissions-mixed", permissions_mixed),
    ("permissions-windows", permissions_windows),
    ("home", home),
    ("connected", connected),
    ("footer-hidden", footer_hidden),
    ("statistics", statistics),
    ("settings", settings_page),
    ("settings-permissions", settings_permissions),
    ("performance", performance),
    ("password", password),
    ("alert", alert),
];

fn saved(s: &mut App) {
    s.onboarding = None;
    s.saved = Some(settings::Settings {
        server_address: "ts.zirkumflex.io".into(),
        nickname: "Jan".into(),
        ..settings::Settings::default()
    });
    s.address = "ts.zirkumflex.io".into();
    s.nickname = "Jan".into();
    s.alerts = Default::default();
    s.sync_fields();
}

fn setup(s: &mut App) {
    s.begin_setup();
    s.nickname = "Jan".into();
    s.onboarding = Some(onboarding::Step::Server);
    s.address = "ts.zirkumflex.io".into();
    s.alerts = Default::default();
    s.sync_fields();
}

fn devices(s: &mut App) {
    setup(s);
    s.onboarding = Some(onboarding::Step::Devices);
}

/// The setup step with the OS reporting `status` for microphone, screen and system audio.
fn permissions_step(s: &mut App, status: [Status; 3]) {
    setup(s);
    s.onboarding = Some(onboarding::Step::Permissions);
    s.permissions.status = status;
}

fn permissions_not_asked(s: &mut App) {
    permissions_step(s, [Status::NotDetermined; 3]);
}

fn permissions_allowed(s: &mut App) {
    permissions_step(s, [Status::Granted; 3]);
}

fn permissions_denied(s: &mut App) {
    permissions_step(s, [Status::Denied; 3]);
    s.permissions.messages[Kind::Microphone.index()] = Some(
        "Microphone access is off. Enable it in System Settings → Privacy & Security → Microphone."
            .into(),
    );
}

fn permissions_mixed(s: &mut App) {
    permissions_step(s, [Status::Granted, Status::Denied, Status::NotDetermined]);
}

/// What Windows reports: only the microphone has a privacy switch.
fn permissions_windows(s: &mut App) {
    permissions_step(
        s,
        [Status::Denied, Status::NotRequired, Status::NotRequired],
    );
}

fn home(s: &mut App) {
    saved(s);
    s.status = "Ready to connect".into();
}

fn member(id: u16, name: &str, is_self: bool, mic_muted: bool) -> client::Member {
    client::Member {
        id,
        name: name.into(),
        is_self,
        mic_muted,
        sound_muted: false,
    }
}

fn connected(s: &mut App) {
    saved(s);
    s.connected = true;
    s.muted = false;
    s.level = 0.3;
    s.state.can_talk = true;
    s.state.server = "Zirkumflex".into();
    s.state.server_version = "6.0.0".into();
    let lobby = client::Channel {
        id: 1,
        name: "Lobby".into(),
        members: vec![
            member(1, "Jan", true, false),
            member(2, "Mara", false, false),
            member(3, "Theo", false, true),
        ],
        members_visible: true,
        join_access: client::JoinAccess::Allowed,
        ..client::Channel::default()
    };
    let channels = vec![
        lobby.clone(),
        client::Channel {
            id: 2,
            name: "Raid night".into(),
            depth: 1,
            members: vec![member(4, "Ines", false, false)],
            members_visible: true,
            join_access: client::JoinAccess::Allowed,
            ..client::Channel::default()
        },
        client::Channel {
            id: 3,
            name: "Focus room".into(),
            has_password: true,
            members_visible: false,
            join_access: client::JoinAccess::Unknown,
            ..client::Channel::default()
        },
        client::Channel {
            id: 4,
            name: "AFK".into(),
            members_visible: true,
            join_access: client::JoinAccess::Denied,
            ..client::Channel::default()
        },
    ];
    s.state.channels = channels;
    s.state.current = Some(lobby);
    s.talking = [2].into();
    s.server_controls = Some(crate::command_flow::AudioState {
        muted: false,
        deafened: false,
    });
    s.controls_pending = false;
    let now = Instant::now();
    for i in 0..60u64 {
        let wave = (i as f64 * 0.35).sin();
        s.connection_history.push(
            now - Duration::from_secs(60 - i),
            usage::ConnectionUsage {
                packet_loss_in: Some((0.4 + 0.4 * wave).max(0.0) as f32),
                packet_loss_out: Some(if (40..44).contains(&i) { 2.5 } else { 0.1 }),
                ping_ms: Some(32.0 + 6.0 * wave),
                ping_deviation_ms: Some(4.0 + 2.0 * wave.abs()),
                idle_seconds: Some(i % 20),
                ..Default::default()
            },
        );
    }
    s.connection_usage = usage::ConnectionUsage {
        packet_loss_in: Some(0.3),
        packet_loss_out: Some(0.1),
        ping_ms: Some(31.0),
        ping_deviation_ms: Some(4.0),
        idle_seconds: Some(3),
        ..Default::default()
    };
    s.streaming.catalog = vec![crate::streaming::StreamInfo {
        id: "s1".into(),
        publisher: 2,
        name: "Mara’s screen".into(),
        viewers: 2,
        has_audio: true,
        server_relayed: false,
    }];
}

/// The hidden footer under load: CPU red, footprint amber.
fn footer_hidden(s: &mut App) {
    connected(s);
    s.footer_hidden = true;
    s.process_usage.cpu_percent = Some(64.0);
    s.process_usage.memory_bytes = Some(812 * 1024 * 1024);
    s.process_usage.memory_detail_bytes = Some(590 * 1024 * 1024);
}

fn statistics(s: &mut App) {
    connected(s);
    s.tab = Tab::Statistics;
    let now = Instant::now();
    s.cpu_history = usage::MetricHistory::new(now);
    s.ram_history = usage::MetricHistory::new(now);
    for i in 0..300u64 {
        let at = now - Duration::from_secs(300 - i);
        let x = i as f64;
        s.cpu_history.push(
            at,
            Some(2.0 + 1.5 * (x * 0.07).sin() + if i % 47 == 0 { 4.0 } else { 0.0 }),
        );
        s.ram_history
            .push(at, Some(92e6 + 4e6 * (x * 0.02).sin() + x * 2e4));
    }
    s.process_usage.cpu_percent = Some(2.4);
}

fn settings_page(s: &mut App) {
    saved(s);
    s.tab = Tab::Settings;
}

/// Settings with a mix of answers, to show the same live status outside setup.
fn settings_permissions(s: &mut App) {
    settings_page(s);
    s.permissions.status = [Status::Granted, Status::Denied, Status::NotDetermined];
}

fn performance(s: &mut App) {
    connected(s);
    s.performance_mode
        .store(true, std::sync::atomic::Ordering::Relaxed);
}

fn password(s: &mut App) {
    connected(s);
    s.password_channel = Some(3);
}

fn alert(s: &mut App) {
    home(s);
    s.alerts.raise(
        Source::Connection,
        "The server did not answer within 10 seconds. Check the address and your network, then try again.",
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn runtime(scene: fn(&mut App), size: (f32, f32)) -> Runtime<App> {
        let mut app = App::with_devices(crate::audio::Devices::default(), None);
        scene(&mut app);
        let mut rt = Runtime::new(app, super::super::view, size, Env::new(true, 2.0));
        rt.on_key = Some(super::super::on_key);
        rt.frame(0.0);
        rt
    }

    #[test]
    fn every_scene_builds_at_the_normal_and_compact_minimum_sizes() {
        for (name, scene) in SCENES {
            for size in [
                crate::NORMAL_MIN_SIZE,
                crate::COMPACT_MIN_SIZE,
                (1400.0, 900.0),
            ] {
                let mut rt = runtime(*scene, size);
                for _ in 0..30 {
                    rt.frame(1.0 / 30.0);
                }
                assert!(
                    rt.find("content").is_some(),
                    "{name} at {size:?} has no content"
                );
            }
        }
    }

    #[test]
    fn header_buttons_switch_pages_and_escape_returns_home() {
        let mut rt = runtime(home, (900.0, 790.0));
        let (x, y) = rt.find("Statistics").expect("statistics button");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..20 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.state.tab, Tab::Statistics);
        rt.key(rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        });
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.tab, Tab::Home);
    }

    #[test]
    fn typing_in_a_locked_field_is_reverted_while_connected() {
        let mut rt = runtime(settings_page, (900.0, 790.0));
        rt.state.session = Some(tokio::sync::mpsc::channel(1).0);
        rt.state.ui.address.replace_text("changed.example");
        let message = crate::Message::Address("changed.example".into());
        let _ = rt.state.update(message);
        rt.state.sync_fields();
        assert_eq!(rt.state.address, "ts.zirkumflex.io");
        assert_eq!(rt.state.ui.address.text, "ts.zirkumflex.io");
    }

    /// The connected scene with enough streams to overflow the pane, whether or not this
    /// platform shows the share card.
    fn crowded_streams(s: &mut App) {
        connected(s);
        let base = s.streaming.catalog[0].clone();
        for id in 0..12 {
            s.streaming.catalog.push(crate::streaming::StreamInfo {
                id: format!("extra-{id}"),
                name: format!("Screen {id}"),
                ..base.clone()
            });
        }
    }

    /// Every scrolling page must move its content under the wheel at a small window size.
    #[test]
    fn long_pages_scroll_under_the_wheel() {
        type Case = (&'static str, fn(&mut App), &'static str, &'static str);
        let cases: [Case; 5] = [
            ("settings", settings_page, "settings-scroll", "save"),
            (
                "statistics",
                statistics,
                "statistics-scroll",
                "record-trace",
            ),
            (
                "streams",
                crowded_streams,
                "streams-scroll",
                "refresh-streams",
            ),
            ("channels", connected, "channel-scroll", "channels"),
            ("setup", devices, "setup-scroll", "refresh-devices"),
        ];
        for (name, scene, viewport, probe) in cases {
            let mut rt = runtime(scene, (760.0, 420.0));
            if name == "channels" {
                // Enough channels to overflow the sidebar.
                let base = rt.state.state.channels[0].clone();
                for id in 10..40 {
                    rt.state.state.channels.push(client::Channel {
                        id,
                        name: format!("Room {id}"),
                        members: Vec::new(),
                        ..base.clone()
                    });
                }
            }
            for _ in 0..10 {
                rt.frame(1.0 / 30.0);
            }
            let before = rt
                .presented(probe)
                .unwrap_or_else(|| panic!("{name}: no {probe}"))
                .0[1];
            let (x, y) = rt
                .find(viewport)
                .unwrap_or_else(|| panic!("{name}: no {viewport}"));
            for _ in 0..6 {
                rt.wheel(x, y, -120.0);
                rt.frame(1.0 / 60.0);
            }
            for _ in 0..30 {
                rt.frame(1.0 / 60.0);
            }
            let after = rt.presented(probe).unwrap().0[1];
            assert!(
                (after - before).abs() > 40.0,
                "{name}: {probe} stayed at y {before} → {after}"
            );
        }
    }
}
