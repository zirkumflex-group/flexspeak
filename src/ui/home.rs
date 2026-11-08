//! The window shell, the saved-server home, the connected sidebar and performance mode.

use std::borrow::Cow;
use std::time::Instant;

use rune::{
    Cx, Drag, El, GrowthSpec, Phase, PointerPress, ScrollSpec, Span, Transition, Tx, col, el, icon,
    rich, row, text,
};
use rune_kit::{Kit, MenuItem, TextInput, context_menu};

use super::theme::{Theme, motion, radius, size, ty};
use super::widgets::{
    Variant, body, button, card, divider, faint, heading, icon_button, labelled, meter, muted,
    overline, pulse, scroll_page,
};
use super::{RailPeek, dispatch, footer, icons, members, send, statistics, streams};
use crate::{App, Message, Tab, client, usage};

pub fn normal(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    let page = match s.tab {
        Tab::Home => home(s, cx, t, k, layer),
        // On the same inset card as the connected panes, so it scrolls clear of the status bar.
        Tab::Statistics => row().w_full().h_full().px(INSET).child(
            pane(t)
                .grow(1.0)
                .min_w(0.0)
                .child(statistics::view(s, cx, t, k)),
        ),
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
        .child(footer::view(s, t))
}

fn header(s: &App, cx: &mut Cx<App>, t: &Theme, _k: &Kit) -> El {
    let mut bar = row()
        .key("header")
        .h(size::HEADER)
        .w_full()
        .pl(INSET + cx.titlebar_inset())
        .pr(if cfg!(windows) { 150.0 } else { 12.0 })
        .gap(size::GAP)
        .center_y()
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
    // Connected, the sidebar names the server; the title bar just names the app, quietly,
    // as a native window's caption does.
    let (title, ink) = match s.tab {
        Tab::Home if s.server().connected && !s.loopback => ("Flexspeak", t.muted),
        Tab::Home => ("Flexspeak", t.text),
        Tab::Statistics => ("Usage & Statistics", t.text),
    };
    bar = bar
        .child(icon(&icons::FLEXSPEAK, 18.0).color(t.accent).a11y_hidden())
        .child(text(title).font(ty::HEADING).color(ink).truncate());
    bar = bar.child(el().grow(1.0));
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
        &icons::SETTINGS_THIN,
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
    bar
}

fn home(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    rail_menu(s, k, layer);
    if !s.loopback && s.sessions.iter().any(|session| session.connected) {
        if !s.server().connected {
            return row()
                .w_full()
                .h_full()
                .pr(INSET)
                .child(server_rail(s, t, k))
                .child(pane(t).grow(1.0).min_w(0.0).child(connecting_pane(s, t)));
        }
        // One server has nothing to switch between, so the rail waits behind the edge.
        let hidden = s.rail_sessions().count() < 2;
        if hidden {
            rail_edge(s, t, k, layer);
        }
        return row()
            .w_full()
            .h_full()
            .pl(if hidden { INSET } else { 0.0 })
            .pr(INSET)
            .gap(INSET)
            .children((!hidden).then(|| server_rail(s, t, k)))
            .child(sidebar(s, cx, t, k, layer))
            .child(
                pane(t)
                    .key("main-pane")
                    .grow(1.0)
                    .min_w(0.0)
                    .child(streams::view(s, cx, t, k, layer)),
            );
    }
    saved_servers(s, t, k)
}

/// The saved servers, each with its own Connect. Only a microphone test holds them back.
/// The gap between the window's edge and its cards, and between the cards.
pub const INSET: f32 = 8.0;

/// A card the connected window is built from: the sidebar and the main pane sit on the window
/// background, each with its own rounded surface and hairline edge.
fn pane(t: &Theme) -> El {
    col()
        .h_full()
        .radius(radius::PANE)
        .fill(t.surface)
        .border(1.0, t.hairline)
        .clip_rounded(radius::PANE)
}

fn saved_servers(s: &App, t: &Theme, k: &Kit) -> El {
    let busy = s.loopback && !s.idle();
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
        let ready = s.permissions.pending.is_none();
        let mut list = col().key("server-list").w_full().gap(4.0);
        for (index, server) in s.servers.iter().enumerate() {
            let session = s.session_at(&server.address);
            list = list.child(server_row(t, index, server, session, busy, ready));
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
        if !s.idle() && !s.server().stopping {
            status = status.child(pulse(if s.loopback { t.live } else { t.accent }));
        }
        status = status.child(muted(t, s.server().status.clone()).grow(1.0));
        if busy && s.loopback {
            status = status.child(button(
                t,
                "stop-test",
                if s.server().stopping {
                    "Stopping…"
                } else {
                    "Stop test"
                },
                Some(&icons::STOP),
                Variant::Secondary,
                (!s.server().stopping).then_some(Message::Stop),
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
    index: usize,
    server: &crate::settings::Server,
    session: Option<&crate::ServerSession>,
    busy: bool,
    ready: bool,
) -> El {
    let mine = session.is_some();
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
    let (label, action) = if let Some(session) = session {
        (
            if session.stopping {
                "Stopping…"
            } else {
                "Cancel"
            },
            (!session.stopping).then_some(Message::StopServer(session.id)),
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
        "Stop the microphone test first".to_owned()
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

const RAIL_TILE: f32 = 44.0;
/// The rail's width: the tile with `INSET` either side, as wide as the gap between the cards.
pub const RAIL: f32 = RAIL_TILE + 2.0 * INSET;

/// Up to two letters that stand for a server: its first two words, or its first two letters.
pub(super) fn initials(name: &str) -> String {
    let mut words = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty());
    let first = words.next().unwrap_or_default();
    let letters: Vec<char> = match words.next() {
        Some(second) => first
            .chars()
            .take(1)
            .chain(second.chars().take(1))
            .collect(),
        None => first.chars().take(2).collect(),
    };
    letters
        .into_iter()
        .flat_map(char::to_uppercase)
        .take(2)
        .collect()
}

/// A colour of its own for each server, the same every time it's connected.
pub(super) fn tile_colour(name: &str) -> rune::Color {
    let hash = name.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    rune::oklch(0.55, 0.13, (hash % 360) as f32)
}

/// A tile's marker for pokes that arrived while another server was active.
fn poke_marker(t: &Theme, pokes: u32) -> El {
    let mut marker = row()
        .absolute(20.0, -6.0)
        .h(20.0)
        .px(5.0)
        .gap(2.0)
        .center()
        .radius(radius::PILL)
        .fill(t.fair)
        .border(2.0, t.canvas)
        .a11y_hidden()
        .child(icon(&icons::HAND, 11.0).color(rune::Color::BLACK));
    if pokes > 1 {
        marker = marker.child(
            text(pokes.min(99).to_string())
                .font(ty::CAPTION)
                .color(rune::Color::BLACK),
        );
    }
    marker
}

fn rail_status_icon(
    t: &Theme,
    on: &'static rune::Icon,
    off: &'static rune::Icon,
    is_off: bool,
) -> El {
    icon(if is_off { off } else { on }, 13.0)
        .color(if is_off { t.poor } else { t.muted })
        .a11y_hidden()
}

/// One server on the rail: its tile, and the mic and sound state beneath it.
fn rail_entry(t: &Theme, session: &crate::ServerSession, active: bool) -> El {
    let name = session.title().to_owned();
    let mut tile = el()
        .size(RAIL_TILE, RAIL_TILE)
        .radius(if active { 14.0 } else { RAIL_TILE / 2.0 })
        .center()
        .fill(tile_colour(&name))
        .motion(motion::QUICK)
        .child(
            text(initials(&name))
                .font(ty::BODY_STRONG)
                .color(rune::Color::WHITE),
        );
    if !session.talking.is_empty() {
        tile = tile.border(2.0, t.good);
    }
    if session.unseen_pokes > 0 {
        tile = tile.child(poke_marker(t, session.unseen_pokes));
    }
    if !session.connected && !session.stopping {
        tile = tile.child(
            el().absolute(32.0, 32.0)
                .size(14.0, 14.0)
                .radius(7.0)
                .center()
                .fill(t.canvas)
                .child(pulse(t.fair)),
        );
    }
    let status = row()
        .gap(4.0)
        .center_y()
        .child(rail_status_icon(
            t,
            &icons::MIC,
            &icons::MIC_OFF,
            session.microphone_off(),
        ))
        .child(rail_status_icon(
            t,
            &icons::HEADPHONES,
            &icons::HEADPHONE_OFF,
            session.deafened,
        ));
    let mut entry = row()
        .key(format!("rail-{}", session.id.0))
        .role("button")
        .w_full()
        .center()
        .help(name.clone())
        .label(name)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .on_press_tx(send(Message::SwitchServer(session.id)))
        .on_secondary_press({
            let id = session.id;
            move |s: &mut App, point: PointerPress, _: &mut Tx| {
                s.ui.rail_menu = Some((id, point));
            }
        })
        .child(col().gap(4.0).center().child(tile).child(status));
    if active {
        entry = entry.child(
            el().absolute(0.0, 10.0)
                .size(3.0, 24.0)
                .radius(1.5)
                .fill(t.accent),
        );
    }
    entry
}

/// The servers you're connected to, Discord-style, down the left of the window.
/// Right-clicking a server in the rail.
fn rail_menu(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some((id, point)) = s.ui.rail_menu else {
        return;
    };
    let Some(session) = s.rail_sessions().find(|session| session.id == id) else {
        return;
    };
    let menu = context_menu("rail-menu", el(), Some(point))
        .min_width(180.0)
        .on_close(|s: &mut App| s.ui.rail_menu = None)
        .group_label(session.title().to_owned())
        .item(
            MenuItem::new("Disconnect")
                .icon(&icons::DISCONNECT)
                .danger(true)
                .disabled(session.stopping)
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    s.ui.rail_menu = None;
                    dispatch(s, tx, Message::StopServer(id));
                }),
        );
    let _ = menu.build(k, layer);
}

fn server_rail(s: &App, t: &Theme, k: &Kit) -> El {
    let mut tiles = col().w_full().pt(4.0).pb(12.0).gap(12.0);
    for session in s.rail_sessions() {
        tiles = tiles.child(rail_entry(t, session, s.active == Some(session.id)));
    }
    tiles = tiles
        .child(
            row()
                .w_full()
                .center()
                .child(el().w(32.0).h(1.0).fill(t.hairline)),
        )
        .child(
            row().w_full().center().child(
                el().key("rail-add")
                    .role("button")
                    .label("Connect to another server")
                    .help("Connect to another server")
                    .size(RAIL_TILE, RAIL_TILE)
                    .radius(RAIL_TILE / 2.0)
                    .center()
                    .fill(t.surface)
                    .border(1.0, t.hairline)
                    .focusable(true)
                    .cursor(rune::Cursor::Pointer)
                    .hover(t.wash(0.08))
                    .on_press_tx(send(Message::OpenServerPicker))
                    .child(icon(&icons::PLUS, 18.0).color(t.good).a11y_hidden()),
            ),
        );
    col()
        .key("server-rail")
        .w(RAIL)
        .h_full()
        .child(scroll_page(k, "rail-scroll", tiles))
}

/// Reaches into the sidebar's empty padding: Windows keeps the outer 8 pt for resizing.
const RAIL_EDGE: f32 = 20.0;
/// The tile, its status icons, the divider and the +, with the rail's own padding.
const RAIL_PEEK_HEIGHT: f32 = 150.0;

/// The rail while hidden: a handle on the window's edge that opens it on hover.
fn rail_edge(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let peek = s.ui.rail_peek;
    let open = peek.open(Instant::now()) || s.ui.rail_menu.is_some();
    let near = peek.inside();
    let height = (s.window_size.1 - size::HEADER - 40.0).max(80.0);
    let (width, length) = if near { (5.0, 60.0) } else { (3.0, 44.0) };
    let ink = if !s.server().talking.is_empty() {
        t.good
    } else if near {
        t.muted
    } else {
        t.faint
    };
    let mut strip = el()
        .key("rail-edge")
        .role("button")
        .label("Show servers")
        .absolute(0.0, size::HEADER)
        .size(RAIL_EDGE, height)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .on_hover_change(|s: &mut App, inside, tx: &mut Tx| {
            s.ui.rail_peek
                .hover(Instant::now(), |peek| peek.strip = inside);
            RailPeek::WAKE
                .into_iter()
                .for_each(|wake| tx.wake_after(wake));
        })
        .on_press(|s: &mut App| s.ui.rail_peek.press());
    if !open {
        strip = strip.child(
            el().absolute(2.0, (height - length) / 2.0)
                .size(width, length)
                .radius(width / 2.0)
                .fill(ink)
                .motion(motion::QUICK)
                .a11y_hidden(),
        );
    }
    layer.push(strip);
    if open {
        layer.push(
            col()
                .key("rail-peek")
                .absolute(INSET, size::HEADER + 4.0)
                .w(RAIL)
                .h(RAIL_PEEK_HEIGHT)
                .radius(radius::PANE)
                .fill(t.raised)
                .border(1.0, t.hairline)
                .box_shadow(super::theme::shadow(
                    0.0,
                    8.0,
                    24.0,
                    rune::Color::BLACK.alpha(0.4),
                ))
                .transition(rune::Transition::opacity().combined(rune::Transition::scale(0.98)))
                .on_hover_change(|s: &mut App, inside, tx: &mut Tx| {
                    s.ui.rail_peek
                        .hover(Instant::now(), |peek| peek.card = inside);
                    RailPeek::WAKE
                        .into_iter()
                        .for_each(|wake| tx.wake_after(wake));
                })
                .child(server_rail(s, t, k)),
        );
    }
}

/// The main pane while the server on screen is still connecting or reconnecting.
fn connecting_pane(s: &App, t: &Theme) -> El {
    let session = s.server();
    col()
        .key("server-connecting")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .padding(size::PAGE)
        .center()
        .gap(12.0)
        .child(heading(t, session.title().to_owned()).truncate_fade())
        .child(
            row()
                .gap(8.0)
                .center_y()
                .child(pulse(t.accent))
                .child(muted(t, session.status.clone())),
        )
        .child(button(
            t,
            "cancel-connecting",
            if session.stopping {
                "Stopping…"
            } else {
                "Cancel"
            },
            Some(&icons::DISCONNECT),
            Variant::Secondary,
            (!session.stopping).then_some(Message::Stop),
        ))
}

fn sidebar(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    members::menu(s, t, k, layer);
    members::ghost(s, t, layer);
    let all = s.subscribe_all.load(std::sync::atomic::Ordering::Relaxed);
    let name = s.server().state.server.clone();
    let server = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            el().size(28.0, 28.0)
                .radius(radius::CONTROL)
                .center()
                .fill(tile_colour(s.server().title()))
                .child(
                    text(initials(s.server().title()))
                        .font(ty::SMALL_STRONG)
                        .color(rune::Color::WHITE),
                )
                .a11y_hidden(),
        )
        .child(
            text(name)
                .font(ty::SUBTITLE)
                .color(t.text)
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
    let server = if s.connected_servers() < 2 {
        server
    } else {
        col().w_full().gap(6.0).child(server).child(
            row()
                .key("shortcut-hint")
                .gap(6.0)
                .center_y()
                .child(icon(&icons::KEYBOARD, 12.0).color(t.faint))
                .child(faint(t, "Shortcuts control this server")),
        )
    };
    pane(t)
        .key("sidebar")
        .w(size::SIDEBAR)
        .padding(12.0)
        .gap(8.0)
        .child(server)
        // Closer to its channels (8) than to the server above it (16).
        .child(overline(t, "Channels").ml(6.0).mt(8.0))
        // The channel list takes the free height, so the voice controls sit at the bottom.
        .child(scroll_page(
            k,
            "channel-scroll",
            channel_tree(s, t).pr(8.0).pb(8.0),
        ))
        .children(selection_bar(s, t))
        .children(server_log(s, cx, t, k))
        .child(voice_controls(s, t))
}

const LOG_ROW: f32 = 24.0;
/// The log's list never gets shorter than two events while open.
const LOG_MIN: f32 = LOG_ROW * 2.0;
/// Dragged shorter than this, the log folds down to its heading.
const LOG_FOLD: f32 = LOG_ROW;
/// The share of the sidebar the log may take; the channels keep the rest.
const LOG_MAX_SHARE: f32 = 0.6;
/// The splitter's grab band above the log.
const SPLITTER: f32 = 9.0;
/// Below this the channel list needs the room more.
const LOG_MIN_WINDOW: f32 = 680.0;

/// The hairline above the log, with a taller band to grab: drag it to resize the log, past
/// the bottom to fold it, or double-click it to fold or open it.
fn log_splitter(s: &App, t: &Theme, max: f32) -> El {
    let resizing = s.ui.log_resizing;
    el().key("log-splitter")
        .role("separator")
        .label("Resize the server log")
        .help("Drag to resize the server log · double-click to fold it")
        .w_full()
        .h(SPLITTER)
        .radius(radius::ROW)
        .center()
        .cursor(rune::Cursor::ResizeRow)
        .hover(t.wash(0.04))
        .on_drag(
            move |s: &mut App, d: &mut Drag, tx: &mut Tx| match d.phase {
                Phase::Down => {
                    s.ui.log_resizing = true;
                    d.data = if s.log_pane.collapsed {
                        0.0
                    } else {
                        f32::from(s.log_pane.height).clamp(LOG_MIN, max)
                    };
                    if d.clicks == 2 {
                        dispatch(s, tx, Message::ToggleLog);
                    }
                }
                Phase::Move => {
                    tx.instant();
                    let height = d.data - d.dy;
                    if height < LOG_FOLD {
                        s.log_pane.collapsed = true;
                    } else {
                        s.log_pane.collapsed = false;
                        s.log_pane.height = height.clamp(LOG_MIN, max).round() as u16;
                    }
                }
                Phase::Up => {
                    s.ui.log_resizing = false;
                    if d.cancelled() {
                        s.log_pane.collapsed = d.data == 0.0;
                        if d.data > 0.0 {
                            s.log_pane.height = d.data.round() as u16;
                        }
                    }
                    dispatch(s, tx, Message::SaveLogPane);
                }
            },
        )
        .child(
            el().w_full()
                .h(if resizing { 2.0 } else { 1.0 })
                .fill(if resizing { t.accent } else { t.hairline })
                .a11y_hidden(),
        )
}

/// What has happened on this server, by hour. It follows the newest entry unless scrolled
/// back or searching; a magnifier appears on hover to search it.
fn server_log(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit) -> Option<El> {
    let log = &s.server().log;
    let height = cx
        .layout_size("window")
        .map_or(s.window_size.1, |size| size[1]);
    if log.is_empty() || height < LOG_MIN_WINDOW {
        return None;
    }
    let sidebar = cx
        .layout_size("sidebar")
        .map_or(height - 200.0, |size| size[1]);
    let max = (sidebar * LOG_MAX_SHARE).max(LOG_MIN);
    let pane = s.log_pane;
    let rows = f32::from(pane.height).clamp(LOG_MIN, max);
    let body = if pane.collapsed {
        None
    } else if log.sections().is_empty() {
        Some(
            col()
                .key("log-none")
                .w_full()
                .h(rows)
                .gap(6.0)
                .center()
                .child(faint(t, "No events match"))
                .child(button(
                    t,
                    "log-clear",
                    "Clear search",
                    None,
                    Variant::Ghost,
                    Some(Message::LogClear),
                )),
        )
    } else {
        let theme = *t;
        let list = col()
            .key("log-rows")
            .w_full()
            .growth(GrowthSpec::default())
            .virtual_rows(
                log.sections().len(),
                LOG_ROW * 8.0,
                log_key,
                move |s: &App, index| log_section(s, &theme, index),
            );
        let following = !log.filtering();
        Some(
            k.scroll_bar(
                col()
                    .key(if following { "log-scroll" } else { "log-found" })
                    .w_full()
                    .h(rows)
                    .scroll(ScrollSpec {
                        stick_bottom: following,
                    })
                    .scroll_edge_fade(12.0)
                    .child(list),
            ),
        )
    };
    Some(
        col()
            .key("server-log")
            .w_full()
            .gap(4.0)
            .child(log_splitter(s, t, max))
            .on_hover_change(|s: &mut App, hovering, _| s.ui.log_hover = hovering)
            .child(log_bar(s, t, k))
            .children(body)
            .children((s.ui.log_people && !pane.collapsed).then(|| log_people(s, t, k))),
    )
}

/// The heading, or while searching the search field and the person filter.
fn log_bar(s: &App, t: &Theme, k: &Kit) -> El {
    let log = &s.server().log;
    let bar = row().w_full().h(size::CONTROL).gap(6.0).center_y();
    let collapsed = s.log_pane.collapsed;
    let fold = icon_button(
        t,
        "log-fold",
        if collapsed {
            &icons::CHEVRON_UP
        } else {
            &icons::CHEVRON_DOWN
        },
        if collapsed {
            "Show the server log"
        } else {
            "Fold the server log"
        },
        Variant::Ghost,
        Some(Message::ToggleLog),
    );
    if !log.searching || collapsed {
        return bar
            .child(overline(t, "Server log").ml(6.0).grow(1.0))
            .children((s.ui.log_hover && !collapsed).then(|| {
                icon_button(
                    t,
                    "log-search-open",
                    &icons::SEARCH,
                    "Search the log",
                    Variant::Ghost,
                    Some(Message::LogSearch(true)),
                )
                .transition(Transition::opacity())
            }))
            .child(fold);
    }
    let placeholder = match log.person() {
        Some(person) => format!("Search {person}"),
        None => "Search the log".to_owned(),
    };
    let input = TextInput::new(&s.ui.log_search)
        .placeholder(&placeholder)
        .label("Search the server log")
        .leading_icon(&icons::SEARCH)
        .width(None)
        .height(size::CONTROL)
        .on_edit(|s: &mut App| &mut s.ui.log_search)
        .on_change(|s: &mut App, tx: &mut Tx| {
            let value = s.ui.log_search.text.clone();
            dispatch(s, tx, Message::LogQuery(value));
        })
        .trailing(icon_button(
            t,
            "log-search-close",
            &icons::X,
            "Close search · Esc",
            Variant::Ghost,
            Some(Message::LogSearch(false)),
        ))
        .build(k);
    let person = match log.person() {
        Some(person) => {
            let short: String = person.chars().take(10).collect();
            let name = if short.len() < person.len() {
                format!("{short}…")
            } else {
                short
            };
            button(
                t,
                "log-person-chip",
                &name,
                Some(&icons::X),
                Variant::Active,
                Some(Message::LogPerson(None)),
            )
        }
        None => icon_button(
            t,
            "log-people",
            &icons::USER,
            "Filter by person",
            if s.ui.log_people {
                Variant::Active
            } else {
                Variant::Secondary
            },
            Some(Message::LogPeople),
        ),
    };
    bar.child(el().grow(1.0).min_w(0.0).child(input))
        .child(person)
}

/// Everyone in the log, to narrow it to one of them.
fn log_people(s: &App, t: &Theme, k: &Kit) -> El {
    const ROW: f32 = 26.0;
    const WIDTH: f32 = 170.0;
    let people = s.server().log.people();
    let everyone: usize = people.iter().map(|(_, count)| count).sum();
    let line = |key: String, name: String, count: usize, message: Message| {
        row()
            .key(key)
            .w_full()
            .h(ROW)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(radius::ROW)
            .hover(t.wash(0.08))
            .on_press_tx(send(message))
            .child(
                text(name)
                    .font(ty::SMALL)
                    .color(t.text)
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate_fade(),
            )
            .child(text(count.to_string()).font(ty::NUMERIC).color(t.faint))
    };
    let mut items = col().w_full().child(line(
        "log-person-all".to_owned(),
        "Everyone".to_owned(),
        everyone,
        Message::LogPerson(None),
    ));
    let rows = people.len() + 1;
    for (index, (name, count)) in people.into_iter().enumerate() {
        let message = Message::LogPerson(Some(name.clone()));
        items = items.child(line(format!("log-person-{index}"), name, count, message));
    }
    col()
        .key("log-people-menu")
        // Under the search bar, below the splitter and the log's 4 pt gap.
        .absolute(
            size::SIDEBAR - 24.0 - WIDTH,
            SPLITTER + 4.0 + size::CONTROL + 4.0,
        )
        .w(WIDTH)
        .padding(4.0)
        // Concentric with its rows: their radius plus this padding.
        .radius(radius::ROW + 4.0)
        .fill(t.raised)
        .border(1.0, t.hairline)
        .transition(Transition::opacity())
        .child(
            k.scroll_bar(
                col()
                    .key("log-people-scroll")
                    .w_full()
                    .h(rows.min(5) as f32 * ROW)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(items),
            ),
        )
}

fn log_key(s: &App, index: usize) -> Cow<'_, str> {
    let section = s.server().log.sections().get(index);
    Cow::Borrowed(section.map_or("", |section| section.key.as_str()))
}

/// One hour: its heading, pinned while the hour scrolls by, and its events.
fn log_section(s: &App, t: &Theme, index: usize) -> El {
    let log = &s.server().log;
    let Some(section) = log.sections().get(index) else {
        return el();
    };
    let entries: Vec<_> = section
        .entries
        .iter()
        .filter_map(|index| log.get(*index))
        .collect();
    rune::memo(
        format!("log-section-{}", section.key),
        (
            section,
            entries,
            log.query(),
            log.person(),
            [
                t.surface, t.faint, t.text, t.good, t.fair, t.poor, t.accent, t.hairline,
            ],
        ),
        || log_section_content(s, t, index),
    )
}

fn log_section_content(s: &App, t: &Theme, index: usize) -> El {
    let log = &s.server().log;
    let Some(section) = log.sections().get(index) else {
        return el();
    };
    let count = section.entries.len();
    let noun = match (log.filtering(), count) {
        (true, 1) => "match",
        (true, _) => "matches",
        (false, 1) => "event",
        (false, _) => "events",
    };
    let mut hour = col().w_full();
    if let Some(day) = &section.day {
        hour = hour.child(faint(t, day.clone()).pl(6.0).py(4.0));
    }
    hour = hour.child(
        row()
            .w_full()
            .h(20.0)
            .pl(21.0)
            .pr(8.0)
            .center_y()
            .fill(t.surface)
            .sticky_top(0.0)
            .child(
                text(section.label.clone())
                    .font(ty::SMALL_STRONG)
                    .color(t.faint)
                    .grow(1.0),
            )
            .child(faint(t, format!("{count} {noun}"))),
    );
    for entry in &section.entries {
        hour = hour.child(log_row(s, t, *entry));
    }
    hour
}

fn log_row(s: &App, t: &Theme, index: usize) -> El {
    use crate::server_log::{Tone, find};
    let log = &s.server().log;
    let Some(entry) = log.get(index) else {
        return el();
    };
    let color = match entry.tone {
        Tone::Arrived => t.good,
        Tone::Moved => t.fair,
        Tone::Left => t.poor,
        Tone::Stream => t.accent,
    };
    let marker = el()
        .w(7.0)
        .h(LOG_ROW)
        .center()
        .child(el().absolute(3.0, 0.0).w(1.0).h_full().fill(t.hairline))
        .child(el().size(7.0, 7.0).radius(3.5).fill(color));
    let lead = if entry.name.is_empty() {
        0
    } else {
        entry.name.len() + 1
    };
    let words = if entry.name.is_empty() {
        entry.action.clone()
    } else {
        format!("{} {}", entry.name, entry.action)
    };
    let mut spans = Vec::new();
    if lead > 0 {
        spans.push(Span::new(0, entry.name.len()).bold());
    }
    let mark = |range: std::ops::Range<usize>| {
        Span::new(range.start, range.end).bg(t.fair.alpha(0.35), 1.0)
    };
    spans.extend(find(&entry.name, log.query()).map(mark));
    spans.extend(
        find(&entry.action, log.query()).map(|range| mark(range.start + lead..range.end + lead)),
    );
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .pl(6.0)
        .pr(8.0)
        .child(marker)
        .child(
            rich(words, spans)
                .font(ty::SMALL)
                .color(t.text)
                .grow(1.0)
                .min_w(0.0)
                .truncate_fade(),
        )
        .child(text(entry.time.clone()).font(ty::NUMERIC).color(t.faint))
}

/// While several members are picked: how many, and how to drop the selection.
fn selection_bar(s: &App, t: &Theme) -> Option<El> {
    if s.server().selected_members.len() < 2 {
        return None;
    }
    Some(
        row()
            .key("selection")
            .w_full()
            .gap(8.0)
            .px(10.0)
            .py(6.0)
            .center_y()
            .radius(radius::ROW)
            .fill(t.accent.alpha(0.10))
            .transition(Transition::opacity())
            .child(
                faint(
                    t,
                    format!(
                        "{} selected · drag onto a channel or right-click",
                        s.server().selected_members.len()
                    ),
                )
                .grow(1.0),
            )
            .child(icon_button(
                t,
                "clear-selection",
                &icons::X,
                "Clear selection · Esc",
                Variant::Ghost,
                Some(Message::ClearSelection),
            )),
    )
}

fn voice_controls(s: &App, t: &Theme) -> El {
    let enabled = s.server().connected && !s.loopback && !s.server().stopping;
    // Deafening keeps the microphone off, but the mute button still shows and changes your own
    // mute, which is what you get back once you undeafen.
    let deafened_open = s.server().deafened && !s.server().muted && s.server().state.can_talk;
    let mut controls = row().w_full().gap(size::GAP);
    for (key, glyph, label, message, active, on) in [
        (
            "mute",
            if deafened_open {
                crate::Icon::Mic.glyph()
            } else {
                s.microphone_icon().glyph()
            },
            if s.server().muted {
                "Unmute microphone"
            } else if deafened_open {
                "Mute microphone · it stays off while you're deafened"
            } else {
                "Mute microphone"
            },
            Message::Mute,
            s.server().muted,
            enabled,
        ),
        (
            "deafen",
            s.playback_icon().glyph(),
            if s.server().deafened {
                "Undeafen"
            } else {
                "Deafen"
            },
            Message::Deafen,
            s.server().deafened,
            enabled,
        ),
    ] {
        controls = controls.child(dock_button(icon_button(
            t,
            key,
            glyph,
            label,
            // Red while off, like the rail's own mic and sound icons.
            if active {
                Variant::Danger
            } else {
                Variant::Secondary
            },
            on.then_some(message),
        )));
    }
    controls = controls
        .child(dock_button(share_button(s, t)))
        .child(dock_button(icon_button(
            t,
            "disconnect",
            &icons::DISCONNECT,
            "Disconnect",
            Variant::Hangup,
            (!s.server().stopping).then_some(Message::Stop),
        )));
    let speaking = s.microphone_meter_level();
    // Lit while you are heard; it eases in and out only as that changes, not with every reading.
    // Push-to-talk and voice activation keep it dark while their gate is closed.
    let talking = speaking > crate::audio::SPEAKING_METER && s.transmitting;
    let mut panel = col()
        .key("voice")
        .w_full()
        .gap(8.0)
        .padding(12.0)
        .radius(radius::NESTED)
        .fill(t.wash(0.035))
        .border(
            1.0,
            if talking {
                t.good.alpha(0.5)
            } else {
                t.hairline
            },
        )
        .motion(motion::SETTLE);
    if s.server().streaming.sharing.status.active {
        panel = panel.child(streams::sharing_status(s, t));
    }
    panel = panel
        .child(controls)
        .child(meter(t, "mic-level", speaking, t.good));
    panel
}

/// The voice panel's buttons share its width evenly, a little taller than the rest.
fn dock_button(button: El) -> El {
    button.grow(1.0).h(32.0)
}

/// Opens the share dialog; lit while you share.
fn share_button(s: &App, t: &Theme) -> El {
    let sharing = s.server().streaming.sharing.status.active;
    let supported = s.server().connected
        && !s.loopback
        && !s.server().stopping
        && s.server().state.server_version.starts_with("6.")
        && crate::streaming::AVAILABLE;
    let help = if sharing {
        "You are sharing · manage or stop"
    } else if !crate::streaming::AVAILABLE {
        "Screen sharing is available on Mac and Windows"
    } else if !supported {
        "Screen sharing needs a TeamSpeak 6 server"
    } else {
        "Share your screen"
    };
    icon_button(
        t,
        "share-screen",
        &icons::MONITOR_UP,
        help,
        if sharing {
            Variant::Active
        } else {
            Variant::Secondary
        },
        (supported || sharing).then_some(Message::OpenShare),
    )
}

fn channel_tree(s: &App, t: &Theme) -> El {
    let mut tree = col().key("channels").w_full().gap(2.0);
    let mut hidden_below = None;
    let channels = &s.server().state.channels;
    for (index, channel) in channels.iter().enumerate() {
        if hidden_below.is_some_and(|depth| channel.depth > depth) {
            continue;
        }
        // Only a channel with people or subchannels under it has anything to fold away.
        let foldable = !channel.members.is_empty()
            || channels
                .get(index + 1)
                .is_some_and(|next| next.depth > channel.depth);
        hidden_below = None;
        let collapsed = s.server().collapsed_channels.contains(&channel.id);
        if collapsed {
            hidden_below = Some(channel.depth);
        }
        let current = s
            .server()
            .state
            .current
            .as_ref()
            .is_some_and(|value| value.id == channel.id);
        let member_versions: Vec<_> = channel
            .members
            .iter()
            .map(|member| {
                let (health, detail) = s.member_health(member, current);
                (
                    health as u8,
                    detail,
                    s.server().talking.contains(&member.id),
                    s.server().selected_members.contains(&member.id),
                    s.ui.drag
                        .as_ref()
                        .is_some_and(|drag| drag.ids.contains(&member.id)),
                    s.is_streaming(member),
                    s.user_audio(&member.uid).muted,
                )
            })
            .collect();
        let node = rune::memo(
            // The memo keys what it returns, so its id is the channel's own key.
            format!("channel-{}", channel.id),
            (
                channel,
                (collapsed, foldable),
                current,
                member_versions,
                t,
                s.can_request_join(channel.id),
                s.server().stopping,
                s.server().connected,
                s.loopback,
                s.server().deafened,
                s.microphone_icon() == crate::Icon::MicOff,
                s.ui.drag.as_ref().and_then(|drag| drag.target),
            ),
            || {
                let hint = if current {
                    "Current channel"
                } else {
                    match channel.join_access {
                        client::JoinAccess::Denied => {
                            "You do not have permission to join this channel"
                        }
                        client::JoinAccess::UnsupportedCodec => {
                            "This channel uses an unsupported audio codec"
                        }
                        _ if channel.has_password => "Click to join · a password will be requested",
                        _ => "Click to join this channel",
                    }
                };
                let joinable = (!s.server().stopping && current) || s.can_request_join(channel.id);
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
                let dropping =
                    s.ui.drag
                        .as_ref()
                        .is_some_and(|drag| drag.target == Some(channel.id));
                if dropping {
                    name = name
                        .fill(t.accent.alpha(0.24))
                        .border(1.0, t.accent.alpha(0.6));
                } else if current {
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
                // An empty channel keeps the arrow's space, so names stay lined up.
                let disclosure = if !foldable {
                    el().key("disclosure").size(18.0, size::ROW).a11y_hidden()
                } else {
                    el().key("disclosure")
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
                        )
                };
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
                        .gap(2.0)
                        .pl(20.0)
                        .transition(Transition::opacity());
                    for member in &channel.members {
                        members = members.child(members::interactive(
                            s,
                            t,
                            member,
                            member_row(s, t, member, current),
                        ));
                    }
                    node = node.child(members);
                }
                node
            },
        );
        tree = tree.child(node);
    }
    tree
}

fn member_row(s: &App, t: &Theme, member: &client::Member, current: bool) -> El {
    let mic_muted =
        member.mic_muted || (member.is_self && s.microphone_icon() == crate::Icon::MicOff);
    let sound_muted = member.sound_muted || (member.is_self && s.server().deafened);
    let talking = s.server().talking.contains(&member.id);
    let mut entry = row()
        .key(format!("member-{}", member.id))
        .w_full()
        .h(24.0)
        .px(6.0)
        .gap(6.0)
        .center_y()
        .radius(radius::ROW)
        .transition(Transition::opacity().combined(Transition::offset(-6.0, 0.0)))
        .child(voice_icon(t, talking, mic_muted, sound_muted))
        .child(
            text(if member.is_self {
                format!("{} (you)", member.name)
            } else {
                member.name.clone()
            })
            .font(ty::BODY)
            .color(if talking { t.good } else { t.text })
            .motion(motion::QUICK)
            .truncate_fade()
            .grow(1.0)
            .min_w(0.0),
        );
    // Connection health only earns a place on the row when something is wrong; your own is
    // also in the footer.
    let (health, detail) = s.member_health(member, current);
    let signal = match health {
        usage::Health::Fair => Some((2, t.fair)),
        usage::Health::Poor => Some((1, t.poor)),
        _ => None,
    };
    if let Some((lit, colour)) = signal {
        let help = format!("{} connection\n{detail}", health.label());
        entry = entry.child(signal_bars(t, lit, colour).help(help.clone()).label(help));
    }
    if s.is_streaming(member) {
        entry = entry.child(
            el().help("Sharing a stream")
                .label("Sharing a stream")
                .child(icon(&icons::SCREEN_SHARE, 13.0).color(t.live)),
        );
    }
    if !member.is_self && s.user_audio(&member.uid).muted {
        entry = entry.child(
            el().help("Muted by you")
                .label("Muted by you")
                .child(icon(&icons::VOLUME_OFF, 13.0).color(t.poor)),
        );
    }
    entry
}

/// Three rising bars, the first `lit` of them in `colour`: a weak signal reads as one.
fn signal_bars(t: &Theme, lit: usize, colour: rune::Color) -> El {
    let mut bars = row().key("signal").h(12.0).gap(2.0).items_end().px(2.0);
    for (index, height) in [5.0, 8.5, 12.0].into_iter().enumerate() {
        bars = bars.child(el().w(3.0).h(height).radius(1.0).fill(if index < lit {
            colour
        } else {
            t.faint.alpha(0.35)
        }));
    }
    bars
}

/// What you'll hear from someone, in one place: a grey mic while quiet, green and lit while
/// they talk, red mic-off when muted, red headphones-off when their sound is off too.
fn voice_icon(t: &Theme, talking: bool, mic_muted: bool, sound_muted: bool) -> El {
    let (glyph, colour, help) = if sound_muted {
        (&icons::HEADPHONE_OFF, t.poor, "Sound muted")
    } else if mic_muted {
        (&icons::MIC_OFF, t.poor, "Microphone muted")
    } else if talking {
        (&icons::MIC, t.good, "Speaking")
    } else {
        (&icons::MIC, t.faint, "Microphone on")
    };
    let lit = talking && !mic_muted && !sound_muted;
    el().key("voice")
        .size(20.0, 20.0)
        .radius(10.0)
        .center()
        .fill(if lit {
            t.good.alpha(0.16)
        } else {
            t.good.alpha(0.0)
        })
        .shadow(0.0, 8.0, t.good.alpha(if lit { 0.45 } else { 0.0 }))
        .motion(motion::QUICK)
        .help(help)
        .label(help)
        .child(
            icon(glyph, 13.0)
                .color(colour)
                .motion(motion::QUICK)
                .a11y_hidden(),
        )
}
