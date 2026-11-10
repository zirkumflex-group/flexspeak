//! Text chat: the column beside the stage, its handle and peek while hidden, the pop-up for a
//! message that arrives then, and the samples setup and Settings choose a layout from.
//!
//! One colour per place, everywhere: the channel is the accent, the server `fair`'s amber and
//! a private message `private`'s violet. Pokes are history, not chat: a faded line with a bell.

use std::time::Instant;

use rune::{
    Color, El, Icon, Link, ScrollSpec, Span, TextStyle, Transition, Tx, col, el, icon, rich, row,
    text,
};
use rune_kit::{Kit, TextInput};

use super::home::tile_colour;
use super::theme::{Theme, motion, radius, shadow, size, ty};
use super::widgets::{Variant, button, divider, icon_button};
use super::{ChatToast, RailPeek, dispatch, icons, notices, send};
use crate::chat::{self, Action, Entry, Filter, Kind, Layout, Scope, TabBar};
use crate::{App, Message};

/// The window has to be this wide to dock the column; narrower, it peeks over the stage.
pub const MIN_DOCK_WINDOW: f32 = 1000.0;
/// The grab band along the window's right edge while the column is hidden.
const EDGE: f32 = 12.0;
/// Lines drawn at once; older ones are still in the history.
const SHOWN: usize = 300;

const BADGE: TextStyle = TextStyle::new(10.5, 650.0).features("'tnum' 1");
const TIME: TextStyle = TextStyle::new(11.0, 400.0).features("'tnum' 1");

/// The column's width beside the stage.
pub fn width(s: &App) -> f32 {
    (s.window_size.0 * 0.3).clamp(280.0, 340.0)
}

fn tint(t: &Theme, scope: &Scope) -> Color {
    match scope {
        Scope::Channel | Scope::Past(_) => t.accent,
        Scope::Server => t.fair,
        Scope::Private(_) => t.private,
    }
}

/// The ink on a filled badge of `color`.
fn on(t: &Theme, color: Color) -> Color {
    if color == t.accent {
        t.on_accent
    } else if t.dark {
        Color::BLACK.alpha(0.85)
    } else {
        Color::WHITE
    }
}

fn avatar(name: &str, size: f32) -> El {
    el().size(size, size)
        .radius(size / 2.0)
        .center()
        .fill(tile_colour(name))
        .child(
            text(chat::initial(name))
                .font(TextStyle::new((size * 0.42).round(), 650.0))
                .color(Color::WHITE),
        )
        .a11y_hidden()
}

/// An unread count on a filled pill.
pub fn badge(t: &Theme, count: u32, color: Color) -> El {
    el().min_w(16.0)
        .h(16.0)
        .px(4.5)
        .center()
        .radius(8.0)
        .fill(color)
        .label(format!("{count} unread"))
        .child(
            text(count.min(99).to_string())
                .font(BADGE)
                .color(on(t, color))
                .a11y_hidden(),
        )
}

/// A small tinted tag: where a line went.
fn tag(label: String, glyph: Option<&'static Icon>, color: Color) -> El {
    row()
        .h(18.0)
        .px(7.0)
        .gap(4.0)
        .center()
        .radius(radius::PILL)
        .fill(color.alpha(0.14))
        .children(glyph.map(|glyph| icon(glyph, 11.0).color(color).a11y_hidden()))
        .child(text(label).font(ty::SMALL_STRONG).color(color))
}

/// What the channel is called in a tab, a tag or the message box.
fn channel_label(name: &str) -> String {
    if name.is_empty() {
        "#Channel".to_owned()
    } else {
        format!("#{name}")
    }
}

/// A line's text, with its web addresses as links that open in the browser.
fn body(t: &Theme, content: &str, color: Color) -> El {
    let found = chat::links(content);
    let spans = found
        .iter()
        .map(|range| {
            Span::new(range.start, range.end)
                .color(t.accent)
                .underline(t.accent.alpha(0.45))
        })
        .collect();
    let mut body = rich(content.to_owned(), spans)
        .font(ty::BODY)
        .color(color)
        .wrap()
        .wrap_anywhere()
        .w_full()
        .selectable(true);
    if found.is_empty() {
        return body;
    }
    for range in found {
        body = body.link(Link::new(range.clone(), content[range].to_owned()));
    }
    body.on_link(|_: &mut App, url: &str, tx: &mut Tx| {
        // Anything but a web address is refused there, and nothing opens.
        let _ = tx.open_url(url);
    })
}

fn time(t: &Theme, entry: &Entry) -> El {
    text(entry.time.clone()).font(TIME).color(t.faint)
}

/// The tag One Feed puts on a line. Pressing it points the message box there; a line from a
/// channel you have left opens that channel's history.
fn place_tag(t: &Theme, entry: &Entry, here: u64) -> El {
    let color = tint(t, &entry.scope);
    let (label, glyph) = match &entry.scope {
        Scope::Channel | Scope::Past(_) => (channel_label(&entry.place), None),
        Scope::Server => ("Server".to_owned(), Some(&icons::GLOBE)),
        Scope::Private(_) if entry.own => (format!("to {}", entry.place), Some(&icons::USER)),
        Scope::Private(_) => ("to you".to_owned(), Some(&icons::USER)),
    };
    let target = if entry.scope == Scope::Channel && entry.room != here {
        Scope::Past(entry.room)
    } else {
        entry.scope.clone()
    };
    tag(label, glyph, color)
        .key(format!("chat-tag-{}", entry.id))
        .role("button")
        .help("Write here")
        .cursor(rune::Cursor::Pointer)
        .hover(color.alpha(0.10))
        .on_press_tx(send(Message::Chat(Action::Target(target))))
}

/// A poke, kept as history: a bell where the avatar would be, faint text, the note in italics.
fn poke_line(t: &Theme, entry: &Entry) -> El {
    let (words, name) = if entry.own {
        (format!("You poked {}", entry.place), 0..3)
    } else {
        (format!("{} poked you", entry.from), 0..entry.from.len())
    };
    let mut lines = col().grow(1.0).min_w(0.0).gap(1.0).child(
        row()
            .w_full()
            .gap(6.0)
            .center_y()
            .child(
                rich(
                    words,
                    vec![Span::new(name.start, name.end).weight(600.0).color(t.muted)],
                )
                .font(ty::SMALL)
                .color(t.faint)
                .truncate_fade()
                .min_w(0.0),
            )
            .child(time(t, entry)),
    );
    if !entry.text.is_empty() {
        let note = format!("“{}”", entry.text);
        let whole = Span::new(0, note.len()).italic();
        lines = lines.child(
            rich(note, vec![whole])
                .font(ty::SMALL)
                .color(t.muted)
                .wrap()
                .wrap_anywhere()
                .w_full()
                .selectable(true),
        );
    }
    row()
        .key(format!("chat-{}", entry.id))
        .role("listitem")
        .w_full()
        .gap(9.0)
        .items_start()
        .opacity(0.85)
        .child(
            el().w(26.0)
                .h(16.0)
                .center()
                .child(icon(&icons::BELL_RING, 14.0).color(t.faint))
                .a11y_hidden(),
        )
        .child(lines)
}

