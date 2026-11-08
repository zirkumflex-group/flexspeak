//! Named states for headless previews: `teamspeak-native --snap out.png --scene connected`.
//! They change only what the view shows; no session, device or file is touched.

use std::time::{Duration, Instant};

use crate::permissions::{Kind, Status};
use crate::{
    App, ServerSession, SettingsPage, Tab,
    alerts::Source,
    audio_processing, client, hub, onboarding, settings, transmission,
    updater::{self, Build, Phase, Progress, Retry},
    usage,
};

type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("setup", setup),
    ("devices", devices),
    ("servers", servers_rail),
    ("servers-poked", servers_poked),
    ("servers-switched", servers_switched),
    ("servers-connecting", servers_connecting),
    ("servers-picker", servers_picker),
    ("setup-ts6-offer", setup_ts6_offer),
    ("setup-ts6-server", setup_ts6_server),
    ("setup-ts6-devices", setup_ts6_devices),
    ("permissions-not-asked", permissions_not_asked),
    ("permissions-allowed", permissions_allowed),
    ("permissions-denied", permissions_denied),
    ("permissions-mixed", permissions_mixed),
    ("permissions-windows", permissions_windows),
    ("home", home),
    ("connected", connected),
    ("connected-quiet", connected_quiet),
    ("footer-hidden", footer_hidden),
    ("statistics", statistics),
    ("settings", settings_page),
    ("settings-appearance", settings_appearance),
    ("settings-omarchy", settings_omarchy),
    ("settings-sounds", settings_sounds),
    ("settings-sounds-pack", settings_sounds_pack),
    ("server-log", server_log),
    ("server-log-hover", server_log_hover),
    ("server-log-tall", server_log_tall),
    ("server-log-folded", server_log_folded),
    ("server-log-search", server_log_search),
    ("server-log-people", server_log_people),
    ("server-log-person", server_log_person),
    ("server-log-none", server_log_none),
    ("settings-sounds-no-teamspeak", settings_sounds_no_teamspeak),
    ("settings-more-themes", settings_more_themes),
    ("settings-audio", settings_audio),
    ("settings-picker", settings_picker),
    ("server-add", server_add),
    ("server-edit", server_edit),
    ("home-connecting", home_connecting),
    ("settings-shortcuts", settings_shortcuts),
    ("settings-shortcuts-recording", settings_shortcuts_recording),
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
    ("member-menu-restricted", member_menu_restricted),
    ("member-groups", member_groups),
    ("member-groups-selection", member_groups_selection),
    ("member-channel-group", member_channel_group),
    ("member-kick", member_kick),
    ("member-ban", member_ban),
    ("servers-menu", servers_menu),
    ("member-volume", member_volume),
    ("member-muted", member_muted),
    ("member-selection", member_selection),
    ("member-drag", member_drag),
    ("poke", poke),
    ("poked", poked),
    ("share", share),
    ("sharing", sharing),
    ("watching", watching),
    ("watching-toolbar", watching_toolbar),
    ("watching-1440p", watching_1440p),
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
    ("update-whats-new", update_whats_new),
    ("update-downloading", update_downloading),
    ("update-ready", update_ready),
    ("update-installing", update_installing),
    ("update-download-failed", update_download_failed),
    ("update-install-failed", update_install_failed),
    ("update-settings", update_settings),
    ("update-settings-changelog", update_settings_changelog),
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
    // Previews start from the default log, whatever this machine last left it at.
    s.log_pane = Default::default();
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

fn setup_ts6_offer(s: &mut App) {
    setup(s);
    s.onboarding = Some(onboarding::Step::Username);
    s.ts6_path = Some("settings.db".into());
    s.ts6 = None;
}

fn setup_ts6_server(s: &mut App) {
    setup_ts6_offer(s);
    let input = s.devices.inputs.first().map(|device| device.name.clone());
    let output = s.devices.outputs.first().map(|device| device.name.clone());
    s.ts6 = Some(crate::ts6_import::Imported {
        nickname: Some("Jan".into()),
        servers: vec![
            "ts.zirkumflex.io".into(),
            "games.example.org:1234".into(),
            "friends.example.net".into(),
        ],
        input,
        output,
        push_to_talk: true,
        ptt_release_ms: 200,
    });
    s.onboarding = Some(onboarding::Step::Server);
}

fn setup_ts6_devices(s: &mut App) {
    setup_ts6_server(s);
    s.onboarding = Some(onboarding::Step::Devices);
}

/// One more server, connected, with nothing on screen for it yet.
fn another_server(s: &mut App, name: &str, address: &str) -> hub::SessionId {
    let id = add_session(s, name, address);
    let session = s.session_mut(id).expect("just added");
    session.connected = true;
    session.state.server = name.into();
    session.state.server_version = "6.0.0".into();
    session.state.can_talk = true;
    id
}

