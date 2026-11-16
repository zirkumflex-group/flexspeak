//! Settings > Whisper: the lists and their keys, the reply key, what happens to whispers to
//! you, and the dialogs that edit a list and import TeamSpeak 3's.

use rune::{Color, El, Icon, col, el, icon, row, text};
use rune_kit::{Checkbox, Kit};

use super::super::theme::{Theme, radius, size, ty};
use super::super::widgets::{Variant, button, error_text, faint, field, group_note, muted, picker};
use super::super::{dispatch, icons};
use super::{Row, Section, actions, badge, item, shortcut_row, switch_to};
use crate::whisper::{Entry, GroupKind, List, Scope, Whom};
use crate::whisper_app::{EDITOR, Msg};
use crate::{App, Message, shortcuts};

fn send(msg: Msg) -> Message {
    Message::Whisper(msg)
}

fn list_icon(list: &List) -> &'static Icon {
    match &list.to {
        Whom::Group(_) => &icons::SHIELD,
        Whom::Listed(entries)
            if !entries.is_empty()
                && entries
                    .iter()
                    .all(|entry| matches!(entry, Entry::Channel { .. })) =>
        {
            &icons::HASH
        }
        Whom::Listed(_) => &icons::USERS,
    }
}

/// A list's kind in the whisper colour, as the page and dialogs show it.
pub(in crate::ui) fn tile(t: &Theme, glyph: &'static Icon, side: f32) -> El {
    el().size(side, side)
        .radius(radius::CONTROL)
        .center()
        .fill(t.private.alpha(if t.dark { 0.22 } else { 0.12 }))
        .child(icon(glyph, side * 0.5).color(t.private).a11y_hidden())
        .a11y_hidden()
}

/// A shortcut as the rows show it, set or not.
fn key_well(t: &Theme, key: &str) -> El {
    row()
        .w(150.0)
        .h(size::CONTROL)
        .px(10.0)
        .center_y()
        .radius(radius::CONTROL)
        .fill(t.wash(0.05))
        .border(1.0, t.hairline)
        .child(
            text(if key.is_empty() { "Not set" } else { key }.to_owned())
                .font(ty::NUMERIC)
                .color(if key.is_empty() { t.faint } else { t.text })
                .truncate_fade(),
        )
}

fn list_row(s: &App, t: &Theme, index: usize, list: &List) -> Row {
    let detail = format!(
        "{} · {}",
        list.summary(),
        s.whisper_server_name(&list.server)
    );
    let e = row()
        .key(format!("whisper-list-{index}"))
        .w_full()
        .min_h(52.0)
        .px(size::CARD_PAD)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(tile(t, list_icon(list), 28.0))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(
                    row()
                        .gap(8.0)
                        .center_y()
                        .child(
                            text(list.name.clone())
                                .font(ty::BODY)
                                .color(t.text)
                                .truncate_fade()
                                .min_w(0.0),
                        )
                        .children(list.toggle.then(|| badge(t, "Toggle"))),
                )
                .child(
                    text(detail)
                        .font(ty::CAPTION)
                        .color(t.faint)
                        .truncate_fade(),
                ),
        )
        .child(key_well(t, list.key.trim()))
        .child(
            button(
                t,
                &format!("edit-whisper-{index}"),
                "Edit",
                Some(&icons::PENCIL),
                Variant::Secondary,
                Some(send(Msg::Edit(index))),
            )
            .help(format!("Edit {}", list.name)),
        );
    item(
        list.name.clone(),
        format!("whisper list {}", list.summary()),
        e,
    )
}

