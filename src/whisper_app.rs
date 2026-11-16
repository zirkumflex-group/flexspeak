//! Settings > Whisper and whispering itself, on the app's side: the lists being edited or
//! imported, the keys that start them, and which server is told to whisper.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use crate::alerts::Source;
use crate::ts3_import::Found;
use crate::whisper::{self, Entry, Group, GroupKind, List, Scope, Target, Whom};
use crate::{App, Task, client, hub, shortcuts};

/// The place the list being edited records its key under, which no saved list has.
pub const EDITOR: usize = usize::MAX;

#[derive(Debug, Clone)]
pub struct Editor {
    /// The saved list being changed; `None` for a new one.
    pub index: Option<usize>,
    pub list: List,
    pub error: Option<String>,
    pub remove_armed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Import {
    pub found: Vec<Candidate>,
    /// Why a file couldn't be read, or that nothing was found.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub found: Found,
    pub picked: bool,
}

/// A whisper in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whispering {
    pub session: hub::SessionId,
    /// The list's place, or `None` for the reply or one person.
    pub list: Option<usize>,
    /// One person whispered to from their menu, as a list of one.
    pub person: Option<List>,
    /// Started by a key held down, which its release ends.
    pub held: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    New,
    /// A new list for one person, from their menu.
    NewWith {
        uid: String,
        name: String,
    },
    /// Adds a person to a saved list, from their menu.
    AddTo {
        list: usize,
        uid: String,
        name: String,
    },
    Edit(usize),
    Name(String),
    Server(usize),
    Toggle(bool),
    AddPerson(usize),
    AddChannel(usize),
    Remove(usize),
    Subchannels(usize, bool),
    /// Whisper to a group instead of people and channels, or back.
    GroupKind(Option<GroupKind>),
    GroupId(usize),
    Scope(usize),
    Save,
    Cancel,
    Delete,
    Accept(bool),
    Duck(bool),
    /// Ends a whisper a toggled list started.
    Stop,
    /// Whispers to one person until stopped, from their menu.
    To {
        uid: String,
        name: String,
    },
    ImportOpen,
    ImportChoose,
    ImportFiles(Vec<PathBuf>),
    ImportPick(usize, bool),
    ImportConfirm,
    ImportCancel,
}

/// People and channels on a server, as the editor offers them.
pub struct Offer {
    pub people: Vec<(String, String)>,
    pub channels: Vec<String>,
    pub server_groups: Vec<(u64, String)>,
    pub channel_groups: Vec<(u64, String)>,
}

impl App {
    /// The saved servers a list can be for, after "Any server".
    pub(crate) fn whisper_servers(&self) -> Vec<(String, String)> {
        self.servers
            .iter()
            .map(|server| {
                let address = server.address.trim().to_owned();
                let name = if server.name.trim().is_empty() {
                    address.clone()
                } else {
                    server.name.trim().to_owned()
                };
                (address, name)
            })
            .collect()
    }

    /// The name a list's server is shown by.
    pub(crate) fn whisper_server_name(&self, address: &str) -> String {
        if address.is_empty() {
            return "Any server".into();
        }
        self.whisper_servers()
            .into_iter()
            .find(|(saved, _)| saved.eq_ignore_ascii_case(address))
            .map_or_else(|| address.to_owned(), |(_, name)| name)
    }

    /// Who and what the connected server has to add to a list.
    pub(crate) fn whisper_offer(&self) -> Option<Offer> {
        if !self.live() {
            return None;
        }
        let state = &self.server().state;
        let nodes = channel_nodes(&state.channels);
        let mut people: Vec<(String, String)> = state
            .channels
            .iter()
            .flat_map(|channel| &channel.members)
            .filter(|member| !member.is_self && !member.uid.is_empty())
            .map(|member| (member.uid.clone(), member.name.clone()))
            .collect();
        people.sort_by_cached_key(|(_, name)| name.to_lowercase());
        people.dedup_by(|a, b| a.0 == b.0);
        let channels = state
            .channels
            .iter()
            .filter(|channel| channel.spacer().is_none())
            .filter_map(|channel| whisper::channel_path(&nodes, channel.id))
            .collect();
        let groups = |groups: &[client::Group]| {
            groups
                .iter()
                .map(|group| (group.id, group.name.clone()))
                .collect()
        };
        Some(Offer {
            people,
            channels,
            server_groups: groups(&state.server_groups),
            channel_groups: groups(&state.channel_groups),
        })
    }

