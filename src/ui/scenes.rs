//! Named states for headless previews: `teamspeak-native --snap out.png --scene connected`.
//! They change only what the view shows; no session, device or file is touched.

use std::time::{Duration, Instant};

use crate::permissions::{Kind, Status};
use crate::{
    App, ServerSession, SettingsPage, Tab,
    alerts::Source,
    audio_processing, chat, client, hub, onboarding, settings, transmission,
    updater::{self, Build, Phase, Progress, Retry},
    usage,
};

type Scene = (&'static str, fn(&mut App));

pub const SCENES: &[Scene] = &[
    ("setup", setup),
    ("devices", devices),
    ("servers", servers_rail),
    ("rail-edge", rail_edge),
    ("rail-edge-hover", rail_edge_hover),
    ("rail-edge-open", rail_edge_open),
    ("rail-edge-talking", rail_edge_talking),
    ("servers-poked", servers_poked),
    ("servers-switched", servers_switched),
    ("servers-connecting", servers_connecting),
    ("server-reconnecting", server_reconnecting),
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
    ("connected-deafened", connected_deafened),
    ("member-states", member_states),
    ("streaming-elsewhere", streaming_elsewhere),
    ("own-channel-only", own_channel_only),
    ("limited", limited),
    ("footer-hidden", footer_hidden),
    ("footer-expanded", footer_expanded),
    ("statistics", statistics),
    ("settings", settings_page),
    ("settings-nickname-empty", settings_nickname_empty),
    ("settings-appearance", settings_appearance),
    ("settings-general", settings_general),
    ("settings-omarchy", settings_omarchy),
    ("monster-sync-syncing", settings_sync_syncing),
    ("monster-sync-up-to-date", settings_sync_up_to_date),
    ("monster-sync-choose", settings_sync_choose),
    ("monster-sync-offline", settings_sync_offline),
    ("monster-empty", monster_empty),
    ("monster-board", monster_board),
    ("monster-add-confirm", monster_add_confirm),
    ("monster-connected", monster_connected),
    ("monster-unboxed", monster_unboxed),
    ("monster-unboxed-rare", monster_unboxed_rare),
    ("monster-unboxed-own", monster_unboxed_own),
    ("monster-search", monster_search),
    ("monster-search-none", monster_search_none),
    ("monster-rarities", monster_rarities),
    ("monster-common", monster_common),
    ("monster-uncommon", monster_uncommon),
    ("monster-rare", monster_rare),
    ("monster-mythic", monster_mythic),
    ("monster-legendary", monster_legendary),
    ("monster-leaderboard", monster_leaderboard),
    (
        "monster-leaderboard-legendary",
        monster_leaderboard_legendary,
    ),
    ("monster-leaderboard-hidden", monster_leaderboard_hidden),
    ("monster-leaderboard-local", monster_leaderboard_local),
    ("monster-person-loading", monster_person_loading),
    ("monster-person", monster_person),
    ("monster-signed-in", monster_signed_in),
    ("monster-loading", monster_loading),
    ("monster-adding", monster_adding),
    ("monster-sign-in-email", monster_sign_in_email),
    ("monster-sign-in-code", monster_sign_in_code),
    ("monster-sign-in-code-wrong", monster_sign_in_code_wrong),
    ("monster-sign-in-code-expired", monster_sign_in_code_expired),
    ("monster-sign-in-passkey", monster_sign_in_passkey),
    (
        "monster-sign-in-passkey-waiting",
        monster_sign_in_passkey_waiting,
    ),
    (
        "monster-sign-in-passkey-existing",
        monster_sign_in_passkey_existing,
    ),
    ("monster-sign-in-identity", monster_sign_in_identity),
    (
        "monster-sign-in-identity-linked",
        monster_sign_in_identity_linked,
    ),
    ("monster-reveal-back", monster_reveal_back),
    ("monster-reveal-uncommon", monster_reveal_uncommon),
    ("monster-reveal-rare", monster_reveal_rare),
    ("monster-reveal-mythic", monster_reveal_mythic),
    ("monster-reveal-legendary", monster_reveal_legendary),
    ("monster-reveal-hush", monster_reveal_hush),
    ("monster-reveal-turn", monster_reveal_turn),
    ("monster-reveal-hit", monster_reveal_hit),
    ("monster-can-hover", monster_can_hover),
    ("monster-can-lid", monster_can_lid),
    ("monster-can-base", monster_can_base),
    ("monster-flavours", monster_flavours),
    ("monster-flavours-empty", monster_flavours_empty),
    ("monster-flavour", monster_flavour),
    ("monster-flavour-new", monster_flavour_new),
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
    ("settings-sounds-open", settings_sounds_open),
    ("settings-sounds-search", settings_sounds_search),
    ("settings-more-themes", settings_more_themes),
    ("settings-audio", settings_audio),
    ("settings-audio-ptt", settings_audio_ptt),
    ("settings-streaming", settings_streaming),
    ("settings-picker", settings_picker),
    ("settings-auto-connect", settings_auto_connect),
    ("settings-default-channel", settings_default_channel),
    ("server-add", server_add),
    ("server-add-connect", server_add_connect),
    ("server-edit", server_edit),
    ("server-edit-remove", server_edit_remove),
    ("home-connecting", home_connecting),
    ("settings-shortcuts", settings_shortcuts),
    ("settings-shortcuts-recording", settings_shortcuts_recording),
    ("settings-shortcuts-taken", settings_shortcuts_taken),
    ("settings-permissions", settings_permissions),
    ("settings-file", settings_file),
    ("settings-search", settings_search),
    ("settings-tray", settings_tray),
    ("settings-empty-search", settings_empty_search),
    ("settings-search-opened", settings_search_opened),
    ("settings-search-picked", settings_search_picked),
    ("settings-connected", settings_connected),
    ("settings-light", settings_light),
    ("themes", themes),
    ("connected-light", connected_light),
    ("audio-processing", audio_processing_page),
    ("push-to-talk", push_to_talk_page),
    ("push-to-talk-connected", push_to_talk_connected),
    ("member-menu", member_menu),
    ("member-menu-auto-join", member_menu_auto_join),
    ("member-menu-restricted", member_menu_restricted),
    ("member-groups", member_groups),
    ("member-groups-selection", member_groups_selection),
    ("member-channel-group", member_channel_group),
    ("member-groups-guest", member_groups_guest),
    ("member-kick", member_kick),
    ("member-kick-several", member_kick_several),
    ("member-ban", member_ban),
    ("servers-menu", servers_menu),
    ("member-volume", member_volume),
    ("member-muted", member_muted),
    ("member-selection", member_selection),
    ("member-drag", member_drag),
    ("poke", poke),
    ("poked", poked),
    ("share", share),
    ("share-opened", share_opened),
    ("share-windows", share_windows),
    ("share-mac", share_mac),
    ("share-advanced", share_advanced),
    ("share-custom", share_custom),
    ("share-quick", share_quick),
    ("share-quick-no-game", share_quick_no_game),
    ("share-quick-streams", share_quick_streams),
    ("channel-tree", channel_tree),
    ("channel-tree-spacers", channel_tree_spacers),
    ("channel-menu", channel_menu),
    ("channel-menu-default", channel_menu_default),
    ("sharing", sharing),
    ("sharing-stream-options", sharing_stream_options),
    (
        "sharing-stream-options-unavailable",
        sharing_stream_options_unavailable,
    ),
    (
        "sharing-fast-capture-fallback",
        sharing_fast_capture_fallback,
    ),
    ("sharing-alone", sharing_alone),
    ("sharing-sound", sharing_sound),
    ("sharing-audio-off", sharing_audio_off),
    ("watching", watching),
    ("watching-solo", watching_solo),
    ("watching-solo-hover", watching_solo_hover),
    ("watching-fullscreen", watching_fullscreen),
    ("watching-others-hidden", watching_others_hidden),
    ("watching-others-hidden-hover", watching_others_hidden_hover),
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
    ("watching-trouble-yours", watching_trouble_yours),
    ("watching-trouble-streamer", watching_trouble_streamer),
    ("watching-trouble-between", watching_trouble_between),
    ("watching-trouble-paused", watching_trouble_paused),
    ("watching-trouble-computer", watching_trouble_computer),
    (
        "watching-trouble-reconnecting",
        watching_trouble_reconnecting,
    ),
    (
        "watching-trouble-streamer-reconnecting",
        watching_trouble_streamer_reconnecting,
    ),
    ("watching-trouble-thumbnail", watching_trouble_thumbnail),
    ("watching-trouble-hover", watching_trouble_hover),
    ("watching-loud", watching_loud),
    ("sharing-live-audio", sharing_live_audio),
    ("install", install),
    ("close-prompt", close_prompt),
    ("close-prompt-no-tray", close_prompt_no_tray),
    ("disconnect-sharing", disconnect_sharing),
    ("password", password),
    ("server-password", server_password),
    ("server-password-wrong", server_password_wrong),
    ("alert", alert),
    ("alert-dropped", alert_dropped),
    ("alert-kicked", alert_kicked),
    ("alert-audio-fallback", alert_audio_fallback),
    ("alert-no-audio", alert_no_audio),
    ("alert-shortcut-taken", alert_shortcut_taken),
    ("alert-several", alert_several),
    ("alert-details", alert_details),
    ("alert-settings", alert_settings),
    ("settings-error-log", settings_error_log),
    ("settings-error-log-empty", settings_error_log_empty),
    ("settings-diagnostics-debug", settings_diagnostics_debug),
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
    ("chat-tabs", chat_tabs),
    ("chat-tabs-private", chat_tabs_private),
    ("chat-tabs-light", chat_tabs_light),
    ("chat-new-messages", chat_new_messages),
    ("chat-line-menu", chat_line_menu),
    ("chat-composer-lines", chat_composer_lines),
    ("chat-composer-limit", chat_composer_limit),
    ("chat-composer-cut", chat_composer_cut),
    ("chat-feed", chat_feed),
    ("chat-feed-private", chat_feed_private),
    ("chat-feed-target", chat_feed_target),
    ("chat-empty", chat_empty),
    ("chat-offline", chat_offline),
    ("chat-tabs-stream", chat_tabs_stream),
    ("chat-feed-stream", chat_feed_stream),
    ("chat-hidden", chat_hidden),
    ("chat-hidden-hover", chat_hidden_hover),
    ("chat-hidden-message", chat_hidden_message),
    ("chat-hidden-private", chat_hidden_private),
    ("chat-peek", chat_peek),
    ("chat-peek-feed", chat_peek_feed),
    ("chat-pokes-tabs", chat_pokes_tabs),
    ("chat-pokes-feed", chat_pokes_feed),
    ("chat-servers", chat_servers),
    ("chat-history-tabs", chat_history_tabs),
    ("chat-history-open", chat_history_open),
    ("chat-grouped", chat_grouped),
    ("chat-grouped-channels", chat_grouped_channels),
    ("chat-grouped-people", chat_grouped_people),
    ("chat-grouped-private", chat_grouped_private),
    ("chat-grouped-history", chat_grouped_history),
    ("chat-tabs-all", chat_tabs_all),
    ("chat-grouped-all", chat_grouped_all),
    ("chat-feed-grouped", chat_feed_grouped),
    ("chat-feed-grouped-channels", chat_feed_grouped_channels),
    ("chat-feed-grouped-people", chat_feed_grouped_people),
    ("chat-feed-grouped-channel", chat_feed_grouped_channel),
    ("chat-feed-grouped-person", chat_feed_grouped_person),
    ("chat-restored", chat_restored),
    ("chat-restored-feed", chat_restored_feed),
    ("settings-chat-history", settings_chat_history),
    ("settings-chat", settings_chat),
    ("setup-chat", setup_chat),
    ("setup-chat-feed", setup_chat_feed),
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
    s.chat_options = Default::default();
    // And without the collection server built in: the online scenes set their own.
    s.monster.server = None;
    s.monster.session = None;
    let servers = vec![
        settings::Server {
            name: "Zirkumflex".into(),
            address: "ts.zirkumflex.io".into(),
            ..Default::default()
        },
        settings::Server {
            name: "Friday Squad".into(),
            address: "voice.friday.gg:9988".into(),
            ..Default::default()
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
        names: [("ts.zirkumflex.io".to_owned(), "Zirkumflex".to_owned())].into(),
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

/// One server: the rail waits behind the left edge.
fn rail_edge(s: &mut App) {
    connected_quiet(s);
    s.server_mut().talking.clear();
}

fn rail_edge_hover(s: &mut App) {
    rail_edge(s);
    s.ui.rail_peek.strip = true;
    // Still waiting out the delay however long the capture takes.
    s.ui.rail_peek.changed = Some(Instant::now() + std::time::Duration::from_secs(3600));
}

fn rail_edge_open(s: &mut App) {
    connected_quiet(s);
    s.ui.rail_peek.pinned = true;
}

fn rail_edge_talking(s: &mut App) {
    connected_quiet(s);
    s.server_mut().talking = [2].into();
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

/// The connection dropped: the last known channels and chat stay, dimmed, under one line.
fn server_reconnecting(s: &mut App) {
    connected(s);
    let session = s.server_mut();
    session.connected = false;
    session.reconnecting = true;
    session.status = crate::client::CONNECTION_LOST.into();
}

fn servers_picker(s: &mut App) {
    servers(s);
    s.ui.server_picker = true;
}

/// A line of chat at a fixed time, so previews don't change with the clock.
fn say(
    s: &mut App,
    scope: chat::Scope,
    own: bool,
    from: &str,
    place: &str,
    text: &str,
    time: &str,
) -> chat::Entry {
    s.server_mut()
        .chat
        .add(
            scope,
            chat::Kind::Message,
            own,
            from.into(),
            place.into(),
            text,
            time.into(),
        )
        .clone()
}

fn poke_line(s: &mut App, own: bool, peer: (&str, &str), text: &str, time: &str) {
    s.server_mut().chat.add(
        chat::Scope::Private(peer.0.into()),
        chat::Kind::Poke,
        own,
        if own { "You" } else { peer.1 }.into(),
        peer.1.into(),
        text,
        time.into(),
    );
}

const MARA: (&str, &str) = ("uid-2", "Mara");
const THEO: (&str, &str) = ("uid-3", "Theo");
const INES: (&str, &str) = ("uid-4", "Ines");

fn mara() -> chat::Scope {
    chat::Scope::Private(MARA.0.into())
}

/// An evening in the Lobby: channel talk, a server note, and Mara writing privately, unread.
fn chat_talk(s: &mut App) {
    use chat::Scope;
    say(
        s,
        Scope::Server,
        false,
        "Lukas",
        "",
        "Server restarts at 4 for the update",
        "20:58",
    );
    say(
        s,
        Scope::Channel,
        false,
        "Theo",
        "Lobby",
        "back, had to restart my router",
        "21:04",
    );
    say(
        s,
        Scope::Channel,
        false,
        "Mara",
        "Lobby",
        "we start the raid at half past, Ines is setting up",
        "21:05",
    );
    say(
        s,
        mara(),
        false,
        "Mara",
        "Mara",
        "psst, can you bring the spare key for the vault run?",
        "21:06",
    );
    say(
        s,
        mara(),
        false,
        "Mara",
        "Mara",
        "and don't tell Theo yet",
        "21:06",
    );
    say(
        s,
        Scope::Channel,
        true,
        "Jan",
        "Lobby",
        "I'll join after this match",
        "21:06",
    );
    say(
        s,
        Scope::Channel,
        false,
        "Theo",
        "Lobby",
        "can someone share the boss notes again? https://notes.zirkumflex.io/vault",
        "21:07",
    );
    let chat = &mut s.server_mut().chat;
    chat.open(Scope::Private(INES.0.into()), INES.1);
    chat.open(Scope::Channel, "");
    chat.mark_unread(&mara());
    chat.mark_unread(&mara());
}

fn chat_tabs(s: &mut App) {
    connected(s);
    chat_talk(s);
}

/// The private tab open: its underline, audience line and close button are violet's.
fn chat_tabs_private(s: &mut App) {
    chat_tabs(s);
    say(
        s,
        mara(),
        true,
        "Jan",
        "Mara",
        "haha ok, I'll grab it before we start",
        "21:08",
    );
    s.server_mut().chat.open(mara(), MARA.1);
    s.server_mut().chat.mark_seen(chat::Layout::Tabs);
    s.server_mut().chat.draft = "on my way".into();
    s.sync_fields();
}

fn chat_tabs_light(s: &mut App) {
    chat_tabs(s);
    s.appearance.scheme = settings::ColorScheme::Light;
}

/// Read back while three lines came in: they wait under a control that jumps to them.
fn chat_new_messages(s: &mut App) {
    chat_tabs(s);
    s.ui.chat_back = true;
    for _ in 0..3 {
        s.server_mut().chat.mark_unread(&chat::Scope::Channel);
    }
}

/// Right-clicking someone's line that holds a link.
fn chat_line_menu(s: &mut App) {
    chat_tabs(s);
    let entry = say(
        s,
        chat::Scope::Channel,
        false,
        MARA.1,
        "Lobby",
        "route is on www.example.org/raid",
        "21:10",
    );
    s.ui.chat_line_menu = Some((entry.id, rune::PointerPress { x: 900.0, y: 420.0 }));
}

/// A draft of three lines: the box grows with it.
fn chat_composer_lines(s: &mut App) {
    chat_tabs(s);
    s.server_mut().chat.draft = "pull order:\n1. tank pulls left\n2. we burn the adds".into();
    s.ui.chat_input.focused = true;
    s.sync_fields();
}

/// Near the limit, the box counts the bytes left.
fn chat_composer_limit(s: &mut App) {
    chat_tabs(s);
    s.server_mut().chat.draft = "notes ".repeat(160);
    s.sync_fields();
}

/// A paste that didn't fit was cut, and the box says so.
fn chat_composer_cut(s: &mut App) {
    chat_tabs(s);
    let chat = &mut s.server_mut().chat;
    chat.draft = "log ".repeat(256);
    chat.draft_cut = true;
    s.sync_fields();
}

fn chat_feed(s: &mut App) {
    chat_tabs(s);
    s.chat_options.layout = chat::Layout::Feed;
    s.server_mut().chat.mark_seen(chat::Layout::Feed);
}

/// The feed narrowed to private messages.
fn chat_feed_private(s: &mut App) {
    chat_feed(s);
    s.server_mut().chat.filter = chat::Filter::Private;
}

/// The feed's message box pointed at Mara, with the list of places open.
fn chat_feed_target(s: &mut App) {
    chat_feed(s);
    s.server_mut().chat.open(mara(), MARA.1);
    s.ui.chat_lists[2].pinned = true;
}

fn chat_empty(s: &mut App) {
    connected(s);
}

/// A private conversation with someone who has left: the message box says so and is locked.
fn chat_offline(s: &mut App) {
    connected(s);
    let nora = chat::Scope::Private("uid-9".into());
    say(
        s,
        nora.clone(),
        false,
        "Nora",
        "Nora",
        "gotta go, see you tomorrow",
        "20:31",
    );
    s.server_mut().chat.open(nora, "Nora");
}

fn chat_tabs_stream(s: &mut App) {
    watching_solo(s);
    chat_talk(s);
}

fn chat_feed_stream(s: &mut App) {
    chat_tabs_stream(s);
    s.chat_options.layout = chat::Layout::Feed;
    s.server_mut().chat.mark_seen(chat::Layout::Feed);
}

/// The column behind the right edge: a handle like the rail's, and nothing over the picture.
fn chat_hidden(s: &mut App) {
    chat_tabs_stream(s);
    s.chat_options.hidden = true;
    s.server_mut().chat.mark_seen(chat::Layout::Feed);
}

fn chat_hidden_hover(s: &mut App) {
    chat_hidden(s);
    s.ui.chat_peek.strip = true;
    // Still waiting out the delay however long the capture takes.
    s.ui.chat_peek.changed = Some(Instant::now() + Duration::from_secs(3600));
}

fn chat_toast(s: &mut App, entry: chat::Entry) {
    s.server_mut().chat.mark_unread(&entry.scope);
    s.ui.chat_toast = Some(super::ChatToast {
        entry,
        until: Instant::now() + Duration::from_secs(3600),
    });
}

/// A channel message while hidden: the pop-up, and a blue dot on the handle.
fn chat_hidden_message(s: &mut App) {
    chat_hidden(s);
    let entry = say(
        s,
        chat::Scope::Channel,
        false,
        "Theo",
        "Lobby",
        "anyone up for one more round after the raid?",
        "21:12",
    );
    chat_toast(s, entry);
}

/// A private one: violet edge on the pop-up, violet dot, and Mara marked in the tree.
fn chat_hidden_private(s: &mut App) {
    chat_hidden(s);
    let entry = say(
        s,
        mara(),
        false,
        "Mara",
        "Mara",
        "you coming? we're about to pull",
        "21:12",
    );
    chat_toast(s, entry);
}

/// The pointer on the handle: the chat peeks over the stream.
fn chat_peek(s: &mut App) {
    chat_hidden(s);
    s.ui.chat_peek.pinned = true;
}

fn chat_peek_feed(s: &mut App) {
    chat_peek(s);
    s.chat_options.layout = chat::Layout::Feed;
}

fn chat_pokes(s: &mut App) {
    connected(s);
    poke_line(s, false, MARA, "you there? raid soon", "20:41");
    say(
        s,
        mara(),
        true,
        "Jan",
        "Mara",
        "sorry, was making food, joining in 5",
        "20:43",
    );
    poke_line(s, true, MARA, "", "20:58");
    say(
        s,
        chat::Scope::Channel,
        false,
        "Theo",
        "Lobby",
        "back, had to restart my router",
        "21:04",
    );
    say(
        s,
        mara(),
        false,
        "Mara",
        "Mara",
        "psst, can you bring the spare key for the vault run?",
        "21:06",
    );
    poke_line(s, true, THEO, "", "21:07");
    say(
        s,
        chat::Scope::Channel,
        false,
        "Theo",
        "Lobby",
        "what",
        "21:07",
    );
    poke_line(s, false, MARA, "come to Raid night pls", "21:09");
}

/// Pokes sit in the private tab with that person, both ways, faded.
fn chat_pokes_tabs(s: &mut App) {
    chat_pokes(s);
    s.server_mut().chat.open(mara(), MARA.1);
}

/// In the feed they sit where they happened.
fn chat_pokes_feed(s: &mut App) {
    chat_pokes(s);
    s.chat_options.layout = chat::Layout::Feed;
}

/// Private messages waiting on a server that isn't on screen mark its tile on the rail.
fn chat_servers(s: &mut App) {
    servers(s);
    if let Some(other) = s
        .sessions
        .iter_mut()
        .find(|session| Some(session.id) != s.active)
    {
        let scope = chat::Scope::Private("uid-7".into());
        other.chat.mark_unread(&scope);
        other.chat.mark_unread(&scope);
        other.chat.mark_unread(&scope);
    }
}

/// Earlier in the evening you were in Raid night and AFK; what was said there stays to read.
fn chat_past(s: &mut App) {
    let chat = &mut s.server_mut().chat;
    chat.enter(2, "Raid night");
    let said = |chat: &mut chat::Chat, from: &str, place: &str, text: &str, time: &str| {
        chat.add(
            chat::Scope::Channel,
            chat::Kind::Message,
            false,
            from.into(),
            place.into(),
            text,
            time.into(),
        );
    };
    said(
        chat,
        "Ines",
        "Raid night",
        "tank goes left, everyone else stacks",
        "20:12",
    );
    said(
        chat,
        "Ines",
        "Raid night",
        "wipe at 2%, again in five",
        "20:31",
    );
    chat.enter(4, "AFK");
    said(chat, "Lukas", "AFK", "brb, dinner", "20:40");
    chat.enter(1, "Lobby");
}

/// Side by side: the channels you left keep a tab after the others, fainter.
fn chat_history_tabs(s: &mut App) {
    connected(s);
    chat_past(s);
    say(
        s,
        chat::Scope::Channel,
        false,
        "Theo",
        "Lobby",
        "back, had to restart my router",
        "21:04",
    );
    say(
        s,
        chat::Scope::Channel,
        false,
        "Mara",
        "Lobby",
        "we start the raid at half past, Ines is setting up",
        "21:05",
    );
}

/// A left channel's tab open: fainter, closable, and the message box offers to join again.
fn chat_history_open(s: &mut App) {
    chat_history_tabs(s);
    s.server_mut().chat.open(chat::Scope::Past(2), "");
}

/// Tabs grouped by kind: channels, the server, people.
fn chat_grouped(s: &mut App) {
    connected(s);
    chat_past(s);
    chat_talk(s);
    s.chat_options.tab_bar = chat::TabBar::Grouped;
}

/// Pointing at the channel group lists the channel you're in and the ones you left.
fn chat_grouped_channels(s: &mut App) {
    chat_grouped(s);
    s.ui.chat_lists[0].pinned = true;
}

fn chat_grouped_people(s: &mut App) {
    chat_grouped(s);
    s.ui.chat_lists[1].pinned = true;
}

/// A private conversation open in the grouped bar: the people group carries its name.
fn chat_grouped_private(s: &mut App) {
    chat_grouped(s);
    s.server_mut().chat.open(mara(), MARA.1);
    s.server_mut().chat.mark_seen(chat::Layout::Tabs);
}

fn chat_grouped_history(s: &mut App) {
    chat_grouped(s);
    s.server_mut().chat.open(chat::Scope::Past(2), "");
}

/// Tabs on its All tab: every conversation together, tagged, and the message box chooses
/// where it sends.
fn chat_tabs_all(s: &mut App) {
    connected(s);
    chat_past(s);
    chat_talk(s);
    s.server_mut().chat.everything = true;
    s.server_mut().chat.mark_seen(chat::Layout::Tabs);
}

fn chat_grouped_all(s: &mut App) {
    chat_tabs_all(s);
    s.chat_options.tab_bar = chat::TabBar::Grouped;
}

/// One Feed with its chips grouped: Channels and People list what they hold.
fn chat_feed_grouped(s: &mut App) {
    chat_grouped(s);
    s.chat_options.layout = chat::Layout::Feed;
    s.server_mut().chat.mark_seen(chat::Layout::Feed);
}

fn chat_feed_grouped_channels(s: &mut App) {
    chat_feed_grouped(s);
    s.ui.chat_lists[0].pinned = true;
}

fn chat_feed_grouped_people(s: &mut App) {
    chat_feed_grouped(s);
    s.ui.chat_lists[1].pinned = true;
}

/// The feed narrowed to a channel you left: its chip carries the name.
fn chat_feed_grouped_channel(s: &mut App) {
    chat_feed_grouped(s);
    s.server_mut().chat.narrow(chat::Scope::Past(2));
}

/// Narrowed to one person: the message box follows.
fn chat_feed_grouped_person(s: &mut App) {
    chat_feed_grouped(s);
    s.server_mut().chat.narrow(mara());
}

/// After a restart: yesterday's conversations are back from the history, a line between the
/// days, and today's follow.
fn chat_restored(s: &mut App) {
    use crate::chat_store::{Loaded, Stored};
    connected(s);
    // Yesterday evening by this computer's clock, whatever today is.
    let midnight = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|time| time.and_local_timezone(chrono::Local).single())
        .map_or(0, |time| time.timestamp());
    let line = |scope: chat::Scope, from: &str, place: &str, text: &str, before: i64| Stored {
        room: if scope == chat::Scope::Channel { 1 } else { 0 },
        scope,
        kind: chat::Kind::Message,
        own: from == "Jan",
        from: from.into(),
        place: place.into(),
        text: text.into(),
        at: midnight - before * 60,
    };
    s.server_mut().chat.restore(Loaded {
        lines: vec![
            line(
                chat::Scope::Channel,
                "Theo",
                "Lobby",
                "same time tomorrow?",
                228,
            ),
            line(
                chat::Scope::Channel,
                "Mara",
                "Lobby",
                "yes, eight sharp",
                226,
            ),
            line(mara(), "Mara", "Mara", "bring the spare key tomorrow", 220),
            line(chat::Scope::Channel, "Jan", "Lobby", "see you then", 219),
        ],
        closed: Vec::new(),
    });
    s.server_mut().chat.enter(1, "Lobby");
    say(
        s,
        chat::Scope::Channel,
        false,
        "Theo",
        "Lobby",
        "back, had to restart my router",
        "21:04",
    );
    say(
        s,
        chat::Scope::Channel,
        false,
        "Mara",
        "Lobby",
        "we start the raid at half past",
        "21:05",
    );
}

fn chat_restored_feed(s: &mut App) {
    chat_restored(s);
    s.chat_options.layout = chat::Layout::Feed;
}

/// Settings › Chat with Clear History pressed once. History is low on the page; at
/// `--size 1180x1300` the armed button shows at the panel's foot.
fn settings_chat_history(s: &mut App) {
    settings_chat(s);
    s.ui.chat_clear_armed = true;
}

fn settings_chat(s: &mut App) {
    settings_at(s, SettingsPage::Chat);
}

fn setup_chat(s: &mut App) {
    setup(s);
    s.chat_options = Default::default();
    s.onboarding = Some(onboarding::Step::Chat);
}

fn setup_chat_feed(s: &mut App) {
    setup_chat(s);
    s.chat_options.layout = chat::Layout::Feed;
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
        server_groups: match id {
            3 => vec![12],
            4 => vec![8, 9],
            _ => vec![8],
        },
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

/// Seeing only your own channel: the toggle beside the server's name is lit, and the other
/// channels keep their people to themselves.
fn own_channel_only(s: &mut App) {
    connected(s);
    s.subscribe_all
        .store(false, std::sync::atomic::Ordering::Relaxed);
    for channel in s.server_mut().state.channels.iter_mut().skip(1) {
        channel.members.clear();
        channel.members_visible = false;
    }
}

/// The server is refusing requests for a while: the footer says so, with no dialog.
fn limited(s: &mut App) {
    connected(s);
    s.server_mut().limited = true;
}

/// Every member row state: you quiet, Mara speaking, Theo muted on a poor connection, Ines
/// with her sound off.
fn member_states(s: &mut App) {
    connected(s);
    let now = Instant::now();
    s.server_mut().client_usage.insert(
        3,
        Ok(usage::ServerUsage::preview(
            usage::ConnectionUsage {
                ping_ms: Some(240.0),
                ping_deviation_ms: Some(12.0),
                packet_loss_in: Some(6.0),
                packet_loss_out: Some(0.0),
                ..Default::default()
            },
            now,
        )),
    );
    for channel in &mut s.server_mut().state.channels {
        for member in &mut channel.members {
            if member.id == 4 {
                member.mic_muted = true;
                member.sound_muted = true;
            }
        }
    }
    // Theo turned down to 30%, which his row marks.
    s.user_audio.insert(
        "uid-3".into(),
        settings::UserAudio {
            volume_percent: 30,
            muted: false,
        },
    );
    s.server_mut().talking = [2].into();
}

/// Ines streams in Raid night while you sit in the Lobby: the tree marks her there too,
/// from the server's flag, with no stream to watch from here.
fn streaming_elsewhere(s: &mut App) {
    connected(s);
    s.server_mut().streaming.streamers = [4].into_iter().collect();
}

/// Deafened with the microphone left unmuted: the mute button shows the mic as off, since
/// nothing is sent, and its tooltip says why. The CPU reads to one decimal.
fn connected_deafened(s: &mut App) {
    connected_quiet(s);
    s.server_mut().muted = false;
    s.server_mut().server_controls = Some(crate::command_flow::AudioState {
        muted: false,
        deafened: true,
    });
    s.process_usage.cpu_percent = Some(23.415);
}

/// The compact footer, the usual one, under load: health and ping only.
fn footer_hidden(s: &mut App) {
    connected(s);
    s.footer_hidden = true;
    s.process_usage.cpu_percent = Some(64.0);
    s.process_usage.memory_bytes = Some(812 * 1024 * 1024);
    s.process_usage.memory_detail_bytes = Some(590 * 1024 * 1024);
}

/// The footer expanded: packet loss and ping with their sparklines, then CPU (one decimal,
/// red under load) and RAM (amber).
fn footer_expanded(s: &mut App) {
    footer_hidden(s);
    s.footer_hidden = false;
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
    s.server_mut().connection_usage.loss_in = usage::LossIn {
        voice: Some(0.2),
        keepalive: Some(0.0),
        control: Some(0.0),
    };
    s.voice_playback = crate::audio::VoicePlayback {
        played: 184_302,
        concealed: 412,
        recovered: 38,
        dropped: 6,
        timed_out: 1,
        queued: 2,
    };
}

/// Signed in to the collection server on the Monster page, settings sync's first check out.
/// The steps run on their own, so no request goes anywhere and nothing is saved.
fn settings_sync_started(s: &mut App, seen: i64) -> crate::settings_sync::SettingsSync {
    signed_in(s);
    let mut sync = crate::settings_sync::SettingsSync::new(seen);
    let _ = sync.signed_in(Instant::now(), settings_sync_context(s));
    sync
}

fn settings_sync_context(s: &App) -> crate::settings_sync::Context<'_> {
    crate::settings_sync::Context {
        signed_in: true,
        onboarding: false,
        local: s.saved.as_ref(),
    }
}

/// The account's copy at `version`, holding what travels from `settings`.
fn settings_sync_copy(
    version: i64,
    settings: &settings::Settings,
) -> crate::monster_api::SyncedSettings {
    crate::monster_api::SyncedSettings {
        version,
        updated_at: 1_700_000_000,
        data: serde_json::to_value(settings::Synced::from_settings(settings)).unwrap_or_default(),
    }
}

/// The server's answer to the first check, as if it came at `at`.
fn settings_sync_answer(
    s: &mut App,
    mut sync: crate::settings_sync::SettingsSync,
    at: Instant,
    result: Result<Option<crate::monster_api::SyncedSettings>, crate::monster_api::Problem>,
) {
    let answer = crate::settings_sync::Answer::Fetched(sync.generation(), result);
    let _ = sync.answer(at, answer, settings_sync_context(s));
    s.settings_sync = sync;
}

fn settings_sync_syncing(s: &mut App) {
    s.settings_sync = settings_sync_started(s, 3);
}

/// The account's copy is the one this computer last saw, checked two minutes ago.
fn settings_sync_up_to_date(s: &mut App) {
    let sync = settings_sync_started(s, 3);
    let copy = s.saved.as_ref().map(|saved| settings_sync_copy(3, saved));
    let now = Instant::now();
    let at = now.checked_sub(Duration::from_secs(125)).unwrap_or(now);
    settings_sync_answer(s, sync, at, Ok(copy));
}

/// First contact, and the account keeps other settings: nothing changes until a choice.
fn settings_sync_choose(s: &mut App) {
    let sync = settings_sync_started(s, 0);
    let theirs = settings::Settings {
        nickname: "Sam".into(),
        ..settings::Settings::default()
    };
    settings_sync_answer(
        s,
        sync,
        Instant::now(),
        Ok(Some(settings_sync_copy(6, &theirs))),
    );
}

fn settings_sync_offline(s: &mut App) {
    let sync = settings_sync_started(s, 3);
    let problem = crate::monster_api::Problem {
        error: crate::monster_api::OFFLINE.into(),
        message: "Can't reach the collection server.".into(),
        tries_left: None,
    };
    settings_sync_answer(s, sync, Instant::now(), Err(problem));
}

fn monster_empty(s: &mut App) {
    saved(s);
    s.monster.open(None);
    s.tab = Tab::Monster;
}

/// A year of cans on and off: weekends and the odd run of busy weeks, so the board has some
/// shape. Rarities come from a fixed key, so they spread as the odds say.
fn year_of_cans() -> Vec<crate::monster::Can> {
    use crate::monster::{Can, FLAVOURS, draw};
    let now = chrono::Utc::now().timestamp();
    const DAY: i64 = 86_400;
    let mut cans = Vec::new();
    let mut state = 7u32;
    for day in (6..364).rev() {
        state = state.wrapping_mul(2_654_435_769).rotate_left(7) ^ day as u32;
        let cans_today = match (state % 11, day % 7) {
            (0..=3, _) => 0,
            (4..=7, 0 | 1) => 3,
            (4..=7, _) => 1,
            (8, _) => 4,
            _ => 2,
        };
        for n in 0..cans_today {
            let flavour = (state as usize + n) % (FLAVOURS.len() - 1);
            let id = cans.len() as i64 + 1;
            let (rarity, seed) = draw(&[7; 32], FLAVOURS[flavour].id, day, id);
            cans.push(Can {
                id,
                flavour: FLAVOURS[flavour].id.into(),
                at: now - day * DAY,
                rarity,
                seed,
            });
        }
    }
    cans
}

/// The flavours of the five newest scene cans, one of each rarity, Legendary newest.
const NEWEST: [&str; 5] = [
    "original",
    "ultra-violet",
    "mango-loco",
    "pacific-punch",
    "ultra-gold",
];

/// The identity of the newest scene can of that rarity.
fn can_rolling(rarity: crate::monster::Rarity) -> String {
    format!("can-{}", year_of_cans().len() + 1 + rarity.index())
}

/// A year of cans, ending with five days in a row of one can of each rarity, so the streak is
/// live and the grid varied.
fn stocked(s: &mut App) {
    use crate::monster::{Can, Rarity};
    monster_empty(s);
    let now = chrono::Utc::now().timestamp();
    const DAY: i64 = 86_400;
    let mut cans = year_of_cans();
    for (i, (rarity, flavour)) in Rarity::ALL.into_iter().zip(NEWEST).enumerate() {
        cans.push(Can {
            id: cans.len() as i64 + 1,
            flavour: flavour.into(),
            at: now - (4 - i as i64) * DAY,
            rarity,
            seed: 40_000 + i as u16 * 977,
        });
    }
    s.monster.cans = cans;
}

fn monster_board(s: &mut App) {
    stocked(s);
}

/// A flavour picked: the page asks before the can is added, since it can't be taken back.
fn monster_add_confirm(s: &mut App) {
    stocked(s);
    let (khaotic, _) = crate::monster::flavour_by_id("khaotic").unwrap();
    s.monster.confirming = Some(khaotic);
}

/// Mara opens a Legendary: her row lights up in its colour and the channel gets the card.
fn monster_unboxed(s: &mut App) {
    unboxed(s, crate::monster::Rarity::Legendary, "Ultra Gold", 487);
}

fn monster_unboxed_rare(s: &mut App) {
    unboxed(s, crate::monster::Rarity::Rare, "Pacific Punch", 212);
}

/// Your own Common, told to the channel once its reveal is over: your row lights up too.
fn monster_unboxed_own(s: &mut App) {
    connected(s);
    chat_talk(s);
    let card = crate::monster::Unboxed {
        rarity: crate::monster::Rarity::Common,
        flavour: "Mango Loco".into(),
        number: 488,
    };
    say(
        s,
        chat::Scope::Channel,
        true,
        "Jan",
        "Lobby",
        &card.announcement(),
        "21:10",
    );
    s.server_mut()
        .unboxed
        .insert(1, crate::monster::Lit::new(card));
}

fn unboxed(s: &mut App, rarity: crate::monster::Rarity, flavour: &str, number: u32) {
    connected(s);
    chat_talk(s);
    let card = crate::monster::Unboxed {
        rarity,
        flavour: flavour.into(),
        number,
    };
    say(
        s,
        chat::Scope::Channel,
        false,
        "Mara",
        "Lobby",
        &card.announcement(),
        "21:09",
    );
    s.server_mut()
        .unboxed
        .insert(2, crate::monster::Lit::new(card));
}

/// Connected, the collection takes the stage beside the channels and chat.
fn monster_connected(s: &mut App) {
    connected(s);
    stocked(s);
}

/// A search narrowing the flavours down.
fn monster_search(s: &mut App) {
    stocked(s);
    s.monster.query = "ultra".into();
    s.sync_fields();
}

fn monster_search_none(s: &mut App) {
    stocked(s);
    s.monster.query = "lemonade".into();
    s.sync_fields();
}

fn monster_rarities(s: &mut App) {
    use crate::monster::Rarity;
    stocked(s);
    s.monster.selected = Some(can_rolling(Rarity::Mythic));
}

/// A year of someone else's cans: `busy` from 0 (now and then) to 3 (most days), and `seed`
/// so no two people match.
fn sample_cans(seed: u32, busy: u32) -> Vec<crate::monster::Can> {
    use crate::monster::{Can, FLAVOURS, draw};
    let now = chrono::Utc::now().timestamp();
    const DAY: i64 = 86_400;
    let mut cans = Vec::new();
    let mut state = seed;
    for day in (1..364).rev() {
        state = state.wrapping_mul(2_654_435_769).rotate_left(7) ^ day as u32;
        let cans_today = match state % 8 {
            n if n < 5 - busy => 0,
            7 => 2,
            _ => 1,
        };
        for n in 0..cans_today {
            // Some people stick to a few flavours, others try everything.
            let range = 6 + (seed as usize % 4) * 5;
            let flavour = (state as usize / 7 + n) % range.min(FLAVOURS.len() - 1);
            let id = cans.len() as i64 + 1;
            let (rarity, card) = draw(&[seed as u8; 32], FLAVOURS[flavour].id, day, id);
            cans.push(Can {
                id,
                flavour: FLAVOURS[flavour].id.into(),
                at: now - day * DAY,
                rarity,
                seed: card,
            });
        }
    }
    cans
}

/// The collection server the online scenes use. Nothing contacts it: scenes only draw.
const SCENE_SERVER: &str = "https://collection.example.com";

/// The scene server, standing in for the one built into the app.
fn use_scene_server(s: &mut App) {
    s.monster.server = Some(SCENE_SERVER.into());
}

/// Signed in to the collection server as Sam, with the collection the other scenes show.
fn signed_in(s: &mut App) {
    stocked(s);
    use_scene_server(s);
    s.monster.session = Some("scene".into());
    s.monster.session_read = true;
    s.monster.me = Some(scene_me());
}

/// Sam's account, as the server describes it.
fn scene_me() -> crate::monster::api::Me {
    crate::monster::api::Me {
        id: 1,
        name: "Sam".into(),
        email: SCENE_EMAIL.into(),
        hidden: false,
        imported: true,
        identities: Vec::new(),
    }
}

fn monster_signed_in(s: &mut App) {
    signed_in(s);
}

/// Just opened with a kept session: the collection is on its way.
fn monster_loading(s: &mut App) {
    signed_in(s);
    s.monster.cans.clear();
    s.monster.loading = true;
}

/// A can on its way to the server, before its card comes back.
fn monster_adding(s: &mut App) {
    signed_in(s);
    s.monster.adding = true;
}

/// The scene people's cans, by their user id: Robin is 2, Kai 3, and so on.
fn scene_cans(user: i64) -> Vec<crate::monster::Can> {
    let busy = [3, 2, 1, 2, 0];
    let n = (user - 2) as u32;
    sample_cans(11 + n * 37, busy[n as usize % busy.len()])
}

/// You and five others, ranked by the server; their standings counted from their cans.
fn with_people(s: &mut App) {
    use crate::monster::{Person, standing};
    signed_in(s);
    let mut people = vec![Person {
        user: 1,
        name: "Sam".into(),
        you: true,
        standing: standing(&s.monster.cans),
    }];
    for (n, name) in ["Robin", "Kai", "Noor", "Alex", "Mika"]
        .into_iter()
        .enumerate()
    {
        let user = n as i64 + 2;
        people.push(Person {
            user,
            name: name.into(),
            you: false,
            standing: standing(&scene_cans(user)),
        });
    }
    s.monster.people = people;
}

fn monster_leaderboard(s: &mut App) {
    with_people(s);
    s.monster.section = crate::monster::Section::Leaderboard;
}

fn monster_leaderboard_legendary(s: &mut App) {
    monster_leaderboard(s);
    s.monster.ranking = crate::monster::Ranking::Legendary;
}

fn monster_leaderboard_hidden(s: &mut App) {
    monster_leaderboard(s);
    s.monster.hidden = true;
}

/// Without a collection server, the tab says where to set one.
fn monster_leaderboard_local(s: &mut App) {
    stocked(s);
    s.monster.section = crate::monster::Section::Leaderboard;
}

/// Kai's board, opened from the leaderboard, while it's on its way.
fn monster_person_loading(s: &mut App) {
    with_people(s);
    s.monster.section = crate::monster::Section::Person(3);
    s.monster.visit = Some(crate::monster::Visit {
        user: 3,
        name: "Kai".into(),
        cans: None,
        problem: None,
    });
}

fn monster_person(s: &mut App) {
    monster_person_loading(s);
    if let Some(visit) = &mut s.monster.visit {
        visit.cans = Some(scene_cans(3));
    }
}

/// The address the sign-in scenes type in.
const SCENE_EMAIL: &str = "sam@example.com";
/// A TeamSpeak identity uid of the usual shape, for the sign-in scenes.
const SCENE_UID: &str = "Zk3vQ0pT8mYw1LhNf6RdXc2Ae9s=";

/// Signing in to the scene server, at this step.
fn gated(s: &mut App, gate: crate::monster::Gate) {
    stocked(s);
    use_scene_server(s);
    s.monster.session_read = true;
    s.monster.cans.clear();
    s.monster.gate = Some(gate);
}

fn monster_sign_in_email(s: &mut App) {
    gated(s, crate::monster::Gate::Email);
    s.ui.monster_email.text = SCENE_EMAIL.into();
}

fn code_step(wrong: bool, tries_left: Option<u32>) -> crate::monster::Gate {
    crate::monster::Gate::Code {
        email: SCENE_EMAIL.into(),
        wrong,
        tries_left,
        resent: false,
    }
}

fn monster_sign_in_code(s: &mut App) {
    gated(s, code_step(false, None));
    s.ui.monster_login_code.text = "482913".into();
}

/// The server's answer to a wrong code says how many tries it has left.
fn monster_sign_in_code_wrong(s: &mut App) {
    gated(s, code_step(true, Some(4)));
    s.ui.monster_login_code.text = "482913".into();
}

/// A code that has run out, in the server's words.
fn monster_sign_in_code_expired(s: &mut App) {
    gated(s, code_step(false, None));
    s.ui.monster_login_code.text = "482913".into();
    s.monster.gate_problem = Some("That code has run out. Send a new one.".into());
}

fn passkey_step(create: bool, waiting: bool) -> crate::monster::Gate {
    crate::monster::Gate::Passkey {
        ticket: "scene-ticket".into(),
        create,
        waiting,
    }
}

fn monster_sign_in_passkey(s: &mut App) {
    gated(s, passkey_step(true, false));
}

/// The passkey page is open in the browser, and the app asks the server until it's done.
fn monster_sign_in_passkey_waiting(s: &mut App) {
    gated(s, passkey_step(true, true));
}

/// Signing in with a passkey made before, from the Email step's second button.
fn monster_sign_in_passkey_existing(s: &mut App) {
    gated(s, passkey_step(false, true));
}

/// Flexspeak's own identity, the one the last step offers.
fn own_identity(linked: bool) -> crate::monster::FoundIdentity {
    crate::monster::FoundIdentity {
        source: crate::monster::OWN_SOURCE,
        name: "Sam".into(),
        uid: SCENE_UID.into(),
        linked,
    }
}

/// Signed in, at the identity step: the session is there, the page not yet.
fn identity_step(s: &mut App, linked: bool) {
    gated(
        s,
        crate::monster::Gate::Identity(vec![own_identity(linked)]),
    );
    s.monster.session = Some("scene".into());
    s.monster.me = Some(scene_me());
    s.monster.own_identity = Some(own_identity(false));
}

fn monster_sign_in_identity(s: &mut App) {
    identity_step(s, false);
}

fn monster_sign_in_identity_linked(s: &mut App) {
    identity_step(s, true);
}

fn monster_featuring(s: &mut App, rarity: crate::monster::Rarity) {
    use crate::monster::Notice;
    stocked(s);
    s.monster.selected = Some(can_rolling(rarity));
    s.monster.notice = Some(Notice::Added(rarity));
}

fn monster_common(s: &mut App) {
    monster_featuring(s, crate::monster::Rarity::Common);
}

fn monster_uncommon(s: &mut App) {
    monster_featuring(s, crate::monster::Rarity::Uncommon);
}

fn monster_rare(s: &mut App) {
    monster_featuring(s, crate::monster::Rarity::Rare);
}

fn monster_mythic(s: &mut App) {
    monster_featuring(s, crate::monster::Rarity::Mythic);
}

fn monster_legendary(s: &mut App) {
    monster_featuring(s, crate::monster::Rarity::Legendary);
}

/// A Legendary being opened, stopped at one beat.
fn revealing_at(s: &mut App, beat: crate::monster::Beat) {
    use crate::monster::{Notice, Rarity, ladder};
    stocked(s);
    let Some(mut reveal) = s.monster.start_reveal(can_rolling(Rarity::Legendary)) else {
        return;
    };
    reveal.step = ladder(Rarity::Legendary)
        .iter()
        .position(|(at, _)| *at == beat)
        .unwrap_or(0);
    s.monster.reveal = Some(reveal);
    s.monster.notice = Some(Notice::Added(Rarity::Legendary));
}

fn monster_reveal_back(s: &mut App) {
    revealing_at(s, crate::monster::Beat::Ack);
}

fn monster_reveal_uncommon(s: &mut App) {
    use crate::monster::{Beat, Rarity};
    revealing_at(s, Beat::Rung(Rarity::Uncommon));
}

fn monster_reveal_rare(s: &mut App) {
    use crate::monster::{Beat, Rarity};
    revealing_at(s, Beat::Rung(Rarity::Rare));
}

fn monster_reveal_mythic(s: &mut App) {
    use crate::monster::{Beat, Rarity};
    revealing_at(s, Beat::Rung(Rarity::Mythic));
}

fn monster_reveal_legendary(s: &mut App) {
    use crate::monster::{Beat, Rarity};
    revealing_at(s, Beat::Rung(Rarity::Legendary));
}

fn monster_reveal_hush(s: &mut App) {
    use crate::monster::{Beat, Rarity};
    revealing_at(s, Beat::Hush(Rarity::Legendary));
}

fn monster_reveal_turn(s: &mut App) {
    revealing_at(s, crate::monster::Beat::Turn);
}

fn monster_reveal_hit(s: &mut App) {
    revealing_at(s, crate::monster::Beat::Hit);
}

/// The pointer resting on the newest card in the grid.
fn monster_can_hover(s: &mut App) {
    use crate::monster::Rarity;
    stocked(s);
    s.monster.selected = Some(can_rolling(Rarity::Common));
    s.ui.can_hover = Some(format!("card-{}", can_rolling(Rarity::Legendary)));
}

/// The large can dragged round and tipped forward, to look into its lid.
fn monster_can_lid(s: &mut App) {
    stocked(s);
    s.ui.can_spin = super::can3d::Spin::posed(-0.5, 0.95);
}

/// And tipped back, to look under it.
fn monster_can_base(s: &mut App) {
    stocked(s);
    s.ui.can_spin = super::can3d::Spin::posed(2.4, -0.7);
}

fn monster_flavours(s: &mut App) {
    stocked(s);
    s.monster.section = crate::monster::Section::Flavours;
}

fn monster_flavours_empty(s: &mut App) {
    monster_empty(s);
    s.monster.section = crate::monster::Section::Flavours;
}

fn monster_flavour(s: &mut App) {
    use crate::monster::{Section, flavour_by_id};
    stocked(s);
    let (gold, _) = flavour_by_id("ultra-gold").unwrap();
    s.monster.section = Section::Flavour(gold);
}

/// A flavour with no cans yet; the scene's cans leave out the catch-all.
fn monster_flavour_new(s: &mut App) {
    use crate::monster::{FLAVOURS, Section};
    stocked(s);
    s.monster.section = Section::Flavour(FLAVOURS.len() - 1);
}

fn settings_page(s: &mut App) {
    settings_at(s, SettingsPage::Server);
}

fn settings_at(s: &mut App, page: SettingsPage) {
    saved(s);
    s.settings_path = Some("/Users/jan/Library/Application Support/Flexspeak/settings.json".into());
    s.log_folder = Some("/Users/jan/Library/Application Support/Flexspeak/logs".into());
    s.open_settings(page);
}

/// Searching for a theme by name, with the long theme list further down the page.
fn settings_more_themes(s: &mut App) {
    settings_appearance(s);
    s.settings_path = Some("/home/jan/.config/flexspeak/settings.json".into());
    s.settings_query = "mocha".into();
}

/// Settings › Sounds: one pack for all of them, and a switch for each group.
fn settings_sounds(s: &mut App) {
    connected(s);
    s.settings_page = Some(SettingsPage::Sounds);
}

/// Settings › General: the tray icon and running setup again.
fn settings_general(s: &mut App) {
    settings_at(s, SettingsPage::General);
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
        (Tone::Moved, "joined Raid night"),
        (Tone::Stream, "started sharing"),
        (Tone::Moved, "was moved to Lobby"),
        (Tone::Stream, "stopped sharing"),
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
    s.sounds.pack = crate::sounds::Pack::Soft;
    s.ui.open = Some("sound-pack");
}

/// A group opened to its sounds with one of them off, and another group switched off whole.
fn settings_sounds_open(s: &mut App) {
    use crate::sounds::{Group, Sound};
    settings_sounds(s);
    s.ui.sound_groups.insert(Group::Streams);
    s.sounds.set_sound(Sound::ViewerLeft, false);
    s.sounds.set_group(Group::Channel, false);
}

/// Searching for one sound lists it while its group is still closed.
fn settings_sounds_search(s: &mut App) {
    settings_sounds(s);
    s.settings_query = "whisper".into();
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

/// Push to talk chosen: its key is set right there, under the mode.
fn settings_audio_ptt(s: &mut App) {
    settings_at(s, SettingsPage::Audio);
    s.transmission.mode = crate::transmission::Mode::PushToTalk;
}

/// An emptied nickname, committed: the reason shows under the field.
fn settings_nickname_empty(s: &mut App) {
    settings_page(s);
    let _ = s.update(crate::Message::Nickname(String::new()));
    let _ = s.update(crate::Message::CommitNickname);
    s.sync_fields();
}

/// Settings > Streaming with both options on.
fn settings_streaming(s: &mut App) {
    settings_at(s, SettingsPage::Streaming);
    s.stream_options = crate::settings::Streaming {
        hardware_av1: true,
        fast_capture: true,
    };
}

/// Adding a server, from the home card's + or Settings.
fn settings_auto_connect(s: &mut App) {
    settings_page(s);
    // A capture must never really connect.
    s.auto_connect_checked = true;
    s.servers[0].auto_connect = true;
}

/// A saved server with a channel to join on connecting, named under its address.
fn settings_default_channel(s: &mut App) {
    settings_auto_connect(s);
    s.servers[0].default_channel = Some(settings::DefaultChannel {
        id: 3,
        name: "Raid night".into(),
    });
}

fn server_add(s: &mut App) {
    settings_page(s);
    let _ = s.update(crate::Message::AddServer);
    let _ = s.update(crate::Message::ServerAddress("eu.voicehub.net".into()));
    let _ = s.update(crate::Message::ServerName("Voice Hub EU".into()));
    s.sync_fields();
}

/// Add Server from Home: saving connects too.
fn server_add_connect(s: &mut App) {
    home(s);
    let _ = s.update(crate::Message::AddServer);
    let _ = s.update(crate::Message::ServerAddress("eu.voicehub.net".into()));
    s.sync_fields();
}

fn server_edit(s: &mut App) {
    settings_page(s);
    let _ = s.update(crate::Message::EditServer(1));
    s.sync_fields();
}

/// Remove pressed once in Edit Server: the second press removes the server.
fn server_edit_remove(s: &mut App) {
    server_edit(s);
    let _ = s.update(crate::Message::RemoveServer);
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

/// Mute cleared, and another app holding the deafen shortcut.
fn settings_shortcuts_taken(s: &mut App) {
    settings_shortcuts(s);
    s.keybind_draft.toggle_mute.clear();
    s.taken_shortcuts = vec![crate::shortcuts::Binding::Deafen];
    s.sync_fields();
}

/// A mix of answers, to show the same live status outside setup.
fn settings_permissions(s: &mut App) {
    settings_at(s, SettingsPage::Permissions);
    s.permissions.status = [Status::Granted, Status::Denied, Status::NotDetermined];
}

fn settings_file(s: &mut App) {
    settings_at(s, SettingsPage::File);
    s.settings_message = Some("Settings reloaded".into());
}

/// A query that finds rows in several categories, listed in the categories' place.
fn settings_search(s: &mut App) {
    settings_at(s, SettingsPage::Server);
    s.settings_query = "mic".into();
    s.sync_fields();
}

/// Settings searched for the notification-area icon's switch (Windows and macOS only).
fn settings_tray(s: &mut App) {
    settings_at(s, SettingsPage::Appearance);
    s.settings_query = "tray".into();
    s.sync_fields();
}

fn settings_empty_search(s: &mut App) {
    settings_at(s, SettingsPage::Server);
    s.settings_query = "bluetooth".into();
    s.sync_fields();
}

/// A search result just opened: its category, with the row it names marked.
fn settings_search_opened(s: &mut App) {
    settings_at(s, SettingsPage::Audio);
    s.ui.settings_target = Some(("row-processing-noise-suppression".into(), 1));
}

/// The arrow keys have moved down the results.
fn settings_search_picked(s: &mut App) {
    settings_search(s);
    s.ui.settings_pick = 2;
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

/// The person is picked to have their streams joined by themselves.
fn member_menu_auto_join(s: &mut App) {
    member_menu(s);
    s.auto_watch.insert("uid-2".into());
}

fn member_menu_restricted(s: &mut App) {
    connected(s);
    s.ui.member_menu = Some((3, rune::PointerPress { x: 150.0, y: 220.0 }));
}

fn with_groups(s: &mut App) {
    let group = |id: u64, name: &str| client::Group {
        id,
        name: name.into(),
        fixed: id == 12,
    };
    s.server_mut().state.server_groups = vec![
        group(12, "Guest"),
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

/// Someone with only the server's default group, which can't be changed.
fn member_groups_guest(s: &mut App) {
    connected(s);
    with_groups(s);
    s.server_mut().group_edit = Some(crate::GroupEditDraft {
        ids: vec![3],
        ..Default::default()
    });
    s.ui.member_menu = Some((3, rune::PointerPress { x: 150.0, y: 220.0 }));
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

/// Kicking several people at once names each of them.
fn member_kick_several(s: &mut App) {
    moderation_draft(s, client::Moderation::KickChannel, "");
    if let Some(draft) = &mut s.server_mut().moderation {
        draft.ids = vec![2, 3, 4];
    }
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
        key: "uid-3".into(),
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

fn share_quick_no_game(s: &mut App) {
    use crate::streaming::publishing::{ShareSource, SourceKind};
    connected_quiet(s);
    s.ui.quick_sources = vec![ShareSource {
        kind: SourceKind::Screen(Some(0)),
        name: "Main screen".into(),
    }];
}

fn share_quick(s: &mut App) {
    use crate::streaming::publishing::{ShareSource, SourceKind};
    share_quick_no_game(s);
    s.ui.quick_sources.push(ShareSource {
        kind: SourceKind::Window(2),
        name: "Counter-Strike 2".into(),
    });
}

/// Someone else is live, so the pane is theirs and going live moves above the voice buttons.
fn share_quick_streams(s: &mut App) {
    use crate::streaming::publishing::{ShareSource, SourceKind};
    connected(s);
    s.ui.quick_sources = vec![
        ShareSource {
            kind: SourceKind::Screen(Some(0)),
            name: "Main screen".into(),
        },
        ShareSource {
            kind: SourceKind::Window(2),
            name: "Counter-Strike 2".into(),
        },
    ];
}

/// A fuller server, as an evening looks: people in some channels and none in others, most of
/// them muted, and a subchannel. You are muted and deafened and no one is sharing.
fn channel_tree(s: &mut App) {
    share_quick_no_game(s);
    let person = |id, name: &str, mic_muted, sound_muted| client::Member {
        sound_muted,
        ..member(id, name, false, mic_muted)
    };
    let room = |id, name: &str, depth, members| client::Channel {
        id,
        name: name.into(),
        depth,
        members,
        members_visible: true,
        join_access: client::JoinAccess::Allowed,
        ..client::Channel::default()
    };
    let lobby = room(
        1,
        "Lobby",
        0,
        vec![
            member(1, "Jan", true, true),
            person(2, "Mara", true, true),
            person(3, "Theo", true, false),
        ],
    );
    s.server_mut().state.channels = vec![
        lobby.clone(),
        room(2, "War room", 0, Vec::new()),
        room(3, "Raid night", 0, vec![person(4, "Ines", true, false)]),
        room(4, "Scrims", 1, Vec::new()),
        room(5, "Strategy", 0, Vec::new()),
        room(
            6,
            "Late shift",
            0,
            vec![
                person(5, "Noor", true, false),
                person(6, "Kofi", false, false),
                person(7, "Lena", true, true),
            ],
        ),
    ];
    s.server_mut().state.current = Some(lobby);
    s.server_mut().talking.clear();
}

/// A server that parts its tree into sections with spacer channels: centred, left, right and
/// repeated labels and an empty one, none of them joinable.
fn channel_tree_spacers(s: &mut App) {
    channel_tree(s);
    let spacer = |id, name: &str| client::Channel {
        id,
        name: name.into(),
        members_visible: true,
        join_access: client::JoinAccess::Allowed,
        ..client::Channel::default()
    };
    let channels = &mut s.server_mut().state.channels;
    channels.insert(1, spacer(101, "[cspacer]━━ Games ━━"));
    channels.insert(4, spacer(102, "[*spacer1]─"));
    channels.insert(6, spacer(103, "[lspacer0]Quiet rooms"));
    channels.push(spacer(104, "[spacer0]"));
    channels.push(spacer(105, "[rspacer]AFK"));
}

/// Right-clicking a channel you can join: Join, and fold it away.
fn channel_menu(s: &mut App) {
    channel_tree(s);
    s.ui.channel_menu = Some((3, rune::PointerPress { x: 150.0, y: 170.0 }));
}

/// The same menu on the channel this server joins on connecting.
fn channel_menu_default(s: &mut App) {
    channel_menu(s);
    let address = s.server().address.trim().to_owned();
    if let Some(server) = s
        .servers
        .iter_mut()
        .find(|server| server.address.trim() == address)
    {
        server.default_channel = Some(settings::DefaultChannel {
            id: 3,
            name: "Raid night".into(),
        });
    }
}

fn share(s: &mut App) {
    connected(s);
    share_sources(s);
    s.server_mut().streaming.share_open = true;
}

/// A stand-in picture of a source, striped in its own colours.
fn fake_still(dark: [u8; 3], light: [u8; 3]) -> crate::streaming::publishing::Still {
    let (width, height) = (320_u32, 180_u32);
    let rgba = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let [r, g, b] = if ((x + y) / 24).is_multiple_of(2) {
                dark
            } else {
                light
            };
            [r, g, b, 255_u8]
        })
        .collect();
    crate::streaming::publishing::Still {
        width,
        height,
        rgba,
    }
}

/// Gives every preview the dialog asked for a stand-in picture, as if each capture came back.
fn finish_share_previews(s: &mut App) {
    let colours = [
        ([30, 70, 130], [30, 90, 160]),
        ([60, 40, 90], [80, 55, 120]),
        ([35, 80, 60], [45, 105, 80]),
        ([90, 60, 30], [120, 80, 40]),
    ];
    let sharing = &mut s.server_mut().streaming.sharing;
    let taking: Vec<_> = sharing
        .previews
        .iter()
        .filter(|preview| {
            matches!(
                preview.state,
                crate::streaming::publishing::PreviewState::Taking
            )
        })
        .map(|preview| preview.source.clone())
        .collect();
    for source in taking {
        let index = sharing
            .sources
            .iter()
            .position(|other| *other == source)
            .unwrap_or(0);
        let (dark, light) = colours[index % colours.len()];
        sharing.finish_preview(&source, Some(fake_still(dark, light)));
    }
}

/// The dialog as the voice panel's share button opens it: the screens, windows and cameras
/// were just found and nothing was picked by hand, so the main screen is.
fn share_opened(s: &mut App) {
    use crate::streaming::publishing::{ShareSource, SourceKind};
    connected(s);
    s.ui.quick_sources = vec![
        ShareSource {
            kind: SourceKind::Screen(Some(1)),
            name: "Main screen".into(),
        },
        ShareSource {
            kind: SourceKind::Window(2),
            name: "Counter-Strike 2".into(),
        },
    ];
    s.server_mut().streaming.share_open = true;
    s.server_mut().streaming.sharing.loading = true;
    let window = |id, name: &str| ShareSource {
        kind: SourceKind::Window(id),
        name: name.into(),
    };
    let sources = vec![
        ShareSource {
            kind: SourceKind::Screen(Some(0)),
            name: "Screen 2".into(),
        },
        ShareSource {
            kind: SourceKind::Screen(Some(1)),
            name: "Main screen".into(),
        },
        window(3, "flexspeak — Visual Studio Code"),
        window(4, "Spotify Premium"),
        window(2, "Counter-Strike 2"),
        window(5, "Patch notes — Firefox"),
        window(6, "File Explorer"),
        window(7, "Steam"),
        ShareSource {
            kind: SourceKind::Camera("demo-camera".into()),
            name: "Camera · Built-in camera".into(),
        },
    ];
    let _ = s.update(crate::Message::ShareSources(Ok(sources)));
    finish_share_previews(s);
}

/// Window picked: the game being played is chosen and listed first, the others below it.
fn share_windows(s: &mut App) {
    use crate::streaming::publishing::SourceGroup;
    share_opened(s);
    let _ = s.update(crate::Message::ShareGroup(SourceGroup::Window));
    finish_share_previews(s);
}

/// On a Mac the system asks which screen or window once sharing starts, so the one tile says so.
fn share_mac(s: &mut App) {
    use crate::streaming::publishing::{ShareSource, SourceKind};
    connected(s);
    s.server_mut().streaming.share_open = true;
    s.server_mut().streaming.sharing.loading = true;
    let sources = vec![
        ShareSource {
            kind: SourceKind::Screen(None),
            name: "Screen or window".into(),
        },
        ShareSource {
            kind: SourceKind::Camera("demo-camera".into()),
            name: "FaceTime HD Camera".into(),
        },
    ];
    let _ = s.update(crate::Message::ShareSources(Ok(sources)));
}

/// Every value one by one, and what gives way when the connection struggles.
fn share_advanced(s: &mut App) {
    share(s);
    s.ui.share_advanced = true;
}

/// A value changed on the Advanced tab: the simple tab picks neither preset and says so.
fn share_custom(s: &mut App) {
    share(s);
    s.server_mut().streaming.sharing.quality.frame_rate = 60;
}

/// Sharing with system audio going out: the sound button is lit and the meter moves.
fn sharing_sound(s: &mut App) {
    sharing(s);
    let status = &mut s.server_mut().streaming.sharing.status;
    status.audio_frames = 1200;
    status.audio_level = 0.45;
}

/// Sharing without system audio: the sound button is off and there is no meter.
fn sharing_audio_off(s: &mut App) {
    sharing(s);
    s.server_mut().streaming.sharing.audio = false;
}

/// The open "You are sharing" dialog says that AV1 and fast capture are both in use.
fn sharing_stream_options(s: &mut App) {
    sharing(s);
    let sharing = &mut s.server_mut().streaming;
    sharing.share_open = true;
    sharing.sharing.status.detail = "Public · Hardware AV1 · Fast capture".into();
    sharing.sharing.status.access = crate::streaming::publishing::Access::Public;
}

/// AV1 and fast capture were asked for but are unavailable: the dialog says what runs.
fn sharing_stream_options_unavailable(s: &mut App) {
    sharing_stream_options(s);
    s.server_mut().streaming.sharing.status.detail =
        "Public · VP8, AV1 unavailable · Standard capture".into();
}

/// Fast capture gave up during the share: a notice says so, and the dialog names the
/// standard capture the share carries on with.
fn sharing_fast_capture_fallback(s: &mut App) {
    sharing_stream_options(s);
    s.server_mut().streaming.sharing.status.detail = "Public · Standard capture".into();
    s.alerts.raise(
        Source::StreamCapture,
        "Fast capture stopped working (the GPU has not finished a frame in 2 s). Your share carries on.",
    );
}

/// Your share is the only one in the channel: the pane says so in place of its empty state.
fn sharing_alone(s: &mut App) {
    sharing(s);
    s.server_mut().streaming.catalog.clear();
}

fn sharing(s: &mut App) {
    connected(s);
    share_sources(s);
    let status = &mut s.server_mut().streaming.sharing.status;
    status.active = true;
    status.live = true;
    status.detail = "Private · Fast capture".into();
    status.source = "Counter-Strike 2".into();
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
            // An older client's name says only what kind of share it is: Kai's name leads.
            ("s3", 5, "Screen share", Phase::Receiving),
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

/// The only stream in the channel: edge to edge, its bar hidden until the pointer is over it.
fn watching_solo(s: &mut App) {
    use crate::streaming::Phase;
    watch(s, &[("s1", 2, "Mara’s screen", Phase::Receiving)]);
}

fn watching_solo_hover(s: &mut App) {
    watching_solo(s);
    s.ui.streams_hover = true;
}

/// The stream fills the screen: no header, sidebar, chat or status bar, its bar on hover.
fn watching_fullscreen(s: &mut App) {
    watching_solo_hover(s);
    s.ui.fullscreen = Some(false);
    s.ui.window_fullscreen = true;
}

/// The other stream tucked away: the watched one fills the pane, a tab brings the other back.
fn watching_others_hidden(s: &mut App) {
    watching(s);
    s.server_mut().streaming.others_hidden = true;
}

fn watching_others_hidden_hover(s: &mut App) {
    watching_others_hidden(s);
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
    status.detail = "Private · Fast capture".into();
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

/// One watched stream's picture reads as `media`.
fn stream_media(s: &mut App, slot: usize, media: crate::streaming::health::Media) {
    if let Some(watch) = s
        .server_mut()
        .streaming
        .watching
        .iter_mut()
        .find(|watch| watch.slot == slot)
    {
        watch.stats.media = media;
    }
}

/// A poor reading that has lasted past the hold, so it shows.
fn poor_reading() -> usage::ConnectionUsage {
    usage::ConnectionUsage {
        ping_ms: Some(240.0),
        ping_deviation_ms: Some(60.0),
        packet_loss_in: Some(6.0),
        packet_loss_out: Some(4.0),
        ..Default::default()
    }
}

fn steady(health: usage::Health) -> usage::SteadyHealth {
    let mut steady = usage::SteadyHealth::default();
    let now = Instant::now();
    steady.update(health, now - Duration::from_secs(10));
    steady.update(health, now);
    steady
}

/// Your own connection struggles too: the stream's trouble is put on your side.
fn watching_trouble_yours(s: &mut App) {
    watching_solo(s);
    s.server_mut().connection_usage = poor_reading();
    s.server_mut().connection_health = steady(usage::Health::Poor);
    stream_media(s, 0, crate::streaming::health::Media::Choppy);
}

/// Mara's connection to the server struggles: the trouble is put on the streamer's side.
fn watching_trouble_streamer(s: &mut App) {
    watching_solo(s);
    s.server_mut().client_usage.insert(
        2,
        Ok(usage::ServerUsage::preview(poor_reading(), Instant::now())),
    );
    s.server_mut()
        .client_health
        .insert(2, steady(usage::Health::Poor));
    stream_media(s, 0, crate::streaming::health::Media::Choppy);
}

/// Both server connections are fine, yet the stream stutters.
fn watching_trouble_between(s: &mut App) {
    watching_solo(s);
    stream_media(s, 0, crate::streaming::health::Media::Choppy);
}

/// The connection is alive but no new picture arrives.
fn watching_trouble_paused(s: &mut App) {
    watching_solo(s);
    stream_media(s, 0, crate::streaming::health::Media::Paused);
}

/// Frames arrive but this computer drops them.
fn watching_trouble_computer(s: &mut App) {
    watching_solo(s);
    stream_media(s, 0, crate::streaming::health::Media::Decoding);
}

/// The connection dropped and is trying to recover; neither side's server link explains it.
fn watching_trouble_reconnecting(s: &mut App) {
    watching_solo(s);
    stream_media(s, 0, crate::streaming::health::Media::Reconnecting);
}

fn watching_trouble_streamer_reconnecting(s: &mut App) {
    watching_trouble_streamer(s);
    stream_media(s, 0, crate::streaming::health::Media::Reconnecting);
}

/// With the pointer over the streams, the sign steps below the floating toolbar.
fn watching_trouble_hover(s: &mut App) {
    watching_trouble_yours(s);
    s.ui.streams_hover = true;
}

/// Trouble on a small tile under the stage shows the sign alone.
fn watching_trouble_thumbnail(s: &mut App) {
    watching_five_below(s);
    stream_media(s, 2, crate::streaming::health::Media::Paused);
    stream_media(s, 1, crate::streaming::health::Media::Choppy);
}

/// A quiet streamer turned up past 100%, with the bar showing.
fn watching_loud(s: &mut App) {
    watching_solo_hover(s);
    s.server_mut().streaming.set_volume(2, 250);
}

/// The open "You are sharing" dialog with system audio on: quality and sound change live.
fn sharing_live_audio(s: &mut App) {
    sharing_stream_options(s);
    s.server_mut().streaming.sharing.audio = true;
    s.server_mut().streaming.sharing.status.audio_frames = 1200;
    s.server_mut().streaming.sharing.status.audio_level = 0.4;
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
    // A few decoded frames were replaced by newer ones before a draw showed them.
    let surface = s.server().streaming.surfaces.get(0).clone();
    for _ in 0..1_786 {
        surface.note_presented();
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

/// The window's close button while connected, with the tray icon on.
fn close_prompt(s: &mut App) {
    connected(s);
    s.appearance.hide_tray_icon = false;
    s.close_prompt = true;
}

/// The same with the tray icon switched off: nowhere to hide, so Quit is the default.
fn close_prompt_no_tray(s: &mut App) {
    connected(s);
    s.appearance.hide_tray_icon = true;
    s.close_prompt = true;
}

/// Disconnect while sharing live asks first.
fn disconnect_sharing(s: &mut App) {
    sharing(s);
    s.disconnect_prompt = true;
}

fn password(s: &mut App) {
    connected(s);
    s.server_mut().password_channel = Some(3);
}

/// The server refused to let you in without its password.
fn server_password(s: &mut App) {
    home(s);
    s.server_password_prompt = Some(crate::ServerPasswordPrompt {
        index: 0,
        title: "Zirkumflex".into(),
        ..Default::default()
    });
    s.ui.prompt_password.focused = true;
}

/// The password tried was wrong: the prompt stays with a reason.
fn server_password_wrong(s: &mut App) {
    server_password(s);
    if let Some(prompt) = &mut s.server_password_prompt {
        prompt.error = Some("That password wasn't accepted.".into());
    }
}

/// A saved server that never answered: the strip offers to try again.
fn alert(s: &mut App) {
    home(s);
    s.alerts.raise_at(
        "ts.zirkumflex.io",
        Some("Zirkumflex".into()),
        Source::Connection,
        "The server did not answer within 10 seconds. Check the address and your network, then try again.",
    );
}

/// Kicked from a server: back on the saved servers, with a way back in.
fn alert_dropped(s: &mut App) {
    home(s);
    s.alerts.raise_at(
        "ts.zirkumflex.io",
        Some("Zirkumflex".into()),
        Source::Dropped,
        "TeamSpeak connection failed: You were kicked from the server: Take a break",
    );
}

/// Kicked: the strip says so and offers no Reconnect.
fn alert_kicked(s: &mut App) {
    home(s);
    s.alerts.raise_refused_at(
        "ts.zirkumflex.io",
        Some("Zirkumflex".into()),
        Source::Dropped,
        crate::client::Refusal::Kicked.sentence(),
        "TeamSpeak connection failed: Connection aborted",
        false,
    );
}

const FALLBACK: &str = "“Arctis Nova 7” isn't connected, so you hear everyone through “Speakers (Realtek Audio)” until it's back.";

/// Headphones unplugged mid-call: still connected, playing through the speakers.
fn alert_audio_fallback(s: &mut App) {
    connected(s);
    s.alerts.raise(Source::AudioFallback, FALLBACK);
}

fn alert_no_audio(s: &mut App) {
    connected(s);
    s.alerts.raise(
        Source::Audio,
        "No playback device is connected. You stay connected, and Flexspeak picks your audio up again as soon as a device works.",
    );
}

const TAKEN: &str = "Another app is using Ctrl+Shift+M, so the mute shortcut is off. Flexspeak takes it as soon as it's free.";

/// Launched while another app holds a shortcut: the other shortcuts work.
fn alert_shortcut_taken(s: &mut App) {
    connected(s);
    s.alerts.raise(Source::Shortcuts, TAKEN);
}

fn alert_several(s: &mut App) {
    alert_shortcut_taken(s);
    s.alerts.raise(Source::AudioFallback, FALLBACK);
    let id = s.active.expect("connected").0;
    for _ in 0..2 {
        s.alerts.raise_in(
            id,
            None,
            Source::Poke,
            "That client is no longer on the server.",
        );
    }
}

/// A long detail, cut to fit the card.
fn alert_details(s: &mut App) {
    home(s);
    s.alerts.raise(
        Source::SettingsLoad,
        "expected `,` or `}` at line 14 column 3 in settings.json. The file was not changed. \
         Repair it and Reload, or Save to replace it with the values shown.",
    );
}

fn settings_error_log_empty(s: &mut App) {
    settings_at(s, SettingsPage::Errors);
}

/// Debug logging on, and a diagnostics bundle just saved for a bug report.
fn settings_diagnostics_debug(s: &mut App) {
    settings_at(s, SettingsPage::Errors);
    s.debug_logging = true;
    s.diagnostics_path =
        Some("/Users/jan/Downloads/flexspeak-diagnostics-20261113-142210.zip".into());
}

/// A session where a few things went wrong: some recovered, one repeated.
fn settings_error_log(s: &mut App) {
    connected(s);
    let id = s.active.expect("connected").0;
    s.alerts.raise(Source::Shortcuts, TAKEN);
    s.alerts.recover(Source::Shortcuts);
    s.alerts.raise(Source::AudioFallback, FALLBACK);
    s.alerts.raise(Source::AudioFallback, FALLBACK);
    s.alerts.recover(Source::AudioFallback);
    s.alerts.raise_at(
        "ts.zirkumflex.io",
        Some("Zirkumflex".into()),
        Source::Dropped,
        "TeamSpeak connection failed: The server closed the connection.",
    );
    s.alerts.recover_at("ts.zirkumflex.io");
    for _ in 0..3 {
        s.alerts.raise_in(
            id,
            None,
            Source::Poke,
            "That client is no longer on the server.",
        );
    }
    s.alerts = {
        // Their notices are long gone; only the log remains.
        let mut alerts = std::mem::take(&mut s.alerts);
        for incident in alerts
            .visible()
            .iter()
            .map(|incident| incident.id)
            .collect::<Vec<_>>()
        {
            alerts.dismiss(incident);
        }
        alerts
    };
    s.settings_path = Some("/Users/jan/Library/Application Support/Flexspeak/settings.json".into());
    s.log_folder = Some("/Users/jan/Library/Application Support/Flexspeak/logs".into());
    s.open_settings(SettingsPage::Errors);
}

/// Over Settings, the cards move to the bottom corner, clear of its header.
fn alert_settings(s: &mut App) {
    settings_shortcuts(s);
    s.alerts.raise(Source::Shortcuts, TAKEN);
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
            mirror: None,
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

/// Settings with a downloaded build waiting and the notice closed. What's New leads with the
/// waiting build's notes.
fn update_settings(s: &mut App) {
    settings_at(s, SettingsPage::Updates);
    with_update(s, Phase::Ready);
    if let Some(release) = &mut s.updater.release {
        *release.notes = demo_notes();
    }
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
        // Hundreds of scene runs take most of the suite's time, so spread them over every core.
        let cases: Vec<_> = SCENES
            .iter()
            .flat_map(|scene| [(scene, crate::MIN_SIZE), (scene, (1400.0, 900.0))])
            .collect();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        std::thread::scope(|scope| {
            for chunk in cases.chunks(cases.len().div_ceil(threads)) {
                scope.spawn(move || {
                    for &((name, scene), size) in chunk {
                        let mut rt = runtime(*scene, size);
                        for _ in 0..30 {
                            rt.frame(1.0 / 30.0);
                        }
                        assert!(
                            rt.find("content").is_some(),
                            "{name} at {size:?} has no content"
                        );
                    }
                });
            }
        });
    }

    fn peek_at(peek: &super::super::RailPeek, base: Instant, ms: u64) -> bool {
        peek.open(base + Duration::from_millis(ms))
    }

    // Sharing, and so going live with one click, only exists on Mac and Windows.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[test]
    fn going_live_is_offered_in_the_empty_pane_or_above_the_voice_buttons_until_you_are_live() {
        let mut rt = runtime(share_quick, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("go-live-0").is_some(), "the main screen");
        assert!(rt.find("go-live-1").is_some(), "the game");
        assert!(rt.find("go-live-options").is_some(), "the dialog");
        assert!(rt.find("quick-share").is_none(), "offered once");

        // Streams fill the pane, so the chips take over.
        let mut rt = runtime(share_quick_streams, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("go-live").is_none());
        assert!(rt.find("quick-share-0").is_some(), "the main screen");
        assert!(rt.find("quick-share-1").is_some(), "the game");

        for scene in [sharing as fn(&mut App), sharing_alone] {
            let mut rt = runtime(scene, (900.0, 790.0));
            settle(&mut rt);
            assert!(rt.find("quick-share").is_none());
            assert!(rt.find("go-live").is_none());
        }
    }

    #[test]
    fn the_hidden_rail_opens_after_a_pause_and_forgives_a_slip() {
        let base = Instant::now();
        let at = |ms| base + Duration::from_millis(ms);
        let mut peek = super::super::RailPeek::default();
        assert!(!peek.open(base));
        peek.hover(at(0), |peek| peek.strip = true);
        assert!(
            !peek_at(&peek, base, 100),
            "crossing the edge doesn't open it"
        );
        assert!(peek_at(&peek, base, 200));

        peek.hover(at(200), |peek| peek.card = true);
        peek.hover(at(210), |peek| peek.strip = false);
        assert!(
            peek_at(&peek, base, 2000),
            "resting on the open rail keeps it"
        );

        peek.hover(at(2000), |peek| peek.card = false);
        assert!(peek_at(&peek, base, 2100), "a brief slip keeps it");
        peek.hover(at(2100), |peek| peek.card = true);
        assert!(peek_at(&peek, base, 2500), "coming back keeps it");
        peek.hover(at(2500), |peek| peek.card = false);
        assert!(!peek_at(&peek, base, 2900));

        peek.hover(at(3000), |peek| peek.strip = true);
        peek.hover(at(3050), |peek| peek.strip = false);
        assert!(
            !peek_at(&peek, base, 3400),
            "leaving before the pause cancels it"
        );
    }

    #[test]
    fn the_keyboard_opens_the_hidden_rail_until_pressed_again() {
        let now = Instant::now();
        let mut peek = super::super::RailPeek::default();
        peek.press();
        assert!(peek.open(now));
        assert!(peek.unpin());
        assert!(!peek.open(now));
    }

    #[test]
    fn one_server_hides_the_rail_and_a_second_brings_it_back() {
        let mut rt = runtime(connected_quiet, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("server-rail").is_none());
        let (x, y) = rt.find("rail-edge").expect("the handle");
        rt.poll = Some(super::super::poll);
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        assert!(
            rt.find("rail-peek").is_none(),
            "not until the pause is over"
        );
        assert!(
            rt.wake_deadline().is_some(),
            "something will wake the window"
        );
        std::thread::sleep(Duration::from_millis(200));
        settle(&mut rt);
        assert!(
            rt.find("rail-peek").is_some(),
            "the delay redraws by itself"
        );
        assert!(rt.find("rail-add").is_some(), "the + stays reachable");

        let mut rt = runtime(servers_rail, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("server-rail").is_some());
        assert!(rt.find("rail-edge").is_none());
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
    fn the_server_heads_the_tree_with_one_server_or_several() {
        let mut rt = runtime(connected, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("server-root").is_some());
        let mut rt = runtime(servers_rail, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("server-root").is_some());
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
        let mut rt = runtime(share_advanced, crate::MIN_SIZE);
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
    fn share_quality_controls_are_independent_validate_and_change_live() {
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
        // While live, quality and sound change at once; what is shared and who may watch stay.
        s.server_mut().streaming.sharing.status.active = true;
        let _ = s.update(Message::ShareQuality(Preset::Performance.quality()));
        let _ = s.update(Message::ShareAudio(true));
        let _ = s.update(Message::ShareAccess(Access::Private));
        let _ = s.update(Message::ShareSource(
            s.server().streaming.sharing.sources[0].clone(),
        ));
        assert_eq!(
            s.server().streaming.sharing.quality,
            Preset::Performance.quality()
        );
        assert!(s.server().streaming.sharing.audio);
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

    #[test]
    fn a_channel_joins_on_a_click_and_offers_a_menu() {
        let mut rt = runtime(channel_tree, (900.0, 790.0));
        settle(&mut rt);
        let name = "channel-2/name";
        let (x, y) = rt.find(name).unwrap();
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        rt.secondary_down(x, y);
        rt.secondary_up(x, y);
        settle(&mut rt);
        assert_eq!(rt.state.ui.channel_menu.map(|(open, _)| open), Some(2));
        rt.key(rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        });
        settle(&mut rt);
        assert!(rt.state.ui.channel_menu.is_none());
        assert_eq!(
            rt.state.server().expected_channel,
            None,
            "the menu didn't join"
        );
        click(&mut rt, name);
        assert_eq!(
            rt.state.server().expected_channel,
            Some(2),
            "one click joins"
        );
        // A spacer is a label: it never joins.
        let mut rt = runtime(channel_tree_spacers, (900.0, 790.0));
        settle(&mut rt);
        let (x, y) = rt.find("channel-101/name").expect("the spacer");
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        settle(&mut rt);
        assert_eq!(rt.state.server().expected_channel, None);
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
        let row = "member-menu-item8-sub-item";
        click(&mut rt, &format!("{row}1"));
        settle(&mut rt);
        click(&mut rt, &format!("{row}4"));
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
    fn searching_names_a_theme_and_opens_its_collection() {
        let mut rt = runtime(settings_more_themes, (1000.0, 1500.0));
        settle(&mut rt);
        assert!(rt.find("found-appearance-catppuccin-mocha").is_some());
        assert!(rt.find("found-appearance-tokyo-night-night").is_none());
        // The page under the results is whole: nothing is filtered out of it.
        assert!(rt.find("extra-tokyo-night-night").is_some());
        assert!(rt.find("open-settings-folder").is_some());
        click(&mut rt, "result-0");
        assert_eq!(rt.state.settings_query, "");
        assert_eq!(
            rt.state
                .ui
                .settings_target
                .as_ref()
                .map(|(key, _)| key.as_str()),
            Some("row-more-themes-catppuccin")
        );
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
        scroll_into_view(
            &mut rt,
            "settings-body-Appearance",
            "tokyo-night-storm-dark",
        );
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
    #[cfg(any(windows, target_os = "macos"))]
    fn the_tray_icon_stays_on_until_switched_off_in_general() {
        let mut rt = runtime(settings_general, (1000.0, 1500.0));
        settle(&mut rt);
        assert!(!rt.state.appearance.hide_tray_icon, "on by default");
        assert!(rt.state.tray_status().is_some());
        let root = std::env::temp_dir().join(format!("flexspeak-tray-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        crate::settings::save(&path, &crate::settings::Settings::default()).unwrap();
        rt.state.settings_path = Some(path.clone());
        scroll_into_view(&mut rt, "settings-body-General", "tray-icon");
        click(&mut rt, "tray-icon");
        assert!(rt.state.appearance.hide_tray_icon);
        assert!(rt.state.tray_status().is_none(), "no icon once it's off");
        let saved = crate::settings::load(&path).unwrap().unwrap();
        assert!(saved.appearance.hide_tray_icon, "saved right away");
        click(&mut rt, "tray-icon");
        assert!(!rt.state.appearance.hide_tray_icon);
        let _ = std::fs::remove_dir_all(root);
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
        // Motion follows More Themes, below the fold.
        scroll_into_view(&mut rt, "settings-body-Appearance", "disable-animations");
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

    #[test]
    fn signing_in_stands_in_the_monster_pages_place_until_it_is_done() {
        let mut rt = runtime(monster_sign_in_passkey, (1100.0, 720.0));
        settle(&mut rt);
        assert!(rt.find("monster-sign-in").is_some());
        assert!(rt.find("monster-add").is_none(), "the page shows behind it");
        // A passkey can't be skipped.
        assert!(rt.find("monster-enter").is_none());

        let mut rt = runtime(monster_sign_in_identity, (1100.0, 720.0));
        settle(&mut rt);
        // Linking the identity is optional: the page opens without it.
        click(&mut rt, "monster-enter");
        settle(&mut rt);
        assert!(rt.find("monster-sign-in").is_none());
        assert!(rt.find("monster-add").is_some());
        assert!(rt.find("monster-account").is_some(), "signed in");
    }

    #[test]
    fn without_a_server_the_page_needs_no_account() {
        let mut rt = runtime(monster_board, (1100.0, 900.0));
        settle(&mut rt);
        assert!(rt.find("monster-sign-in").is_none());
        assert!(rt.find("monster-account").is_none());
        click(&mut rt, "monster-tab-leaderboard");
        settle(&mut rt);
        assert!(rt.find("monster-leaderboard-local").is_some());
        assert!(
            rt.state.monster.sent.is_empty(),
            "nothing asked of a server"
        );
    }

    #[test]
    fn the_identity_is_linked_through_the_server() {
        use crate::monster::{Answer, Effect, api};
        let mut rt = runtime(monster_sign_in_identity, (1100.0, 900.0));
        settle(&mut rt);
        click(&mut rt, "monster-link-identity-0");
        settle(&mut rt);
        assert_eq!(rt.state.monster.sent, [Effect::Link]);
        assert!(
            rt.find("monster-linked-0").is_none(),
            "not until it answers"
        );
        deliver(
            &mut rt,
            crate::Message::MonsterAnswer(Answer::Linked(Ok(api::Me {
                id: 1,
                name: "Sam".into(),
                email: SCENE_EMAIL.into(),
                hidden: false,
                imported: true,
                identities: vec![api::LinkedIdentity {
                    uid: SCENE_UID.into(),
                    source: "Flexspeak".into(),
                    linked_at: 1,
                }],
            }))),
        );
        settle(&mut rt);
        assert!(rt.find("monster-linked-0").is_some());
        click(&mut rt, "monster-enter");
        settle(&mut rt);
        assert_eq!(rt.state.monster.gate, None);
    }

    #[test]
    fn a_leaderboard_row_opens_that_persons_board_and_back_returns() {
        use crate::monster::{Answer, Effect, Ranking, Section, api};
        let mut rt = runtime(monster_leaderboard, (1100.0, 1200.0));
        settle(&mut rt);
        click(&mut rt, "monster-person-3");
        settle(&mut rt);
        assert_eq!(rt.state.monster.section, Section::Person(3));
        assert_eq!(rt.state.monster.sent, [Effect::Person(3)]);
        assert!(rt.find("monster-person-loading").is_some());
        let cans = scene_cans(3);
        deliver(
            &mut rt,
            crate::Message::MonsterAnswer(Answer::Person(
                3,
                Ok(api::Collection {
                    user: 3,
                    name: "Kai".into(),
                    cans: cans
                        .iter()
                        .map(|can| api::Can {
                            id: can.id,
                            flavour: can.flavour.clone(),
                            at: can.at,
                            rarity: can.rarity,
                            seed: can.seed,
                            legacy: false,
                        })
                        .collect(),
                }),
            )),
        );
        settle(&mut rt);
        assert!(rt.find("monster-person-loading").is_none());
        assert!(rt.find("monster-person").is_some());
        // Someone else's cards can't be featured or removed.
        let selected = rt.state.monster.selected.clone();
        let card = format!("card-{}", cans.last().unwrap().identity());
        click(&mut rt, &card);
        settle(&mut rt);
        assert_eq!(rt.state.monster.selected, selected);
        let back = rt.state.back();
        rt.state.update(back);
        settle(&mut rt);
        assert_eq!(rt.state.monster.section, Section::Leaderboard);
        assert_eq!(
            rt.state.monster.sent.last(),
            Some(&Effect::Leaderboard(Ranking::Collection)),
            "back to a fresh leaderboard"
        );
    }

    #[test]
    fn changing_the_ranking_asks_the_server_again() {
        use crate::monster::{Effect, Ranking};
        let mut rt = runtime(monster_leaderboard, (1100.0, 1200.0));
        settle(&mut rt);
        click(&mut rt, "monster-rank-legendary");
        settle(&mut rt);
        assert_eq!(rt.state.monster.ranking, Ranking::Legendary);
        assert_eq!(
            rt.state.monster.sent,
            [Effect::Leaderboard(Ranking::Legendary)]
        );
    }

    #[test]
    fn the_leaderboard_never_ranks_by_cans_drunk() {
        use crate::monster::{Can, Person, Ranking, ranked, standing};
        let can = |flavour: &str, n: i64| Can {
            id: n,
            flavour: flavour.into(),
            at: 0,
            rarity: crate::monster::Rarity::Common,
            seed: 0,
        };
        // A hundred cans of one flavour against three different flavours.
        let heavy: Vec<Can> = (0..100).map(|n| can("original", n)).collect();
        let varied: Vec<Can> = ["original", "ultra-gold", "mango-loco"]
            .iter()
            .enumerate()
            .map(|(n, flavour)| can(flavour, n as i64))
            .collect();
        let people = [
            Person {
                user: 1,
                name: "Heavy".into(),
                you: false,
                standing: standing(&heavy),
            },
            Person {
                user: 2,
                name: "Varied".into(),
                you: false,
                standing: standing(&varied),
            },
        ];
        assert_eq!(ranked(&people, Ranking::Flavours, true)[0].user, 2);
    }

    #[test]
    fn hiding_takes_you_off_the_leaderboard() {
        use crate::monster::Effect;
        let mut rt = runtime(monster_leaderboard, (1100.0, 1200.0));
        settle(&mut rt);
        assert!(rt.find("monster-person-1").is_some());
        click(&mut rt, "monster-hide-me");
        settle(&mut rt);
        assert_eq!(rt.state.monster.sent, [Effect::Hide(true)]);
        assert!(rt.find("monster-person-1").is_none(), "at once");
        assert!(rt.find("monster-show-me").is_some());
        click(&mut rt, "monster-show-me");
        settle(&mut rt);
        assert_eq!(rt.state.monster.sent.last(), Some(&Effect::Hide(false)));
        assert!(rt.find("monster-person-1").is_some());
    }

    #[test]
    fn a_code_is_sent_only_to_something_shaped_like_an_address() {
        use crate::monster::{Answer, Effect, Gate};
        let mut rt = runtime(monster_sign_in_email, (1100.0, 720.0));
        settle(&mut rt);
        click(&mut rt, "monster-send-code");
        settle(&mut rt);
        assert_eq!(
            rt.state.monster.sent,
            [Effect::LoginStart(SCENE_EMAIL.into())]
        );
        assert_eq!(rt.state.monster.gate, Some(Gate::Email), "until it answers");
        deliver(
            &mut rt,
            crate::Message::MonsterAnswer(Answer::LoginStarted(SCENE_EMAIL.into(), Ok(()))),
        );
        settle(&mut rt);
        assert_eq!(rt.state.monster.gate, Some(code_step(false, None)));

        let mut rt = runtime(
            |s| {
                monster_sign_in_email(s);
                s.ui.monster_email.text = "sam".into();
            },
            (1100.0, 720.0),
        );
        settle(&mut rt);
        click(&mut rt, "monster-send-code");
        settle(&mut rt);
        assert_eq!(rt.state.monster.gate, Some(Gate::Email));
        assert!(rt.state.monster.sent.is_empty());
    }

    #[test]
    fn a_wrong_code_shows_the_tries_the_server_counts() {
        use crate::monster::{Answer, Effect, api};
        let mut rt = runtime(monster_sign_in_code, (1100.0, 720.0));
        settle(&mut rt);
        click(&mut rt, "monster-check-code");
        settle(&mut rt);
        assert_eq!(
            rt.state.monster.sent,
            [Effect::LoginFinish {
                email: SCENE_EMAIL.into(),
                code: "482913".into()
            }]
        );
        deliver(
            &mut rt,
            crate::Message::MonsterAnswer(Answer::LoginFinished(Err(api::Problem {
                error: api::problem::WRONG_CODE.into(),
                message: "That code doesn't match.".into(),
                tries_left: Some(2),
            }))),
        );
        settle(&mut rt);
        assert_eq!(rt.state.monster.gate, Some(code_step(true, Some(2))));
        assert!(rt.find("monster-wrong-code").is_some());
        // Typing again clears it.
        deliver(&mut rt, crate::Message::MonsterSignInTyped);
        settle(&mut rt);
        assert!(rt.find("monster-wrong-code").is_none());
    }

    #[test]
    fn the_passkey_opens_in_the_browser_and_cancel_stops_waiting() {
        use crate::monster::Effect;
        let mut rt = runtime(monster_sign_in_passkey, (1100.0, 720.0));
        settle(&mut rt);
        click(&mut rt, "monster-passkey");
        settle(&mut rt);
        assert_eq!(
            rt.state.monster.sent[0],
            Effect::Browse(format!("{SCENE_SERVER}/passkey#ticket=scene-ticket"))
        );
        assert!(matches!(rt.state.monster.sent[1], Effect::Poll { .. }));
        assert!(rt.find("monster-passkey-browser").is_some());
        click(&mut rt, "monster-passkey-cancel");
        settle(&mut rt);
        assert_eq!(rt.state.monster.gate, Some(passkey_step(true, false)));
    }

    #[test]
    fn a_flavour_tapped_online_opens_its_card_when_the_server_answers() {
        use crate::monster::{Answer, Effect, Rarity, api};
        let mut rt = runtime(monster_signed_in, (1100.0, 1500.0));
        settle(&mut rt);
        let before = rt.state.monster.cans.len();
        click(&mut rt, "monster-flavour-khaotic");
        settle(&mut rt);
        assert!(rt.state.monster.sent.is_empty(), "it asks first");
        click(&mut rt, "add-can-confirm");
        settle(&mut rt);
        let [Effect::Add(add)] = &rt.state.monster.sent[..] else {
            panic!("{:?}", rt.state.monster.sent)
        };
        assert_eq!(add.flavour, "khaotic");
        assert_eq!(
            rt.state.monster.cans.len(),
            before,
            "nothing until it answers"
        );
        // While it's out, another tap waits.
        click(&mut rt, "monster-flavour-khaotic");
        settle(&mut rt);
        assert_eq!(rt.state.monster.sent.len(), 1);
        deliver(
            &mut rt,
            crate::Message::MonsterAnswer(Answer::Added(Ok(api::Can {
                id: 999,
                flavour: "khaotic".into(),
                at: chrono::Utc::now().timestamp(),
                rarity: Rarity::Rare,
                seed: 5,
                legacy: false,
            }))),
        );
        settle(&mut rt);
        assert_eq!(rt.state.monster.cans.len(), before + 1);
        assert!(rt.state.monster.reveal.is_some(), "it opens");

        // A refusal shows the server's words and adds nothing.
        let mut rt = runtime(monster_signed_in, (1100.0, 1500.0));
        settle(&mut rt);
        click(&mut rt, "monster-flavour-khaotic");
        settle(&mut rt);
        click(&mut rt, "add-can-confirm");
        settle(&mut rt);
        deliver(
            &mut rt,
            crate::Message::MonsterAnswer(Answer::Added(Err(api::Problem {
                error: api::problem::SLOW_DOWN.into(),
                message: "Too many cans at once. Wait a minute.".into(),
                tries_left: None,
            }))),
        );
        settle(&mut rt);
        assert_eq!(rt.state.monster.cans.len(), before);
        assert!(rt.find("monster-notice").is_some());
        assert!(rt.state.monster.reveal.is_none());
    }

    #[test]
    fn sign_out_returns_to_the_first_step() {
        use crate::monster::{Effect, Gate};
        let mut rt = runtime(monster_signed_in, (1100.0, 900.0));
        settle(&mut rt);
        click(&mut rt, "monster-sign-out");
        settle(&mut rt);
        assert_eq!(
            rt.state.monster.sent,
            [Effect::Keep(None), Effect::SignOut("scene".into())]
        );
        assert_eq!(rt.state.monster.gate, Some(Gate::Email));
        assert!(rt.find("monster-sign-in").is_some());
        assert!(rt.state.monster.cans.is_empty());
    }

    /// Hands the app a message from outside the view, as a finished task does, and redraws.
    fn deliver(rt: &mut Runtime<App>, message: crate::Message) {
        let _ = rt.state.update(message);
        rt.invalidate();
    }

    fn settle(rt: &mut Runtime<App>) {
        for _ in 0..30 {
            rt.frame(1.0 / 60.0);
        }
    }

    /// Scrolls `body` a notch at a time until `key` sits wholly inside it.
    fn scroll_into_view(rt: &mut Runtime<App>, body: &str, key: &str) {
        for _ in 0..40 {
            let ([_, top, _, height], _) =
                rt.presented(body).unwrap_or_else(|| panic!("no {body}"));
            let ([_, y, _, h], _) = rt.presented(key).unwrap_or_else(|| panic!("no {key}"));
            if y >= top && y + h <= top + height {
                return;
            }
            let (x, center) = rt.find(body).unwrap();
            rt.wheel(x, center, if y < top { 60.0 } else { -60.0 });
            settle(rt);
        }
        panic!("{key} never came into view");
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

    fn temp_settings(rt: &mut Runtime<App>, name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "flexspeak-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        rt.state.settings_path = Some(root.join("settings.json"));
        root
    }

    #[test]
    fn the_chat_column_switches_tabs_takes_typing_hides_and_comes_back() {
        let mut rt = runtime(chat_tabs, (1180.0, 790.0));
        let root = temp_settings(&mut rt, "chat-column");
        settle(&mut rt);
        assert!(rt.find("chat").is_some() && rt.find("chat-edge").is_none());
        assert_eq!(rt.state.server().chat.unread(&mara()), 2);

        click(&mut rt, "chat-tab-uid-2");
        assert_eq!(rt.state.server().chat.target, mara());
        assert_eq!(
            rt.state.server().chat.unread(&mara()),
            0,
            "opening it reads it"
        );
        assert!(rt.state.ui.chat_input.focused, "ready to answer");
        rt.key(rune::KeyEvent {
            key: rune::Key::Text("ok".into()),
            mods: rune::Mods::default(),
        });
        settle(&mut rt);
        assert_eq!(rt.state.server().chat.draft, "ok");
        click(&mut rt, "chat-tab-uid-2-close");
        assert_eq!(rt.state.server().chat.target, chat::Scope::Channel);
        assert!(rt.find("chat-tab-uid-2").is_none());

        click(&mut rt, "chat-hide");
        assert!(rt.state.chat_options.hidden);
        assert!(rt.find("chat").is_none(), "the stage has the room");
        assert!(rt.find("chat-edge").is_some(), "a handle stays");
        assert!(
            crate::settings::load(&root.join("settings.json"))
                .unwrap()
                .unwrap()
                .chat
                .hidden,
            "saved right away"
        );
        click(&mut rt, "chat-edge");
        assert!(!rt.state.chat_options.hidden && rt.find("chat").is_some());

        // ⇧⌘C does the same from the keyboard.
        rt.key(rune::KeyEvent {
            key: rune::Key::Command('c'),
            mods: rune::Mods {
                shift: true,
                cmd: true,
                ..Default::default()
            },
        });
        settle(&mut rt);
        assert!(rt.state.chat_options.hidden);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_narrow_window_folds_the_chat_column_to_its_handle() {
        let mut rt = runtime(chat_tabs, crate::MIN_SIZE);
        settle(&mut rt);
        assert!(rt.find("chat").is_none() && rt.find("chat-edge").is_some());
        assert!(!rt.state.chat_options.hidden, "the choice itself stays");
        let mut rt = runtime(chat_tabs, (1180.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("chat").is_some());
    }

    #[test]
    fn double_clicking_someone_opens_their_private_chat() {
        let mut rt = runtime(connected, (1180.0, 790.0));
        settle(&mut rt);
        let (x, y) = rt.find("member-2").expect("Mara's row");
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        settle(&mut rt);
        assert_eq!(
            rt.state.server().chat.target,
            chat::Scope::Private("uid-2".into())
        );
        assert!(rt.state.ui.chat_input.focused, "ready to write");
        assert!(rt.find("chat-tab-uid-2").is_some());
        // Your own row does nothing.
        let (x, y) = rt.find("member-1").unwrap();
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
        }
        settle(&mut rt);
        assert_eq!(
            rt.state.server().chat.target,
            chat::Scope::Private("uid-2".into())
        );
    }

    #[test]
    fn a_left_channel_keeps_a_tab_to_read_and_offers_to_join_again() {
        let mut rt = runtime(chat_history_tabs, (1180.0, 790.0));
        settle(&mut rt);
        click(&mut rt, "chat-tab-past-2");
        assert_eq!(rt.state.server().chat.target, chat::Scope::Past(2));
        let lines = rt.state.server().chat.shown(chat::Layout::Tabs);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.place == "Raid night"));
        // Nothing can be written there, but the channel can be joined from the message box.
        assert!(rt.find("chat-join").is_some());
        rt.state.server_mut().chat.draft = "hello".into();
        let _ = rt.state.update(crate::Message::Chat(chat::Action::Send));
        assert_eq!(rt.state.server().chat.draft, "hello", "nothing was sent");
        click(&mut rt, "chat-tab-past-2-close");
        assert_eq!(rt.state.server().chat.target, chat::Scope::Channel);
        assert!(rt.find("chat-tab-past-2").is_none());
        assert!(rt.find("chat-tab-past-4").is_some());
    }

    #[test]
    fn grouped_tabs_list_their_conversations_under_the_pointer() {
        let mut rt = runtime(chat_grouped, (1180.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("chat-tab-past-2").is_none(), "one tab per kind");
        assert!(rt.find("chat-group-channels-list").is_none());
        // Pressing the channel group lists them at once; pointing does after a moment.
        click(&mut rt, "chat-group-channels");
        assert!(rt.find("chat-group-channels-list").is_some());
        click(&mut rt, "chat-group-channels-row1");
        assert_eq!(rt.state.server().chat.target, chat::Scope::Past(2));
        assert!(rt.find("chat-group-channels-list").is_none());
        // Pressing the group goes back to the channel you're in.
        click(&mut rt, "chat-group-channels");
        assert_eq!(rt.state.server().chat.target, chat::Scope::Channel);
        // Pressing People opens whoever is waiting.
        click(&mut rt, "chat-group-people");
        assert_eq!(rt.state.server().chat.target, mara());
        assert_eq!(rt.state.server().chat.unread(&mara()), 0);
    }

    #[test]
    fn the_all_tab_shows_every_conversation_and_a_tag_leads_back_to_one() {
        let mut rt = runtime(chat_tabs, (1180.0, 790.0));
        settle(&mut rt);
        assert_eq!(rt.state.server().chat.shown(chat::Layout::Tabs).len(), 4);
        click(&mut rt, "chat-tab-all");
        assert!(rt.state.server().chat.everything);
        assert_eq!(rt.state.server().chat.shown(chat::Layout::Tabs).len(), 7);
        assert_eq!(
            rt.state.server().chat.unread_private(),
            0,
            "all of it is read"
        );
        // The message box chooses where it sends, without leaving the tab.
        click(&mut rt, "chat-to");
        click(&mut rt, "chat-to-row1");
        assert_eq!(rt.state.server().chat.target, chat::Scope::Server);
        assert!(rt.state.server().chat.everything);
        // A line's tag opens its conversation.
        click(&mut rt, "chat-tag-3");
        assert_eq!(rt.state.server().chat.target, mara());
        assert!(!rt.state.server().chat.everything);
        // Grouped has the same tab.
        let mut rt = runtime(chat_grouped, (1180.0, 790.0));
        settle(&mut rt);
        click(&mut rt, "chat-tab-all");
        assert!(rt.state.server().chat.everything);
        click(&mut rt, "chat-group-channels");
        assert!(!rt.state.server().chat.everything);
    }

    #[test]
    fn grouped_chips_narrow_the_feed_to_one_channel_or_person() {
        let mut rt = runtime(chat_feed_grouped, (1180.0, 790.0));
        settle(&mut rt);
        let shown = |rt: &Runtime<App>| rt.state.server().chat.shown(chat::Layout::Feed).len();
        assert_eq!(shown(&rt), 10);
        // The chip itself is every channel; its list picks one.
        click(&mut rt, "chat-group-channels");
        assert_eq!(rt.state.server().chat.filter, chat::Filter::Channel);
        assert_eq!(shown(&rt), 7);
        assert!(rt.find("chat-group-channels-list").is_some());
        click(&mut rt, "chat-group-channels-row1");
        assert_eq!(
            rt.state.server().chat.filter,
            chat::Filter::Only(chat::Scope::Past(2))
        );
        assert_eq!(shown(&rt), 2);
        assert_eq!(rt.state.server().chat.target, chat::Scope::Channel);
        // A person: the feed is theirs alone and the message box follows.
        click(&mut rt, "chat-group-people");
        assert_eq!(rt.state.server().chat.filter, chat::Filter::Private);
        click(&mut rt, "chat-group-people-row0");
        assert_eq!(rt.state.server().chat.filter, chat::Filter::Only(mara()));
        assert_eq!(rt.state.server().chat.target, mara());
        assert_eq!(shown(&rt), 2);
        click(&mut rt, "chat-filter-All");
        assert_eq!(shown(&rt), 10);
    }

    #[test]
    fn one_feed_narrows_and_its_tags_point_the_message_box() {
        let mut rt = runtime(chat_feed, (1180.0, 790.0));
        settle(&mut rt);
        assert_eq!(rt.state.server().chat.shown(chat::Layout::Feed).len(), 7);
        click(&mut rt, "chat-filter-Private");
        assert_eq!(rt.state.server().chat.filter, chat::Filter::Private);
        assert_eq!(rt.state.server().chat.shown(chat::Layout::Feed).len(), 2);
        // Pressing a line's tag answers there; Escape goes back to the channel.
        click(&mut rt, "chat-tag-3");
        assert_eq!(rt.state.server().chat.target, mara());
        rt.key(rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        });
        settle(&mut rt);
        assert_eq!(rt.state.server().chat.target, chat::Scope::Channel);
        // The chip lists every place to send to.
        click(&mut rt, "chat-to");
        assert!(rt.state.ui.chat_lists[2].open(Instant::now()));
        assert!(rt.find("chat-to-list").is_some());
        // Server is the second row; choosing it closes the list.
        click(&mut rt, "chat-to-row1");
        assert_eq!(rt.state.server().chat.target, chat::Scope::Server);
        assert!(rt.find("chat-to-list").is_none());
    }

    #[test]
    fn a_pop_up_replies_in_a_peek_or_docks_the_chat() {
        let mut rt = runtime(chat_hidden_private, (1180.0, 790.0));
        let root = temp_settings(&mut rt, "chat-toast");
        settle(&mut rt);
        // A private message shows its count on the handle, not just a dot.
        assert!(rt.find("chat-toast").is_some() && rt.find("chat-edge-count").is_some());
        assert!(rt.find("chat-peek").is_none());
        click(&mut rt, "chat-toast-reply");
        assert!(rt.find("chat-toast").is_none());
        assert!(
            rt.find("chat-peek").is_some(),
            "the chat peeks over the stage"
        );
        assert!(rt.state.chat_options.hidden, "and stays hidden otherwise");
        assert_eq!(rt.state.server().chat.target, mara());
        assert_eq!(rt.state.server().chat.unread_private(), 0);

        let mut rt = runtime(chat_hidden_message, (1180.0, 790.0));
        let other = temp_settings(&mut rt, "chat-toast-open");
        settle(&mut rt);
        click(&mut rt, "chat-toast-open");
        assert!(!rt.state.chat_options.hidden && rt.find("chat").is_some());
        assert!(rt.find("chat-toast").is_none() && rt.find("chat-edge").is_none());
        for root in [root, other] {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn clearing_the_chat_history_takes_two_presses() {
        let mut rt = runtime(settings_chat, (1180.0, 790.0));
        let root = temp_settings(&mut rt, "chat-clear");
        settle(&mut rt);
        scroll_into_view(&mut rt, "settings-body-Chat", "chat-history-clear");
        click(&mut rt, "chat-history-clear");
        assert!(rt.state.ui.chat_clear_armed, "the first press only asks");
        assert!(rt.state.settings_message.is_none());
        click(&mut rt, "chat-history-clear");
        assert!(!rt.state.ui.chat_clear_armed);
        assert_eq!(
            rt.state.settings_message.as_deref(),
            Some("Chat history cleared")
        );
        // Leaving Settings between the two presses forgets the first.
        click(&mut rt, "chat-history-clear");
        rt.state.close_settings();
        assert!(!rt.state.ui.chat_clear_armed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_restored_conversation_names_the_day_it_was_said() {
        let mut rt = runtime(chat_restored, (1180.0, 790.0));
        settle(&mut rt);
        let today = chrono::Local::now().date_naive();
        let yesterday = today.pred_opt().unwrap();
        assert!(rt.find(&format!("chat-day-{yesterday}")).is_some());
        assert!(rt.find(&format!("chat-day-{today}")).is_some());
        // Only today's lines: no line names the day.
        let mut rt = runtime(chat_tabs, (1180.0, 790.0));
        settle(&mut rt);
        assert!(rt.find(&format!("chat-day-{today}")).is_none());
    }

    #[test]
    fn setup_and_settings_choose_the_chat_layout() {
        let mut rt = runtime(setup_chat, (900.0, 790.0));
        settle(&mut rt);
        click(&mut rt, "chat-layout-Feed");
        assert_eq!(rt.state.chat_options.layout, chat::Layout::Feed);
        assert_eq!(rt.state.onboarding, Some(onboarding::Step::Chat));

        let mut rt = runtime(settings_chat, (1180.0, 790.0));
        let root = temp_settings(&mut rt, "chat-settings");
        settle(&mut rt);
        click(&mut rt, "chat-layout-Feed");
        // The page is taller than the window on some platforms.
        scroll_into_view(&mut rt, "settings-body-Chat", "chat-popups");
        click(&mut rt, "chat-popups");
        let saved = crate::settings::load(&root.join("settings.json"))
            .unwrap()
            .unwrap();
        assert_eq!(saved.chat.layout, chat::Layout::Feed);
        assert!(!saved.chat.popups);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn choosing_the_accounts_settings_applies_them_and_sends_nothing_back() {
        let root =
            std::env::temp_dir().join(format!("flexspeak-settings-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let path = root.join("settings.json");
        let mut rt = runtime(settings_sync_choose, (1100.0, 900.0));
        settle(&mut rt);
        rt.state.settings_path = Some(path.clone());
        assert!(rt.find("settings-sync-computer").is_some());
        click(&mut rt, "settings-sync-account");
        settle(&mut rt);
        assert_eq!(rt.state.nickname, "Sam");
        assert!(rt.find("settings-sync-choice").is_none());
        let saved = crate::settings::load(&path).unwrap().unwrap();
        assert_eq!(saved.nickname, "Sam");
        assert_eq!(saved.settings_sync_version, 6);
        // The next ticks see the change it made, and send none of it back.
        for _ in 0..4 {
            deliver(&mut rt, crate::Message::UsageTick);
        }
        assert!(
            rt.state.settings_sync_sent.is_empty(),
            "{:?}",
            rt.state.settings_sync_sent
        );
        // A check that finds the version it remembers changes nothing.
        let current = rt.state.saved.clone().unwrap();
        let generation = rt.state.settings_sync.generation();
        deliver(
            &mut rt,
            crate::Message::SettingsSyncAnswer(crate::settings_sync::Answer::Fetched(
                generation,
                Ok(Some(settings_sync_copy(6, &current))),
            )),
        );
        assert_eq!(rt.state.saved.as_ref(), Some(&current));
        assert!(rt.state.settings_sync_sent.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_flavour_picked_and_then_cancelled_adds_nothing() {
        let mut rt = runtime(monster_board, (1100.0, 1500.0));
        settle(&mut rt);
        let before = rt.state.monster.cans.len();
        click(&mut rt, "monster-flavour-khaotic");
        settle(&mut rt);
        click(&mut rt, "add-can-cancel");
        settle(&mut rt);
        assert!(rt.find("add-can-confirm").is_none());
        assert_eq!(rt.state.monster.cans.len(), before);
        assert!(rt.state.monster.reveal.is_none());
    }

    #[test]
    fn tapping_a_flavour_adds_a_can_of_it_and_opens_its_card() {
        let mut rt = runtime(monster_board, (1100.0, 1500.0));
        settle(&mut rt);
        let before = rt.state.monster.cans.len();
        click(&mut rt, "monster-flavour-khaotic");
        settle(&mut rt);
        assert_eq!(rt.state.monster.cans.len(), before, "it asks first");
        assert!(rt.find("add-can-confirm").is_some());
        click(&mut rt, "add-can-confirm");
        settle(&mut rt);
        assert!(rt.find("add-can-confirm").is_none());
        assert_eq!(rt.state.monster.cans.len(), before + 1);
        let added = rt.state.monster.cans.last().unwrap().clone();
        assert_eq!(added.flavour, "khaotic");
        assert!(added.at > 0, "saved with when it was drunk");
        assert!(rt.state.monster.reveal.is_some(), "it opens");
        // While it opens, another tap waits, and asks nothing.
        click(&mut rt, "monster-flavour-khaotic");
        settle(&mut rt);
        assert!(rt.find("add-can-confirm").is_none());
        assert_eq!(rt.state.monster.cans.len(), before + 1);
        // Skipped here, since a test has no timers to carry it through its beats.
        let shown = format!("showcase-{}", added.identity());
        click(&mut rt, &shown);
        settle(&mut rt);
        assert!(rt.state.monster.reveal.is_none());
        assert!(rt.find(&format!("card-{}", added.identity())).is_some());

        let other = rt.state.monster.cans[before - 1].identity();
        click(&mut rt, &format!("card-{other}"));
        settle(&mut rt);
        assert!(rt.find(&format!("showcase-{other}")).is_some());
    }

    #[test]
    fn the_most_drunk_flavours_lead_as_favourites() {
        let mut rt = runtime(monster_board, (1100.0, 1500.0));
        settle(&mut rt);
        let top = crate::monster::favourites(&rt.state.monster.cans, "")[0].0;
        let id = crate::monster::FLAVOURS[top].id;
        let (_, favourite_y) = rt.find(&format!("monster-flavour-{id}")).unwrap();
        let (_, never_y) = rt.find("monster-flavour-other").unwrap();
        assert!(favourite_y < never_y);

        let mut rt = runtime(monster_empty, (1100.0, 1500.0));
        settle(&mut rt);
        assert!(
            rt.find("monster-flavour-original").is_some(),
            "all flavours, nothing drunk yet"
        );
    }

    #[test]
    fn a_search_narrows_the_flavours_and_enter_adds_the_first() {
        let mut rt = runtime(monster_search, (1100.0, 1500.0));
        settle(&mut rt);
        assert!(rt.find("monster-flavour-ultra-gold").is_some());
        assert!(rt.find("monster-flavour-original").is_none());
        let first = crate::monster::favourites(&rt.state.monster.cans, "ultra")[0].0;
        let before = rt.state.monster.cans.len();
        click(&mut rt, "monster-search");
        key(&mut rt, rune::Key::Enter, false);
        settle(&mut rt);
        assert_eq!(rt.state.monster.confirming, Some(first), "it asks first");
        click(&mut rt, "add-can-confirm");
        settle(&mut rt);
        assert_eq!(rt.state.monster.cans.len(), before + 1);
        assert_eq!(
            rt.state.monster.cans.last().unwrap().flavour,
            crate::monster::FLAVOURS[first].id
        );

        let mut rt = runtime(monster_search_none, (1100.0, 1500.0));
        settle(&mut rt);
        assert!(rt.find("monster-flavour-original").is_none());
    }

    #[test]
    fn nothing_on_the_page_gives_a_face_down_card_away() {
        use crate::monster::Rarity;
        let mut rt = runtime(monster_reveal_rare, (1100.0, 1500.0));
        settle(&mut rt);
        let id = can_rolling(Rarity::Legendary);
        assert!(rt.find("reveal-back").is_some());
        assert!(rt.find("reveal-face").is_none());
        assert!(
            rt.find("monster-notice").is_none(),
            "no 'Legendary card added'"
        );
        assert!(rt.find(&format!("card-{id}")).is_none(), "not in the grid");

        let mut rt = runtime(monster_reveal_hit, (1100.0, 1500.0));
        settle(&mut rt);
        assert!(rt.find("reveal-face").is_some());
        assert!(rt.find("monster-notice").is_some());
    }

    #[test]
    fn escape_finishes_a_reveal_before_leaving_the_page() {
        let mut rt = runtime(monster_reveal_mythic, (1100.0, 1500.0));
        settle(&mut rt);
        key(&mut rt, rune::Key::Escape, false);
        settle(&mut rt);
        assert!(rt.state.monster.reveal.is_none());
        assert_eq!(rt.state.tab, Tab::Monster);
    }

    #[test]
    fn the_pointer_over_a_card_is_remembered_and_forgotten_when_it_leaves() {
        use crate::monster::Rarity;
        let mut rt = runtime(monster_board, (1100.0, 1500.0));
        settle(&mut rt);
        let key = format!("card-{}", can_rolling(Rarity::Mythic));
        let (x, y) = rt.find(&key).unwrap();
        rt.pointer_move(x, y);
        settle(&mut rt);
        assert_eq!(rt.state.ui.can_hover.as_deref(), Some(key.as_str()));
        rt.pointer_move(5.0, 5.0);
        settle(&mut rt);
        assert_eq!(rt.state.ui.can_hover, None);
    }

    #[test]
    fn dragging_the_large_can_spins_it_without_choosing_anything() {
        use crate::monster::Rarity;
        let mut rt = runtime(monster_board, (1100.0, 1500.0));
        settle(&mut rt);
        let shown = format!("showcase-{}", can_rolling(Rarity::Legendary));
        let (x, y) = rt.find(&shown).unwrap();
        let before = format!("{:?}", rt.state.ui.can_spin);
        rt.pointer_move(x, y);
        rt.pointer_down(x, y);
        rt.pointer_move(x + 80.0, y + 30.0);
        rt.pointer_up(x + 80.0, y + 30.0);
        settle(&mut rt);
        assert_ne!(format!("{:?}", rt.state.ui.can_spin), before);
        assert!(rt.find(&shown).is_some(), "the same card is still up");
        assert!(rt.state.monster.reveal.is_none());
    }

    #[test]
    fn back_climbs_out_of_a_flavour_one_level_at_a_time() {
        use crate::monster::{Section, flavour_by_id};
        let mut rt = runtime(monster_board, (1100.0, 1500.0));
        settle(&mut rt);
        click(&mut rt, "monster-tab-flavours");
        settle(&mut rt);
        assert_eq!(rt.state.monster.section, Section::Flavours);
        scroll_into_view(&mut rt, "monster-scroll-flavours", "flavour-ultra-gold");
        click(&mut rt, "flavour-ultra-gold");
        settle(&mut rt);
        let (gold, _) = flavour_by_id("ultra-gold").unwrap();
        assert_eq!(rt.state.monster.section, Section::Flavour(gold));
        assert!(rt.find("variant-Legendary").is_some());

        key(&mut rt, rune::Key::Escape, false);
        settle(&mut rt);
        assert_eq!(rt.state.monster.section, Section::Flavours);
        click(&mut rt, "back");
        settle(&mut rt);
        assert_eq!(rt.state.monster.section, Section::Overview);
        assert_eq!(rt.state.tab, Tab::Monster);
        key(&mut rt, rune::Key::Escape, false);
        settle(&mut rt);
        assert_eq!(rt.state.tab, Tab::Home);
    }

    #[test]
    fn a_legend_entry_opens_that_flavours_page() {
        use crate::monster::{Section, flavour_by_id};
        let mut rt = runtime(monster_board, (1100.0, 1500.0));
        settle(&mut rt);
        let top = rt.state.monster.cans.clone();
        let today = chrono::Local::now().date_naive();
        let board = crate::monster::Board::new(&top, today, |_| today);
        let id = board.by_flavour[0].0.id;
        click(&mut rt, &format!("legend-{id}"));
        settle(&mut rt);
        let (index, _) = flavour_by_id(id).unwrap();
        assert_eq!(rt.state.monster.section, Section::Flavour(index));
    }

    #[test]
    fn a_flavour_page_counts_only_that_flavours_rarities_and_marks_the_rest_missing() {
        let mut rt = runtime(monster_flavour_new, (1100.0, 1500.0));
        settle(&mut rt);
        for rarity in crate::monster::Rarity::ALL {
            assert!(
                rt.find(&format!("variant-{}", rarity.name())).is_none(),
                "no card to show"
            );
            assert!(
                rt.find(&format!("variant-empty-{}", rarity.name()))
                    .is_some(),
                "an empty slot instead"
            );
        }
    }

    #[test]
    fn pressing_a_card_on_a_flavour_page_brings_it_up_there() {
        // Tall enough that the oldest card, last in the grid, is on screen to press.
        let mut rt = runtime(monster_flavour, (1100.0, 3200.0));
        settle(&mut rt);
        let (gold, _) = crate::monster::flavour_by_id("ultra-gold").unwrap();
        let cans = rt.state.monster.cans_of(gold);
        let oldest = cans[0].identity();
        click(&mut rt, &format!("card-{oldest}"));
        settle(&mut rt);
        assert!(rt.find(&format!("showcase-{oldest}")).is_some());
    }

    #[test]
    fn the_collection_shows_a_page_of_cards_at_a_time() {
        let mut rt = runtime(monster_board, (900.0, 790.0));
        settle(&mut rt);
        let total = rt.state.monster.cans.len();
        assert!(total > crate::monster::PAGE);
        let oldest = rt.state.monster.cans[0].identity();
        assert!(rt.find(&format!("card-{oldest}")).is_none());
        assert!(rt.find("monster-more").is_some());
    }

    #[test]
    fn five_streams_arrange_themselves_to_the_window() {
        let row_of = |rt: &Runtime<App>, id: &str| rt.find(&format!("tile-{id}")).unwrap().1;
        // The stage to itself: the chat column docked beside it leaves fewer across.
        fn watching_five(s: &mut App) {
            super::watching_five(s);
            s.chat_options.hidden = true;
        }
        let mut wide = runtime(watching_five, (1700.0, 900.0));
        settle(&mut wide);
        assert_eq!(row_of(&wide, "s1"), row_of(&wide, "s2"), "two lead");
        assert_eq!(row_of(&wide, "s3"), row_of(&wide, "s5"), "three across");
        assert!(row_of(&wide, "s3") > row_of(&wide, "s1"), "under them");
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
        assert!(
            rt.find("log-search-open").is_some(),
            "faint at rest, but there"
        );
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
    fn several_streams_keep_their_tools_in_the_stage_header() {
        let mut rt = runtime(watching, (900.0, 790.0));
        settle(&mut rt);
        assert!(rt.find("stage-header").is_some());
        assert!(rt.find("refresh-streams").is_some(), "there at rest");
        assert!(rt.find("stage-tools").is_none(), "nothing floats");
    }

    #[test]
    fn a_lone_streams_toolbar_appears_while_the_pointer_is_over_it() {
        let mut rt = runtime(watching_solo, (900.0, 790.0));
        settle(&mut rt);
        assert!(
            rt.find("stage-header").is_none(),
            "the picture has the pane"
        );
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

        // Double-clicking the stage takes it full screen with the others tucked away; leaving
        // brings them back, and the grid button returns to the grid.
        let (x, y) = rt.find("tile-s4").unwrap();
        rt.pointer_move(x, y);
        rt.frame(1.0 / 60.0);
        for _ in 0..2 {
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(1.0 / 60.0);
        }
        settle(&mut rt);
        assert_eq!(rt.state.ui.fullscreen, Some(false));
        assert!(rt.state.server().streaming.others_hidden);
        assert_eq!(rt.state.server().streaming.focused.as_deref(), Some("s4"));
        click(&mut rt, "fullscreen");
        assert_eq!(rt.state.ui.fullscreen, None);
        assert!(!rt.state.server().streaming.others_hidden);
        click(&mut rt, "unfocus");
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
    fn search_lists_results_for_the_categories_and_escape_clears_it_before_closing() {
        let mut rt = runtime(settings_page, (900.0, 790.0));
        press(&mut rt, "nav-Shortcuts");
        // `/` jumps to the search field from anywhere in the window.
        key(&mut rt, rune::Key::Text("/".into()), false);
        assert!(rt.state.ui.settings_search.focused);
        for c in ["d", "e", "a", "f"] {
            key(&mut rt, rune::Key::Text(c.into()), false);
        }
        assert_eq!(rt.state.settings_query, "deaf");
        assert!(rt.find("settings-nav").is_none(), "results take its place");
        assert!(
            rt.find("found-shortcuts-toggle-deafen").is_some(),
            "the deafen shortcut matches"
        );
        assert!(
            rt.find("found-server-nickname").is_none(),
            "server rows don't"
        );
        // The page stays as it was while results are listed.
        assert!(rt.find("shortcuts").is_some());
        key(&mut rt, rune::Key::Escape, false);
        assert_eq!(rt.state.settings_query, "");
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Shortcuts));
        assert!(rt.find("settings-nav").is_some());
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
        assert!(rt.find("found-appearance-ember").is_some());
        search(&mut rt, "permissions");
        assert!(rt.find("found-permissions-microphone").is_some());
        search(&mut rt, "bluetooth");
        assert!(rt.find("no-results").is_some());
        // A saved server is found by what its row holds, and says which category it is in.
        search(&mut rt, "password");
        assert!(rt.find("found-server-zirkumflex").is_some());
        search(&mut rt, "auto connect");
        assert!(rt.find("found-server-zirkumflex").is_some());
        // The words may be spread over a row and its category.
        search(&mut rt, "audio output");
        assert!(rt.find("found-audio-output").is_some());
        assert!(rt.find("found-audio-microphone").is_none());
        // History is the chat's, not the changelog's or the error log's.
        search(&mut rt, "history");
        assert!(rt.find("found-chat-keep-chat-history").is_some());
        assert!(
            rt.find("found-updates-what-s-new").is_none()
                && rt.find("found-error-log-error-log").is_none()
        );
    }

    #[test]
    fn choosing_a_result_opens_its_category_at_that_row() {
        let mut rt = runtime(settings_page, (900.0, 790.0));
        let target = |rt: &Runtime<App>| rt.state.ui.settings_target.clone().map(|(key, _)| key);
        let shown = |rt: &Runtime<App>, body: &str, key: &str| {
            let ([_, top, _, height], _) = rt.presented(body).unwrap();
            let ([_, y, _, h], _) = rt.presented(key).unwrap();
            y >= top && y + h <= top + height
        };
        if !rt.state.ui.settings_search.focused {
            key(&mut rt, rune::Key::Text("/".into()), false);
        }
        // The best title leads: the microphone itself before what only mentions it.
        key(&mut rt, rune::Key::Text("mic".into()), false);
        assert!(rt.find("found-audio-microphone").is_some());
        key(&mut rt, rune::Key::Enter, false);
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Audio));
        assert_eq!(rt.state.settings_query, "");
        assert_eq!(target(&rt).as_deref(), Some("row-audio-microphone"));
        // A row far down its page is brought into view.
        key(&mut rt, rune::Key::Text("echo".into()), false);
        key(&mut rt, rune::Key::Enter, false);
        settle(&mut rt);
        assert_eq!(
            target(&rt).as_deref(),
            Some("row-processing-echo-cancellation")
        );
        assert!(shown(
            &rt,
            "settings-body-Audio",
            "row-processing-echo-cancellation"
        ));
        // The mark goes when its own timer says so, not an earlier jump's.
        let (_, jump) = rt.state.ui.settings_target.clone().unwrap();
        let _ = rt
            .state
            .update(crate::Message::SettingsTargetSeen(jump - 1));
        assert!(rt.state.ui.settings_target.is_some());
        let _ = rt.state.update(crate::Message::SettingsTargetSeen(jump));
        assert!(rt.state.ui.settings_target.is_none());
        // The arrow keys move through the results and come round at the ends.
        key(&mut rt, rune::Key::Text("suppression".into()), false);
        assert_eq!(rt.state.ui.settings_pick, 0);
        key(&mut rt, rune::Key::Down, false);
        assert_eq!(rt.state.ui.settings_pick, 1);
        key(&mut rt, rune::Key::Up, false);
        key(&mut rt, rune::Key::Up, false);
        assert!(
            rt.state.ui.settings_pick > 1,
            "up from the first is the last"
        );
        key(&mut rt, rune::Key::Down, false);
        assert_eq!(rt.state.ui.settings_pick, 0);
        key(&mut rt, rune::Key::Escape, false);
        // A sound inside a closed group is found too, and its group opens for it.
        key(&mut rt, rune::Key::Text("whisper to".into()), false);
        click(&mut rt, "result-0");
        settle(&mut rt);
        assert_eq!(rt.state.settings_page, Some(SettingsPage::Sounds));
        let key_of = target(&rt).unwrap();
        assert_eq!(key_of, "row-sounds-someone-is-whispering-to-you");
        assert!(shown(&rt, "settings-body-Sounds", &key_of));
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
        // Settings opened only for the dialog, so it closes with it.
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
    fn processing_switches_apply_while_idle_and_during_a_session() {
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
        let id = add_session(&mut rt.state, "Busy", "busy.example");
        rt.state.activate(Some(id));
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
            before.echo_cancellation != crate::audio_processing::AVAILABLE
        );
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
                // Changed outside a handler, so the next frame must be told to re-run the view.
                rt.invalidate();
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
