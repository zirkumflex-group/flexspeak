//! What you can do with the people in the channel tree: pick them (⌘/Ctrl-click adds to the
//! selection), open their context menu to poke, move, kick, ban or regroup them, and drag them
//! onto a channel. The menu greys out poking and moving, and leaves out kicking and banning,
//! when the server's permission hints rule them out; the server has the last word.

use rune::{Drag, El, Phase, PointerPress, Tx, el, icon, row, text};
use rune_kit::{Kit, MenuEntry, MenuItem, context_menu};

use super::theme::{Theme, radius, ty};
use super::{GroupsMenu, MemberDrag, dispatch, icons};
use crate::{App, GroupMark, Message, client};

/// How far the pointer travels before a press on a member becomes a drag.
const DRAG_START: f32 = 4.0;

/// The member row with selection, its context menu and dragging.
pub fn interactive(s: &App, t: &Theme, member: &client::Member, row: El) -> El {
    let id = member.id;
    let selected = s.server().selected_members.contains(&id);
    let dragged =
        s.ui.drag
            .as_ref()
            .is_some_and(|drag| drag.ids.contains(&id));
    let live = s.server().connected && !s.loopback && !s.server().stopping;
    let mut row = row
        .role("button")
        .label(member.name.clone())
        .checked(selected)
        .help(help(s, member));
    // Selection is outlined, so it never reads as the current channel's tint.
    row = if selected {
        row.fill(t.wash(0.07)).border(1.0, t.accent.alpha(0.55))
    } else {
        row.hover(t.wash(0.05))
    };
    if dragged {
        row = row.opacity(0.45);
    }
    if !live {
        return row;
    }
    row.focusable(true)
        .cursor(rune::Cursor::Pointer)
        .on_pointer_move_local(|s: &mut App, p, tx: &mut Tx| {
            s.ui.row_origin = (p.window_x - p.x, p.window_y - p.y);
            tx.live_only();
        })
        .on_secondary_press(move |s: &mut App, point: PointerPress, _tx: &mut Tx| {
            if !s.server().selected_members.contains(&id) {
                s.server_mut().selected_members.clear();
            }
            s.ui.member_menu = Some((id, point));
            s.ui.member_stats = false;
            s.ui.channel_menu = None;
            s.ui.move_menu = false;
            s.ui.groups_menu = None;
            s.server_mut().group_edit = None;
        })
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| {
            let at = super::drag_point(s, d);
            match d.phase {
                Phase::Down => s.ui.drag = None,
                Phase::Move => {
                    let travelled = d.dx.hypot(d.dy) >= DRAG_START;
                    if s.ui.drag.is_none() && travelled {
                        let ids = dragged_ids(s, id);
                        if s.can_move(&ids) {
                            s.ui.drag = Some(MemberDrag {
                                ids,
                                at,
                                target: None,
                            });
                        }
                    }
                    if let Some(drag) = &mut s.ui.drag {
                        drag.at = at;
                        tx.element_at(at.0, at.1, |s: &mut App, picked, _tx: &mut Tx| {
                            // A spacer is a label, not somewhere to go.
                            let target =
                                picked.and_then(|p| channel_in_path(p.path())).filter(|id| {
                                    s.server().state.channels.iter().any(|channel| {
                                        channel.id == *id && channel.spacer().is_none()
                                    })
                                });
                            if let Some(drag) = &mut s.ui.drag {
                                drag.target = target;
                            }
                        });
                    }
                }
                Phase::Up => {
                    let drag = s.ui.drag.take();
                    if d.cancelled() {
                        return;
                    }
                    match drag {
                        Some(MemberDrag {
                            ids,
                            target: Some(channel),
                            ..
                        }) => dispatch(s, tx, Message::MoveMembers(ids, channel)),
                        Some(_) => {}
                        None => dispatch(s, tx, Message::SelectMember(id, tx.mods().cmd)),
                    }
                }
            }
        })
        // A double-click opens the private chat with them; the first click has selected them.
        .on_double_press_tx(move |s: &mut App, tx: &mut Tx| {
            dispatch(s, tx, Message::Chat(crate::chat::Action::Message(id)));
        })
}