/// One line of the history.
fn line(t: &Theme, entry: &Entry, layout: Layout, here: u64) -> El {
    if entry.kind == Kind::Poke {
        return poke_line(t, entry);
    }
    let mut meta = row().w_full().gap(6.0).center_y().child(
        text(entry.from.clone())
            .font(ty::BODY_STRONG)
            .color(t.text)
            .truncate_fade()
            .min_w(0.0),
    );
    if layout == Layout::Feed {
        meta = meta.child(place_tag(t, entry, here));
    }
    meta = meta.child(time(t, entry));
    let mut line =
        row()
            .key(format!("chat-{}", entry.id))
            .role("listitem")
            .w_full()
            .gap(9.0)
            .items_start()
            .child(avatar(&entry.from, 26.0))
            .child(col().grow(1.0).min_w(0.0).gap(1.0).child(meta).child(body(
                t,
                &entry.text,
                t.text,
            )));
    // In the feed a private line sits on a tint, so it can't pass for channel talk.
    if layout == Layout::Feed && entry.scope.is_private() {
        line = line
            .px(8.0)
            .py(7.0)
            .radius(8.0)
            .fill(t.private.alpha(if t.dark { 0.10 } else { 0.07 }));
    }
    line
}

/// The lists that open under a pointer: the channel group, the people group, and the message
/// box's choice of where to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum List {
    Channels = 0,
    People = 1,
    SendTo = 2,
}

impl List {
    const ALL: [Self; 3] = [Self::Channels, Self::People, Self::SendTo];

    fn key(self) -> &'static str {
        match self {
            Self::Channels => "chat-group-channels",
            Self::People => "chat-group-people",
            Self::SendTo => "chat-to",
        }
    }
}

/// What the column's top bar and message box show, for the real chat and the samples alike.
struct Head<'a> {
    channel: &'a str,
    target: &'a Scope,
    filter: &'a Filter,
    /// Tabs is on its All tab.
    everything: bool,
    /// Lines are tagged and the message box chooses where it sends: One Feed, or Tabs' All tab.
    feed: bool,
    /// Each private conversation: its key, the person's name, and what's unread.
    peers: Vec<(&'a str, &'a str, u32)>,
    /// Channels you have left that still have lines: number and name.
    past: Vec<(u64, &'a str)>,
    unread_channel: u32,
    unread_server: u32,
    tab_bar: TabBar,
    /// Whether the bar ends in the button that hides the column.
    hide: bool,
}

impl<'a> Head<'a> {
    fn of(s: &'a App, hide: bool) -> Self {
        let session = s.server();
        let chat = &session.chat;
        Self {
            channel: session
                .state
                .current
                .as_ref()
                .map_or("", |channel| channel.name.as_str()),
            target: &chat.target,
            filter: &chat.filter,
            everything: chat.everything && s.chat_options.layout == Layout::Tabs,
            feed: chat.tagged(s.chat_options.layout),
            peers: chat
                .peers
                .iter()
                .map(|peer| {
                    (
                        peer.key.as_str(),
                        peer.name.as_str(),
                        chat.unread(&Scope::Private(peer.key.clone())),
                    )
                })
                .collect(),
            past: chat
                .past()
                .map(|room| (room.id, room.name.as_str()))
                .collect(),
            unread_channel: chat.unread(&Scope::Channel),
            unread_server: chat.unread(&Scope::Server),
            tab_bar: s.chat_options.tab_bar,
            hide,
        }
    }

    fn unread_private(&self) -> u32 {
        self.peers.iter().map(|(_, _, unread)| unread).sum()
    }

    fn peer_name(&self, key: &str) -> &'a str {
        self.peers
            .iter()
            .find(|(other, _, _)| *other == key)
            .map_or("", |(_, name, _)| *name)
    }

    fn past_name(&self, id: u64) -> &'a str {
        self.past
            .iter()
            .find(|(other, _)| *other == id)
            .map_or("", |(_, name)| *name)
    }

    /// Whether a conversation's tab is the open one.
    fn on(&self, scope: &Scope) -> bool {
        !self.everything && self.target == scope
    }

    /// What pointing the message box somewhere asks for: on Tabs' All tab that leaves the view
    /// alone, elsewhere it opens the conversation.
    fn aim(&self, scope: Scope) -> Action {
        if self.everything {
            Action::SendTo(scope)
        } else {
            Action::Target(scope)
        }
    }

    fn channel_name(&self) -> &'a str {
        if self.channel.is_empty() {
            "Channel"
        } else {
            self.channel
        }
    }
}

fn hide_button(t: &Theme) -> El {
    icon_button(
        t,
        "chat-hide",
        &icons::PANEL_RIGHT_CLOSE,
        if cfg!(target_os = "macos") {
            "Hide chat · ⇧⌘C"
        } else {
            "Hide chat · Ctrl+Shift+C"
        },
        Variant::Ghost,
        Some(Message::Chat(Action::Toggle)),
    )
}

/// The bar both layouts share: what leads scrolls sideways, the hide button stays put.
fn bar(t: &Theme, strip: El, hide: bool) -> El {
    row()
        .key("chat-bar")
        .w_full()
        .h(40.0)
        .pl(8.0)
        .pr(6.0)
        .gap(4.0)
        .center_y()
        .child(
            el().key("chat-bar-strip")
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .scroll_x()
                .scroll_edge_fade(12.0)
                .child(strip),
        )
        .children(hide.then(|| hide_button(t)))
}

/// The key of a conversation's tab in the side-by-side bar.
pub fn tab_key(scope: &Scope) -> String {
    match scope {
        Scope::Channel => "chat-tab-channel".to_owned(),
        Scope::Server => "chat-tab-server".to_owned(),
        Scope::Private(key) => format!("chat-tab-{key}"),
        Scope::Past(id) => format!("chat-tab-past-{id}"),
    }
}

