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
use rune_kit::{Kit, Menu, MenuItem, Placement, TextInput};

use super::home::tile_colour;
use super::theme::{Theme, motion, radius, shadow, size, ty};
use super::widgets::{Variant, button, divider, icon_button};
use super::{ChatToast, RailPeek, dispatch, icons, notices, send};
use crate::chat::{self, Action, Entry, Filter, Kind, Layout, Scope};
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
        Scope::Channel => t.accent,
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

/// The tag One Feed puts on a line. Pressing it points the message box there.
fn place_tag(t: &Theme, entry: &Entry) -> El {
    let color = tint(t, &entry.scope);
    let (label, glyph) = match &entry.scope {
        Scope::Channel => (channel_label(&entry.place), None),
        Scope::Server => ("Server".to_owned(), Some(&icons::GLOBE)),
        Scope::Private(_) if entry.own => (format!("to {}", entry.place), Some(&icons::USER)),
        Scope::Private(_) => ("to you".to_owned(), Some(&icons::USER)),
    };
    tag(label, glyph, color)
        .key(format!("chat-tag-{}", entry.id))
        .role("button")
        .help("Write here")
        .cursor(rune::Cursor::Pointer)
        .hover(color.alpha(0.10))
        .on_press_tx(send(Message::Chat(Action::Target(entry.scope.clone()))))
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
fn line(t: &Theme, entry: &Entry, layout: Layout) -> El {
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
        meta = meta.child(place_tag(t, entry));
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

/// What the column's top bar shows, for the real chat and for the samples alike.
struct Head<'a> {
    channel: &'a str,
    target: &'a Scope,
    filter: Filter,
    /// Each private conversation: its key, the person's name, and what's unread.
    peers: Vec<(&'a str, &'a str, u32)>,
    unread_channel: u32,
    unread_server: u32,
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
            filter: chat.filter,
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
            unread_channel: chat.unread(&Scope::Channel),
            unread_server: chat.unread(&Scope::Server),
            hide,
        }
    }

    fn unread_private(&self) -> u32 {
        self.peers.iter().map(|(_, _, unread)| unread).sum()
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

/// One tab. The open one is underlined in its place's colour; a private one can be closed.
fn tab(t: &Theme, lead: El, label: &str, scope: Scope, selected: bool, unread: u32) -> El {
    let color = tint(t, &scope);
    let key = match &scope {
        Scope::Channel => "chat-tab-channel".to_owned(),
        Scope::Server => "chat-tab-server".to_owned(),
        Scope::Private(key) => format!("chat-tab-{key}"),
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
                    t.muted
                })
                .max_w(96.0)
                .truncate_fade(),
        );
    if unread > 0 {
        tab = tab.child(badge(t, unread, color));
    }
    if selected {
        if let Scope::Private(peer) = &scope {
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
                    .on_press_tx(send(Message::Chat(Action::ClosePeer(peer.clone())))),
            );
        }
        tab.child(
            el().pin(Some(8.0), None, Some(8.0), Some(-5.0))
                .h(2.0)
                .radius(1.0)
                .fill(color)
                .a11y_hidden(),
        )
    } else {
        tab.focusable(true)
            .cursor(rune::Cursor::Pointer)
            .hover(t.wash(0.06))
            .press(t.wash(0.10))
            .focus_ring(2.0, -2.0, t.accent.alpha(0.7))
            .on_press_tx(send(Message::Chat(Action::Target(scope))))
    }
}

/// Tabs: the channel, the server, then one per person.
fn tabs(t: &Theme, head: &Head) -> El {
    let glyph = |glyph: &'static Icon, scope: &Scope| {
        icon(glyph, 14.0)
            .color(if head.target == scope {
                tint(t, scope)
            } else {
                t.muted
            })
            .a11y_hidden()
    };
    let mut strip = row()
        .role("tablist")
        .label("Conversations")
        .h_full()
        .gap(0.0)
        .center_y()
        .child(tab(
            t,
            glyph(&icons::HASH, &Scope::Channel),
            if head.channel.is_empty() {
                "Channel"
            } else {
                head.channel
            },
            Scope::Channel,
            *head.target == Scope::Channel,
            head.unread_channel,
        ))
        .child(tab(
            t,
            glyph(&icons::GLOBE, &Scope::Server),
            "Server",
            Scope::Server,
            *head.target == Scope::Server,
            head.unread_server,
        ));
    for (key, name, unread) in &head.peers {
        let scope = Scope::Private((*key).to_owned());
        let selected = *head.target == scope;
        strip = strip.child(tab(t, avatar(name, 16.0), name, scope, selected, *unread));
    }
    bar(t, strip, head.hide)
}