/// A member row's tooltip: the full name, which a long one loses on the row, their volume when
/// you changed it, then what the row does.
fn help(s: &App, member: &client::Member) -> String {
    let mut help = member.name.clone();
    if member.is_self {
        help.push_str(" (you)\nRight-click for options · drag onto a channel to join it");
        return help;
    }
    let audio = s.user_audio(&member.uid);
    if audio.muted {
        help.push_str("\nMuted by you");
    } else if !member.uid.is_empty() && audio.volume_percent != 100 {
        help.push_str(&format!("\nVolume {}%", audio.volume_percent));
    }
    help.push_str(if cfg!(target_os = "macos") {
        "\nClick to select · ⌘-click to add · double-click to message · right-click for options"
    } else {
        "\nClick to select · Ctrl-click to add · double-click to message · right-click for options"
    });
    help
}

/// The selection when the pressed member is part of it, else just that member.
fn dragged_ids(s: &App, id: u16) -> Vec<u16> {
    if s.server().selected_members.contains(&id) && s.server().selected_members.len() > 1 {
        s.server().selected_members.iter().copied().collect()
    } else {
        vec![id]
    }
}

/// The channel whose node holds an element, from its key path (`channel-42`).
pub fn channel_in_path(path: &str) -> Option<u64> {
    path.rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter_map(|part| part.strip_prefix("channel-"))
        .find_map(|id| id.parse().ok())
}

fn close(s: &mut App) {
    s.ui.member_menu = None;
    s.ui.member_stats = false;
    s.ui.move_menu = false;
    s.ui.groups_menu = None;
    s.server_mut().group_edit = None;
}

/// A group submenu's rows; ticking one applies it straight away and leaves the menu open.
fn group_entries(rows: Vec<MenuEntry>) -> Vec<MenuEntry> {
    if rows.is_empty() {
        return vec![MenuEntry::Label("Waiting for the server…".into())];
    }
    rows
}

/// Tickable rows: any number of server groups for everyone picked.
fn server_group_rows(s: &App, ids: &[u16]) -> Vec<MenuEntry> {
    let draft = s.server().group_edit.as_ref();
    s.server()
        .state
        .server_groups
        .iter()
        .map(|group| {
            let id = group.id;
            let wanted = draft.and_then(|draft| draft.wanted.get(&id).copied());
            let mark = GroupMark::of(wanted, s.server_group_mark(ids, id));
            let mut item = MenuItem::new(group.name.clone())
                .checked(mark == GroupMark::On)
                .disabled(group.fixed)
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    dispatch(s, tx, Message::ToggleServerGroup(id));
                });
            if mark == GroupMark::Mixed {
                // Plain text, as a menu's trailing detail reads, not a badge.
                item = item.shortcut("Some");
            }
            MenuEntry::item(item)
        })
        .collect()
}

/// Rows with one choice: someone has one channel group per channel.
fn channel_group_rows(s: &App, ids: &[u16]) -> Vec<MenuEntry> {
    let picked = s
        .server()
        .group_edit
        .as_ref()
        .and_then(|draft| draft.channel_group)
        .or_else(|| s.shared_channel_group(ids));
    s.server()
        .state
        .channel_groups
        .iter()
        .map(|group| {
            let id = group.id;
            MenuEntry::item(
                MenuItem::new(group.name.clone())
                    .checked(picked == Some(id))
                    .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                        dispatch(s, tx, Message::PickChannelGroup(id));
                    }),
            )
        })
        .collect()
}