    /// A list's key, or the reply's, as the shortcut rows show it.
    pub(crate) fn whisper_key(&self, binding: shortcuts::Binding) -> String {
        match binding {
            shortcuts::Binding::WhisperReply => self.whisper.reply_key.clone(),
            shortcuts::Binding::Whisper(EDITOR) => self
                .whisper_editor
                .as_ref()
                .map(|editor| editor.list.key.clone())
                .unwrap_or_default(),
            shortcuts::Binding::Whisper(index) => self
                .whisper
                .lists
                .get(index)
                .map(|list| list.key.clone())
                .unwrap_or_default(),
            other => self.keybind_draft.get(other).to_owned(),
        }
    }

    /// The channels and people a whisper reaches on the server shown, while one is going on
    /// there; the reply reaches whoever whispered last.
    pub(crate) fn whisper_reach(&self) -> Option<(Vec<u64>, Vec<u16>)> {
        let whispering = self.whispering.as_ref()?;
        if Some(whispering.session) != self.active {
            return None;
        }
        let session = self.server();
        let resolved = match self.whispering_list() {
            None => whisper::Resolved::Listed {
                channels: Vec::new(),
                clients: session.whisperer.into_iter().collect(),
            },
            Some(list) => {
                let (channels, clients, own) = nodes(&session.state);
                whisper::resolve(list, &channels, &clients, own)
            }
        };
        match resolved {
            whisper::Resolved::Listed { channels, clients } => Some((channels, clients)),
            whisper::Resolved::Group { .. } => Some((Vec::new(), Vec::new())),
        }
    }

    /// What the whisper going on is called, and how many of its targets are here.
    pub(crate) fn whisper_status(&self) -> Option<(String, String)> {
        let whispering = self.whispering.as_ref()?;
        if Some(whispering.session) != self.active {
            return None;
        }
        if let Some(person) = &whispering.person {
            let (_, clients, _) = nodes(&self.server().state);
            let (here, _) = whisper::present(person, &[], &clients);
            let detail = if here > 0 {
                "Here"
            } else {
                "Not on this server"
            };
            return Some((person.name.clone(), detail.into()));
        }
        let Some(index) = whispering.list else {
            let session = self.server();
            let name = session.whisperer.and_then(|id| {
                session
                    .state
                    .channels
                    .iter()
                    .flat_map(|channel| &channel.members)
                    .find(|member| member.id == id)
                    .map(|member| member.name.clone())
            });
            return Some(match name {
                Some(name) => ("Whispering back".into(), name),
                None => (
                    "Whispering back".into(),
                    "Nobody has whispered to you".into(),
                ),
            });
        };
        let list = self.whisper.lists.get(index)?;
        let (channels, clients, _) = nodes(&self.server().state);
        let detail = match &list.to {
            Whom::Group(group) => group_title(&self.server().state, group),
            Whom::Listed(_) => {
                let (here, all) = whisper::present(list, &channels, &clients);
                if here == 0 {
                    "None of them are here".into()
                } else if here == all {
                    "All here".into()
                } else {
                    format!("{here} of {all} here")
                }
            }
        };
        Some((list.name.clone(), detail))
    }

    /// The list a whisper going on is to; `None` for the reply.
    fn whispering_list(&self) -> Option<&List> {
        let whispering = self.whispering.as_ref()?;
        match &whispering.person {
            Some(person) => Some(person),
            None => self.whisper.lists.get(whispering.list?),
        }
    }

