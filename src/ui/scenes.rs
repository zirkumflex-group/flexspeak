//! Named states for headless previews: `teamspeak-native --snap out.png --scene connected`.
//! They change only what the view shows; no session, device or file is touched.

use std::time::{Duration, Instant};

use crate::permissions::{Kind, Status};
use crate::{
    App, SettingsPage, Tab,
    alerts::Source,
    audio_processing, client, onboarding, settings, transmission,
    updater::{self, Build, Phase, Progress, Retry},
    usage,
};

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
    ("settings-appearance", settings_appearance),
    ("settings-audio", settings_audio),
    ("settings-picker", settings_picker),
    ("server-add", server_add),
    ("server-edit", server_edit),
    ("home-connecting", home_connecting),
    ("settings-shortcuts", settings_shortcuts),
    ("settings-permissions", settings_permissions),
    ("settings-file", settings_file),
    ("settings-search", settings_search),
    ("settings-empty-search", settings_empty_search),
    ("settings-connected", settings_connected),
    ("settings-light", settings_light),
    ("themes", themes),
    ("connected-light", connected_light),
    ("audio-processing", audio_processing_page),
    ("push-to-talk", push_to_talk_page),
    ("push-to-talk-connected", push_to_talk_connected),
    ("member-menu", member_menu),
    ("member-selection", member_selection),
    ("member-drag", member_drag),
    ("poke", poke),
    ("poked", poked),
    ("share", share),
    ("sharing", sharing),
    ("watching", watching),
    ("streams-live", streams_live),
    ("sharing-watching", sharing_watching),
    ("watching-five", watching_five),
    ("watching-five-below", watching_five_below),
    ("watching-five-beside", watching_five_beside),
    ("watching-five-drag", watching_five_drag),
    ("watching-five-beside-drag", watching_five_beside_drag),
    ("watching-grid", watching_grid),
    ("watching-focus", watching_focus),
    ("watching-stats", watching_stats),
    ("install", install),
    ("password", password),
    ("alert", alert),
    ("update-available", update_available),
    ("update-downloading", update_downloading),
    ("update-ready", update_ready),
    ("update-installing", update_installing),
    ("update-download-failed", update_download_failed),
    ("update-install-failed", update_install_failed),
    ("update-settings", update_settings),
    ("update-settings-downloading", update_settings_downloading),
    ("update-check-failed", update_check_failed),
];

/// Scenes keep a fixed scheme, so previews don't depend on the machine's appearance.
fn dark(s: &mut App) {
    s.appearance = settings::Appearance {
        scheme: settings::ColorScheme::Dark,
        ..Default::default()
    };
}

fn saved(s: &mut App) {
    dark(s);
    s.onboarding = None;
    let servers = vec![
        settings::Server {
            name: "Zirkumflex".into(),
            address: "ts.zirkumflex.io".into(),
        },
        settings::Server {
            name: "Friday Squad".into(),
            address: "voice.friday.gg:9988".into(),
        },
        settings::Server::new("ts3.example.org"),
    ];
    s.saved = Some(settings::Settings {
        servers: servers.clone(),
        nickname: "Jan".into(),
        ..settings::Settings::default()
    });
    s.servers = servers;
    s.address = "ts.zirkumflex.io".into();
    s.nickname = "Jan".into();
    s.alerts = Default::default();
    s.sync_fields();
}

fn setup(s: &mut App) {
    dark(s);
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
        can_poke: None,
        can_move: (!is_self).then_some(id != 3),
    }
}

fn connected(s: &mut App) {
    saved(s);
    s.connected = true;
    s.muted = false;
    s.level = 0.3;
    s.transmitting = true;
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
    settings_at(s, SettingsPage::Server);
}

fn settings_at(s: &mut App, page: SettingsPage) {
    saved(s);
    s.settings_path = Some("/Users/jan/Library/Application Support/Flexspeak/settings.json".into());
    s.open_settings(page);
}

fn settings_appearance(s: &mut App) {
    settings_at(s, SettingsPage::Appearance);
}

fn settings_audio(s: &mut App) {
    settings_at(s, SettingsPage::Audio);
}