/// The member context menu: poke one person, or move, regroup, kick or ban everyone picked.
pub fn menu(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let Some((id, point)) = s.ui.member_menu else {
        return;
    };
    let Some(member) = s.find_member(id) else {
        return;
    };
    let ids = dragged_ids(s, id);
    let title = if ids.len() > 1 {
        format!("{} people", ids.len())
    } else if member.is_self {
        format!("{} (you)", member.name)
    } else {
        member.name.clone()
    };
    // Icons stay neutral, so red is left to kicking and banning; anything unavailable fades.
    let tinted = |glyph: &'static rune::Icon, color: rune::Color, enabled: bool| {
        icon(glyph, 14.0).color(if enabled { color } else { t.faint })
    };
    let neutral = |glyph: &'static rune::Icon, enabled: bool| tinted(glyph, t.muted, enabled);
    if s.ui.member_stats && ids.len() == 1 {
        stats(s, t, k, layer, member, title, point);
        return;
    }
    let mut menu = context_menu("member-menu", el(), Some(point))
        .min_width(200.0)
        .on_close(close)
        .group_label(title);
    if ids.len() == 1 && !member.is_self {
        menu = menu.item(
            MenuItem::new("Message")
                .lead(neutral(&icons::MESSAGE, true))
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    close(s);
                    dispatch(s, tx, Message::Chat(crate::chat::Action::Message(id)));
                }),
        );
        let denied = member.can_poke == Some(false);
        let mut poke = MenuItem::new("Poke…")
            .lead(neutral(&icons::HAND, !denied))
            .disabled(denied)
            .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                close(s);
                dispatch(s, tx, Message::OpenPoke(id));
            });
        if denied {
            poke = poke.description("Not allowed by the server");
        }
        menu = menu.item(poke);
    }
    if ids.len() == 1 && !member.is_self && !member.uid.is_empty() {
        let audio = s.user_audio(&member.uid);
        menu = menu
            .item(
                MenuItem::new("Volume…")
                    .lead(neutral(&icons::VOLUME_2, true))
                    .description(format!("{}%", audio.volume_percent))
                    .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                        close(s);
                        dispatch(s, tx, Message::OpenUserVolume(id));
                    }),
            )
            .item(
                MenuItem::new(if audio.muted { "Unmute" } else { "Mute" })
                    .lead(if audio.muted {
                        neutral(&icons::VOLUME_2, true)
                    } else {
                        neutral(&icons::VOLUME_OFF, true)
                    })
                    .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                        close(s);
                        dispatch(s, tx, Message::ToggleUserMute(id));
                    }),
            )
            .item(
                MenuItem::new("Auto-join streams")
                    .lead(neutral(&icons::MONITOR_PLAY, true))
                    .checked(s.auto_watch.contains(&member.uid))
                    .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                        close(s);
                        dispatch(s, tx, Message::ToggleAutoWatch(id));
                    }),
            );
    }
    if ids.len() == 1 {
        menu = menu.item(
            MenuItem::new("Stats")
                .lead(neutral(&icons::CHART_LINE, true))
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    dispatch(s, tx, Message::MemberStats(id));
                }),
        );
    }
    let channels: Vec<MenuEntry> = s
        .server()
        .state
        .channels
        .iter()
        .filter(|channel| {
            channel.spacer().is_none()
                && !ids
                    .iter()
                    .all(|id| channel.members.iter().any(|member| member.id == *id))
        })
        .map(|channel| {
            let ids = ids.clone();
            let target = channel.id;
            MenuEntry::item(
                MenuItem::new(format!(
                    "{}{}",
                    "    ".repeat(channel.depth.min(6)),
                    channel.name
                ))
                .lead(if channel.has_password {
                    neutral(&icons::LOCK, true)
                } else {
                    neutral(&icons::VOLUME, true)
                })
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    close(s);
                    dispatch(s, tx, Message::MoveMembers(ids.clone(), target));
                }),
            )
        })
        .collect();
    let movable = s.can_move(&ids) && !channels.is_empty();
    let mut move_to = MenuItem::new(if member.is_self && ids.len() == 1 {
        "Join channel"
    } else {
        "Move to channel"
    })
    .lead(neutral(&icons::MOVE, movable))
    .disabled(!movable)
    .on_select(|s: &mut App| {
        s.ui.move_menu = !s.ui.move_menu;
        s.ui.groups_menu = None;
    });
    if movable {
        move_to = move_to.submenu(channels, s.ui.move_menu);
    } else if !s.can_move(&ids) {
        move_to = move_to.description("Not allowed by the server");
    }
    menu = menu.item(move_to);
    menu = menu.separator();
    for (label, which, rows) in [
        (
            "Server groups",
            GroupsMenu::Server,
            server_group_rows(s, &ids),
        ),
        (
            "Channel group",
            GroupsMenu::Channel,
            channel_group_rows(s, &ids),
        ),
    ] {
        let group_ids = ids.clone();
        menu = menu.item(
            MenuItem::new(label)
                .lead(neutral(&icons::SHIELD, true))
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    s.ui.move_menu = false;
                    s.ui.groups_menu = (s.ui.groups_menu != Some(which)).then_some(which);
                    dispatch(s, tx, Message::OpenGroups(group_ids.clone()));
                })
                .submenu(group_entries(rows), s.ui.groups_menu == Some(which)),
        );
    }
    // Permission hints are per target, so everyone picked has to allow it.
    let allowed = |hint: fn(&client::Member) -> Option<bool>| {
        ids.iter().all(|id| {
            s.find_member(*id)
                .is_some_and(|member| !member.is_self && hint(member) != Some(false))
        })
    };
    let actions: [(&str, &'static rune::Icon, client::Moderation, bool); 3] = [
        (
            "Kick from channel…",
            &icons::USER_X,
            client::Moderation::KickChannel,
            allowed(|member| member.can_kick_channel),
        ),
        (
            "Kick from server…",
            &icons::DISCONNECT,
            client::Moderation::KickServer,
            allowed(|member| member.can_kick_server),
        ),
        (
            "Ban…",
            &icons::BAN,
            client::Moderation::Ban(Some(super::modals::DEFAULT_BAN_SECONDS)),
            allowed(|member| member.can_ban),
        ),
    ];
    for (label, glyph, action, _) in actions.into_iter().filter(|action| action.3) {
        let ids = ids.clone();
        menu = menu.item(
            MenuItem::new(label)
                .lead(tinted(glyph, t.poor, true))
                .danger(true)
                .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                    close(s);
                    dispatch(s, tx, Message::OpenModeration(ids.clone(), action));
                }),
        );
    }
    if ids.len() > 1 {
        menu = menu
            .separator()
            .item(
                MenuItem::new("Clear selection").on_select_tx(|s: &mut App, tx: &mut Tx| {
                    close(s);
                    dispatch(s, tx, Message::ClearSelection);
                }),
            );
    }
    // A context menu anchors at its point; the trigger only receives focus back.
    let _ = menu.build(k, layer);
}