/// Four servers at once, showing each kind of state the rail draws.
fn servers(s: &mut App) -> [hub::SessionId; 3] {
    connected(s);
    let mine = s.server_mut();
    mine.muted = false;
    mine.talking = [2].into();
    let raid = another_server(s, "Raid Night", "raid.example");
    if let Some(session) = s.session_mut(raid) {
        session.muted = true;
    }
    let work = another_server(s, "Work", "work.example");
    if let Some(session) = s.session_mut(work) {
        session.muted = true;
        session.deafened = true;
    }
    let friends = add_session(s, "Friends", "friends.example");
    if let Some(session) = s.session_mut(friends) {
        session.status = "Resolving server and connecting…".into();
    }
    [raid, work, friends]
}

fn servers_rail(s: &mut App) {
    servers(s);
}

/// Someone poked you twice on a server you aren't looking at.
fn servers_poked(s: &mut App) {
    let [_, work, _] = servers(s);
    if let Some(session) = s.session_mut(work) {
        session.unseen_pokes = 2;
    }
}

/// Switched to a server where the mic is off: the panel and shortcuts follow it.
fn servers_switched(s: &mut App) {
    let [raid, ..] = servers(s);
    s.activate(Some(raid));
    if let Some(session) = s.session_mut(raid) {
        let lobby = client::Channel {
            id: 1,
            name: "Raid night".into(),
            members: vec![
                member(1, "Jan", true, true),
                member(2, "Theo", false, false),
            ],
            members_visible: true,
            join_access: client::JoinAccess::Allowed,
            ..client::Channel::default()
        };
        session.state.channels = vec![lobby.clone()];
        session.state.current = Some(lobby);
        session.server_controls = Some(crate::command_flow::AudioState {
            muted: true,
            deafened: false,
        });
    }
}

/// A second server still connecting, chosen from the rail.
fn servers_connecting(s: &mut App) {
    let [.., friends] = servers(s);
    s.activate(Some(friends));
}

fn servers_picker(s: &mut App) {
    servers(s);
    s.ui.server_picker = true;
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
    s.server_mut().status = "Ready to connect".into();
}

fn member(id: u16, name: &str, is_self: bool, mic_muted: bool) -> client::Member {
    client::Member {
        uid: format!("uid-{id}"),
        id,
        name: name.into(),
        is_self,
        mic_muted,
        sound_muted: false,
        can_poke: None,
        can_move: (!is_self).then_some(id != 3),
        can_kick_channel: (!is_self).then_some(id != 3),
        can_kick_server: (!is_self).then_some(id != 3),
        can_ban: (!is_self).then_some(id != 3),
        server_groups: if id == 4 { vec![8, 9] } else { vec![8] },
        channel_group: if id == 4 { 5 } else { 7 },
    }
}

/// A session as `connected` needs it, without a real connection behind it.
fn add_session(s: &mut App, name: &str, address: &str) -> hub::SessionId {
    let id = hub::SessionId(s.next_session);
    s.next_session += 1;
    s.sessions.push(ServerSession::new(
        id,
        name.into(),
        address.into(),
        String::new(),
        s.home.streaming.surfaces.clone(),
        s.home.streaming.visible.clone(),
    ));
    id
}

