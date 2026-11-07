//! What you can do with the people in the channel tree: pick them (⌘/Ctrl-click adds to the
//! selection), open their context menu to poke or move them, and drag them onto a channel.
//! Moving others needs the server's move permission; the menu greys out what the server's
//! permission hints rule out, and the server has the last word.

use rune::{Drag, El, Phase, PointerPress, Tx, el, icon, row, text};
use rune_kit::{Kit, MenuEntry, MenuItem, context_menu};

use super::theme::{Theme, radius, ty};
use super::{MemberDrag, dispatch, icons};
use crate::{App, Message, client};

/// How far the pointer travels before a press on a member becomes a drag.
const DRAG_START: f32 = 4.0;

/// The member row with selection, its context menu and dragging.
pub fn interactive(s: &App, t: &Theme, member: &client::Member, row: El) -> El {
    let id = member.id;
    let selected = s.selected_members.contains(&id);
    let dragged =
        s.ui.drag
            .as_ref()
            .is_some_and(|drag| drag.ids.contains(&id));
    let live = s.connected && !s.loopback && !s.stopping;
    let mut row = row
        .role("button")
        .label(member.name.clone())
        .checked(selected)
        .help(if member.is_self {
            "Right-click for options · drag onto a channel to join it"
        } else {
            "Click to select · ⌘/Ctrl-click to add · right-click to poke or move"
        });
    row = if selected {
        row.fill(t.accent.alpha(0.16))
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
            if !s.selected_members.contains(&id) {
                s.selected_members.clear();
            }
            s.ui.member_menu = Some((id, point));
            s.ui.move_menu = false;
        })
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| {
            let at = (s.ui.row_origin.0 + d.x, s.ui.row_origin.1 + d.y);
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
                            if let Some(drag) = &mut s.ui.drag {
                                drag.target = picked.and_then(|p| channel_in_path(p.path()));
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
}

/// The selection when the pressed member is part of it, else just that member.
fn dragged_ids(s: &App, id: u16) -> Vec<u16> {
    if s.selected_members.contains(&id) && s.selected_members.len() > 1 {
        s.selected_members.iter().copied().collect()
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
    s.ui.move_menu = false;
}

/// The member context menu: poke one person, or move everyone picked to a channel.
pub fn menu(s: &App, _t: &Theme, k: &Kit, layer: &mut Vec<El>) {
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
    let mut menu = context_menu("member-menu", el(), Some(point))
        .min_width(200.0)
        .on_close(close)
        .group_label(title);
    if ids.len() == 1 && !member.is_self {
        let denied = member.can_poke == Some(false);
        let mut poke = MenuItem::new("Poke…")
            .icon(&icons::HAND)
            .disabled(denied)
            .on_select_tx(move |s: &mut App, tx: &mut Tx| {
                close(s);
                dispatch(s, tx, Message::OpenPoke(id));
            });
        if denied {
            poke = poke.description("The server doesn't let you poke them");
        }
        menu = menu.item(poke);
    }
    let channels: Vec<MenuEntry> = s
        .state
        .channels
        .iter()
        .filter(|channel| {
            !ids.iter()
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
                .icon(if channel.has_password {
                    &icons::LOCK
                } else {
                    &icons::VOLUME
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
    .icon(&icons::MOVE)
    .disabled(!movable)
    .on_select(|s: &mut App| s.ui.move_menu = !s.ui.move_menu);
    if movable {
        move_to = move_to.submenu(channels, s.ui.move_menu);
    } else if !s.can_move(&ids) {
        move_to = move_to.description("The server doesn't let you move them");
    }
    menu = menu.item(move_to);
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
        s.state
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
    use super::channel_in_path;

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