/// The member menu's Stats page: how long they have been online and inactive, and what the
/// server keeps about them, in place of the actions until Back.
fn stats(
    s: &App,
    t: &Theme,
    k: &Kit,
    layer: &mut Vec<El>,
    member: &client::Member,
    title: String,
    point: PointerPress,
) {
    let mut menu = context_menu("member-menu", el(), Some(point))
        .min_width(260.0)
        .on_close(close)
        .group_label(title)
        .item(
            MenuItem::new("Back")
                .lead(icon(&icons::ARROW_LEFT, 14.0).color(t.muted))
                .on_select(|s: &mut App| s.ui.member_stats = false),
        )
        .separator();
    let Some((at, stats)) = s.server().member_stats.get(&member.id) else {
        menu = menu.group_label("Asking the server…");
        let _ = menu.build(k, layer);
        return;
    };
    // Both were true when the server answered and have kept counting since.
    let since = at.elapsed();
    let now = chrono::Local::now();
    let mut rows: Vec<(&'static str, Vec<StatRow>)> = vec![
        ("This visit", Vec::new()),
        ("History", Vec::new()),
        ("Client", Vec::new()),
    ];
    if let Some(online) = stats.online {
        rows[0]
            .1
            .push(("Online for", &icons::CLOCK, span(online + since)));
    }
    if let Some(idle) = stats.idle {
        let idle = idle + since;
        let value = if idle < ACTIVE {
            "Active now".to_owned()
        } else {
            let from = now - chrono::Duration::from_std(idle).unwrap_or_default();
            format!("{} · since {}", span(idle), from.format("%H:%M"))
        };
        rows[0].1.push(("Inactive", &icons::MOON, value));
    }
    if let Some(at) = stats.first_seen {
        rows[1]
            .1
            .push(("First seen", &icons::CALENDAR, day(at, now)));
    }
    if let Some(at) = stats.last_connected {
        rows[1]
            .1
            .push(("Last visit", &icons::HISTORY, moment(at, now)));
    }
    if let Some(count) = stats.connections {
        rows[1].1.push(("Visits", &icons::HASH, thousands(count)));
    }
    let client = [stats.version.as_deref(), stats.platform.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if !client.is_empty() {
        rows[2].1.push(("Version", &icons::MONITOR, client));
    }
    if let Some(country) = stats.country.as_deref().filter(|code| !code.is_empty()) {
        rows[2]
            .1
            .push(("Country", &icons::GLOBE, country.to_uppercase()));
    }
    if let (Some(down), Some(up)) = (stats.month_down, stats.month_up) {
        rows[2].1.push((
            "This month",
            &icons::DOWNLOAD,
            format!(
                "↓ {}  ↑ {}",
                crate::usage::format_bytes(down as f64),
                crate::usage::format_bytes(up as f64)
            ),
        ));
    }
    let mut first = true;
    for (section, entries) in rows.into_iter().filter(|(_, entries)| !entries.is_empty()) {
        if !first {
            menu = menu.separator();
        }
        first = false;
        menu = menu.group_label(section);
        for (label, glyph, value) in entries {
            menu = menu.item(
                MenuItem::new(label)
                    .lead(icon(glyph, 14.0).color(t.muted))
                    .shortcut(value),
            );
        }
    }
    if first {
        menu = menu.group_label("The server didn't say");
    }
    let _ = menu.build(k, layer);
}

/// One line of the Stats page: its label, icon and value.
type StatRow = (&'static str, &'static rune::Icon, String);

/// Inactive for less than this reads as active: the server counts from their last sound or
/// click.
const ACTIVE: std::time::Duration = std::time::Duration::from_secs(60);

/// A length of time in its two largest units: "3 d 4 h", "2 h 14 min", "45 min".
fn span(time: std::time::Duration) -> String {
    let minutes = time.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    match (days, hours) {
        (0, 0) if minutes == 0 => "under a minute".into(),
        (0, 0) => format!("{minutes} min"),
        (0, _) if minutes == 0 => format!("{hours} h"),
        (0, _) => format!("{hours} h {minutes} min"),
        (_, 0) => format!("{days} d"),
        _ => format!("{days} d {hours} h"),
    }
}

fn local(unix: i64) -> Option<chrono::DateTime<chrono::Local>> {
    chrono::DateTime::from_timestamp(unix, 0).map(|at| at.with_timezone(&chrono::Local))
}

/// A date, with its year only when it isn't this one: "14 Mar 2024", "2 Nov".
fn day(unix: i64, now: chrono::DateTime<chrono::Local>) -> String {
    use chrono::Datelike;
    local(unix).map_or_else(String::new, |at| {
        if at.year() == now.year() {
            at.format("%-d %b").to_string()
        } else {
            at.format("%-d %b %Y").to_string()
        }
    })
}

/// A moment, by its day when that is recent: "Today 09:12", "Yesterday 21:40", "2 Nov, 18:05".
fn moment(unix: i64, now: chrono::DateTime<chrono::Local>) -> String {
    let Some(at) = local(unix) else {
        return String::new();
    };
    let time = at.format("%H:%M");
    match (now.date_naive() - at.date_naive()).num_days() {
        0 => format!("Today {time}"),
        1 => format!("Yesterday {time}"),
        _ => format!("{}, {time}", day(unix, now)),
    }
}

/// A count with thin separators every three digits: "1 284".
fn thousands(count: u32) -> String {
    let digits = count.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push('\u{202f}');
        }
        out.push(digit);
    }
    out
}