/// One tab. The open one is underlined in its place's colour; a private one or a left channel's
/// can be closed. A left channel's is fainter: it is there to read.
fn tab(t: &Theme, lead: El, label: &str, scope: Scope, selected: bool, unread: u32) -> El {
    let color = tint(t, &scope);
    let key = tab_key(&scope);
    let close = match &scope {
        Scope::Private(key) => Some(Action::ClosePeer(key.clone())),
        Scope::Past(id) => Some(Action::ClosePast(*id)),
        Scope::Channel | Scope::Server => None,
    };
    let rest = if matches!(scope, Scope::Past(_)) {
        t.faint
    } else {
        t.muted
    };
    let mut tab = row()
        .key(key.clone())
        .role("tab")
        .label(label.to_owned())
        .checked(selected)
        .h(30.0)
        .px(7.0)
        .gap(5.0)
        .center_y()
        .radius(radius::CONTROL)
        .motion(motion::QUICK)
        .child(lead)
        .child(
            text(label.to_owned())
                .font(if selected { ty::BODY_STRONG } else { ty::BODY })
                .color(if selected {
                    color
                } else if unread > 0 {
                    t.text
                } else {
                    rest
                })
                .max_w(96.0)
                .truncate_fade(),
        );
    if unread > 0 {
        tab = tab.child(badge(t, unread, color));
    }
    if selected {
        if let Some(close) = close {
            tab = tab.child(
                el().key(format!("{key}-close"))
                    .role("button")
                    .label("Close this conversation")
                    .help("Close this conversation")
                    .size(16.0, 16.0)
                    .center()
                    .radius(4.0)
                    .cursor(rune::Cursor::Pointer)
                    .hover(t.wash(0.10))
                    .child(icon(&icons::X, 11.0).color(t.muted).a11y_hidden())
                    .on_press_tx(send(Message::Chat(close))),
            );
        }
        tab.child(underline(color))
    } else {
        tab.focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(t.wash(0.06))
            .press(t.wash(0.10))
            .focus_ring(2.0, -2.0, t.accent.alpha(0.7))
            .on_press_tx(send(Message::Chat(Action::Target(scope))))
    }
}

/// The mark under the open tab.
fn underline(color: Color) -> El {
    el().pin(Some(8.0), None, Some(8.0), Some(-5.0))
        .h(2.0)
        .radius(1.0)
        .fill(color)
        .a11y_hidden()
}

/// A tab's leading glyph, in its place's colour while open.
fn glyph(t: &Theme, glyph: &'static Icon, scope: &Scope, open: bool) -> El {
    let rest = if matches!(scope, Scope::Past(_)) {
        t.faint
    } else {
        t.muted
    };
    icon(glyph, 14.0)
        .color(if open { tint(t, scope) } else { rest })
        .a11y_hidden()
}

/// The tab at the far left that shows every conversation together again.
fn all_tab(t: &Theme, selected: bool) -> El {
    let mut tab = row()
        .key("chat-tab-all")
        .role("tab")
        .label("All")
        .help("Every conversation together")
        .checked(selected)
        .h(30.0)
        .px(7.0)
        .gap(5.0)
        .center_y()
        .radius(radius::CONTROL)
        .motion(motion::QUICK)
        .child(
            icon(&icons::LAYOUT_GRID, 14.0)
                .color(if selected { t.text } else { t.muted })
                .a11y_hidden(),
        )
        .child(
            text("All")
                .font(if selected { ty::BODY_STRONG } else { ty::BODY })
                .color(if selected { t.text } else { t.muted }),
        );
    if selected {
        tab = tab.child(underline(t.text));
    } else {
        tab = tab
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(t.wash(0.06))
            .press(t.wash(0.10))
            .focus_ring(2.0, -2.0, t.accent.alpha(0.7))
            .on_press_tx(send(Message::Chat(Action::Everything)));
    }
    tab
}

/// Tabs, every conversation side by side: the channel, the server, one per person, then the
/// channels you have left. The bar scrolls sideways when they don't fit.
fn tabs(t: &Theme, head: &Head) -> El {
    if head.tab_bar == TabBar::Grouped {
        return groups(t, head);
    }
    let on = |scope: &Scope| head.on(scope);
    let mut strip = row()
        .role("tablist")
        .label("Conversations")
        .h_full()
        .gap(0.0)
        .center_y()
        .child(all_tab(t, head.everything))
        .child(tab(
            t,
            glyph(t, &icons::HASH, &Scope::Channel, on(&Scope::Channel)),
            head.channel_name(),
            Scope::Channel,
            on(&Scope::Channel),
            head.unread_channel,
        ))
        .child(tab(
            t,
            glyph(t, &icons::GLOBE, &Scope::Server, on(&Scope::Server)),
            "Server",
            Scope::Server,
            on(&Scope::Server),
            head.unread_server,
        ));
    for (key, name, unread) in &head.peers {
        let scope = Scope::Private((*key).to_owned());
        let selected = on(&scope);
        strip = strip.child(tab(t, avatar(name, 16.0), name, scope, selected, *unread));
    }
    for (id, name) in &head.past {
        let scope = Scope::Past(*id);
        let selected = on(&scope);
        strip = strip.child(tab(
            t,
            glyph(t, &icons::HASH, &scope, selected),
            name,
            scope,
            selected,
            0,
        ));
    }
    bar(t, strip, head.hide)
}

/// One tab that stands for a kind of conversation. It shows the open one of its kind, and a
/// chevron when pointing at it lists the others.
#[allow(clippy::too_many_arguments)]
fn group_tab(
    t: &Theme,
    list: List,
    lead: El,
    label: &str,
    color: Color,
    selected: bool,
    unread: u32,
    more: bool,
    press: Action,
) -> El {
    let ink = if selected {
        color
    } else if unread > 0 {
        t.text
    } else {
        t.muted
    };
    let mut tab = row()
        .key(list.key())
        .role("tab")
        .label(label.to_owned())
        .checked(selected)
        .h(30.0)
        .px(7.0)
        .gap(5.0)
        .center_y()
        .radius(radius::CONTROL)
        .motion(motion::QUICK)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.06))
        .press(t.wash(0.10))
        .focus_ring(2.0, -2.0, t.accent.alpha(0.7))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| {
            dispatch(s, tx, Message::Chat(press.clone()));
            // A press lists the others at once, as pointing does after a moment.
            if more {
                s.ui.chat_lists[list as usize].press();
            }
        })
        .child(lead)
        .child(
            text(label.to_owned())
                .font(if selected { ty::BODY_STRONG } else { ty::BODY })
                .color(ink)
                .max_w(96.0)
                .truncate_fade(),
        );
    if unread > 0 {
        tab = tab.child(badge(t, unread, color));
    }
    if more {
        tab = tab
            .child(icon(&icons::CHEVRON_DOWN, 11.0).color(ink).a11y_hidden())
            .help("Point here to see them all")
            .on_hover_change(move |s: &mut App, inside, tx: &mut Tx| {
                hover_list(s, tx, list, inside)
            });
    }
    if selected {
        tab = tab.child(underline(color));
    }
    tab
}