    /// A whisper key pressed or let go: `list` is its place, or `None` for the reply.
    pub(crate) fn whisper_shortcut(&mut self, list: Option<usize>, pressed: bool) {
        let toggle = list
            .and_then(|index| self.whisper.lists.get(index))
            .is_some_and(|list| list.toggle);
        let same = self
            .whispering
            .as_ref()
            .is_some_and(|whispering| whispering.list == list);
        if toggle {
            if pressed {
                if same {
                    self.stop_whisper();
                } else {
                    self.start_whisper(list, false);
                }
            }
        } else if pressed {
            if !same {
                self.start_whisper(list, true);
            }
        } else if same {
            self.stop_whisper();
        }
    }

    pub(crate) fn start_whisper(&mut self, list: Option<usize>, held: bool) {
        let Some(id) = self.active.filter(|_| self.live()) else {
            return;
        };
        let target = match list {
            None => Target::Reply,
            Some(index) => {
                let Some(saved) = self.whisper.lists.get(index) else {
                    return;
                };
                if !saved.for_server(&self.server().address) {
                    self.raise_active(Source::Whisper, "That whisper list is for another server.");
                    return;
                }
                Target::List(saved.clone())
            }
        };
        if self.whispering.is_some() {
            self.stop_whisper();
        }
        if !self.send_to(id, client::Command::Whisper(Some(target))) {
            return;
        }
        self.whisper_shared.held.store(true, Ordering::Release);
        self.whispering = Some(Whispering {
            session: id,
            list,
            person: None,
            held,
        });
        self.alerts.recover(Source::Whisper);
    }

    pub(crate) fn stop_whisper(&mut self) {
        self.whisper_shared.held.store(false, Ordering::Release);
        if let Some(whispering) = self.whispering.take() {
            self.send_to(whispering.session, client::Command::Whisper(None));
        }
    }

    /// A key recorded for the reply or the list being edited.
    pub(crate) fn whisper_recorded(&mut self, binding: shortcuts::Binding, shortcut: String) {
        let mut next = self.whisper.clone();
        match binding {
            shortcuts::Binding::WhisperReply => next.reply_key = shortcut.clone(),
            _ => {
                let Some(editor) = &self.whisper_editor else {
                    self.end_recording();
                    return;
                };
                let mut list = editor.list.clone();
                list.key = shortcut.clone();
                match editor.index {
                    Some(index) if index < next.lists.len() => next.lists[index] = list,
                    _ => next.lists.push(list),
                }
            }
        }
        if let Err(error) = self.keybinds.parse_with(&next) {
            if let Some(capture) = &mut self.key_capture {
                capture.error = Some(format!("{error:#}"));
            }
            return;
        }
        self.end_recording();
        match binding {
            shortcuts::Binding::WhisperReply => {
                self.save_whisper(next);
            }
            _ => {
                if let Some(editor) = &mut self.whisper_editor {
                    editor.list.key = shortcut;
                    editor.error = None;
                }
            }
        }
    }

    /// Applies new whisper settings, their keys, and saves them. False when they can't apply.
    pub(crate) fn save_whisper(&mut self, next: whisper::Settings) -> bool {
        self.alerts.begin(Source::Shortcuts);
        if let Err(error) = self.keybinds.parse_with(&next) {
            self.alerts.raise(Source::Shortcuts, format!("{error:#}"));
            return false;
        }
        // A whisper going on stops when its list changes under it.
        self.stop_whisper();
        let before = std::mem::replace(&mut self.whisper, next.clone());
        let keys = self.keybinds.clone();
        if self.key_capture.is_none()
            && let Err(error) = self.configure_shortcuts(&keys)
        {
            self.whisper = before;
            self.alerts.raise(Source::Shortcuts, format!("{error:#}"));
            return false;
        }
        self.whisper_shared.configure(&next);
        self.save_preference(
            |saved| saved.whisper = next,
            "The whisper settings apply for now but were not saved",
        );
        true
    }