/// One Feed: chips that narrow the stream.
fn filters(t: &Theme, head: &Head) -> El {
    let channel = channel_label(head.channel);
    let mut strip = row()
        .role("radiogroup")
        .label("Show")
        .h_full()
        .gap(6.0)
        .center_y();
    for filter in Filter::ALL {
        let (label, glyph, unread, color): (&str, Option<&'static Icon>, u32, Color) = match filter
        {
            Filter::All => ("All", None, 0, t.accent),
            Filter::Channel => (&channel, None, head.unread_channel, t.accent),
            Filter::Server => ("Server", Some(&icons::GLOBE), head.unread_server, t.fair),
            Filter::Private => (
                "Private",
                Some(&icons::USER),
                head.unread_private(),
                t.private,
            ),
        };
        let selected = head.filter == filter;
        let ink = if selected { t.canvas } else { t.muted };
        let mut chip = row()
            .key(format!("chat-filter-{filter:?}"))
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
        if unread > 0 && !selected {
            chip = chip.child(badge(t, unread, color));
        }
        if !selected {
            chip = chip
                .focusable(true)
                .cursor(rune::Cursor::Pointer)
                .hover(t.wash(0.06))
                .press(t.wash(0.10))
                .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
                .on_press_tx(send(Message::Chat(Action::Filter(filter))));
        }
        strip = strip.child(chip);
    }
    bar(t, strip, head.hide)
}

/// Under the tabs: who reads what is written here.
fn audience(t: &Theme, head: &Head) -> El {
    let (glyph, words, name): (&'static Icon, String, &str) = match head.target {
        Scope::Channel if head.channel.is_empty() => (
            &icons::HASH,
            "Everyone in your channel sees this".to_owned(),
            "",
        ),
        Scope::Channel => (
            &icons::HASH,
            format!("Everyone in {} sees this", head.channel),
            head.channel,
        ),
        Scope::Server => (
            &icons::GLOBE,
            "Everyone on the server sees this".to_owned(),
            "",
        ),
        Scope::Private(key) => {
            let name = head
                .peers
                .iter()
                .find(|(other, _, _)| other == key)
                .map_or("", |(_, name, _)| name);
            (&icons::USER, format!("Only you and {name} see this"), name)
        }
    };
    let spans = words
        .find(name)
        .filter(|_| !name.is_empty())
        .map(|at| Span::new(at, at + name.len()).weight(600.0).color(t.muted))
        .into_iter()
        .collect();
    row()
        .key("chat-audience")
        .w_full()
        .px(14.0)
        .pt(10.0)
        .gap(6.0)
        .center_y()
        .child(icon(glyph, 12.0).color(t.faint).a11y_hidden())
        .child(
            rich(words, spans)
                .font(ty::CAPTION)
                .color(t.faint)
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0),
        )
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

/// One Feed's choice of where the message box sends, as a chip that opens a list.
fn target_menu(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, head: &Head) -> El {
    let color = tint(t, head.target);
    let name_of = |key: &str| {
        head.peers
            .iter()
            .find(|(other, _, _)| *other == key)
            .map_or("", |(_, name, _)| *name)
    };
    let (label, glyph) = match head.target {
        Scope::Channel => (channel_label(head.channel), None),
        Scope::Server => ("Server".to_owned(), Some(&icons::GLOBE)),
        Scope::Private(key) => (format!("To {}", name_of(key)), Some(&icons::USER)),
    };
    let trigger = row()
        .key("chat-target")
        .role("button")
        .label(format!("Sending to {label}"))
        .help("Choose where this goes")
        .h(24.0)
        .px(8.0)
        .gap(4.0)
        .center()
        .radius(radius::CONTROL)
        .fill(color.alpha(0.14))
        .focusable(true)
        .cursor(rune::Cursor::Pointer)
        .hover(color.alpha(0.08))
        .focus_ring(2.0, 2.0, t.accent.alpha(0.7))
        .on_press(|s: &mut App| s.ui.chat_menu = !s.ui.chat_menu)
        .children(glyph.map(|glyph| icon(glyph, 12.0).color(color).a11y_hidden()))
        .child(
            text(label)
                .font(ty::SMALL_STRONG)
                .color(color)
                .max_w(96.0)
                .truncate_fade(),
        )
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(color).a11y_hidden());
    let choose = |label: String, detail: &str, scope: Scope| {
        let chosen = *head.target == scope;
        MenuItem::new(label)
            .description(detail.to_owned())
            .checked(chosen)
            .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                dispatch(s, tx, Message::Chat(Action::Target(scope.clone())));
            })
    };
    let mut menu = Menu::new("chat-target-menu")
        .trigger(trigger)
        .open(s.ui.chat_menu)
        .placement(Placement::TopStart)
        .min_width(220.0)
        .on_close(|s: &mut App| s.ui.chat_menu = false)
        .group_label("Send to")
        .item(choose(
            channel_label(head.channel),
            "Everyone in your channel",
            Scope::Channel,
        ))
        .item(choose(
            "Server".to_owned(),
            "Everyone on the server",
            Scope::Server,
        ));
    if !head.peers.is_empty() {
        menu = menu.separator();
    }
    for (key, name, _) in &head.peers {
        menu = menu.item(choose(
            (*name).to_owned(),
            "Only them",
            Scope::Private((*key).to_owned()),
        ));
    }
    menu.build(k, layer)
}