/// Tabs grouped by kind: channels, the server, people. Each group shows the conversation of
/// its kind that is open; pointing at it lists the rest (`lists`).
fn groups(t: &Theme, head: &Head) -> El {
    let channel_open = !head.everything && head.target.is_channel();
    let channel = match head.target {
        Scope::Past(id) => head.past_name(*id),
        _ => head.channel_name(),
    };
    let mut strip = row()
        .role("tablist")
        .label("Conversations")
        .h_full()
        .gap(0.0)
        .center_y()
        .child(all_tab(t, head.everything))
        .child(group_tab(
            t,
            List::Channels,
            glyph(t, &icons::HASH, &Scope::Channel, channel_open),
            channel,
            t.accent,
            channel_open,
            head.unread_channel,
            !head.past.is_empty(),
            Action::Target(Scope::Channel),
        ))
        .child(tab(
            t,
            glyph(t, &icons::GLOBE, &Scope::Server, head.on(&Scope::Server)),
            "Server",
            Scope::Server,
            head.on(&Scope::Server),
            head.unread_server,
        ));
    if !head.peers.is_empty() {
        let open = match head.target {
            Scope::Private(key) if !head.everything => Some(head.peer_name(key)),
            _ => None,
        };
        let lead = match open {
            Some(name) => avatar(name, 16.0),
            None => glyph(t, &icons::USER, &Scope::Server, false),
        };
        // What's unread in the others; the open one's own count is read as it arrives.
        strip = strip.child(group_tab(
            t,
            List::People,
            lead,
            open.unwrap_or("People"),
            t.private,
            open.is_some(),
            head.unread_private(),
            head.peers.len() > 1 || open.is_none(),
            Action::People,
        ));
    }
    bar(t, strip, head.hide)
}

/// The pointer came onto or left a list's tab or the list itself.
fn hover_list(s: &mut App, tx: &mut Tx, list: List, inside: bool) {
    s.ui.chat_lists[list as usize].hover(Instant::now(), |peek| peek.strip = inside);
    RailPeek::WAKE
        .into_iter()
        .for_each(|wake| tx.wake_after(wake));
}

/// One row of a list: what it leads with, a name, a note, and what's unread.
#[allow(clippy::too_many_arguments)]
fn list_row(
    t: &Theme,
    list: List,
    index: usize,
    lead: El,
    name: &str,
    note: &str,
    scope: Scope,
    chosen: bool,
    unread: u32,
    press: Action,
) -> El {
    let color = tint(t, &scope);
    let mut row = row()
        .key(format!("{}-row{index}", list.key()))
        .role("menuitemradio")
        .label(name.to_owned())
        .checked(chosen)
        .w_full()
        .h(30.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .radius(radius::ROW)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.07))
        .press(t.wash(0.11))
        .focus_ring(2.0, -2.0, t.accent.alpha(0.7))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| {
            s.ui.chat_lists[list as usize] = RailPeek::default();
            dispatch(s, tx, Message::Chat(press.clone()));
        })
        .child(el().w(16.0).center().child(lead))
        .child(
            text(name.to_owned())
                .font(if chosen { ty::BODY_STRONG } else { ty::BODY })
                .color(if chosen { color } else { t.text })
                .truncate_fade()
                .min_w(0.0),
        )
        .child(
            text(note.to_owned())
                .font(ty::CAPTION)
                .color(t.faint)
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0),
        );
    if unread > 0 {
        row = row.child(badge(t, unread, color));
    }
    if chosen {
        row = row.child(icon(&icons::CHECK, 13.0).color(color).a11y_hidden());
    }
    row
}

/// The rows of each list. A group's rows open a conversation in Tabs and narrow the stream to
/// it in One Feed; the message box's rows choose where it sends.
fn list_rows(t: &Theme, head: &Head, list: List, narrow: bool) -> Vec<El> {
    let on = |scope: &Scope| {
        if narrow {
            *head.filter == Filter::Only(scope.clone())
        } else if list == List::SendTo {
            head.target == scope
        } else {
            head.on(scope)
        }
    };
    let press = |scope: &Scope| {
        if narrow {
            Action::Narrow(scope.clone())
        } else if list == List::SendTo {
            head.aim(scope.clone())
        } else {
            Action::Target(scope.clone())
        }
    };
    let channel = |index: usize| {
        list_row(
            t,
            list,
            index,
            glyph(t, &icons::HASH, &Scope::Channel, on(&Scope::Channel)),
            head.channel_name(),
            "you are here",
            Scope::Channel,
            on(&Scope::Channel),
            head.unread_channel,
            press(&Scope::Channel),
        )
    };
    let people = |start: usize| {
        head.peers
            .iter()
            .enumerate()
            .map(move |(index, (key, name, unread))| {
                let scope = Scope::Private((*key).to_owned());
                let (chosen, press) = (on(&scope), press(&scope));
                list_row(
                    t,
                    list,
                    start + index,
                    avatar(name, 16.0),
                    name,
                    "",
                    scope,
                    chosen,
                    *unread,
                    press,
                )
            })
    };
    match list {
        List::Channels => std::iter::once(channel(0))
            .chain(head.past.iter().enumerate().map(|(index, (id, name))| {
                let scope = Scope::Past(*id);
                let (chosen, press) = (on(&scope), press(&scope));
                list_row(
                    t,
                    list,
                    index + 1,
                    glyph(t, &icons::HASH, &scope, chosen),
                    name,
                    "history",
                    scope,
                    chosen,
                    0,
                    press,
                )
            }))
            .collect(),
        List::People => people(0).collect(),
        // Where a message can go: never a channel you have left.
        List::SendTo => std::iter::once(channel(0))
            .chain(std::iter::once(list_row(
                t,
                list,
                1,
                glyph(t, &icons::GLOBE, &Scope::Server, on(&Scope::Server)),
                "Server",
                "everyone",
                Scope::Server,
                on(&Scope::Server),
                head.unread_server,
                press(&Scope::Server),
            )))
            .chain(people(2))
            .collect(),
    }
}