pub(super) fn sections(s: &App, t: &Theme, k: &Kit) -> Vec<Section> {
    let mut rows: Vec<Row> = s
        .whisper
        .lists
        .iter()
        .enumerate()
        .map(|(index, list)| list_row(s, t, index, list))
        .collect();
    if rows.is_empty() {
        rows.push(item(
            "",
            "",
            row()
                .key("whisper-empty")
                .w_full()
                .px(size::CARD_PAD)
                .py(14.0)
                .gap(12.0)
                .center_y()
                .child(tile(t, &icons::WHISPER, 28.0))
                .child(
                    col()
                        .grow(1.0)
                        .gap(2.0)
                        .child(text("No whisper lists yet").font(ty::BODY).color(t.text))
                        .child(faint(
                            t,
                            "Make one, or import the lists you had in TeamSpeak 3.",
                        )),
                ),
        ));
    }
    rows.push(item(
        "New List",
        "new add whisper list import teamspeak ts3 whisperlist",
        actions([
            button(
                t,
                "import-whisper",
                "Import from TeamSpeak 3…",
                Some(&icons::DOWNLOAD),
                Variant::Secondary,
                Some(send(Msg::ImportOpen)),
            )
            .help("Pick a .whisperlist file TeamSpeak 3 exported, or the lists in its settings"),
            button(
                t,
                "new-whisper",
                "New List",
                Some(&icons::PLUS),
                Variant::Secondary,
                Some(send(Msg::New)),
            ),
        ]),
    ));
    let lists = Section::boxed(
        "whisper-lists",
        "Whisper Lists",
        "whisper lists push to whisper talk to people channels groups",
        rows,
    )
    .note(group_note(
        t,
        "Hold a list's shortcut to talk only to the people and channels on it. You can also drop a .whisperlist file here.",
    ));
    let reply = Section::boxed(
        "whisper-reply",
        "Reply",
        "whisper reply back answer",
        vec![item(
            "Whisper back",
            "whisper reply back answer last",
            shortcut_row(
                s,
                t,
                k,
                shortcuts::Binding::WhisperReply,
                "Whisper back",
                Some("Held. Talks to whoever whispered to you last."),
            ),
        )],
    );
    let accept = s.whisper.accept;
    let duck = s.whisper.duck;
    let receiving = Section::boxed(
        "whisper-receiving",
        "Receiving",
        "whisper receive incoming hear",
        vec![
            item(
                "Accept whispers",
                "accept allow block incoming whispers",
                super::super::widgets::form_row(
                    t,
                    "Accept whispers",
                    Some("Off: whispers to you are dropped, with no sound."),
                    switch_to(
                        "whisper-accept",
                        "Accept whispers",
                        accept,
                        true,
                        k,
                        send(Msg::Accept(!accept)),
                    ),
                ),
            ),
            item(
                "Quieter channel while someone whispers",
                "duck lower quieter volume whisper",
                super::super::widgets::form_row(
                    t,
                    "Quieter channel while someone whispers",
                    Some("Everyone else plays at half volume until the whisper ends."),
                    switch_to(
                        "whisper-duck",
                        "Quieter channel while someone whispers",
                        duck,
                        accept,
                        k,
                        send(Msg::Duck(!duck)),
                    ),
                ),
            ),
        ],
    );
    let mut sections = vec![lists];
    if shortcuts::supported() {
        sections.push(reply);
    }
    sections.push(receiving);
    sections
}

/// The page's own column takes dropped `.whisperlist` files.
pub(super) fn drop_zone(page: El) -> El {
    page.accepts_extensions(&["whisperlist"])
        .on_drop_files(|s: &mut App, files, tx| {
            dispatch(s, tx, send(Msg::ImportFiles(files.to_vec())));
        })
}

fn labelled_field(t: &Theme, label: &str, detail: Option<&str>, field: El) -> El {
    let mut e = col()
        .w_full()
        .gap(6.0)
        .child(text(label.to_owned()).font(ty::BODY_STRONG).color(t.text))
        .child(field);
    if let Some(detail) = detail {
        e = e.child(faint(t, detail.to_owned()));
    }
    e
}