    pub(crate) fn update_whisper(&mut self, msg: Msg) -> Task {
        match msg {
            Msg::New => self.open_editor(
                None,
                List::new(whisper::new_id(&self.whisper.lists), "New list".into()),
            ),
            Msg::NewWith { uid, name } => {
                let mut list = List::new(whisper::new_id(&self.whisper.lists), name.clone());
                list.server = self.server().address.trim().to_owned();
                list.to = Whom::Listed(vec![Entry::Client { uid, name }]);
                self.open_editor(None, list);
            }
            Msg::AddTo { list, uid, name } => {
                let mut next = self.whisper.clone();
                if let Some(saved) = next.lists.get_mut(list) {
                    if let Whom::Listed(entries) = &mut saved.to
                        && !entries.iter().any(|entry| matches!(entry, Entry::Client { uid: known, .. } if *known == uid))
                    {
                        entries.push(Entry::Client { uid, name });
                    }
                    self.save_whisper(next);
                }
                self.ui.member_menu = None;
                self.ui.open = None;
            }
            Msg::Edit(index) => {
                if let Some(list) = self.whisper.lists.get(index).cloned() {
                    self.open_editor(Some(index), list);
                }
            }
            Msg::Name(name) => {
                if let Some(editor) = &mut self.whisper_editor {
                    editor.list.name = name.chars().take(64).collect();
                    editor.error = None;
                }
            }
            Msg::Server(index) => {
                let servers = self.whisper_servers();
                if let Some(editor) = &mut self.whisper_editor {
                    editor.list.server = match index {
                        0 => String::new(),
                        index => servers
                            .get(index - 1)
                            .map(|(address, _)| address.clone())
                            .unwrap_or_default(),
                    };
                }
            }
            Msg::Toggle(toggle) => {
                if let Some(editor) = &mut self.whisper_editor {
                    editor.list.toggle = toggle;
                }
            }
            Msg::AddPerson(index) => {
                let person = self
                    .whisper_offer()
                    .and_then(|offer| offer.people.get(index).cloned());
                if let (Some((uid, name)), Some(editor)) = (person, &mut self.whisper_editor) {
                    editor.add(Entry::Client { uid, name });
                }
            }
            Msg::AddChannel(index) => {
                let path = self
                    .whisper_offer()
                    .and_then(|offer| offer.channels.get(index).cloned());
                if let (Some(path), Some(editor)) = (path, &mut self.whisper_editor) {
                    editor.add(Entry::Channel {
                        path,
                        subchannels: false,
                    });
                }
            }
            Msg::Remove(index) => {
                if let Some(editor) = &mut self.whisper_editor
                    && let Whom::Listed(entries) = &mut editor.list.to
                    && index < entries.len()
                {
                    entries.remove(index);
                }
            }
            Msg::Subchannels(index, on) => {
                if let Some(editor) = &mut self.whisper_editor
                    && let Whom::Listed(entries) = &mut editor.list.to
                    && let Some(Entry::Channel { subchannels, .. }) = entries.get_mut(index)
                {
                    *subchannels = on;
                }
            }
            Msg::GroupKind(kind) => {
                let first = self.whisper_offer().map(|offer| {
                    (
                        offer.server_groups.first().map(|g| g.0),
                        offer.channel_groups.first().map(|g| g.0),
                    )
                });
                if let Some(editor) = &mut self.whisper_editor {
                    editor.list.to = match kind {
                        None => Whom::Listed(Vec::new()),
                        Some(kind) => {
                            let id = match (kind, first) {
                                (GroupKind::ServerGroup, Some((Some(id), _))) => id,
                                (GroupKind::ChannelGroup, Some((_, Some(id)))) => id,
                                _ => 0,
                            };
                            let scope = match &editor.list.to {
                                Whom::Group(group) => group.scope,
                                Whom::Listed(_) => Scope::Everywhere,
                            };
                            Whom::Group(Group { kind, id, scope })
                        }
                    };
                }
            }
            Msg::GroupId(index) => {
                let offer = self.whisper_offer();
                if let Some(editor) = &mut self.whisper_editor
                    && let Whom::Group(group) = &mut editor.list.to
                    && let Some(offer) = offer
                {
                    let groups = if group.kind == GroupKind::ServerGroup {
                        &offer.server_groups
                    } else {
                        &offer.channel_groups
                    };
                    if let Some((id, _)) = groups.get(index) {
                        group.id = *id;
                    }
                }
            }
            Msg::Scope(index) => {
                if let Some(editor) = &mut self.whisper_editor
                    && let Whom::Group(group) = &mut editor.list.to
                    && let Some(scope) = Scope::ALL.get(index)
                {
                    group.scope = *scope;
                }
            }
            Msg::Save => self.save_editor(),
            Msg::Cancel => {
                self.end_recording();
                self.whisper_editor = None;
            }
            Msg::Delete => {
                let Some(editor) = &mut self.whisper_editor else {
                    return Task::none();
                };
                if !editor.remove_armed {
                    editor.remove_armed = true;
                    return Task::none();
                }
                if let Some(index) = editor.index {
                    let mut next = self.whisper.clone();
                    if index < next.lists.len() {
                        next.lists.remove(index);
                    }
                    if self.save_whisper(next) {
                        self.whisper_editor = None;
                    }
                }
            }
            Msg::Stop => self.stop_whisper(),
            Msg::To { uid, name } => {
                self.ui.member_menu = None;
                self.ui.open = None;
                let Some(id) = self.active.filter(|_| self.live()) else {
                    return Task::none();
                };
                let mut list = List::new(String::new(), name.clone());
                list.to = Whom::Listed(vec![Entry::Client { uid, name }]);
                self.stop_whisper();
                if self.send_to(
                    id,
                    client::Command::Whisper(Some(Target::List(list.clone()))),
                ) {
                    self.whisper_shared.held.store(true, Ordering::Release);
                    self.whispering = Some(Whispering {
                        session: id,
                        list: None,
                        person: Some(list),
                        held: false,
                    });
                }
            }
            Msg::Accept(accept) => {
                let next = whisper::Settings {
                    accept,
                    ..self.whisper.clone()
                };
                self.save_whisper(next);
            }
            Msg::Duck(duck) => {
                let next = whisper::Settings {
                    duck,
                    ..self.whisper.clone()
                };
                self.save_whisper(next);
            }
            Msg::ImportOpen => {
                let mut import = Import::default();
                for path in crate::ts3_import::settings_paths() {
                    match crate::ts3_import::read_settings(&path) {
                        Ok(found) => import.add(found),
                        Err(error) => {
                            tracing::info!(target: "flexspeak::whisper", %error, "TeamSpeak settings unreadable")
                        }
                    }
                }
                self.whisper_import = Some(import);
                return Task::ChooseWhisperLists;
            }
            Msg::ImportChoose => return Task::ChooseWhisperLists,
            Msg::ImportFiles(paths) => {
                let import = self.whisper_import.get_or_insert_with(Import::default);
                import.error = None;
                for path in paths.iter().take(32) {
                    match crate::ts3_import::read_file(path) {
                        Ok(found) => import.add(found),
                        Err(error) => import.error = Some(format!("{error:#}")),
                    }
                }
            }
            Msg::ImportPick(index, picked) => {
                if let Some(candidate) = self
                    .whisper_import
                    .as_mut()
                    .and_then(|import| import.found.get_mut(index))
                {
                    candidate.picked = picked;
                }
            }
            Msg::ImportConfirm => {
                let Some(import) = self.whisper_import.take() else {
                    return Task::none();
                };
                let mut next = self.whisper.clone();
                let mut added = 0;
                for candidate in import
                    .found
                    .into_iter()
                    .filter(|candidate| candidate.picked)
                {
                    let mut list = candidate.found.list;
                    if !list.server.is_empty()
                        && !self
                            .servers
                            .iter()
                            .any(|server| server.address.trim().eq_ignore_ascii_case(&list.server))
                    {
                        list.server = String::new();
                    }
                    let mut keyed = next.clone();
                    keyed.lists.retain(|saved| saved.id != list.id);
                    keyed.lists.push(list.clone());
                    if self.keybinds.parse_with(&keyed).is_err() {
                        list.key = String::new();
                    }
                    match next.lists.iter().position(|saved| saved.id == list.id) {
                        Some(at) => next.lists[at] = list,
                        None => next.lists.push(list),
                    }
                    added += 1;
                }
                tracing::info!(target: "flexspeak::whisper", lists = added, "imported");
                if added > 0 {
                    self.save_whisper(next);
                }
            }
            Msg::ImportCancel => self.whisper_import = None,
        }
        Task::none()
    }