/// The label that follows the pointer while members are dragged.
pub fn ghost(s: &App, t: &Theme, layer: &mut Vec<El>) {
    let Some(drag) = &s.ui.drag else {
        return;
    };
    let who = match drag.ids.as_slice() {
        [one] => s
            .find_member(*one)
            .map_or_else(|| "1 person".to_owned(), |member| member.name.clone()),
        many => format!("{} people", many.len()),
    };
    let target = drag.target.and_then(|id| {
        s.server()
            .state
            .channels
            .iter()
            .find(|channel| channel.id == id)
            .map(|channel| channel.name.clone())
    });
    let label = match target {
        Some(channel) => format!("{who} → {channel}"),
        None => who,
    };
    // Offset from the pointer, so picking the channel underneath never finds the label.
    layer.push(
        row()
            .key("member-drag")
            .absolute(drag.at.0 + 14.0, drag.at.1 + 12.0)
            .gap(6.0)
            .px(10.0)
            .py(5.0)
            .center_y()
            .radius(radius::PILL)
            .fill(t.raised)
            .border(1.0, t.accent.alpha(0.5))
            .box_shadow(super::theme::shadow(
                0.0,
                4.0,
                12.0,
                rune::Color::BLACK.alpha(0.3),
            ))
            .a11y_hidden()
            .child(icon(&icons::MOVE, 13.0).color(t.accent))
            .child(text(label).font(ty::SMALL).color(t.text)),
    );
}