fn connected(s: &mut App) {
    saved(s);
    let id = add_session(s, "Zirkumflex", "ts.zirkumflex.io");
    s.activate(Some(id));
    s.server_mut().connected = true;
    s.server_mut().muted = false;
    s.level = 0.3;
    s.transmitting = true;
    s.server_mut().state.can_talk = true;
    s.server_mut().state.server = "Zirkumflex".into();
    s.server_mut().state.server_version = "6.0.0".into();
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
    s.server_mut().state.channels = channels;
    s.server_mut().state.current = Some(lobby);
    s.server_mut().talking = [2].into();
    s.server_mut().server_controls = Some(crate::command_flow::AudioState {
        muted: false,
        deafened: false,
    });
    s.server_mut().controls_pending = false;
    let now = Instant::now();
    for i in 0..60u64 {
        let wave = (i as f64 * 0.35).sin();
        s.server_mut().connection_history.push(
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
    s.server_mut().connection_usage = usage::ConnectionUsage {
        packet_loss_in: Some(0.3),
        packet_loss_out: Some(0.1),
        ping_ms: Some(31.0),
        ping_deviation_ms: Some(4.0),
        idle_seconds: Some(3),
        ..Default::default()
    };
    s.server_mut().streaming.catalog = vec![crate::streaming::StreamInfo {
        id: "s1".into(),
        publisher: 2,
        name: "Mara’s screen".into(),
        viewers: 2,
        has_audio: true,
        server_relayed: false,
    }];
}

/// No one is sharing, and you are muted and deafened: the channel header over the empty state.
fn connected_quiet(s: &mut App) {
    connected(s);
    s.server_mut().streaming.catalog.clear();
    s.server_mut().muted = true;
    s.server_mut().deafened = true;
    s.server_mut().server_controls = Some(crate::command_flow::AudioState {
        muted: true,
        deafened: true,
    });
    s.level = 0.0;
    s.transmitting = false;
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

/// Searching brings the long theme list and its note into view.
fn settings_more_themes(s: &mut App) {
    settings_appearance(s);
    s.settings_path = Some("/home/jan/.config/flexspeak/settings.json".into());
    s.settings_query = "mocha".into();
}

/// Searching brings the list of sounds into view.
fn settings_sounds(s: &mut App) {
    connected(s);
    s.settings_page = Some(SettingsPage::Audio);
    s.settings_query = "sound cues".into();
}

/// Ten hours of a busy server, running past midnight.
fn server_log(s: &mut App) {
    use crate::server_log::Tone;
    use chrono::{Duration, NaiveDate};
    connected(s);
    let start = NaiveDate::from_ymd_opt(2026, 11, 8)
        .unwrap()
        .and_hms_opt(14, 16, 0)
        .unwrap();
    let people = ["Mara", "Theo", "Ines", "Lukas", "Priya with a long name"];
    let happenings = [
        (Tone::Arrived, "connected"),
        (Tone::Moved, "→ Raid night"),
        (Tone::Stream, "started streaming"),
        (Tone::Moved, "→ Lobby"),
        (Tone::Stream, "stopped streaming"),
        (Tone::Left, "lost connection"),
        (Tone::Arrived, "connected"),
        (Tone::Left, "disconnected"),
    ];
    let log = &mut s.server_mut().log;
    log.add_at(start, Tone::Arrived, "", "Connected");
    for index in 0..240 {
        let (tone, action) = happenings[index % happenings.len()];
        let at = start + Duration::minutes(index as i64 * 5 / 2 + 2);
        log.add_at(at, tone, people[index * 3 % people.len()], action);
    }
}

/// The log dragged up to take more of the sidebar.
fn server_log_tall(s: &mut App) {
    server_log(s);
    s.log_pane.height = 300;
}

/// The log folded down to its heading, giving the channels the sidebar.
fn server_log_folded(s: &mut App) {
    server_log(s);
    s.log_pane.collapsed = true;
}

fn server_log_hover(s: &mut App) {
    server_log(s);
    s.ui.log_hover = true;
}

fn server_log_search(s: &mut App) {
    server_log(s);
    s.server_mut().log.searching = true;
    s.server_mut().log.set_query("raid".into());
    s.sync_fields();
}

fn server_log_people(s: &mut App) {
    server_log(s);
    s.server_mut().log.searching = true;
    s.ui.log_people = true;
}

fn server_log_person(s: &mut App) {
    server_log(s);
    s.server_mut().log.searching = true;
    s.server_mut().log.set_person(Some("Mara".into()));
}

fn server_log_none(s: &mut App) {
    server_log(s);
    s.server_mut().log.searching = true;
    s.server_mut().log.set_query("zzz".into());
    s.sync_fields();
}

/// The Soft pack chosen, with the pack menu open.
fn settings_sounds_pack(s: &mut App) {
    settings_sounds(s);
    for sound in crate::sounds::Sound::ALL {
        s.sounds.set_source(sound, crate::sounds::Source::Soft);
    }
    s.ui.open = Some("sound-pack");
}

/// On a computer without TeamSpeak, with the pack menu open.
fn settings_sounds_no_teamspeak(s: &mut App) {
    settings_sounds(s);
    s.sound_bank = crate::sounds::Bank::default();
    s.ui.open = Some("sound-pack");
}

/// On an Omarchy desktop, following its Nord theme.
fn settings_omarchy(s: &mut App) {
    settings_appearance(s);
    s.settings_path = Some("/home/jan/.config/flexspeak/settings.json".into());
    let nord = s.theme_library.find("omarchy-nord").and_then(|t| t.dark);
    s.omarchy = nord.map(|colors| crate::omarchy::Current {
        name: "Nord".into(),
        dark: true,
        colors,
    });
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
    let id = add_session(s, "Friday Squad", "voice.friday.gg:9988");
    s.activate(Some(id));
    s.server_mut().status = "Connecting to voice.friday.gg:9988…".into();
}

/// The microphone menu open inside the window: it draws above the panel.
fn settings_picker(s: &mut App) {
    settings_audio(s);
    s.ui.open = Some("input-device");
}

fn settings_shortcuts_recording(s: &mut App) {
    settings_shortcuts(s);
    let _ = s.update(crate::Message::RecordShortcut(
        crate::shortcuts::Binding::Deafen,
    ));
    s.sync_fields();
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
    s.server_mut().talking = [2].into();
}

fn member_menu(s: &mut App) {
    connected(s);
    s.ui.member_menu = Some((2, rune::PointerPress { x: 150.0, y: 200.0 }));
}

fn member_menu_restricted(s: &mut App) {
    connected(s);
    s.ui.member_menu = Some((3, rune::PointerPress { x: 150.0, y: 220.0 }));
}

fn with_groups(s: &mut App) {
    let group = |id: u64, name: &str| client::Group {
        id,
        name: name.into(),
    };
    s.server_mut().state.server_groups = vec![
        group(6, "Server Admin"),
        group(7, "Moderator"),
        group(8, "Member"),
        group(9, "Streamer"),
        group(10, "Trusted"),
    ];
    s.server_mut().state.channel_groups = vec![
        group(5, "Channel Admin"),
        group(6, "Operator"),
        group(7, "Guest"),
    ];
}

fn member_groups(s: &mut App) {
    connected(s);
    with_groups(s);
    s.server_mut().group_edit = Some(crate::GroupEditDraft {
        ids: vec![2],
        wanted: [(7, true), (10, true)].into(),
        channel_group: Some(6),
    });
    s.ui.member_menu = Some((2, rune::PointerPress { x: 150.0, y: 200.0 }));
    s.ui.groups_menu = Some(super::GroupsMenu::Server);
}

fn member_channel_group(s: &mut App) {
    member_groups(s);
    s.ui.groups_menu = Some(super::GroupsMenu::Channel);
}

fn member_groups_selection(s: &mut App) {
    connected(s);
    with_groups(s);
    s.server_mut().selected_members = [2, 4].into();
    s.server_mut().group_edit = Some(crate::GroupEditDraft {
        ids: vec![2, 4],
        ..Default::default()
    });
    s.ui.member_menu = Some((4, rune::PointerPress { x: 150.0, y: 240.0 }));
    s.ui.groups_menu = Some(super::GroupsMenu::Server);
}

fn moderation_draft(s: &mut App, action: client::Moderation, reason: &str) {
    connected(s);
    s.server_mut().moderation = Some(crate::ModerationDraft {
        ids: vec![2],
        action,
        reason: reason.into(),
    });
    s.sync_fields();
}

fn member_kick(s: &mut App) {
    moderation_draft(s, client::Moderation::KickServer, "Please read the rules");
}

fn member_ban(s: &mut App) {
    moderation_draft(s, client::Moderation::Ban(Some(604_800)), "Repeated spam");
}

fn member_selection(s: &mut App) {
    connected(s);
    s.server_mut().selected_members = [2, 4].into();
    s.ui.member_menu = Some((4, rune::PointerPress { x: 150.0, y: 240.0 }));
    s.ui.move_menu = true;
}

fn member_volume(s: &mut App) {
    connected(s);
    s.user_audio.insert(
        "uid-3".into(),
        settings::UserAudio {
            volume_percent: 65,
            muted: false,
        },
    );
    s.ui.user_volume = Some(super::UserVolumeTarget {
        uid: "uid-3".into(),
        name: "Theo".into(),
    });
}

fn member_muted(s: &mut App) {
    connected(s);
    s.user_audio.insert(
        "uid-2".into(),
        settings::UserAudio {
            volume_percent: 100,
            muted: true,
        },
    );
}

/// Right-clicked a server in the rail.
fn servers_menu(s: &mut App) {
    let [_, raid, _] = servers(s);
    s.ui.rail_menu = Some((raid, rune::PointerPress { x: 60.0, y: 180.0 }));
}

fn member_drag(s: &mut App) {
    connected(s);
    s.server_mut().selected_members = [2, 4].into();
    s.ui.drag = Some(super::MemberDrag {
        ids: vec![2, 4],
        at: (150.0, 250.0),
        target: Some(4),
    });
}

fn poke(s: &mut App) {
    connected(s);
    s.server_mut().poke = Some(crate::PokeDraft {
        id: 2,
        name: "Mara".into(),
        message: "Raid starts in 5".into(),
    });
    s.sync_fields();
}

fn poked(s: &mut App) {
    connected(s);
    s.server_mut().pokes.push(crate::Poke {
        from: "Theo".into(),
        message: "Can you hear me?".into(),
    });
}

fn share_sources(s: &mut App) {
    use crate::streaming::publishing::{ShareSource, SourceKind};
    s.server_mut().streaming.sharing.sources = vec![
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
    s.server_mut().streaming.sharing.selected =
        s.server_mut().streaming.sharing.sources.first().cloned();
    let source = s.server().streaming.sharing.selected.clone().unwrap();
    let (width, height) = (320_u32, 180_u32);
    let rgba = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let band = ((x + y) / 24).is_multiple_of(2);
            [
                30,
                if band { 70 } else { 90 },
                if band { 130 } else { 160 },
                255_u8,
            ]
        })
        .collect();
    let sharing = &mut s.server_mut().streaming.sharing;
    assert!(sharing.begin_preview(&source));
    sharing.finish_preview(
        &source,
        Some(crate::streaming::publishing::Still {
            width,
            height,
            rgba,
        }),
    );
}

fn share(s: &mut App) {
    connected(s);
    share_sources(s);
    s.server_mut().streaming.share_open = true;
}

fn sharing(s: &mut App) {
    connected(s);
    share_sources(s);
    let status = &mut s.server_mut().streaming.sharing.status;
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
        let surface = s.server_mut().streaming.surfaces.get(slot).clone();
        let generation = surface.begin();
        s.server_mut()
            .streaming
            .update(crate::streaming::Event::State {
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
    s.server_mut().streaming.sync_frame();
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
    let lobby = s
        .server_mut()
        .state
        .current
        .as_mut()
        .expect("the demo joins the lobby");
    lobby.members.extend([
        member(5, "Kai", false, false),
        member(6, "Noor", false, false),
        member(7, "Lin", false, false),
    ]);
    s.server_mut().state.channels[0] = lobby.clone();
}

fn watching_five_below(s: &mut App) {
    watching_five(s);
    s.server_mut().streaming.focused = Some("s2".into());
}

fn watching_five_beside(s: &mut App) {
    watching_five_below(s);
    s.server_mut().streaming.strip = crate::streaming::Strip::Beside;
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

/// The pointer over the streams, with their toolbar showing.
fn watching_toolbar(s: &mut App) {
    watching_grid(s);
    s.ui.streams_hover = true;
}

fn watching(s: &mut App) {
    use crate::streaming::Phase;
    watch(s, &[("s1", 2, "Mara’s screen", Phase::Receiving)]);
    s.server_mut()
        .streaming
        .catalog
        .push(crate::streaming::StreamInfo {
            id: "s2".into(),
            publisher: 4,
            name: "Ines’s game".into(),
            viewers: 1,
            has_audio: false,
            server_relayed: false,
        });
}

/// A 2560x1440 source shrunk into the stage: one-pixel lines show whether downscaling
/// averages them or aliases.
fn watching_1440p(s: &mut App) {
    watching(s);
    let surface = s.server().streaming.surfaces.get(0).clone();
    let Some(frame) = surface.latest() else {
        return;
    };
    let (width, height) = (2560_u32, 1440_u32);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            // Alternating one-pixel columns on the left, rows on the right, a grid below.
            let line = if y >= height / 2 {
                x % 8 == 0 || y % 8 == 0
            } else if x < width / 2 {
                x % 2 == 0
            } else {
                y % 2 == 0
            };
            let v = if line { 235 } else { 30 };
            pixels.extend([v, v, v, 255]);
        }
    }
    let image = crate::streaming::Image {
        width,
        height,
        pixels,
    };
    surface.publish(frame.generation, image, 0.0);
    s.server_mut().streaming.sync_frame();
}

/// Streams running in the channel that you haven't joined.
fn streams_live(s: &mut App) {
    connected(s);
    for (id, publisher, name, viewers, has_audio) in [
        ("s2", 4, "Ines’s game", 1, false),
        ("s3", 3, "Theo’s window", 0, true),
    ] {
        s.server_mut()
            .streaming
            .catalog
            .push(crate::streaming::StreamInfo {
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
    let status = &mut s.server_mut().streaming.sharing.status;
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
    s.server_mut().streaming.focused = Some("s2".into());
    s.server_mut().streaming.set_volume(4, 40);
}

fn watching_stats(s: &mut App) {
    watching(s);
    s.server_mut().streaming.show_stats = true;
    if let Some(watch) = s.server_mut().streaming.watching.first_mut() {
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
    s.server_mut().password_channel = Some(3);
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
    number: 25,
};

/// A release build that has found build 26. Nothing is fetched; the scene only sets state.
fn with_update(s: &mut App, phase: Phase) {
    let mut u = updater::Updater::new(Some(RUNNING), Ok(updater::Package::MacApp), true);
    u.release = Some(updater::Release {
        tag: "v0.1.0-build.7".into(),
        build: Build {
            number: 26,
            ..RUNNING
        },
        asset: updater::Asset {
            name: updater::MACOS_DMG.into(),
            url: String::new(),
            size: 25_237_970,
            digest: None,
        },
        checksums_url: String::new(),
        notes: Box::default(),
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

fn update_whats_new(s: &mut App) {
    update_available(s);
    if let Some(release) = &mut s.updater.release {
        *release.notes = demo_notes();
    }
    s.ui.update_hover = true;
}

fn demo_notes() -> updater::Notes {
    updater::Notes {
        new: vec![
            "Tokyo Night, Catppuccin and Omarchy themes".into(),
            "Your own themes in the settings file".into(),
        ],
        fixed: vec!["The channel error after restarting".into()],
        changed: vec!["Shortcuts can be a single key".into()],
    }
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

/// The same page with every earlier release listed.
fn update_settings_changelog(s: &mut App) {
    update_settings(s);
    s.ui.changelog_open = true;
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
    use super::super::home::{initials, tile_colour};
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
    fn clicking_tiles_switches_servers_and_the_plus_offers_the_rest() {
        let mut rt = runtime(servers_rail, (900.0, 790.0));
        settle(&mut rt);
        let first = rt.state.active.unwrap();
        let other = rt.state.sessions[1].id;
        click(&mut rt, &format!("rail-{}", other.0));
        assert_eq!(rt.state.active, Some(other));
        assert_eq!(rt.state.active, Some(other));
        click(&mut rt, &format!("rail-{}", first.0));
        assert_eq!(rt.state.active, Some(first));
        click(&mut rt, "rail-add");
        assert!(rt.state.ui.server_picker);
        assert!(
            rt.find("picker-connect-1").is_some(),
            "Friday Squad isn't connected"
        );
        assert!(rt.find("picker-connect-0").is_none(), "Zirkumflex is");
    }

    #[test]
    fn the_shortcut_hint_needs_more_than_one_server() {
        let mut rt = runtime(connected, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("shortcut-hint").is_none());
        let mut rt = runtime(servers_rail, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("shortcut-hint").is_some());
    }

    #[test]
    fn a_tile_shows_the_server_state_without_a_fake_list() {
        assert_eq!(initials("Raid Night"), "RN");
        assert_eq!(initials("Zirkumflex"), "ZI");
        assert_eq!(initials("ts.example.org"), "TE");
        assert_eq!(initials(""), "");
        assert_eq!(tile_colour("Raid Night"), tile_colour("Raid Night"));
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
        let quality = s.server().streaming.sharing.quality;
        assert_eq!(
            (quality.frame_rate, quality.video_kbps, quality.audio_kbps),
            (24, 8000, 192)
        );
        assert_eq!(quality.resolution, Resolution::P1440);
        assert!(quality.preset().is_none());
        let _ = s.update(Message::ShareFrameRate(0));
        assert_eq!(s.server().streaming.sharing.quality, quality);
        let camera = s.server().streaming.sharing.sources.last().unwrap().clone();
        let _ = s.update(Message::ShareSource(camera.clone()));
        assert!(
            !s.server().streaming.sharing.audio,
            "Camera selection must not unexpectedly share system sound"
        );
        s.server_mut().streaming.sharing.status.active = true;
        let _ = s.update(Message::ShareQuality(Preset::Saver.quality()));
        let _ = s.update(Message::ShareAccess(Access::Private));
        let _ = s.update(Message::ShareSource(
            s.server().streaming.sharing.sources[0].clone(),
        ));
        assert_eq!(s.server().streaming.sharing.quality, quality);
        assert_eq!(s.server().streaming.sharing.access, Access::Public);
        assert_eq!(s.server().streaming.sharing.selected, Some(camera));
    }

    #[test]
    fn right_clicking_a_server_in_the_rail_offers_disconnect() {
        let mut rt = runtime(servers_rail, (900.0, 790.0));
        settle(&mut rt);
        let second = rt.state.rail_sessions().nth(1).unwrap();
        let (id, active) = (second.id, rt.state.active);
        let key = format!("rail-{}", id.0);
        let (x, y) = rt.find(&key).unwrap_or_else(|| panic!("no {key}"));
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        settle(&mut rt);
        assert_eq!(rt.state.ui.rail_menu.map(|(open, _)| open), Some(id));
        assert_eq!(rt.state.active, active, "right-clicking doesn't switch");
        click(&mut rt, "rail-menu-item1");
        assert!(rt.state.ui.rail_menu.is_none());
        assert!(rt.state.session(id).is_none_or(|session| session.stopping));
    }

    /// Ticking groups keeps the menu open, so several can be picked in a row.
    #[test]
    fn group_ticks_leave_the_menu_open() {
        let mut rt = runtime(member_groups, (900.0, 790.0));
        settle(&mut rt);
        rt.state
            .server_mut()
            .group_edit
            .as_mut()
            .unwrap()
            .wanted
            .clear();
        let row = "member-menu-item6-sub-item";
        click(&mut rt, &format!("{row}0"));
        settle(&mut rt);
        click(&mut rt, &format!("{row}3"));
        settle(&mut rt);
        assert!(
            rt.state.ui.member_menu.is_some(),
            "still open after ticking"
        );
        assert_eq!(
            rt.state.server().group_edit.as_ref().unwrap().wanted,
            [(6, true), (9, true)].into()
        );
    }

    /// Crossing other rows on the way must not throw off where the drop lands.
    #[test]
    fn dragging_someone_targets_the_channel_under_the_cursor() {
        let mut rt = runtime(connected, (900.0, 790.0));
        settle(&mut rt);
        let (x, y) = rt.find("member-2").expect("Mara");
        let (past_x, past_y) = rt.find("member-3").expect("Theo");
        let (to_x, to_y) = rt.find("channel-2").expect("Raid night");
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        rt.pointer_down(x, y);
        rt.pointer_move(past_x, past_y);
        rt.frame(1.0 / 60.0);
        rt.pointer_move(to_x, to_y);
        rt.frame(1.0 / 60.0);
        rt.frame(1.0 / 60.0);
        let drag = rt.state.ui.drag.clone().expect("dragging");
        assert_eq!(drag.target, Some(2), "the channel under the cursor");
        assert!(
            (drag.at.0 - to_x).abs() < 1.0 && (drag.at.1 - to_y).abs() < 1.0,
            "the drag follows the cursor: {:?} against {:?}",
            drag.at,
            (to_x, to_y),
        );
        rt.pointer_up(to_x, to_y);
        settle(&mut rt);
        assert!(rt.state.ui.drag.is_none());
    }

    #[test]
    fn hovering_the_update_notice_shows_what_changed() {
        fn scene(s: &mut App) {
            update_available(s);
            if let Some(release) = &mut s.updater.release {
                *release.notes = demo_notes();
            }
        }
        let mut rt = runtime(scene, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("update-notes").is_none(), "hidden until hovered");
        let (x, y) = rt.find("update-notice").expect("the notice");
        rt.pointer_move(x, y);
        settle(&mut rt);
        assert!(rt.find("update-notes").is_some());
        rt.pointer_move(20.0, 700.0);
        settle(&mut rt);
        assert!(rt.find("update-notes").is_none());
    }

    #[test]
    fn omarchy_sets_the_theme_until_switched_off() {
        let mut rt = runtime(settings_omarchy, (1000.0, 1500.0));
        settle(&mut rt);
        let nord = rt.state.omarchy.as_ref().unwrap().colors;
        assert_eq!(super::super::theme(&rt.state).surface, nord.surface);
        click(&mut rt, "follow-omarchy");
        assert!(rt.state.appearance.ignore_omarchy);
        assert_ne!(super::super::theme(&rt.state).surface, nord.surface);
    }

    #[test]
    fn searching_narrows_more_themes_to_one_collection_and_keeps_open_folder() {
        let mut rt = runtime(settings_more_themes, (1000.0, 1500.0));
        settle(&mut rt);
        assert!(rt.find("extra-catppuccin-mocha").is_some());
        assert!(rt.find("extra-tokyo-night-night").is_none());
        assert!(rt.find("open-settings-folder").is_some());
    }

    #[test]
    fn the_changelog_shows_the_latest_build_and_opens_to_all_earlier_ones() {
        let entries = crate::changelog::entries();
        let latest = format!("build-{}", entries[0].build.number);
        let oldest = format!("build-{}", entries[entries.len() - 1].build.number);
        let mut rt = runtime(update_settings, (1000.0, 1500.0));
        settle(&mut rt);
        assert!(rt.find(&latest).is_some());
        assert!(rt.find(&oldest).is_none());
        click(&mut rt, "changelog-earlier");
        settle(&mut rt);
        assert!(rt.find(&latest).is_some());
        assert!(rt.find(&oldest).is_some());
        click(&mut rt, "changelog-earlier");
        settle(&mut rt);
        assert!(rt.find(&oldest).is_none());
    }

    #[test]
    fn the_omarchy_section_only_shows_on_omarchy() {
        let mut rt = runtime(settings_appearance, (1000.0, 1500.0));
        settle(&mut rt);
        assert!(rt.find("follow-omarchy").is_none());
    }

    #[test]
    fn choosing_a_bundled_theme_uses_it_and_a_builtin_replaces_it() {
        let mut rt = runtime(settings_appearance, (1000.0, 1500.0));
        settle(&mut rt);
        click(&mut rt, "tokyo-night-storm-dark");
        assert_eq!(
            rt.state.appearance.dark_theme_id.as_deref(),
            Some("tokyo-night-storm")
        );
        let chosen = super::super::theme(&rt.state);
        let expected = super::super::theme::Theme::extra(
            rt.state
                .theme_library
                .find("tokyo-night-storm")
                .and_then(|theme| theme.dark.as_ref())
                .unwrap(),
            true,
        );
        assert_eq!(chosen.surface, expected.surface);
    }

    #[test]
    fn animations_stay_on_until_switched_off_in_appearance() {
        let mut rt = runtime(settings_appearance, (900.0, 790.0));
        settle(&mut rt);
        assert!(!rt.state.appearance.disable_animations, "on by default");
        let root = std::env::temp_dir().join(format!("flexspeak-motion-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        crate::settings::save(&path, &crate::settings::Settings::default()).unwrap();
        rt.state.settings_path = Some(path.clone());
        click(&mut rt, "disable-animations");
        assert!(rt.state.appearance.disable_animations);
        let saved = crate::settings::load(&path).unwrap().unwrap();
        assert!(saved.appearance.disable_animations, "saved right away");
        click(&mut rt, "disable-animations");
        assert!(!rt.state.appearance.disable_animations);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pressing_change_records_the_next_key_combination() {
        use crate::shortcuts::Binding;
        let ctrl_shift = rune::Mods {
            ctrl: true,
            shift: true,
            ..rune::Mods::default()
        };
        let mut rt = runtime(settings_shortcuts, (900.0, 790.0));
        settle(&mut rt);
        let root = std::env::temp_dir().join(format!("flexspeak-record-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        rt.state.settings_path = Some(path.clone());
        let before = rt.state.keybind_draft.clone();

        click(&mut rt, "change-Mute");
        assert!(
            rt.state.ui.key_capture.focused,
            "the recorder takes the keys"
        );
        rt.key(rune::KeyEvent {
            key: rune::Key::Text("j".into()),
            mods: rune::Mods::default(),
        });
        settle(&mut rt);
        assert_eq!(
            rt.state.keybind_draft.toggle_mute, "J",
            "a single key is enough"
        );
        assert!(
            rt.state.ui.key_capture.text.is_empty(),
            "keys aren't typed in"
        );

        click(&mut rt, "change-Mute");
        rt.key(rune::KeyEvent {
            key: rune::Key::Text("k".into()),
            mods: ctrl_shift,
        });
        settle(&mut rt);
        assert_eq!(rt.state.keybind_draft.toggle_mute, "Ctrl+Shift+K");
        assert!(rt.state.key_capture.is_none());
        let saved = crate::settings::load(&path).unwrap().unwrap();
        assert_eq!(
            saved.keybinds.toggle_mute, "Ctrl+Shift+K",
            "saved right away"
        );

        // A combination already in use is refused and the recorder keeps listening.
        click(&mut rt, "change-Deafen");
        rt.key(rune::KeyEvent {
            key: rune::Key::Text("k".into()),
            mods: ctrl_shift,
        });
        settle(&mut rt);
        assert_eq!(rt.state.keybind_draft.toggle_deafen, before.toggle_deafen);
        assert!(rt.state.key_capture.as_ref().unwrap().error.is_some());

        // Escape stops listening and leaves the shortcut alone, without closing Settings.
        rt.key(rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        });
        settle(&mut rt);
        assert!(rt.state.key_capture.is_none());
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Shortcuts));
        assert_eq!(
            rt.state.keybind_draft.get(Binding::Deafen),
            before.toggle_deafen
        );

        // Only push-to-talk may be emptied.
        click(&mut rt, "change-PushToTalk");
        rt.key(rune::KeyEvent {
            key: rune::Key::Backspace,
            mods: rune::Mods::default(),
        });
        settle(&mut rt);
        assert!(rt.state.keybind_draft.push_to_talk.is_empty());
        let _ = std::fs::remove_dir_all(root);
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
        let watching = &rt.state.server().streaming.watching;
        watching.iter().map(|w| w.stream.id.as_str()).collect()
    }

    #[test]
    fn five_streams_arrange_themselves_to_the_window() {
        let row_of = |rt: &Runtime<App>, id: &str| rt.find(&format!("tile-{id}")).unwrap().1;
        let mut wide = runtime(watching_five, (1700.0, 900.0));
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
    fn hovering_the_server_log_offers_search_and_escape_closes_it() {
        let mut rt = runtime(server_log, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("log-search-open").is_none(), "hidden at rest");
        click(&mut rt, "server-log");
        settle(&mut rt);
        click(&mut rt, "log-search-open");
        settle(&mut rt);
        assert!(rt.state.server().log.searching);
        assert!(rt.state.ui.log_search.focused);
        click(&mut rt, "log-people");
        settle(&mut rt);
        click(&mut rt, "log-person-0");
        settle(&mut rt);
        assert!(rt.state.server().log.person().is_some());
        assert!(rt.find("log-people-menu").is_none(), "closes on choosing");
        key(&mut rt, rune::Key::Escape, false);
        settle(&mut rt);
        assert!(!rt.state.server().log.searching);
        assert!(!rt.state.server().log.filtering());
    }

    #[test]
    fn the_server_log_stays_out_of_small_windows() {
        let mut rt = runtime(server_log, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("server-log").is_some());
        let mut small = runtime(server_log, crate::MIN_SIZE);
        settle(&mut small);
        assert!(small.find("server-log").is_none());
    }

    #[test]
    fn the_streams_toolbar_appears_while_the_pointer_is_over_them() {
        let mut rt = runtime(watching, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("refresh-streams").is_none(), "hidden at rest");
        let (x, y) = rt.find("stage").unwrap();
        rt.pointer_move(x, y);
        settle(&mut rt);
        assert!(rt.find("refresh-streams").is_some());
        rt.pointer_move(1.0, 1.0);
        settle(&mut rt);
        assert!(rt.find("refresh-streams").is_none(), "hidden again");
    }

    #[test]
    fn the_mouse_switches_layouts_focuses_and_rearranges_streams() {
        let mut rt = runtime(watching_five, (900.0, 790.0));
        settle(&mut rt);
        let (x, y) = rt.find("stage").unwrap();
        rt.pointer_move(x, y);
        settle(&mut rt);

        drag(&mut rt, "tile-s1", "tile-s2");
        assert_eq!(order(&rt), ["s2", "s1", "s3", "s4", "s5"]);

        click(&mut rt, "tile-s3");
        assert_eq!(rt.state.server().streaming.focused.as_deref(), Some("s3"));
        assert_eq!(
            rt.state.server().streaming.strip,
            crate::streaming::Strip::Below
        );

        click(&mut rt, "layout-beside");
        assert_eq!(
            rt.state.server().streaming.strip,
            crate::streaming::Strip::Beside
        );
        assert_eq!(rt.state.server().streaming.focused.as_deref(), Some("s3"));

        // A thumbnail dropped on the stage takes it and hands over its place in the strip.
        drag(&mut rt, "tile-s4", "tile-s3");
        assert_eq!(rt.state.server().streaming.focused.as_deref(), Some("s4"));
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
        assert_eq!(rt.state.server().streaming.focused, None);

        click(&mut rt, "layout-below");
        assert_eq!(rt.state.server().streaming.focused.as_deref(), Some("s2"));
        click(&mut rt, "layout-grid");
        assert_eq!(rt.state.server().streaming.focused, None);

        click(&mut rt, "focus");
        assert!(
            rt.state.server().streaming.focused.is_some(),
            "the maximize button"
        );
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
        let id = add_session(&mut rt.state, "Locked", "locked.example");
        rt.state.activate(Some(id));
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
        let id = add_session(&mut rt.state, "Locked", "locked.example");
        rt.state.activate(Some(id));
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
        let base = s.server().streaming.catalog[0].clone();
        for id in 0..12 {
            s.server_mut()
                .streaming
                .catalog
                .push(crate::streaming::StreamInfo {
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
                let base = rt.state.server_mut().state.channels[0].clone();
                for id in 10..40 {
                    rt.state.server_mut().state.channels.push(client::Channel {
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