fn entry_row(t: &Theme, k: &Kit, s: &App, index: usize, entry: &Entry) -> El {
    let offer = s.whisper_offer();
    let (glyph, detail, here) = match entry {
        Entry::Client { uid, .. } => {
            let found = s.server().state.channels.iter().find_map(|channel| {
                channel
                    .members
                    .iter()
                    .find(|member| member.uid == *uid)
                    .map(|_| channel.name.clone())
            });
            match found {
                Some(channel) if s.live() => {
                    (&icons::USER, format!("Here, in {channel}"), Some(true))
                }
                _ if s.live() => (
                    &icons::USER,
                    "Not on this server right now".into(),
                    Some(false),
                ),
                _ => (&icons::USER, "Person".into(), None),
            }
        }
        Entry::Channel { path, .. } => {
            let here = offer.as_ref().map(|offer| offer.channels.contains(path));
            let parent = path
                .rsplit_once('/')
                .map(|(parent, _)| crate::whisper::readable_path(parent));
            let detail = match (here, parent) {
                (Some(false), _) => "Not on this server".to_owned(),
                (_, Some(parent)) => format!("Channel in {parent}"),
                (_, None) => "Channel".to_owned(),
            };
            (&icons::HASH, detail, here)
        }
    };
    let mut e = row()
        .key(format!("whisper-entry-{index}"))
        .w_full()
        .min_h(48.0)
        .px(12.0)
        .py(6.0)
        .gap(10.0)
        .center_y()
        .child(icon(glyph, 16.0).color(t.muted).a11y_hidden())
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(1.0)
                .child(
                    text(entry.title().to_owned())
                        .font(ty::BODY)
                        .color(if here == Some(false) { t.muted } else { t.text })
                        .truncate_fade(),
                )
                .child(
                    text(detail)
                        .font(ty::CAPTION)
                        .color(t.faint)
                        .truncate_fade(),
                ),
        );
    if let Entry::Channel { subchannels, .. } = entry {
        let on = *subchannels;
        e = e.child(
            Checkbox::new(on)
                .label("Subchannels")
                .on_toggle_tx(move |s: &mut App, tx| {
                    dispatch(s, tx, send(Msg::Subchannels(index, !on)));
                })
                .build(k)
                .key(format!("whisper-subchannels-{index}")),
        );
    }
    if let Some(here) = here.filter(|_| matches!(entry, Entry::Client { .. })) {
        e = e.child(
            text(if here { "online" } else { "away" })
                .font(ty::CAPTION)
                .color(if here { t.good } else { t.faint }),
        );
    }
    e.child(super::super::widgets::icon_button(
        t,
        &format!("whisper-remove-{index}"),
        &icons::X,
        format!("Remove {}", entry.title()),
        Variant::Ghost,
        Some(send(Msg::Remove(index))),
    ))
}