/// The lists that are open, each hung from its tab (or, for the message box's, above it). They
/// go on the layer above the column, so call this after the column is placed.
fn lists(s: &App, t: &Theme, layer: &mut Vec<El>) {
    let now = Instant::now();
    let head = Head::of(s, false);
    let tabs = s.chat_options.layout == Layout::Tabs;
    let grouped = head.tab_bar == TabBar::Grouped;
    let (window_w, window_h) = s.window_size;
    // Until its tab has been laid out, a list hangs from where the column starts.
    let column = if s.chat_docked() {
        window_w - width(s)
    } else {
        window_w - 8.0 - width(s)
    };
    for list in List::ALL {
        let shown = match list {
            List::Channels => grouped && !head.past.is_empty(),
            List::People => grouped && !head.peers.is_empty(),
            List::SendTo => head.feed,
        };
        if !shown || !s.ui.chat_lists[list as usize].open(now) {
            continue;
        }
        // In One Feed a group's rows narrow the stream; in Tabs they open the conversation.
        let rows = list_rows(t, &head, list, !tabs && list != List::SendTo);
        let wide = 232.0_f32.min(width(s) - 16.0);
        let anchor = rune::memo::layout(list.key());
        let left = anchor
            .map_or(column + 8.0, |rect| rect[0])
            .min(window_w - wide - 8.0)
            .max(8.0);
        let card = col()
            .key(format!("{}-list", list.key()))
            .role("menu")
            .label(match list {
                List::Channels => "Channels",
                List::People => "People",
                List::SendTo => "Send to",
            })
            .w(wide)
            .padding(4.0)
            .gap(1.0)
            .radius(radius::CARD)
            .fill(t.raised)
            .border(1.0, t.hairline)
            .box_shadow(shadow(0.0, 8.0, 24.0, Color::BLACK.alpha(0.4)))
            .transition(Transition::opacity().combined(Transition::offset(0.0, -4.0)))
            .on_hover_change(move |s: &mut App, inside, tx: &mut Tx| {
                s.ui.chat_lists[list as usize].hover(Instant::now(), |peek| peek.card = inside);
                RailPeek::WAKE
                    .into_iter()
                    .for_each(|wake| tx.wake_after(wake));
            })
            .children(rows);
        layer.push(match list {
            // Above the message box, growing upwards.
            List::SendTo => {
                let top = anchor.map_or(window_h - 40.0 - 84.0, |rect| rect[1]);
                card.pin(Some(left), None, None, Some(window_h - top + 4.0))
            }
            _ => {
                let bottom = anchor.map_or(size::HEADER + 36.0, |rect| rect[1] + rect[3]);
                card.absolute(left, bottom + 6.0)
            }
        });
    }
}

/// One of One Feed's chips. `list` gives it a chevron and the list that opens under the pointer.
#[allow(clippy::too_many_arguments)]
fn chip(
    t: &Theme,
    key: &str,
    label: &str,
    glyph: Option<&'static Icon>,
    selected: bool,
    unread: (u32, Color),
    list: Option<List>,
    press: Action,
) -> El {
    let ink = if selected { t.canvas } else { t.muted };
    let mut chip = row()
        .key(key.to_owned())
        .role("radio")
        .label(label.to_owned())
        .checked(selected)
        .h(22.0)
        .px(9.0)
        .gap(4.0)
        .center()
        .radius(radius::PILL)
        .fill(if selected { t.text } else { t.raised })
        .motion(motion::QUICK)
        .children(glyph.map(|glyph| icon(glyph, 11.0).color(ink).a11y_hidden()))
        .child(
            text(label.to_owned())
                .font(ty::SMALL_STRONG)
                .color(ink)
                .max_w(110.0)
                .truncate_fade(),
        );
    if unread.0 > 0 && !selected {
        chip = chip.child(badge(t, unread.0, unread.1));
    }
    if let Some(list) = list {
        chip = chip
            .child(icon(&icons::CHEVRON_DOWN, 10.0).color(ink).a11y_hidden())
            .help("Point here to pick one")
            .on_hover_change(move |s: &mut App, inside, tx: &mut Tx| {
                hover_list(s, tx, list, inside)
            });
    }
    // A chip with a list stays pressable while chosen: pressing it widens back to its kind.
    if !selected || list.is_some() {
        chip = chip
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(t.wash(0.06))
            .press(t.wash(0.10))
            .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                dispatch(s, tx, Message::Chat(press.clone()));
                if let Some(list) = list {
                    s.ui.chat_lists[list as usize].press();
                }
            });
    }
    chip
}

/// One Feed: chips that narrow the stream. Side by side there is one per kind. Grouped, the
/// channel and people chips also list each channel and person, to narrow to just one.
fn filters(t: &Theme, head: &Head) -> El {
    let channel = channel_label(head.channel);
    let grouped = head.tab_bar == TabBar::Grouped;
    let mut strip = row()
        .role("radiogroup")
        .label("Show")
        .h_full()
        .gap(6.0)
        .center_y();
    for filter in Filter::ALL {
        let selected = *head.filter == filter;
        let key = format!("chat-filter-{filter:?}");
        let chip = match filter {
            Filter::All => chip(
                t,
                &key,
                "All",
                None,
                selected,
                (0, t.accent),
                None,
                Action::Filter(filter),
            ),
            Filter::Channel if grouped => {
                // Narrowed to one channel, the chip carries its name.
                let (label, one) = match head.filter {
                    Filter::Only(Scope::Channel) => (channel.clone(), true),
                    Filter::Only(Scope::Past(id)) => (channel_label(head.past_name(*id)), true),
                    _ => ("Channels".to_owned(), false),
                };
                chip(
                    t,
                    List::Channels.key(),
                    &label,
                    None,
                    selected || one,
                    (head.unread_channel, t.accent),
                    (!head.past.is_empty()).then_some(List::Channels),
                    Action::Filter(filter),
                )
            }
            Filter::Channel => chip(
                t,
                &key,
                &channel,
                None,
                selected,
                (head.unread_channel, t.accent),
                None,
                Action::Filter(filter),
            ),
            Filter::Server => chip(
                t,
                &key,
                "Server",
                Some(&icons::GLOBE),
                selected,
                (head.unread_server, t.fair),
                None,
                Action::Filter(filter),
            ),
            Filter::Private if grouped => {
                let (label, one) = match head.filter {
                    Filter::Only(Scope::Private(key)) => (head.peer_name(key), true),
                    _ => ("People", false),
                };
                chip(
                    t,
                    List::People.key(),
                    label,
                    Some(&icons::USER),
                    selected || one,
                    (head.unread_private(), t.private),
                    (head.peers.len() > 1 || (one && !head.peers.is_empty()))
                        .then_some(List::People),
                    Action::Filter(filter),
                )
            }
            Filter::Private => chip(
                t,
                &key,
                "Private",
                Some(&icons::USER),
                selected,
                (head.unread_private(), t.private),
                None,
                Action::Filter(filter),
            ),
            Filter::Only(_) => continue,
        };
        strip = strip.child(chip);
    }
    bar(t, strip, head.hide)
}