    fn open_editor(&mut self, index: Option<usize>, list: List) {
        self.end_recording();
        self.ui.member_menu = None;
        self.ui.open = None;
        self.whisper_editor = Some(Editor {
            index,
            list,
            error: None,
            remove_armed: false,
        });
        if self.settings_page.is_none() {
            self.open_settings(crate::SettingsPage::Whisper);
        }
        self.sync_fields();
    }

    fn save_editor(&mut self) {
        self.end_recording();
        let Some(editor) = self.whisper_editor.clone() else {
            return;
        };
        let mut list = editor.list;
        list.name = list.name.trim().to_owned();
        if list.name.is_empty() {
            if let Some(editor) = &mut self.whisper_editor {
                editor.error = Some("Give the list a name.".into());
            }
            return;
        }
        let mut next = self.whisper.clone();
        match editor.index {
            Some(index) if index < next.lists.len() => next.lists[index] = list,
            _ => next.lists.push(list),
        }
        if let Err(error) = self.keybinds.parse_with(&next) {
            if let Some(editor) = &mut self.whisper_editor {
                editor.error = Some(format!("{error:#}"));
            }
            return;
        }
        if self.save_whisper(next) {
            self.whisper_editor = None;
        }
    }
}

impl Editor {
    fn add(&mut self, entry: Entry) {
        let Whom::Listed(entries) = &mut self.list.to else {
            return;
        };
        if !entries.contains(&entry)
            && !entries.iter().any(|known| match (known, &entry) {
                (Entry::Client { uid: a, .. }, Entry::Client { uid: b, .. }) => a == b,
                (Entry::Channel { path: a, .. }, Entry::Channel { path: b, .. }) => a == b,
                _ => false,
            })
        {
            entries.push(entry);
        }
    }
}