/// Adding a server, from the home card's + or Settings.
fn server_add(s: &mut App) {
    settings_page(s);
    let _ = s.update(crate::Message::AddServer);
    let _ = s.update(crate::Message::ServerAddress("eu.voicehub.net".into()));
    let _ = s.update(crate::Message::ServerName("Voice Hub EU".into()));
    s.sync_fields();
}

fn server_edit(s: &mut App) {
    settings_page(s);
    let _ = s.update(crate::Message::EditServer(1));
    s.sync_fields();
}

/// Connecting to the second server: the others wait for it.
fn home_connecting(s: &mut App) {
    home(s);
    s.address = "voice.friday.gg:9988".into();
    s.session = Some(tokio::sync::mpsc::channel(1).0);
    s.status = "Connecting to voice.friday.gg:9988…".into();
}

/// The microphone menu open inside the window: it draws above the panel.
fn settings_picker(s: &mut App) {
    settings_audio(s);
    s.ui.open = Some("input-device");
}

fn settings_shortcuts(s: &mut App) {
    settings_at(s, SettingsPage::Shortcuts);
    s.keybind_draft.toggle_mute = "Ctrl+Shift+M".into();
    s.keybind_draft.toggle_deafen = "Ctrl+Shift+D".into();
    s.sync_fields();
}

/// A mix of answers, to show the same live status outside setup.
fn settings_permissions(s: &mut App) {
    settings_at(s, SettingsPage::Permissions);
    s.permissions.status = [Status::Granted, Status::Denied, Status::NotDetermined];
}

fn settings_file(s: &mut App) {
    settings_at(s, SettingsPage::File);
    s.settings_message = Some("Settings saved".into());
}

/// A query that finds rows in several categories.
fn settings_search(s: &mut App) {
    settings_at(s, SettingsPage::Server);
    s.settings_query = "mic".into();
    s.sync_fields();
}

fn settings_empty_search(s: &mut App) {
    settings_at(s, SettingsPage::Server);
    s.settings_query = "bluetooth".into();
    s.sync_fields();
}

/// Settings over the connected server, with the server fields locked.
fn settings_connected(s: &mut App) {
    connected(s);
    // A session locks the server fields; the dropped receiver is never read in a preview.
    s.session = Some(tokio::sync::mpsc::channel(1).0);
    s.settings_path = Some("/Users/jan/Library/Application Support/Flexspeak/settings.json".into());
    s.open_settings(SettingsPage::Server);
}

fn settings_light(s: &mut App) {
    settings_audio(s);
    s.appearance.scheme = settings::ColorScheme::Light;
}

/// Different themes for light and dark, in dark.
fn themes(s: &mut App) {
    settings_appearance(s);
    s.appearance.light_theme = settings::ThemeName::Grove;
    s.appearance.dark_theme = settings::ThemeName::Ember;
}

fn connected_light(s: &mut App) {
    connected(s);
    s.appearance.scheme = settings::ColorScheme::Light;
    s.appearance.light_theme = settings::ThemeName::Ocean;
}

/// Settings with every microphone effect on and voice activation chosen.
fn audio_processing_page(s: &mut App) {
    settings_at(s, SettingsPage::Audio);
    s.audio_processing = audio_processing::Settings {
        noise_suppression: true,
        suppression_level: audio_processing::SuppressionLevel::High,
        automatic_gain: true,
        echo_cancellation: true,
        typing_suppression: true,
    };
    s.transmission = transmission::Settings {
        mode: transmission::Mode::VoiceActivation,
        voice_threshold_db: -45,
        ..transmission::Settings::default()
    };
}

/// Settings with push-to-talk chosen and its key set.
fn push_to_talk_page(s: &mut App) {
    settings_at(s, SettingsPage::Audio);
    s.transmission = transmission::Settings {
        mode: transmission::Mode::PushToTalk,
        ptt_release_ms: 200,
        ..transmission::Settings::default()
    };
    s.keybinds.push_to_talk = "Ctrl+Shift+Space".into();
    s.keybind_draft.push_to_talk = "Ctrl+Shift+Space".into();
    s.sync_fields();
}

/// Connected in push-to-talk with the key up: the voice panel waits instead of lighting up.
fn push_to_talk_connected(s: &mut App) {
    connected(s);
    s.transmission = transmission::Settings {
        mode: transmission::Mode::PushToTalk,
        ..transmission::Settings::default()
    };
    s.transmitting = false;
    s.talking = [2].into();
}

