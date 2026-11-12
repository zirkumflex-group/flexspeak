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
    overline, pulse, scroll_page, struck,
};
use super::{RailPeek, chat, dispatch, footer, icons, members, send, statistics, streams};
use crate::{App, Message, Tab, client, usage};

pub fn normal(s: &App, cx: &mut Cx<App>, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> El {
    // Full screen, the streams are all there is: no header, sidebar, chat or status bar.
    if streams::fullscreen(s) {
        return pane(t)
            .fill(t.stage)
            .key("main-pane")
            .w_full()
            .h_full()
            .child(streams::view(s, cx, t, k, layer));
    }
    let page = match s.tab {
        Tab::Home => home(s, cx, t, k, layer),
        // On the same panel as the connected window, so it scrolls clear of the status bar.
        Tab::Statistics => row().w_full().h_full().child(
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
                .child(server_rail(s, t, k))
                .child(pane(t).grow(1.0).min_w(0.0).child(connecting_pane(s, t)));
        }
        // One server has nothing to switch between, so the rail waits behind the edge.
        let hidden = s.rail_sessions().count() < 2;
        if hidden {
            rail_edge(s, t, k, layer);
        }
        // The chat column docks beside the stage, or waits behind the right edge as the rail
        // does behind the left.
        let chat = chat::column(s, t, k, layer);
        if chat.is_none() {
            chat::edge(s, t, k, layer);
        }
        return row()
            .w_full()
            .h_full()
            .children((!hidden).then(|| server_rail(s, t, k)))
            .child(sidebar(s, cx, t, k, layer))
            .child(sidebar_edge(s, t))
            .child(
                pane(t)
                    .fill(t.stage)
                    .key("main-pane")
                    .grow(1.0)
                    .min_w(0.0)
                    .child(streams::view(s, cx, t, k, layer)),
            )
            .children(
                chat.map(|chat| [vertical_rule(t), chat])
                    .into_iter()
                    .flatten(),
            );
    }
    saved_servers(s, t, k)
}

/// The saved servers, each with its own Connect. Only a microphone test holds them back.
/// The gap between the window's edge and its cards, and between the cards.
pub const INSET: f32 = 8.0;

/// A panel the connected window is built from. Panels run edge to edge between the title bar and
/// the status bar, as a desktop app's do, with one hairline where two meet.
fn pane(t: &Theme) -> El {
    col().h_full().fill(t.surface)
}

/// The hairline between two panels, full height.
fn vertical_rule(t: &Theme) -> El {
    el().w(1.0).h_full().fill(t.hairline).a11y_hidden()
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
        let mut list = col().key("server-list").w_full().gap(2.0);
        for (index, server) in s.servers.iter().enumerate() {
            let session = s.session_at(&server.address);
            if index > 0 {
                list = list.child(divider(t).mx(8.0));
            }
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
                    "Stop Test"
                },
                Some(&icons::STOP),
                Variant::Secondary,
                (!s.server().stopping).then_some(Message::Stop),
            ));
        }
        content = content.child(
            status
                .key("status")
                .pt(4.0)
                .transition(Transition::opacity()),
        );
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
        .child(content);
    // A snapshot is one frame: it shows the card settled, not the first frame of its fade.
    let hero = if super::snapping() {
        hero
    } else {
        hero.sequence(rune::Sequence::new(ENTRANCE).delay(0.05))
    };
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
    // A plain list row, led by the same coloured tile the server has on the rail and in the
    // sidebar; only the server being connected is tinted.
    row()
        .key(format!("server-{index}"))
        .w_full()
        .min_h(52.0)
        .px(8.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(radius::ROW)
        .fill(if mine {
            t.accent.alpha(0.10)
        } else {
            t.accent.alpha(0.0)
        })
        .motion(motion::QUICK)
        .child(
            el().size(32.0, 32.0)
                .radius(radius::CONTROL)
                .center()
                .fill(tile_colour(server.title()))
                .child(
                    text(initials(server.title()))
                        .font(ty::SMALL_STRONG)
                        .color(rune::Color::WHITE),
                )
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

/// Your microphone or sound on one server: plain while on, struck through in red while off,
/// as the tree draws everyone's.
fn rail_status_icon(
    t: &Theme,
    on: &'static rune::Icon,
    off: &'static rune::Icon,
    is_off: bool,
) -> El {
    if is_off {
        struck(t, off, &icons::STRIKE, 13.0)
    } else {
        icon(on, 13.0).color(t.muted).a11y_hidden()
    }
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
    let unread = session.chat.unread_private();
    if session.unseen_pokes > 0 {
        tile = tile.child(poke_marker(t, session.unseen_pokes));
    } else if unread > 0 && !active {
        // Private messages waiting on a server that isn't on screen.
        tile = tile.child(
            chat::badge(t, unread, t.private)
                .absolute(22.0, -5.0)
                .border(2.0, t.canvas)
                .h(20.0)
                .min_w(20.0)
                .radius(10.0),
        );
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
            &icons::MIC_OFF_GLYPH,
            session.microphone_off(),
        ))
        .child(rail_status_icon(
            t,
            &icons::HEADPHONES,
            &icons::HEADPHONE_OFF_GLYPH,
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
    // A handle, not a status light: it stays quiet while people talk.
    let ink = if near { t.muted } else { t.faint };
    let mut strip = el()
        .key("rail-edge")
        .role("button")
        .label("Show servers")
        .help("Show servers")
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
                // Opens where the pointer already is, level with the handle.
                .absolute(INSET, size::HEADER + (height - RAIL_PEEK_HEIGHT) / 2.0)
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
    channel_menu(s, k, layer);
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
            // Lit only while it narrows the tree: seeing every channel is the usual state.
            if all { Variant::Ghost } else { Variant::Active },
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
        .w(f32::from(s.sidebar_width))
        .padding(12.0)
        .gap(8.0)
        .child(server)
        // The channel list takes the free height, so the voice controls sit at the bottom. The
        // tree needs no heading: the server above it says whose channels they are.
        .child(scroll_page(
            k,
            "channel-scroll",
            channel_tree(s, t).pr(8.0).pb(8.0),
        ))
        .children(selection_bar(s, t))
        .children(server_log(s, cx, t, k))
        .child(voice_controls(s, t))
}

/// The hairline between the sidebar and the stage, which drags to resize the sidebar; a
/// double-click puts it back. Its grip is wider than the line, on the panels' own colours.
fn sidebar_edge(s: &App, t: &Theme) -> El {
    let resizing = s.ui.sidebar_resizing;
    row()
        .key("sidebar-edge")
        .role("separator")
        .label("Resize the sidebar")
        .help("Drag to resize the sidebar · double-click to reset it")
        .w(5.0)
        .h_full()
        .cursor(rune::Cursor::ResizeColumn)
        .on_drag(|s: &mut App, d: &mut Drag, tx: &mut Tx| match d.phase {
            Phase::Down => {
                s.ui.sidebar_resizing = true;
                d.data = f32::from(s.sidebar_width);
                if d.clicks == 2 {
                    s.sidebar_width = super::SIDEBAR_WIDTH;
                }
            }
            Phase::Move => {
                tx.instant();
                s.sidebar_width = super::sidebar_width(Some((d.data + d.dx).round() as u16));
            }
            Phase::Up => {
                s.ui.sidebar_resizing = false;
                if d.cancelled() {
                    s.sidebar_width = d.data.round() as u16;
                }
                dispatch(s, tx, Message::SaveSidebarWidth);
            }
        })
        .child(el().w(2.0).h_full().fill(t.surface).a11y_hidden())
        .child(
            el().w(1.0)
                .h_full()
                .fill(if resizing { t.accent } else { t.hairline })
                .a11y_hidden(),
        )
        .child(el().w(2.0).h_full().fill(t.stage).a11y_hidden())
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
                    "Clear Search",
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
            // Faint until the pointer is over the log, so it can be found without hovering.
            .children((!collapsed).then(|| {
                icon_button(
                    t,
                    "log-search-open",
                    &icons::SEARCH,
                    "Search the log",
                    Variant::Ghost,
                    Some(Message::LogSearch(true)),
                )
                .opacity(if s.ui.log_hover { 1.0 } else { 0.4 })
                .motion(motion::QUICK)
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
        // The X clears what's typed first; with nothing typed it closes the search.
        .trailing(if log.query().is_empty() {
            icon_button(
                t,
                "log-search-close",
                &icons::X,
                "Close search · Esc",
                Variant::Ghost,
                Some(Message::LogSearch(false)),
            )
        } else {
            icon_button(
                t,
                "log-search-close",
                &icons::X,
                "Clear the search",
                Variant::Ghost,
                Some(Message::LogQuery(String::new())),
            )
        })
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
            f32::from(s.sidebar_width) - 24.0 - WIDTH,
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
    let mark = |range: std::ops::Range<usize>| {
        Span::new(range.start, range.end).bg(t.fair.alpha(0.35), 1.0)
    };
    // The name and what happened are separate, so a long name shortens before the event.
    let mut name_spans = vec![Span::new(0, entry.name.len()).bold()];
    name_spans.extend(find(&entry.name, log.query()).map(mark));
    let action_spans: Vec<Span> = find(&entry.action, log.query()).map(mark).into_iter().collect();
    // The event keeps room for about 18 characters before it shortens as well.
    let action_room = (entry.action.chars().count() as f32 * 6.0).min(110.0);
    let help = format!(
        "{words}\n{}",
        entry.at.format("%A, %-d %B %Y, %H:%M:%S")
    );
    let line = row()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .center_y()
        .children((lead > 0).then(|| {
            rich(entry.name.clone(), name_spans)
                .font(ty::SMALL)
                .color(t.text)
                .shrink()
                .min_w(24.0)
                .truncate_fade()
        }))
        .child(
            rich(entry.action.clone(), action_spans)
                .font(ty::SMALL)
                .color(t.text)
                .shrink()
                .min_w(action_room)
                .truncate_fade(),
        );
    row()
        .w_full()
        .gap(8.0)
        .center_y()
        .pl(6.0)
        .pr(8.0)
        .help(help.clone())
        .label(help)
        .child(marker)
        .child(line)
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
    // Deafening keeps the microphone off, but the mute button still changes your own mute,
    // which is what you get back once you undeafen. The icon shows what is sent: nothing.
    let deafened_open = s.server().deafened && !s.server().muted && s.server().state.can_talk;
    let mut controls = row().w_full().gap(size::GAP);
    for (key, glyph, label, message, active, on) in [
        (
            "mute",
            s.microphone_icon().glyph(),
            microphone_help(s, deafened_open),
            Message::Mute,
            s.server().muted,
            enabled,
        ),
        (
            "deafen",
            s.playback_icon().glyph(),
            with_key(
                if s.server().deafened {
                    "Undeafen"
                } else {
                    "Deafen"
                }
                .to_owned(),
                &s.keybinds.toggle_deafen,
            ),
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
            // Red while off, the colour of the strike on the tree's and the rail's own icons.
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
    // The bar sits at the foot of the sidebar with no frame or rule of its own; the meter under the
    // buttons shows when you are heard.
    let mut panel = col().key("voice").w_full().gap(8.0).pt(4.0);
    if s.server().streaming.sharing.status.active {
        panel = panel.child(streams::sharing_status(s, t));
    }
    panel
        .children(quick_share(s, t))
        .child(controls)
        .child(meter(t, "mic-level", speaking, t.good))
}

/// One click to go live while other streams fill the pane, which offers the same itself when
/// it is empty: your main screen, then the game you're playing. Plain until you are live;
/// the live colour belongs to streams that are running.
/// The microphone button's tooltip: why it is on or off, what a click does with its global
/// key, and how you talk in push-to-talk or voice activation.
fn microphone_help(s: &App, deafened_open: bool) -> String {
    let action = if s.server().muted {
        "Unmute microphone"
    } else if deafened_open {
        "Mute microphone · it stays off while you're deafened"
    } else {
        "Mute microphone"
    };
    let mut help = format!(
        "Microphone: {}
{}",
        s.microphone_status(),
        with_key(action.to_owned(), &s.keybinds.toggle_mute)
    );
    match s.transmission.mode {
        crate::transmission::Mode::PushToTalk => {
            let key = s.keybinds.push_to_talk.trim();
            help.push_str(if key.is_empty() {
                "
Push-to-talk · no key set"
            } else {
                "
Push-to-talk · hold "
            });
            help.push_str(key);
        }
        crate::transmission::Mode::VoiceActivation => help.push_str("
Voice activation"),
        crate::transmission::Mode::Continuous => {}
    }
    help
}

/// A control's help with the global key bound to it, as "Settings · Ctrl+," reads.
fn with_key(mut help: String, key: &str) -> String {
    let key = key.trim();
    if !key.is_empty() {
        help.push_str(" · ");
        help.push_str(key);
    }
    help
}

fn quick_share(s: &App, t: &Theme) -> Option<El> {
    let sources = streams::quick_sources(s);
    if sources.is_empty() || !streams::on_stage(s) {
        return None;
    }
    let mut chips = row().w_full().gap(6.0);
    for (index, source) in sources.iter().enumerate() {
        let help = streams::quick_share_help(s, source);
        chips = chips.child(
            row()
                .key(format!("quick-share-{index}"))
                .role("button")
                .label(help.clone())
                .help(help)
                .grow(1.0)
                .min_w(0.0)
                .h(26.0)
                .px(7.0)
                .gap(5.0)
                .center_y()
                .radius(radius::PILL)
                .fill(t.wash(0.05))
                .border(1.0, t.hairline)
                .hover(t.wash(0.10))
                .press(t.wash(0.14))
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(send(Message::QuickShare(source.clone())))
                .child(
                    icon(streams::quick_icon(source), 12.0)
                        .color(t.muted)
                        .a11y_hidden(),
                )
                .child(
                    text(source.name.clone())
                        .font(ty::CAPTION)
                        .color(t.text)
                        .truncate_fade()
                        .grow(1.0)
                        .min_w(0.0)
                        .a11y_hidden(),
                ),
        );
    }
    Some(
        col()
            .key("quick-share")
            .w_full()
            .gap(6.0)
            .child(overline(t, "Go live").ml(2.0))
            .child(chips),
    )
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

/// How far a channel's subchannels step in.
const CHANNEL_INDENT: f32 = 14.0;
/// How far its members do: their icons and names line up with its subchannels', one level
/// below the channel's own.
const MEMBER_INDENT: f32 = 32.0;

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
                    s.user_audio(&member.uid).volume_percent,
                    private_unread(s, member),
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
                if let Some(spacer) = channel.spacer() {
                    return spacer_row(t, channel.id, &spacer);
                }
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
                        _ if channel.has_password => {
                            "Double-click to join · a password will be requested"
                        }
                        _ => "Double-click to join · right-click for options",
                    }
                };
                // The full name leads, since a long one is cut on the row.
                let hint = format!("{}\n{hint}", channel.name);
                let joinable = (!s.server().stopping && current) || s.can_request_join(channel.id);
                // Where people are reads first: an empty channel steps back, one you can't
                // join further still.
                let ink = if current {
                    t.accent
                } else if !joinable {
                    t.faint
                } else if channel.members.is_empty() {
                    t.muted
                } else {
                    t.text
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
                // A double-click joins, as in TeamSpeak, so a slip of one click never moves
                // you mid-conversation; Return joins the focused row.
                let id = channel.id;
                if joinable {
                    name = name
                        .focusable(true)
                        .cursor(rune::Cursor::Pointer)
                        .hover(t.wash(0.05))
                        .press(t.wash(0.09))
                        .on_double_press_tx(send(Message::Join(id)))
                        .on_key(move |s: &mut App, key: &rune::KeyEvent, tx: &mut Tx| {
                            if key.key != rune::Key::Enter {
                                return false;
                            }
                            dispatch(s, tx, Message::Join(id));
                            true
                        });
                }
                if s.server().connected && !s.loopback && !s.server().stopping {
                    name = name.on_secondary_press(
                        move |s: &mut App, point: PointerPress, _: &mut Tx| {
                            s.ui.member_menu = None;
                            s.ui.channel_menu = Some((id, point));
                        },
                    );
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
                    .pl(channel.depth.min(8) as f32 * CHANNEL_INDENT)
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
                        .pl(MEMBER_INDENT)
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

/// A spacer channel: a section label across the tree, with nothing to join or click.
fn spacer_row(t: &Theme, id: u64, spacer: &client::Spacer) -> El {
    let label = text(match spacer.align {
        // Enough copies to cross the widest sidebar; the row clips the rest.
        client::SpacerAlign::Repeat if !spacer.text.is_empty() => {
            spacer.text.repeat(160 / spacer.text.chars().count().max(1) + 1)
        }
        _ => spacer.text.clone(),
    })
    .font(ty::SMALL_STRONG)
    .color(t.faint)
    .min_w(0.0)
    .a11y_hidden();
    let line = row()
        .key("name")
        .w_full()
        .h(size::ROW)
        .px(8.0)
        .center_y()
        .label(spacer.text.clone())
        .help(spacer.text.clone());
    let line = match spacer.align {
        client::SpacerAlign::Left => line.child(label.truncate_fade()),
        client::SpacerAlign::Center => line.justify_center().child(label.truncate_fade()),
        client::SpacerAlign::Right => line.justify_end().child(label.truncate_fade()),
        client::SpacerAlign::Repeat => line.child(label.truncate_fade_width(0.0).grow(1.0)),
    };
    // Lined up with the channel names beside the disclosure arrows' column.
    col()
        .key(format!("channel-{id}"))
        .w_full()
        .pl(20.0)
        .child(line)
}

/// Right-clicking a channel: join it, or fold it.
fn channel_menu(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some((id, point)) = s.ui.channel_menu else {
        return;
    };
    let channels = &s.server().state.channels;
    let Some(index) = channels.iter().position(|channel| channel.id == id) else {
        return;
    };
    let channel = &channels[index];
    let current = s
        .server()
        .state
        .current
        .as_ref()
        .is_some_and(|value| value.id == id);
    let foldable = !channel.members.is_empty()
        || channels
            .get(index + 1)
            .is_some_and(|next| next.depth > channel.depth);
    let collapsed = s.server().collapsed_channels.contains(&id);
    let mut join = MenuItem::new("Join")
        .icon(&icons::CONNECT)
        .disabled(current || !s.can_request_join(id))
        .on_select_tx(move |s: &mut App, tx: &mut Tx| {
            s.ui.channel_menu = None;
            dispatch(s, tx, Message::Join(id));
        });
    if current {
        join = join.description("You're in this channel");
    }
    let menu = context_menu("channel-menu", el(), Some(point))
        .min_width(180.0)
        .on_close(|s: &mut App| s.ui.channel_menu = None)
        .group_label(channel.name.clone())
        .item(join)
        .item(
            MenuItem::new(if collapsed { "Expand" } else { "Collapse" })
                .icon(if collapsed { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT })
                .disabled(!foldable)
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    s.ui.channel_menu = None;
                    dispatch(s, tx, Message::ToggleChannel(id));
                }),
        );
    let _ = menu.build(k, layer);
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
            // Someone talking reads in a heavier weight as well as by the ring.
            .font(if talking { ty::BODY_STRONG } else { ty::BODY })
            .color(t.text)
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
            el().help("Sharing")
                .label("Sharing")
                .child(icon(&icons::SCREEN_SHARE, 13.0).color(t.live)),
        );
    }
    let unread = private_unread(s, member);
    if unread > 0 {
        entry = entry
            .child(chat::badge(t, unread, t.private).help("Private messages you haven't read"));
    }
    let audio = s.user_audio(&member.uid);
    if !member.is_self && !audio.muted && !member.uid.is_empty() && audio.volume_percent != 100 {
        let help = format!("Volume {}% · set by you", audio.volume_percent);
        entry = entry.child(
            el().help(help.clone())
                .label(help)
                .child(icon(&icons::VOLUME_2, 13.0).color(t.faint)),
        );
    }
    if !member.is_self && audio.muted {
        entry = entry.child(
            el().help("Muted by you")
                .label("Muted by you")
                .child(struck(
                    t,
                    &icons::VOLUME_OFF_GLYPH,
                    &icons::VOLUME_OFF_CROSS,
                    13.0,
                )),
        );
    }
    entry
}

/// Private messages from this person you haven't read.
fn private_unread(s: &App, member: &client::Member) -> u32 {
    if member.is_self {
        return 0;
    }
    s.server()
        .chat
        .unread(&crate::chat::Scope::Private(client::chat_key(
            &member.uid,
            member.id,
        )))
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

/// What you'll hear from someone, in one place: headphones struck through in red when their
/// sound is off, a microphone struck through when muted, otherwise a ring that fills and glows
/// green while they talk.
fn voice_icon(t: &Theme, talking: bool, mic_muted: bool, sound_muted: bool) -> El {
    let slot = el().key("voice").size(20.0, 20.0).center();
    let muted = if sound_muted {
        Some((&icons::HEADPHONE_OFF_GLYPH, "Sound muted"))
    } else if mic_muted {
        Some((&icons::MIC_OFF_GLYPH, "Microphone muted"))
    } else {
        None
    };
    if let Some((glyph, help)) = muted {
        return slot
            .help(help)
            .label(help)
            .child(struck(t, glyph, &icons::STRIKE, 13.0).key("muted"));
    }
    let (ring, fill, glow) = if talking {
        (t.good, t.good, t.good.alpha(0.55))
    } else {
        (t.faint.alpha(0.7), t.good.alpha(0.0), t.good.alpha(0.0))
    };
    let help = if talking { "Speaking" } else { "Not speaking" };
    slot.help(help).label(help).child(
        el().key("ring")
            .size(8.0, 8.0)
            .radius(4.0)
            .fill(fill)
            .border(1.5, ring)
            .shadow(0.0, 8.0, glow)
            .motion(motion::QUICK)
            .a11y_hidden(),
    )
}