/// The lines, newest at the foot, following new ones while scrolled there.
fn history(t: &Theme, k: &Kit, lines: Vec<El>, key: &str, empty: &str) -> El {
    if lines.is_empty() {
        return col().grow(1.0).min_h(0.0).w_full().center().child(
            text(empty.to_owned())
                .font(ty::SMALL)
                .color(t.faint)
                .key("chat-empty"),
        );
    }
    k.scroll_bar(
        col()
            .key(key.to_owned())
            .role("list")
            .label("Messages")
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .scroll(ScrollSpec { stick_bottom: true })
            .scroll_edge_fade(14.0)
            .child(
                // A short conversation rests on the message box, as the newest line always does.
                col()
                    .key("chat-lines")
                    .w_full()
                    .grow(1.0)
                    .justify_end()
                    .px(14.0)
                    .py(10.0)
                    .gap(10.0)
                    .children(lines),
            ),
    )
}

/// What the message box says while empty, and whether it can send.
fn prompt(s: &App, head: &Head) -> (String, bool) {
    match head.target {
        Scope::Channel | Scope::Server => ("Write a message".to_owned(), true),
        Scope::Private(key) if s.chat_member(key).is_some() => ("Write a message".to_owned(), true),
        Scope::Private(key) => (
            format!("{} isn’t on the server", head.peer_name(key)),
            false,
        ),
        Scope::Past(_) => ("You left this channel".to_owned(), false),
    }
}

/// The line above the message box that says who gets what is written: a glyph or the person's
/// initial, the name in the place's colour, and who reads it.
fn recipient(t: &Theme, head: &Head) -> El {
    let color = tint(t, head.target);
    let (lead, name, note) = match head.target {
        Scope::Channel => (
            icon(&icons::HASH, 13.0).color(color).a11y_hidden(),
            head.channel_name(),
            "everyone in the channel",
        ),
        Scope::Server => (
            icon(&icons::GLOBE, 13.0).color(color).a11y_hidden(),
            "Server",
            "everyone on the server",
        ),
        Scope::Private(key) => (
            avatar(head.peer_name(key), 16.0),
            head.peer_name(key),
            "only the two of you",
        ),
        Scope::Past(id) => (
            icon(&icons::HASH, 13.0).color(t.faint).a11y_hidden(),
            head.past_name(*id),
            "history",
        ),
    };
    let ink = if matches!(head.target, Scope::Past(_)) {
        t.muted
    } else {
        color
    };
    row()
        .key(List::SendTo.key())
        .w_full()
        .h(28.0)
        .pl(10.0)
        .pr(6.0)
        .gap(6.0)
        .center_y()
        .child(text("To").font(ty::CAPTION).color(t.faint))
        .child(lead)
        .child(
            text(name.to_owned())
                .font(ty::SMALL_STRONG)
                .color(ink)
                .max_w(150.0)
                .truncate_fade(),
        )
        .child(
            text(format!("· {note}"))
                .font(ty::CAPTION)
                .color(t.faint)
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0),
        )
}

/// The message box's frame: who it goes to above, the field below.
fn send_box(t: &Theme, to: El, field: El, edge: Color) -> El {
    el().key("chat-composer")
        .w_full()
        .px(12.0)
        .pt(4.0)
        .pb(12.0)
        .child(
            col()
                .w_full()
                .radius(8.0)
                .fill(t.recess)
                .border(1.0, edge)
                .motion(motion::QUICK)
                .child(to)
                .child(el().h(1.0).w_full().fill(t.hairline).a11y_hidden())
                .child(field),
        )
}

/// A small text button inside the recipient line.
fn inline_button(t: &Theme, key: &str, label: &str, help: &str, message: Message) -> El {
    el().key(key.to_owned())
        .role("button")
        .label(label.to_owned())
        .help(help.to_owned())
        .h(20.0)
        .px(8.0)
        .center()
        .radius(radius::CONTROL)
        .fill(t.wash(0.06))
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(t.wash(0.06))
        .press(t.wash(0.10))
        .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
        .on_press_tx(send(message))
        .child(text(label.to_owned()).font(ty::SMALL_STRONG).color(t.text))
}

/// The message box. Who gets the message is named above the field; in One Feed that line also
/// chooses it, listing the places when pointed at or pressed.
fn composer(s: &App, t: &Theme, k: &Kit, head: &Head) -> El {
    let feed = head.feed;
    let everything = head.everything;
    let (placeholder, enabled) = prompt(s, head);
    let ready = enabled && !s.server().chat.draft.trim().is_empty();
    let field = TextInput::new(&s.ui.chat_input)
        .placeholder(&placeholder)
        .label(&placeholder)
        .plain()
        .width(None)
        .height(34.0)
        .disabled(!enabled)
        .on_edit(|s: &mut App| &mut s.ui.chat_input)
        .on_change(|s: &mut App, tx: &mut Tx| {
            let value = s.ui.chat_input.text.clone();
            dispatch(s, tx, Message::Chat(Action::Draft(value)));
        })
        .on_submit(send(Message::Chat(Action::Send)))
        // In the feed, Escape lets go of a private or server target.
        .on_key(move |s: &mut App, key: &rune::KeyEvent, tx: &mut Tx| {
            let elsewhere = s.server().chat.target != Scope::Channel;
            if key.key == rune::Key::Escape && feed && elsewhere {
                let back = if everything {
                    Action::SendTo(Scope::Channel)
                } else {
                    Action::Target(Scope::Channel)
                };
                dispatch(s, tx, Message::Chat(back));
                return true;
            }
            false
        })
        .trailing(icon_button(
            t,
            "chat-send",
            &icons::SEND,
            "Send · Return",
            Variant::Ghost,
            ready.then_some(Message::Chat(Action::Send)),
        ))
        .build(k);
    let mut to = recipient(t, head);
    if feed {
        let list = List::SendTo;
        to = to
            .role("button")
            .label("Choose who gets this")
            .help("Choose who gets this")
            .focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(t.wash(0.04))
            .focus_ring(2.0, -2.0, t.accent.alpha(0.7))
            .on_hover_change(move |s: &mut App, inside, tx: &mut Tx| {
                hover_list(s, tx, list, inside)
            })
            .on_press(move |s: &mut App| s.ui.chat_lists[list as usize].press())
            .child(icon(&icons::CHEVRON_UP, 12.0).color(t.faint).a11y_hidden());
        if *head.target != Scope::Channel {
            to = to.child(
                el().key("chat-to-clear")
                    .role("button")
                    .label("Back to the channel")
                    .help("Back to the channel · Esc")
                    .size(20.0, 20.0)
                    .center()
                    .radius(4.0)
                    .cursor(rune::Cursor::Pointer)
                    .hover(t.wash(0.10))
                    .child(icon(&icons::X, 12.0).color(t.muted).a11y_hidden())
                    .on_press_tx(send(Message::Chat(head.aim(Scope::Channel)))),
            );
        }
    }
    // A channel you left can be joined again from here.
    if let Scope::Past(id) = head.target
        && s.server().can_request_join(s.loopback, *id)
    {
        to = to.child(inline_button(
            t,
            "chat-join",
            "Join",
            "Join this channel again",
            Message::Join(*id),
        ));
    }
    let edge = if s.ui.chat_input.focused && enabled {
        tint(t, head.target).alpha(0.55)
    } else if head.target.is_private() {
        t.private.alpha(0.3)
    } else {
        t.hairline
    };
    send_box(t, to, field, edge)
}