#[cfg(test)]
mod tests {
    use super::{channel_in_path, moment, span, thousands};
    use std::time::Duration;

    #[test]
    fn stats_read_in_words_people_use() {
        assert_eq!(span(Duration::from_secs(30)), "under a minute");
        assert_eq!(span(Duration::from_secs(45 * 60)), "45 min");
        assert_eq!(span(Duration::from_secs(2 * 3600)), "2 h");
        assert_eq!(span(Duration::from_secs(2 * 3600 + 14 * 60)), "2 h 14 min");
        assert_eq!(
            span(Duration::from_secs(3 * 86400 + 4 * 3600 + 59)),
            "3 d 4 h"
        );
        assert_eq!(span(Duration::from_secs(86400 + 30 * 60)), "1 d");
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1284), "1\u{202f}284");
        assert_eq!(thousands(1_000_000), "1\u{202f}000\u{202f}000");
        let now = chrono::Local::now();
        let at = now.timestamp();
        assert!(moment(at, now).starts_with("Today "));
        assert!(moment(at - 86400, now).starts_with("Yesterday "));
        assert!(moment(at - 10 * 86400, now).contains(", "));
    }

    #[test]
    fn a_drop_finds_the_innermost_channel_in_a_key_path() {
        assert_eq!(
            channel_in_path("window/content/sidebar/channel-scroll/channels/channel-7/name"),
            Some(7)
        );
        assert_eq!(
            channel_in_path("channels/channel-7/members/channel-9/member-3"),
            Some(9)
        );
        assert_eq!(channel_in_path("channels/channel-scroll/footer"), None);
    }
}