fn member_menu(s: &mut App) {
    connected(s);
    s.ui.member_menu = Some((2, rune::PointerPress { x: 150.0, y: 200.0 }));
}

fn member_selection(s: &mut App) {
    connected(s);
    s.selected_members = [2, 4].into();
    s.ui.member_menu = Some((4, rune::PointerPress { x: 150.0, y: 240.0 }));
    s.ui.move_menu = true;
}

fn member_drag(s: &mut App) {
    connected(s);
    s.selected_members = [2, 4].into();
    s.ui.drag = Some(super::MemberDrag {
        ids: vec![2, 4],
        at: (150.0, 250.0),
        target: Some(4),
    });
}

fn poke(s: &mut App) {
    connected(s);
    s.poke = Some(crate::PokeDraft {
        id: 2,
        name: "Mara".into(),
        message: "Raid starts in 5".into(),
    });
    s.sync_fields();
}

fn poked(s: &mut App) {
    connected(s);
    s.pokes.push(crate::Poke {
        from: "Theo".into(),
        message: "Can you hear me?".into(),
    });
}

fn share_sources(s: &mut App) {
    use crate::streaming::publishing::{ShareSource, SourceKind};
    s.streaming.sharing.sources = vec![
        ShareSource {
            kind: SourceKind::Screen(Some(1)),
            name: "Built-in Retina Display".into(),
        },
        ShareSource {
            kind: SourceKind::Window(2),
            name: "Code — flexspeak".into(),
        },
        ShareSource {
            kind: SourceKind::Camera("demo-camera".into()),
            name: "Camera · Built-in camera".into(),
        },
    ];
    s.streaming.sharing.selected = s.streaming.sharing.sources.first().cloned();
}

fn share(s: &mut App) {
    connected(s);
    share_sources(s);
    s.streaming.share_open = true;
}

fn sharing(s: &mut App) {
    connected(s);
    share_sources(s);
    let status = &mut s.streaming.sharing.status;
    status.active = true;
    status.live = true;
    status.detail = "Sharing Built-in Retina Display · 720p · 30 fps".into();
    status.viewers = vec![(2, "Mara".into(), true)];
    status.requests = vec![(4, "Ines".into())];
}

/// Streams watched without frames yet: what a fresh view looks like before video arrives.
fn watch(s: &mut App, streams: &[(&str, u16, &str, crate::streaming::Phase)]) {
    connected(s);
    for (slot, (id, publisher, name, phase)) in streams.iter().enumerate() {
        let info = crate::streaming::StreamInfo {
            id: (*id).into(),
            publisher: *publisher,
            name: (*name).into(),
            viewers: 2,
            has_audio: true,
            server_relayed: false,
        };
        let surface = s.streaming.surfaces.get(slot).clone();
        let generation = surface.begin();
        s.streaming.update(crate::streaming::Event::State {
            generation,
            slot,
            stream: info,
            phase: *phase,
            detail: "Receiving stream · audio follows playback volume and deafen".into(),
        });
        if *phase == crate::streaming::Phase::Receiving {
            surface.publish(generation, demo_picture(slot), 0.0);
        }
    }
    s.streaming.sync_frame();
}

/// A stand-in for decoded video: each stream its own colour, and the fourth a 4:3 source.
fn demo_picture(slot: usize) -> crate::streaming::Image {
    const COLOURS: [[u8; 3]; 5] = [
        [46, 96, 168],
        [168, 72, 60],
        [52, 130, 96],
        [132, 92, 170],
        [176, 132, 48],
    ];
    let [r, g, b] = COLOURS[slot % COLOURS.len()];
    let (width, height) = if slot == 3 { (320, 240) } else { (480, 270) };
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let stripe = ((x + y) / 24) % 2 == 0;
            let shade = 150 + 105 * y / height;
            let tint = if stripe { shade } else { shade * 4 / 5 };
            pixels.extend([r, g, b].map(|c| (u32::from(c) * tint / 255) as u8));
            pixels.push(255);
        }
    }
    crate::streaming::Image {
        width,
        height,
        pixels,
    }
}