/// The chat: bar, history and message box, on `fill`.
fn panel(s: &App, t: &Theme, k: &Kit, docked: bool) -> El {
    let layout = s.chat_options.layout;
    let chat = &s.server().chat;
    let head = Head::of(s, docked);
    let shown = chat.shown(layout);
    let skip = shown.len().saturating_sub(SHOWN);
    // Rebuilt only when a line arrives or the view changes, not with every meter reading.
    let lines = rune::memo(
        if docked {
            "chat-history"
        } else {
            "chat-history-peek"
        },
        (
            chat.version,
            &chat.target,
            &chat.filter,
            chat.everything,
            layout,
            t,
        ),
        || {
            // Tabs' All tab draws lines as the feed does, each tagged with where it went.
            let style = if head.feed {
                Layout::Feed
            } else {
                Layout::Tabs
            };
            history(
                t,
                k,
                shown[skip..]
                    .iter()
                    .map(|entry| line(t, entry, style, chat.here))
                    .collect(),
                "chat-scroll",
                match (style, &chat.target) {
                    (Layout::Tabs, Scope::Past(_)) => "Nothing was said here",
                    (Layout::Tabs, _) => "No messages here yet",
                    (Layout::Feed, _) => "No messages yet",
                },
            )
        },
    );
    col()
        .key("chat")
        .role("group")
        .label("Chat")
        .h_full()
        .child(match layout {
            Layout::Tabs => tabs(t, &head),
            Layout::Feed => filters(t, &head),
        })
        .child(divider(t))
        .child(lines)
        .child(composer(s, t, k, &head))
}

/// The column docked beside the stage, when there is room and it isn't hidden.
pub fn column(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> Option<El> {
    s.chat_docked().then(|| {
        lists(s, t, layer);
        panel(s, t, k, true).w(width(s)).fill(t.surface)
    })
}

/// While the column is hidden: a handle on the window's right edge, the server rail's twin on
/// the left. It wears a dot for anything unread, peeks the chat over the stage while the pointer
/// is on it, and docks the column when pressed.
pub fn edge(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let now = Instant::now();
    // A list hanging off the peeking column keeps it up.
    let open = s.ui.chat_peek_open(now) || s.ui.chat_lists.iter().any(|list| list.open(now));
    let near = s.ui.chat_peek.inside();
    let height = (s.window_size.1 - size::HEADER - 40.0).max(80.0);
    let (thick, length) = if near { (5.0, 60.0) } else { (3.0, 44.0) };
    let chat = &s.server().chat;
    let dot = if chat.unread_private() > 0 {
        Some(t.private)
    } else if chat.unread_public() > 0 {
        Some(t.accent)
    } else {
        None
    };
    let label = match dot {
        Some(_) => "Show chat · new messages",
        None => "Show chat",
    };
    let mut strip = el()
        .key("chat-edge")
        .role("button")
        .label(label)
        .pin(None, Some(size::HEADER), Some(0.0), None)
        .size(EDGE, height)
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .on_hover_change(|s: &mut App, inside, tx: &mut Tx| {
            s.ui.chat_peek
                .hover(Instant::now(), |peek| peek.strip = inside);
            RailPeek::WAKE
                .into_iter()
                .for_each(|wake| tx.wake_after(wake));
        })
        .on_press_tx(send(Message::Chat(Action::Toggle)));
    if !open {
        let top = (height - length) / 2.0;
        strip = strip.child(
            el().absolute(EDGE - 2.0 - thick, top)
                .size(thick, length)
                .radius(thick / 2.0)
                .fill(if near { t.muted } else { t.faint })
                .motion(motion::QUICK)
                .a11y_hidden(),
        );
        if let Some(color) = dot {
            strip = strip.child(
                el().key("chat-edge-dot")
                    .absolute(EDGE - 11.0, top - 14.0)
                    .size(9.0, 9.0)
                    .radius(4.5)
                    .fill(color)
                    .border(1.5, t.stage)
                    .transition(Transition::opacity().combined(Transition::scale(0.6)))
                    .a11y_hidden(),
            );
        }
    }
    layer.push(strip);
    if open {
        let peek = panel(s, t, k, false);
        layer.push(
            el().key("chat-peek")
                .pin(None, Some(size::HEADER + 8.0), Some(8.0), Some(48.0))
                .w(width(s))
                .radius(radius::PANE)
                .clip_rounded(radius::PANE)
                .fill(t.raised)
                .border(1.0, t.hairline)
                .box_shadow(shadow(0.0, 8.0, 24.0, Color::BLACK.alpha(0.4)))
                .transition(Transition::opacity().combined(Transition::scale(0.98)))
                .on_hover_change(|s: &mut App, inside, tx: &mut Tx| {
                    s.ui.chat_peek
                        .hover(Instant::now(), |peek| peek.card = inside);
                    RailPeek::WAKE
                        .into_iter()
                        .for_each(|wake| tx.wake_after(wake));
                })
                .child(peek.w_full()),
        );
        lists(s, t, layer);
    }
}

/// A message that arrived while the chat was hidden, on a notice card: who, where it was sent,
/// what it says, and Reply or Open Chat. A private one wears the violet edge.
pub fn toast(s: &App, t: &Theme) -> Option<El> {
    let entry = &s.ui.chat_toast.as_ref()?.entry;
    let color = tint(t, &entry.scope);
    let (label, glyph) = match &entry.scope {
        Scope::Channel | Scope::Past(_) => (channel_label(&entry.place), None),
        Scope::Server => ("Server".to_owned(), Some(&icons::GLOBE)),
        Scope::Private(_) => ("to you".to_owned(), Some(&icons::USER)),
    };
    let top = row()
        .w_full()
        .gap(10.0)
        .items_start()
        .child(avatar(&entry.from, 28.0))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(3.0)
                .child(
                    row()
                        .w_full()
                        .gap(6.0)
                        .center_y()
                        .child(
                            text(entry.from.clone())
                                .font(ty::BODY_STRONG)
                                .color(t.text)
                                .truncate_fade()
                                .min_w(0.0),
                        )
                        .child(tag(label, glyph, color)),
                )
                .child(
                    text(entry.text.clone())
                        .font(ty::BODY)
                        .color(t.text)
                        .wrap()
                        .wrap_anywhere()
                        .max_lines(4)
                        .w_full(),
                ),
        )
        .child(
            icon_button(
                t,
                "chat-toast-close",
                &icons::CLOSE,
                "Close",
                Variant::Ghost,
                Some(Message::Chat(Action::DismissToast)),
            )
            .mt(-4.0)
            .mr(-6.0),
        );
    let actions = row()
        .w_full()
        .gap(size::GAP)
        .justify_end()
        .child(button(
            t,
            "chat-toast-open",
            "Open Chat",
            None,
            Variant::Ghost,
            Some(Message::Chat(Action::Open(entry.scope.clone()))),
        ))
        .child(button(
            t,
            "chat-toast-reply",
            "Reply",
            Some(&icons::REPLY),
            Variant::Primary,
            Some(Message::Chat(Action::Reply(entry.scope.clone()))),
        ));
    let mut card = notices::card(t, "chat-toast", format!("{}: {}", entry.from, entry.text))
        .on_hover_change(|s: &mut App, inside, tx: &mut Tx| {
            s.ui.chat_toast_hover = inside;
            // Let go of, it stays a moment longer rather than vanish under the pointer.
            if !inside && let Some(toast) = &mut s.ui.chat_toast {
                toast.until = toast.until.max(Instant::now() + ChatToast::AFTER_HOVER);
                tx.wake_after(ChatToast::AFTER_HOVER);
            }
        })
        .child(top)
        .child(actions);
    if entry.scope.is_private() {
        card = card.outline(1.0, 0.0, t.private.alpha(0.5));
    }
    Some(card)
}