fn editor_dialog(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let Some(editor) = &s.whisper_editor else {
        return;
    };
    let list = &editor.list;
    let mut servers = vec!["Any server".to_owned()];
    servers.extend(s.whisper_servers().into_iter().map(|(_, name)| name));
    let server_name = s.whisper_server_name(&list.server);
    let mut menus = Vec::new();
    let top = row()
        .w_full()
        .gap(12.0)
        .child(el().grow(1.0).min_w(0.0).child(labelled_field(
            t,
            "Name",
            None,
            field(
                k,
                &s.ui.whisper_name,
                |s| &mut s.ui.whisper_name,
                "Raid leads, Officers…",
                Some(|value| Message::Whisper(Msg::Name(value))),
                Some(send(Msg::Save)),
            ),
        )))
        .child(el().w(200.0).child(labelled_field(
            t,
            "Server",
            None,
            picker(
                k,
                s,
                "whisper-server",
                servers,
                Some(server_name.as_str()),
                "Any server",
                true,
                200.0,
                |index| Message::Whisper(Msg::Server(index)),
                &mut menus,
            ),
        )));
    let mut body = col().w_full().px(16.0).pb(16.0).gap(14.0).child(top);
    if shortcuts::supported() {
        let toggle = list.toggle;
        body = body.child(
            col()
                .w_full()
                .gap(4.0)
                .child(shortcut_row(
                    s,
                    t,
                    k,
                    shortcuts::Binding::Whisper(EDITOR),
                    "Shortcut",
                    Some(if toggle {
                        "Press to start whispering, press again to stop."
                    } else {
                        "Hold to whisper; let go to talk to your channel again."
                    }),
                ))
                .child(
                    row().w_full().px(size::CARD_PAD).child(
                        Checkbox::new(toggle)
                            .label("Press once to start, again to stop")
                            .on_toggle_tx(move |s: &mut App, tx| {
                                dispatch(s, tx, send(Msg::Toggle(!toggle)));
                            })
                            .build(k)
                            .key("whisper-toggle"),
                    ),
                ),
        );
    }
    let offer = s.whisper_offer();
    let group = match &list.to {
        Whom::Group(group) => Some(*group),
        Whom::Listed(_) => None,
    };
    let mut kinds = vec!["People and channels".to_owned()];
    kinds.extend(GroupKind::ALL.iter().map(|kind| kind.title().to_owned()));
    let kind_name = group.map_or("People and channels", |group| group.kind.title());
    body = body.child(labelled_field(
        t,
        "Whisper to",
        None,
        picker(
            k,
            s,
            "whisper-kind",
            kinds,
            Some(kind_name),
            "People and channels",
            true,
            260.0,
            |index| {
                Message::Whisper(Msg::GroupKind(
                    index
                        .checked_sub(1)
                        .and_then(|at| GroupKind::ALL.get(at).copied()),
                ))
            },
            &mut menus,
        ),
    ));
    match (&list.to, group) {
        (Whom::Listed(entries), _) => {
            let mut card = col()
                .key("whisper-entries")
                .w_full()
                .radius(radius::CARD)
                .fill(t.surface)
                .border(1.0, t.hairline);
            if entries.is_empty() {
                card = card.child(
                    el().w_full()
                        .px(12.0)
                        .py(14.0)
                        .child(muted(t, "Nobody yet. Add people and channels below.")),
                );
            }
            for (index, entry) in entries.iter().enumerate() {
                if index > 0 {
                    card = card.child(el().w_full().h(1.0).fill(t.hairline).a11y_hidden());
                }
                card = card.child(entry_row(t, k, s, index, entry));
            }
            body = body.child(card);
            match &offer {
                Some(offer) => {
                    body = body.child(
                        row()
                            .w_full()
                            .gap(8.0)
                            .child(el().grow(1.0).child(picker(
                                k,
                                s,
                                "whisper-add-person",
                                offer.people.iter().map(|(_, name)| name.clone()).collect(),
                                None,
                                "Add a person…",
                                true,
                                200.0,
                                |index| Message::Whisper(Msg::AddPerson(index)),
                                &mut menus,
                            )))
                            .child(
                                el().grow(1.0).child(picker(
                                    k,
                                    s,
                                    "whisper-add-channel",
                                    offer
                                        .channels
                                        .iter()
                                        .map(|path| crate::whisper::readable_path(path))
                                        .collect(),
                                    None,
                                    "Add a channel…",
                                    true,
                                    200.0,
                                    |index| Message::Whisper(Msg::AddChannel(index)),
                                    &mut menus,
                                )),
                            ),
                    );
                }
                None => {
                    body = body.child(faint(
                        t,
                        "Connect to a server to add its people and channels.",
                    ));
                }
            }
        }
        (Whom::Group(_), Some(group)) => {
            let mut controls = row().w_full().gap(8.0);
            if group.kind.has_id() {
                let groups = offer.as_ref().map_or_else(Vec::new, |offer| {
                    if group.kind == GroupKind::ServerGroup {
                        offer.server_groups.clone()
                    } else {
                        offer.channel_groups.clone()
                    }
                });
                let current = groups
                    .iter()
                    .find(|(id, _)| *id == group.id)
                    .map(|(_, name)| name.clone())
                    .unwrap_or_else(|| format!("Group {}", group.id));
                controls = controls.child(el().grow(1.0).child(picker(
                    k,
                    s,
                    "whisper-group",
                    groups.into_iter().map(|(_, name)| name).collect(),
                    Some(current.as_str()),
                    "Choose a group…",
                    offer.is_some(),
                    200.0,
                    |index| Message::Whisper(Msg::GroupId(index)),
                    &mut menus,
                )));
            }
            controls = controls.child(
                el().grow(1.0).child(picker(
                    k,
                    s,
                    "whisper-scope",
                    Scope::ALL
                        .iter()
                        .map(|scope| scope.title().to_owned())
                        .collect(),
                    Some(group.scope.title()),
                    "Everywhere",
                    true,
                    200.0,
                    |index| Message::Whisper(Msg::Scope(index)),
                    &mut menus,
                )),
            );
            body = body.child(controls);
        }
        _ => {}
    }
    if let Some(error) = &editor.error {
        body = body.child(error_text(t, error.clone()));
    }
    let mut buttons = row().w_full().gap(size::GAP).center_y().pt(4.0);
    if editor.index.is_some() {
        buttons = buttons.child(button(
            t,
            "delete-whisper",
            if editor.remove_armed {
                "Press Again to Delete"
            } else {
                "Delete List"
            },
            Some(&icons::TRASH),
            Variant::Danger,
            Some(send(Msg::Delete)),
        ));
    }
    buttons = buttons
        .child(el().grow(1.0))
        .child(button(
            t,
            "cancel-whisper",
            "Cancel",
            None,
            Variant::Secondary,
            Some(send(Msg::Cancel)),
        ))
        .child(
            button(
                t,
                "save-whisper",
                "Save",
                None,
                Variant::Primary,
                Some(send(Msg::Save)),
            )
            .default_button(),
        );
    rune_kit::Dialog::new(
        "whisper-editor",
        if editor.index.is_some() {
            "Edit Whisper List"
        } else {
            "New Whisper List"
        },
    )
    .description("People are kept by identity, so they're found again after they reconnect.")
    .open(true)
    .fit()
    .width(600.0)
    .on_close(|s: &mut App| {
        let _ = s.update(Message::Whisper(Msg::Cancel));
        s.sync_fields();
    })
    .child(body.child(buttons))
    .build(k, layer);
    layer.extend(menus);
}