fn watching_five(s: &mut App) {
    use crate::streaming::Phase;
    watch(
        s,
        &[
            ("s1", 2, "Mara’s screen", Phase::Receiving),
            ("s2", 3, "Theo’s game", Phase::Receiving),
            ("s3", 5, "Kai’s window", Phase::Receiving),
            ("s4", 6, "Noor’s old capture", Phase::Receiving),
            ("s5", 7, "Lin’s second monitor", Phase::Negotiating),
        ],
    );
    let lobby = s.state.current.as_mut().expect("the demo joins the lobby");
    lobby.members.extend([
        member(5, "Kai", false, false),
        member(6, "Noor", false, false),
        member(7, "Lin", false, false),
    ]);
    s.state.channels[0] = lobby.clone();
}

fn watching_five_below(s: &mut App) {
    watching_five(s);
    s.streaming.focused = Some("s2".into());
}

fn watching_five_beside(s: &mut App) {
    watching_five_below(s);
    s.streaming.strip = crate::streaming::Strip::Beside;
}

fn watching_five_drag(s: &mut App) {
    watching_five(s);
    s.ui.tile_drag = Some(super::TileDrag {
        id: "s1".into(),
        at: (620.0, 330.0),
        target: Some("s2".into()),
    });
}

fn watching_five_beside_drag(s: &mut App) {
    watching_five_beside(s);
    s.ui.tile_drag = Some(super::TileDrag {
        id: "s3".into(),
        at: (420.0, 330.0),
        target: Some("s2".into()),
    });
}

fn watching(s: &mut App) {
    use crate::streaming::Phase;
    watch(s, &[("s1", 2, "Mara’s screen", Phase::Receiving)]);
    s.streaming.catalog.push(crate::streaming::StreamInfo {
        id: "s2".into(),
        publisher: 4,
        name: "Ines’s game".into(),
        viewers: 1,
        has_audio: false,
        server_relayed: false,
    });
}

/// Streams running in the channel that you haven't joined.
fn streams_live(s: &mut App) {
    connected(s);
    for (id, publisher, name, viewers, has_audio) in [
        ("s2", 4, "Ines’s game", 1, false),
        ("s3", 3, "Theo’s window", 0, true),
    ] {
        s.streaming.catalog.push(crate::streaming::StreamInfo {
            id: id.into(),
            publisher,
            name: name.into(),
            viewers,
            has_audio,
            server_relayed: false,
        });
    }
}

/// Your own share running while you watch someone else's.
fn sharing_watching(s: &mut App) {
    watching(s);
    share_sources(s);
    let status = &mut s.streaming.sharing.status;
    status.active = true;
    status.live = true;
    status.detail = "Sharing Built-in Retina Display · 720p · 30 fps".into();
    status.viewers = vec![(2, "Mara".into(), true)];
}

fn watching_grid(s: &mut App) {
    use crate::streaming::Phase;
    watch(
        s,
        &[
            ("s1", 2, "Mara’s screen", Phase::Receiving),
            ("s2", 4, "Ines’s game", Phase::Negotiating),
            ("s3", 3, "Theo’s window", Phase::Requesting),
        ],
    );
}

fn watching_focus(s: &mut App) {
    watching_grid(s);
    s.streaming.focused = Some("s2".into());
    s.streaming.set_volume(4, 40);
}

fn watching_stats(s: &mut App) {
    watching(s);
    s.streaming.show_stats = true;
    if let Some(watch) = s.streaming.watching.first_mut() {
        watch.stats = crate::streaming::Stats {
            codec: "VP8".into(),
            width: 1280,
            height: 720,
            fps: 29.8,
            packets: 18_204,
            decoded_frames: 1_790,
            bytes: 21_000_000,
            decoder: "libvpx".into(),
            ice_path: "host → srflx (UDP)".into(),
            ..Default::default()
        };
    }
}