impl Import {
    fn add(&mut self, found: Vec<Found>) {
        for found in found {
            if !self
                .found
                .iter()
                .any(|known| known.found.list.id == found.list.id)
            {
                self.found.push(Candidate {
                    found,
                    picked: true,
                });
            }
        }
    }
}

/// A group whisper's target in words, with the group's name when the server has said it.
pub(crate) fn group_title(state: &client::Snapshot, group: &Group) -> String {
    let named = |groups: &[client::Group], kind: &str| {
        groups
            .iter()
            .find(|known| known.id == group.id)
            .map_or_else(
                || format!("{kind} {}", group.id),
                |known| format!("{kind} {}", known.name),
            )
    };
    let who = match group.kind {
        GroupKind::ServerGroup => named(&state.server_groups, "Server group"),
        GroupKind::ChannelGroup => named(&state.channel_groups, "Channel group"),
        kind => kind.title().to_owned(),
    };
    match group.scope {
        Scope::Everywhere => who,
        scope => format!("{who} · {}", scope.title().to_lowercase()),
    }
}

pub(crate) fn channel_nodes(channels: &[client::Channel]) -> Vec<whisper::ChannelNode> {
    channels
        .iter()
        .map(|channel| whisper::ChannelNode {
            id: channel.id,
            parent: channel.parent,
            name: channel.name.clone(),
        })
        .collect()
}

/// The server's channels and people, and your own client ID.
pub(crate) fn nodes(
    state: &client::Snapshot,
) -> (Vec<whisper::ChannelNode>, Vec<whisper::ClientNode>, u16) {
    let mut own = 0;
    let clients = state
        .channels
        .iter()
        .flat_map(|channel| {
            channel
                .members
                .iter()
                .map(move |member| (channel.id, member))
        })
        .map(|(channel, member)| {
            if member.is_self {
                own = member.id;
            }
            whisper::ClientNode {
                id: member.id,
                uid: member.uid.clone(),
                channel,
            }
        })
        .collect();
    (channel_nodes(&state.channels), clients, own)
}