fn import_dialog(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    let Some(import) = &s.whisper_import else {
        return;
    };
    let mut body = col().w_full().px(16.0).pb(16.0).gap(12.0);
    if import.found.is_empty() {
        body = body.child(muted(
            t,
            "No lists yet. Choose a .whisperlist file TeamSpeak 3 exported (Tools › Whisper Lists › Export), or drop one on this window.",
        ));
    } else {
        let mut card = col()
            .key("whisper-import-list")
            .w_full()
            .radius(radius::CARD)
            .fill(t.surface)
            .border(1.0, t.hairline);
        for (index, candidate) in import.found.iter().enumerate() {
            let found = &candidate.found;
            let list = &found.list;
            let mut detail = vec![list.summary()];
            detail.push(if list.server.is_empty() {
                if found.server_name.is_empty() {
                    "Any server".into()
                } else {
                    found.server_name.clone()
                }
            } else {
                s.whisper_server_name(&list.server)
            });
            if !list.key.is_empty() {
                detail.push(list.key.clone());
            }
            let (note, color) = import_note(s, candidate, t);
            if index > 0 {
                card = card.child(el().w_full().h(1.0).fill(t.hairline).a11y_hidden());
            }
            let picked = candidate.picked;
            card = card.child(
                row()
                    .key(format!("whisper-import-{index}"))
                    .w_full()
                    .min_h(52.0)
                    .px(12.0)
                    .py(8.0)
                    .gap(12.0)
                    .center_y()
                    .child(
                        Checkbox::new(picked)
                            .label(format!("Import {}", list.name))
                            .hide_label()
                            .on_toggle_tx(move |s: &mut App, tx| {
                                dispatch(s, tx, send(Msg::ImportPick(index, !picked)));
                            })
                            .build(k)
                            .key(format!("whisper-import-pick-{index}")),
                    )
                    .child(tile(t, list_icon(list), 28.0))
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(2.0)
                            .child(
                                text(list.name.clone())
                                    .font(ty::BODY)
                                    .color(if picked { t.text } else { t.muted })
                                    .truncate_fade(),
                            )
                            .child(
                                text(detail.join(" · "))
                                    .font(ty::CAPTION)
                                    .color(t.faint)
                                    .truncate_fade(),
                            ),
                    )
                    .child(pill(t, &note, color)),
            );
        }
        body = body.child(card).child(faint(
            t,
            "Channels are found by their names, so a list works on any server that has them.",
        ));
    }
    if let Some(error) = &import.error {
        body = body.child(error_text(t, error.clone()));
    }
    let picked = import
        .found
        .iter()
        .filter(|candidate| candidate.picked)
        .count();
    let buttons = row()
        .w_full()
        .gap(size::GAP)
        .center_y()
        .pt(4.0)
        .child(button(
            t,
            "whisper-import-file",
            "Choose File…",
            Some(&icons::FOLDER_OPEN),
            Variant::Secondary,
            Some(send(Msg::ImportChoose)),
        ))
        .child(el().grow(1.0))
        .child(button(
            t,
            "whisper-import-cancel",
            "Cancel",
            None,
            Variant::Secondary,
            Some(send(Msg::ImportCancel)),
        ))
        .child(
            button(
                t,
                "whisper-import-confirm",
                match picked {
                    0 => "Import".to_owned(),
                    1 => "Import 1 List".to_owned(),
                    n => format!("Import {n} Lists"),
                },
                None,
                Variant::Primary,
                (picked > 0).then(|| send(Msg::ImportConfirm)),
            )
            .default_button(),
        );
    rune_kit::Dialog::new("whisper-import", "Import from TeamSpeak 3")
        .description("Nothing in TeamSpeak is changed. Pick the lists to bring over.")
        .open(true)
        .fit()
        .width(640.0)
        .on_close(|s: &mut App| {
            let _ = s.update(Message::Whisper(Msg::ImportCancel));
        })
        .child(
            body.accepts_extensions(&["whisperlist"])
                .on_drop_files(|s: &mut App, files, tx| {
                    dispatch(s, tx, send(Msg::ImportFiles(files.to_vec())));
                })
                .child(buttons),
        )
        .build(k, layer);
}