fn install(s: &mut App) {
    home(s);
    s.install_offer = Some(crate::install::Offer {
        from: "/Users/jan/Downloads/Flexspeak.app".into(),
        to: "/Applications/Flexspeak.app".into(),
    });
    s.install_prompt = true;
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

const RUNNING: Build = Build {
    major: 0,
    minor: 1,
    patch: 0,
    number: 6,
};

/// A release build that has found build 7. Nothing is fetched; the scene only sets state.
fn with_update(s: &mut App, phase: Phase) {
    let mut u = updater::Updater::new(Some(RUNNING), Ok(updater::Package::MacApp), true);
    u.release = Some(updater::Release {
        tag: "v0.1.0-build.7".into(),
        build: Build {
            number: 7,
            ..RUNNING
        },
        asset: updater::Asset {
            name: updater::MACOS_DMG.into(),
            url: String::new(),
            size: 25_237_970,
            digest: None,
        },
        checksums_url: String::new(),
    });
    u.phase = phase;
    s.updater = u;
}

fn downloading() -> Phase {
    Phase::Downloading(Progress {
        received: 12_118_000,
        total: Some(25_237_970),
        bytes_per_second: Some(3_420_000.0),
    })
}

fn update_available(s: &mut App) {
    home(s);
    with_update(s, Phase::Available);
}

fn update_downloading(s: &mut App) {
    connected(s);
    with_update(s, downloading());
}

fn update_ready(s: &mut App) {
    connected(s);
    with_update(s, Phase::Ready);
}

fn update_installing(s: &mut App) {
    connected(s);
    with_update(s, Phase::Installing);
}

fn update_download_failed(s: &mut App) {
    home(s);
    with_update(
        s,
        Phase::Failed {
            retry: Retry::Download,
            detail: "The download ended after 12118000 of 25237970 bytes.".into(),
        },
    );
}

fn update_install_failed(s: &mut App) {
    home(s);
    with_update(
        s,
        Phase::Failed {
            retry: Retry::Install,
            detail: "Cannot update /Applications/Flexspeak.app: Permission denied (os error 13). Check that you can write to /Applications.".into(),
        },
    );
}

/// Settings with a downloaded build waiting and the notice closed.
fn update_settings(s: &mut App) {
    settings_at(s, SettingsPage::Updates);
    with_update(s, Phase::Ready);
    s.updater.dismissed = true;
}

fn update_settings_downloading(s: &mut App) {
    settings_at(s, SettingsPage::Updates);
    with_update(s, downloading());
    s.updater.auto_download = false;
    s.updater.dismissed = true;
}

/// A failed check stays on the Settings page; nothing pops up.
fn update_check_failed(s: &mut App) {
    settings_at(s, SettingsPage::Updates);
    s.updater = updater::Updater::new(Some(RUNNING), Ok(updater::Package::MacApp), false);
    s.updater.check_error = Some("error sending request for url (https://api.github.com/repos/zirkumflex-group/flexspeak/releases/latest): dns error".into());
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
    fn every_scene_builds_at_the_minimum_and_a_large_size() {
        for (name, scene) in SCENES {
            for size in [crate::MIN_SIZE, (1400.0, 900.0)] {
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
    fn sharing_dialog_fits_small_windows_and_keeps_actions_visible_while_scrolling() {
        let mut rt = runtime(share, crate::MIN_SIZE);
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        let panel = rt.presented("share-dialog").unwrap().0;
        assert!(
            panel[1] >= 0.0 && panel[1] + panel[3] <= crate::MIN_SIZE.1,
            "Share dialog must fit the window: {panel:?}"
        );
        let before = rt.presented("start-sharing").unwrap().0;
        assert!(before[1] >= 0.0 && before[1] + before[3] <= crate::MIN_SIZE.1);
        let audio_before = rt.presented("share-audio-bitrate").unwrap().0[1];
        let (x, y) = rt.find("share-scroll").unwrap();
        for _ in 0..8 {
            rt.wheel(x, y, -120.0);
            rt.frame(1.0 / 60.0);
        }
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        let audio_after = rt.presented("share-audio-bitrate").unwrap().0[1];
        assert!(
            audio_after < audio_before - 40.0,
            "The advanced controls must scroll into view"
        );
        assert_eq!(
            rt.presented("start-sharing").unwrap().0,
            before,
            "Go Live stays fixed"
        );
    }

    #[test]
    fn share_quality_controls_are_independent_validate_and_lock_while_live() {
        use crate::Message;
        use crate::streaming::publishing::{Access, Preset, Resolution};
        let mut rt = runtime(share, (900.0, 790.0));
        let s = &mut rt.state;
        let _ = s.update(Message::ShareFrameRate(24));
        let _ = s.update(Message::ShareResolution(Resolution::P1440));
        let _ = s.update(Message::ShareVideoBitrate(8000));
        let _ = s.update(Message::ShareAudioBitrate(192));
        let _ = s.update(Message::ShareAccess(Access::Public));
        let quality = s.streaming.sharing.quality;
        assert_eq!(
            (quality.frame_rate, quality.video_kbps, quality.audio_kbps),
            (24, 8000, 192)
        );
        assert_eq!(quality.resolution, Resolution::P1440);
        assert!(quality.preset().is_none());
        let _ = s.update(Message::ShareFrameRate(0));
        assert_eq!(s.streaming.sharing.quality, quality);
        let camera = s.streaming.sharing.sources.last().unwrap().clone();
        let _ = s.update(Message::ShareSource(camera.clone()));
        assert!(
            !s.streaming.sharing.audio,
            "Camera selection must not unexpectedly share system sound"
        );
        s.streaming.sharing.status.active = true;
        let _ = s.update(Message::ShareQuality(Preset::Saver.quality()));
        let _ = s.update(Message::ShareAccess(Access::Private));
        let _ = s.update(Message::ShareSource(s.streaming.sharing.sources[0].clone()));
        assert_eq!(s.streaming.sharing.quality, quality);
        assert_eq!(s.streaming.sharing.access, Access::Public);
        assert_eq!(s.streaming.sharing.selected, Some(camera));
    }

    fn settle(rt: &mut Runtime<App>) {
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
    }

    fn click(rt: &mut Runtime<App>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(rt);
    }

    fn drag(rt: &mut Runtime<App>, from: &str, to: &str) {
        let (x, y) = rt.find(from).unwrap_or_else(|| panic!("no {from}"));
        let (to_x, to_y) = rt.find(to).unwrap_or_else(|| panic!("no {to}"));
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        rt.pointer_down(x, y);
        rt.pointer_move((x + to_x) / 2.0, (y + to_y) / 2.0);
        rt.frame(1.0 / 60.0);
        rt.pointer_move(to_x, to_y);
        rt.frame(1.0 / 60.0);
        rt.frame(1.0 / 60.0);
        assert_eq!(
            rt.state
                .ui
                .tile_drag
                .as_ref()
                .and_then(|d| d.target.clone()),
            Some(to.trim_start_matches("tile-").to_owned()),
            "dragging {from} over {to} must offer that drop"
        );
        rt.pointer_up(to_x, to_y);
        settle(rt);
        assert!(rt.state.ui.tile_drag.is_none());
    }

    fn order(rt: &Runtime<App>) -> Vec<&str> {
        let watching = &rt.state.streaming.watching;
        watching.iter().map(|w| w.stream.id.as_str()).collect()
    }

    #[test]
    fn five_streams_arrange_themselves_to_the_window() {
        let row_of = |rt: &Runtime<App>, id: &str| rt.find(&format!("tile-{id}")).unwrap().1;
        let mut wide = runtime(watching_five, (1400.0, 900.0));
        settle(&mut wide);
        assert_eq!(row_of(&wide, "s1"), row_of(&wide, "s3"), "three across");
        let mut normal = runtime(watching_five, (900.0, 790.0));
        settle(&mut normal);
        assert_eq!(row_of(&normal, "s1"), row_of(&normal, "s2"), "two across");
        assert!(row_of(&normal, "s3") > row_of(&normal, "s1"));
        assert!(normal.find("streams-scroll").is_none(), "all five fit");
        let mut small = runtime(watching_five, crate::MIN_SIZE);
        settle(&mut small);
        assert!(small.find("streams-scroll").is_some(), "too small to fit");
    }

    #[test]
    fn the_mouse_switches_layouts_focuses_and_rearranges_streams() {
        let mut rt = runtime(watching_five, (900.0, 790.0));
        settle(&mut rt);

        drag(&mut rt, "tile-s1", "tile-s2");
        assert_eq!(order(&rt), ["s2", "s1", "s3", "s4", "s5"]);

        click(&mut rt, "tile-s3");
        assert_eq!(rt.state.streaming.focused.as_deref(), Some("s3"));
        assert_eq!(rt.state.streaming.strip, crate::streaming::Strip::Below);

        click(&mut rt, "layout-beside");
        assert_eq!(rt.state.streaming.strip, crate::streaming::Strip::Beside);
        assert_eq!(rt.state.streaming.focused.as_deref(), Some("s3"));

        // A thumbnail dropped on the stage takes it and hands over its place in the strip.
        drag(&mut rt, "tile-s4", "tile-s3");
        assert_eq!(rt.state.streaming.focused.as_deref(), Some("s4"));
        assert_eq!(order(&rt), ["s2", "s1", "s4", "s3", "s5"]);

        // Double-clicking the stage returns to the grid, as the grid button does.
        let (x, y) = rt.find("tile-s4").unwrap();
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 60.0);
        }
        settle(&mut rt);
        assert_eq!(rt.state.streaming.focused, None);

        click(&mut rt, "layout-below");
        assert_eq!(rt.state.streaming.focused.as_deref(), Some("s2"));
        click(&mut rt, "layout-grid");
        assert_eq!(rt.state.streaming.focused, None);

        click(&mut rt, "focus");
        assert!(rt.state.streaming.focused.is_some(), "the maximize button");
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

    fn press(rt: &mut Runtime<App>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        for _ in 0..20 {
            rt.frame(1.0 / 60.0);
        }
    }

    fn key(rt: &mut Runtime<App>, key: rune::Key, cmd: bool) {
        rt.key(rune::KeyEvent {
            key,
            mods: rune::Mods {
                cmd,
                ..Default::default()
            },
        });
        for _ in 0..20 {
            rt.frame(1.0 / 60.0);
        }
    }

    #[test]
    fn settings_opens_over_home_switches_categories_and_closes() {
        let mut rt = runtime(home, (900.0, 790.0));
        press(&mut rt, "Settings");
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Server));
        assert_eq!(rt.state.tab, Tab::Home, "Settings floats over the page");
        assert!(rt.find("settings-window").is_some());
        press(&mut rt, "nav-Audio");
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Audio));
        assert!(rt.find("refresh-devices").is_some());
        press(&mut rt, "close-settings");
        assert_eq!(rt.state.settings_page, None);
        // It reopens on the category it was left on, from ⌘, too.
        key(&mut rt, rune::Key::Command(','), true);
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Audio));
        key(&mut rt, rune::Key::Escape, false);
        assert_eq!(rt.state.settings_page, None);
        assert_eq!(rt.state.tab, Tab::Home);
    }

    #[test]
    fn settings_closes_when_pressed_outside_the_panel() {
        let mut rt = runtime(settings_page, (1200.0, 900.0));
        for _ in 0..20 {
            rt.frame(1.0 / 60.0);
        }
        rt.pointer_down(6.0, 450.0);
        rt.pointer_up(6.0, 450.0);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.settings_page, None);
    }

    #[test]
    fn search_narrows_to_matching_rows_and_escape_clears_it_before_closing() {
        let mut rt = runtime(settings_page, (900.0, 790.0));
        press(&mut rt, "nav-Shortcuts");
        // `/` jumps to the search field from anywhere in the window.
        key(&mut rt, rune::Key::Text("/".into()), false);
        assert!(rt.state.ui.settings_search.focused);
        for c in ["d", "e", "a", "f"] {
            key(&mut rt, rune::Key::Text(c.into()), false);
        }
        assert_eq!(rt.state.settings_query, "deaf");
        assert!(rt.find("results").is_some());
        assert!(
            rt.find("shortcuts").is_some(),
            "the deafen shortcut matches"
        );
        assert!(rt.find("connection").is_none(), "server rows don't");
        key(&mut rt, rune::Key::Escape, false);
        assert_eq!(rt.state.settings_query, "");
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Shortcuts));
        key(&mut rt, rune::Key::Escape, false);
        assert_eq!(rt.state.settings_page, None);
    }

    #[test]
    fn a_search_finds_every_category_by_name_and_says_when_nothing_matches() {
        let mut rt = runtime(settings_page, (900.0, 790.0));
        let search = |rt: &mut Runtime<App>, query: &str| {
            key(rt, rune::Key::Escape, false);
            if rt.state.settings_page.is_none() {
                key(rt, rune::Key::Command(','), true);
            }
            if !rt.state.ui.settings_search.focused {
                key(rt, rune::Key::Text("/".into()), false);
            }
            key(rt, rune::Key::Text(query.into()), false);
            assert_eq!(rt.state.settings_query, query);
        };
        search(&mut rt, "ember");
        assert!(rt.find("themes").is_some());
        assert!(rt.find("Ember-dark").is_some());
        search(&mut rt, "permissions");
        assert!(rt.find("permission-0").is_some());
        search(&mut rt, "bluetooth");
        assert!(rt.find("no-results").is_some());
    }

    #[test]
    fn escape_closes_the_server_dialog_before_the_settings_window() {
        let mut rt = runtime(server_edit, (900.0, 790.0));
        assert!(rt.find("server-editor-dialog").is_some());
        key(&mut rt, rune::Key::Escape, false);
        assert!(rt.state.server_editor.is_none());
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Server));
        press(&mut rt, "edit-server-2");
        assert_eq!(rt.state.server_editor.as_ref().unwrap().index, Some(2));
    }

    #[test]
    fn each_home_row_connects_to_its_own_server() {
        let mut rt = runtime(home, (900.0, 790.0));
        assert!(rt.find("server-2").is_some());
        // The card fades in before it takes presses.
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
        // The + opens Settings on the server list with a new server.
        press(&mut rt, "add-server");
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Server));
        assert_eq!(rt.state.server_editor.as_ref().unwrap().index, None);
        key(&mut rt, rune::Key::Escape, false);
        key(&mut rt, rune::Key::Escape, false);
        assert_eq!(rt.state.settings_page, None);
        press(&mut rt, "connect-1");
        assert_eq!(rt.state.address, "voice.friday.gg:9988");
    }

    #[test]
    fn settings_stays_closed_during_setup() {
        let mut app = App::with_devices(crate::audio::Devices::default(), None);
        setup(&mut app);
        let _ = app.update(crate::Message::OpenSettings(SettingsPage::Audio));
        assert_eq!(app.settings_page, None);
        home(&mut app);
        let _ = app.update(crate::Message::OpenSettings(SettingsPage::Audio));
        assert_eq!(app.settings_page, Some(SettingsPage::Audio));
    }

    #[test]
    fn theme_swatches_choose_a_theme_for_one_mode_and_scheme_cards_switch_modes() {
        let mut rt = runtime(settings_appearance, (900.0, 790.0));
        // Nothing to store to: the choices apply without a settings file to write.
        rt.state.saved = None;
        rt.frame(1.0 / 60.0);
        let press = |rt: &mut Runtime<App>, label: &str| {
            let (x, y) = rt.find(label).unwrap_or_else(|| panic!("no {label}"));
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 60.0);
        };
        press(&mut rt, "Grove-dark");
        assert_eq!(rt.state.appearance.dark_theme, settings::ThemeName::Grove);
        assert_eq!(
            rt.state.appearance.light_theme,
            settings::ThemeName::Flexspeak
        );
        press(&mut rt, "Iris-light");
        assert_eq!(rt.state.appearance.light_theme, settings::ThemeName::Iris);
        press(&mut rt, "scheme-Light");
        assert_eq!(rt.state.appearance.scheme, settings::ColorScheme::Light);
        assert!(!super::super::theme(&rt.state).dark);
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

    // The switches exist only where WebRTC processing is built (Mac and Windows).
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn processing_switches_apply_while_idle_and_lock_during_a_session() {
        let mut rt = runtime(settings_audio, (900.0, 2000.0));
        for _ in 0..10 {
            rt.frame(1.0 / 30.0);
        }
        let (x, y) = rt
            .find("echo-cancellation-switch")
            .expect("echo cancellation switch");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(1.0 / 60.0);
        assert_eq!(
            rt.state.audio_processing.echo_cancellation,
            crate::audio_processing::AVAILABLE
        );
        let before = rt.state.audio_processing;
        rt.state.session = Some(tokio::sync::mpsc::channel(1).0);
        for _ in 0..10 {
            rt.frame(1.0 / 30.0);
        }
        if let Some((x, y)) = rt.find("echo-cancellation-switch") {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rt.state.audio_processing, before);
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
            (
                "settings",
                settings_audio,
                "settings-scroll",
                "refresh-devices",
            ),
            (
                "statistics",
                statistics,
                "statistics-scroll",
                "record-trace",
            ),
            ("streams", crowded_streams, "streams-scroll", "tile-s1"),
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