/// What the message box says while empty, and whether it can send.
fn prompt(s: &App, head: &Head, layout: Layout) -> (String, bool) {
    match head.target {
        // The chip beside the box already names the place.
        Scope::Channel | Scope::Server if layout == Layout::Feed => {
            ("Write a message".to_owned(), true)
        }
        Scope::Channel => (format!("Message {}", channel_label(head.channel)), true),
        Scope::Server => ("Message the whole server".to_owned(), true),
        Scope::Private(key) => {
            let name = head
                .peers
                .iter()
                .find(|(other, _, _)| other == key)
                .map_or("them", |(_, name, _)| name);
            if s.chat_member(key).is_some() {
                (format!("Message {name}"), true)
            } else {
                (format!("{name} isn’t on the server"), false)
            }
        }
    }
}

/// The message box. It always says where it sends; One Feed adds the chip that changes it.
fn composer(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, head: &Head, layout: Layout) -> El {
    let (placeholder, enabled) = prompt(s, head, layout);
    let ready = enabled && !s.server().chat.draft.trim().is_empty();
    let input = TextInput::new(&s.ui.chat_input)
        .placeholder(&placeholder)
        .label(&placeholder)
        .width(None)
        .height(32.0)
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
            if key.key == rune::Key::Escape && layout == Layout::Feed && elsewhere {
                dispatch(s, tx, Message::Chat(Action::Target(Scope::Channel)));
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
    row()
        .key("chat-composer")
        .w_full()
        .px(12.0)
        .pt(4.0)
        .pb(12.0)
        .gap(6.0)
        .center_y()
        .children((layout == Layout::Feed).then(|| target_menu(s, t, k, layer, head)))
        .child(el().grow(1.0).min_w(0.0).child(input))
}

/// The chat: bar, history and message box, on `fill`.
fn panel(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>, docked: bool) -> El {
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
        (chat.version, &chat.target, chat.filter, layout, t),
        || {
            history(
                t,
                k,
                shown[skip..]
                    .iter()
                    .map(|entry| line(t, entry, layout))
                    .collect(),
                "chat-scroll",
                match layout {
                    Layout::Tabs => "No messages here yet",
                    Layout::Feed => "No messages yet",
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
        .children((layout == Layout::Tabs).then(|| audience(t, &head)))
        .child(lines)
        .child(composer(s, t, k, layer, &head, layout))
}

/// The column docked beside the stage, when there is room and it isn't hidden.
pub fn column(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) -> Option<El> {
    s.chat_docked()
        .then(|| panel(s, t, k, layer, true).w(width(s)).fill(t.surface))
}

/// While the column is hidden: a handle on the window's right edge, the server rail's twin on
/// the left. It wears a dot for anything unread, peeks the chat over the stage while the pointer
/// is on it, and docks the column when pressed.
pub fn edge(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let open = s.ui.chat_peek_open(Instant::now()) || s.ui.chat_menu;
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
        let peek = panel(s, t, k, layer, false);
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
    }
}

/// A message that arrived while the chat was hidden, on a notice card: who, where it was sent,
/// what it says, and Reply or Open Chat. A private one wears the violet edge.
pub fn toast(s: &App, t: &Theme) -> Option<El> {
    let entry = &s.ui.chat_toast.as_ref()?.entry;
    let color = tint(t, &entry.scope);
    let (label, glyph) = match &entry.scope {
        Scope::Channel => (channel_label(&entry.place), None),
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
fn sample(t: &Theme, k: &Kit, layout: Layout) -> El {
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
        filter: Filter::All,
        peers,
        unread_channel: 0,
        unread_server: 0,
        hide: false,
    };
    let lines = entries
        .iter()
        .filter(|entry| match layout {
            Layout::Tabs => entry.scope == target,
            // Enough to show one of each kind of tag.
            Layout::Feed => matches!(entry.id, 1 | 3 | 4),
        })
        .map(|entry| line(t, entry, layout))
        .collect();
    let mut box_ = row()
        .w_full()
        .h(32.0)
        .px(10.0)
        .gap(6.0)
        .center_y()
        .radius(radius::CONTROL)
        .fill(t.recess)
        .border(1.0, t.hairline);
    if layout == Layout::Feed {
        box_ = box_.pl(4.0).child(
            row()
                .h(24.0)
                .px(8.0)
                .gap(4.0)
                .center()
                .radius(radius::CONTROL)
                .fill(t.accent.alpha(0.14))
                .child(text("#Lobby").font(ty::SMALL_STRONG).color(t.accent))
                .child(icon(&icons::CHEVRON_DOWN, 12.0).color(t.accent)),
        );
    }
    box_ = box_
        .child(
            text(match layout {
                Layout::Tabs => "Message #Lobby",
                Layout::Feed => "Write a message",
            })
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
        .child(el().w_full().px(12.0).pb(12.0).pt(4.0).child(box_))
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
                        .h(280.0)
                        .radius(radius::CONTROL)
                        .clip_rounded(radius::CONTROL)
                        .border(1.0, t.hairline)
                        .child(sample(t, k, layout)),
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