/// What importing one list will do, beyond copying it.
fn import_note(s: &App, candidate: &crate::whisper_app::Candidate, t: &Theme) -> (String, Color) {
    let list = &candidate.found.list;
    if s.whisper.lists.iter().any(|saved| saved.id == list.id) {
        return ("Replaces the one you have".into(), t.fair);
    }
    if let Some(key) = &candidate.found.unmapped_key {
        let _ = key;
        return ("Its shortcut can't be carried over".into(), t.fair);
    }
    if !list.key.is_empty() {
        let mut next = s.whisper.clone();
        next.lists.push(list.clone());
        if s.keybinds.parse_with(&next).is_err() {
            return (
                format!("{} is taken here, so no shortcut", list.key),
                t.fair,
            );
        }
    }
    ("Ready".into(), t.good)
}

fn pill(t: &Theme, label: &str, color: Color) -> El {
    el().px(8.0)
        .py(2.0)
        .radius(radius::PILL)
        .fill(color.alpha(if t.dark { 0.16 } else { 0.12 }))
        .child(text(label.to_owned()).font(ty::CAPTION).color(color))
}

pub(super) fn dialogs(s: &App, t: &Theme, k: &Kit, layer: &mut Vec<El>) {
    editor_dialog(s, t, k, layer);
    import_dialog(s, t, k, layer);
}