/// A layout with a short made-up conversation in it, to choose from. Nothing in it responds.
fn sample(t: &Theme, k: &Kit, layout: Layout, tab_bar: TabBar) -> El {
    let entries = chat::demo();
    let (target, peers) = match layout {
        Layout::Tabs => (
            Scope::Channel,
            vec![("mara", "Mara", 2), ("ines", "Ines", 0)],
        ),
        Layout::Feed => (Scope::Channel, vec![("mara", "Mara", 0)]),
    };
    let head = Head {
        channel: "Lobby",
        target: &target,
        filter: &Filter::All,
        everything: false,
        feed: layout == Layout::Feed,
        peers,
        past: Vec::new(),
        unread_channel: 0,
        unread_server: 0,
        tab_bar,
        hide: false,
    };
    let lines = entries
        .iter()
        .filter(|entry| match layout {
            Layout::Tabs => entry.scope == target,
            // Enough to show one of each kind of tag.
            Layout::Feed => matches!(entry.id, 1 | 3 | 4),
        })
        .map(|entry| line(t, entry, layout, 0))
        .collect();
    let mut to = recipient(t, &head);
    if layout == Layout::Feed {
        to = to.child(icon(&icons::CHEVRON_UP, 12.0).color(t.faint));
    }
    let field = row()
        .w_full()
        .h(34.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .child(
            text("Write a message")
                .font(ty::BODY)
                .color(t.faint)
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(icon(&icons::SEND, 15.0).color(t.faint));
    col()
        .w_full()
        .h_full()
        .fill(t.surface)
        .child(match layout {
            Layout::Tabs => tabs(t, &head),
            Layout::Feed => filters(t, &head),
        })
        .child(divider(t))
        .child(history(t, k, lines, &format!("chat-sample-{layout:?}"), ""))
        .child(send_box(t, to, field, t.hairline))
        .inert()
        .a11y_hidden()
}

/// The two layouts side by side, each a card with a sample; the chosen one is ringed. Used by
/// setup's chat step and Settings › Chat.
pub fn layout_choice(s: &App, t: &Theme, k: &Kit) -> El {
    let current = s.chat_options.layout;
    row()
        .key("chat-layouts")
        .w_full()
        .gap(12.0)
        .flex_wrap()
        .role("radiogroup")
        .label("Chat layout")
        .children(Layout::ALL.into_iter().map(|layout| {
            let selected = layout == current;
            let mark = el()
                .size(16.0, 16.0)
                .radius(8.0)
                .center()
                .mt(1.0)
                .a11y_hidden();
            let mark = if selected {
                mark.fill(t.accent)
                    .child(icon(&icons::CHECK, 11.0).color(t.on_accent))
            } else {
                mark.border(1.5, t.faint)
            };
            let mut card = col()
                .key(format!("chat-layout-{layout:?}"))
                .role("radio")
                .label(layout.title())
                .checked(selected)
                .grow(1.0)
                .w(0.0)
                .min_w(250.0)
                .padding(8.0)
                .gap(10.0)
                .radius(radius::CARD)
                .fill(t.surface)
                .border(1.0, if selected { t.accent } else { t.hairline })
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(t.wash(0.04))
                .press(t.wash(0.08))
                .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
                .motion(motion::QUICK)
                .on_press_tx(send(Message::Chat(Action::Layout(layout))))
                .child(
                    el().w_full()
                        .h(302.0)
                        .radius(radius::CONTROL)
                        .clip_rounded(radius::CONTROL)
                        .border(1.0, t.hairline)
                        .child(sample(t, k, layout, s.chat_options.tab_bar)),
                )
                .child(
                    row()
                        .w_full()
                        .px(4.0)
                        .pb(4.0)
                        .gap(10.0)
                        .items_start()
                        .child(mark)
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .gap(2.0)
                                .child(
                                    text(layout.title())
                                        .font(ty::BODY_STRONG)
                                        .color(if selected { t.text } else { t.muted }),
                                )
                                .child(
                                    text(layout.detail())
                                        .font(ty::SMALL)
                                        .color(t.muted)
                                        .wrap()
                                        .w_full(),
                                ),
                        ),
                );
            if selected {
                card = card.outline(1.0, 0.0, t.accent);
            }
            card
        }))
}
