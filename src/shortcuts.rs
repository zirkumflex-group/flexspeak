use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, bail};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use rune::{Key, KeyEvent, PhysicalKeyCode};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Keybinds {
    pub toggle_mute: String,
    pub toggle_deafen: String,
    // Older two-key profiles retain their bindings without claiming a new global key.
    #[serde(default)]
    pub push_to_talk: String,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            toggle_mute: "Ctrl+Shift+M".into(),
            toggle_deafen: "Ctrl+Shift+D".into(),
            push_to_talk: "Ctrl+Shift+Space".into(),
        }
    }
}

impl Keybinds {
    /// The keys that are set, each with its action. Any of them may be left empty.
    pub fn parse(&self) -> Result<Vec<(Binding, HotKey)>> {
        let mut keys: Vec<(Binding, HotKey)> = Vec::new();
        for (binding, what) in [
            (Binding::Mute, "mute"),
            (Binding::Deafen, "deafen"),
            (Binding::PushToTalk, "push-to-talk"),
        ] {
            let shortcut = self.get(binding).trim();
            if shortcut.is_empty() {
                continue;
            }
            let key: HotKey = shortcut
                .parse()
                .with_context(|| format!("Invalid {what} shortcut"))?;
            if keys.iter().any(|(_, other)| other.id() == key.id()) {
                bail!("Mute, deafen and push-to-talk must use different shortcuts.");
            }
            keys.push((binding, key));
        }
        Ok(keys)
    }
}

/// Which shortcut a setting or recording is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Mute,
    Deafen,
    PushToTalk,
}

impl Keybinds {
    pub fn get(&self, binding: Binding) -> &str {
        match binding {
            Binding::Mute => &self.toggle_mute,
            Binding::Deafen => &self.toggle_deafen,
            Binding::PushToTalk => &self.push_to_talk,
        }
    }

    pub fn with(&self, binding: Binding, shortcut: String) -> Self {
        let mut next = self.clone();
        match binding {
            Binding::Mute => next.toggle_mute = shortcut,
            Binding::Deafen => next.toggle_deafen = shortcut,
            Binding::PushToTalk => next.push_to_talk = shortcut,
        }
        next
    }
}

/// What a key press means while a shortcut is being recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    /// Nothing to act on yet, such as a key with no name.
    Ignored,
    Cancel,
    Clear,
    Shortcut(String),
}

/// The key's name in the shortcut grammar. The physical key is the same whatever the keyboard
/// layout and whichever modifiers are held, unlike the character it typed.
fn key_name(key: &KeyEvent, physical: Option<PhysicalKeyCode>) -> Option<String> {
    if let Some(physical) = physical {
        let name = format!("{physical:?}");
        let short = name
            .strip_prefix("Key")
            .or_else(|| name.strip_prefix("Digit"))
            .filter(|rest| rest.len() == 1);
        return Some(short.unwrap_or(&name).to_owned());
    }
    let character = match &key.key {
        Key::Function(number) => return Some(format!("F{number}")),
        Key::Enter => return Some("Enter".into()),
        Key::Tab => return Some("Tab".into()),
        Key::Command(character) => *character,
        Key::Text(text) => text.chars().next()?,
        _ => return None,
    };
    match character {
        ' ' => Some("Space".into()),
        character if character.is_ascii_alphanumeric() => {
            Some(character.to_ascii_uppercase().to_string())
        }
        _ => None,
    }
}

/// Turns one key press into a shortcut like `Ctrl+Shift+M`.
pub fn record(key: &KeyEvent, physical: Option<PhysicalKeyCode>) -> Recorded {
    let mods = key.mods;
    let control = mods.ctrl || (mods.cmd && !cfg!(target_os = "macos"));
    let command = mods.logo || (mods.cmd && cfg!(target_os = "macos"));
    let plain = !(control || command || mods.alt || mods.shift);
    match key.key {
        Key::Modifier { .. } => return Recorded::Ignored,
        Key::Escape if plain => return Recorded::Cancel,
        Key::Backspace | Key::Delete if plain => return Recorded::Clear,
        _ => {}
    }
    let Some(name) = key_name(key, physical) else {
        return Recorded::Ignored;
    };
    let mut parts = Vec::new();
    if control {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    if mods.shift {
        parts.push("Shift");
    }
    if command {
        parts.push(if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Super"
        });
    }
    parts.push(&name);
    Recorded::Shortcut(parts.join("+"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Mute,
    Deafen,
    PushToTalk(bool),
}

#[derive(Default)]
struct Edges {
    ids: Vec<(Binding, u32)>,
    down: [bool; 3],
}

impl Edges {
    fn bind(&mut self, ids: Vec<(Binding, u32)>) {
        let mut down = [false; 3];
        for (index, (_, id)) in ids.iter().enumerate() {
            down[index] = self
                .ids
                .iter()
                .position(|(_, old)| old == id)
                .is_some_and(|old| self.down[old]);
        }
        self.ids = ids;
        self.down = down;
    }

    fn event(&mut self, event: GlobalHotKeyEvent) -> Option<Action> {
        let index = self.ids.iter().position(|(_, id)| *id == event.id)?;
        let binding = self.ids[index].0;
        let pressed = event.state == HotKeyState::Pressed;
        let was_down = std::mem::replace(&mut self.down[index], pressed);
        if binding == Binding::PushToTalk {
            return (pressed != was_down || (cfg!(windows) && pressed))
                .then_some(Action::PushToTalk(pressed));
        }
        // Windows uses MOD_NOREPEAT; its synthetic release can arrive after a fast second tap.
        (pressed && (cfg!(windows) || !was_down)).then_some(if binding == Binding::Mute {
            Action::Mute
        } else {
            Action::Deafen
        })
    }
}

pub struct Shortcuts {
    manager: Option<GlobalHotKeyManager>,
    /// The keys asked for, each with its action; one left empty has none.
    bindings: Vec<(Binding, HotKey)>,
    /// Those of them another app holds. The rest work, and these are tried again until free.
    taken: Vec<HotKey>,
    edges: Arc<Mutex<Edges>>,
    ptt: Arc<AtomicBool>,
}

pub fn supported() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("WAYLAND_DISPLAY").is_none()
}

impl Shortcuts {
    /// Called once on the native event-loop thread, before registering any keys.
    pub fn new(ptt: Arc<AtomicBool>) -> (Self, mpsc::UnboundedReceiver<Action>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let edges = Arc::new(Mutex::new(Edges::default()));
        let callback_edges = edges.clone();
        let callback_ptt = ptt.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event| {
            if let Ok(mut edges) = callback_edges.lock()
                && let Some(action) = edges.event(event)
            {
                // Releases reach the audio worker even if the UI is busy or its queue is backed up.
                if let Action::PushToTalk(pressed) = action {
                    callback_ptt.store(pressed, Ordering::Release);
                }
                // Human key presses wake the UI directly; no idle polling or repeat messages.
                let _ = sender.send(action);
            }
        }));
        (
            Self {
                manager: None,
                bindings: Vec::new(),
                taken: Vec::new(),
                edges,
                ptt,
            },
            receiver,
        )
    }

    pub fn configure(&mut self, keys: &Keybinds) -> Result<()> {
        let next = keys.parse()?;
        if !supported() {
            bail!("Global shortcuts need X11; native Wayland is not supported.");
        }
        if self.bindings == next {
            return Ok(());
        }
        if self.manager.is_none() {
            self.manager =
                Some(GlobalHotKeyManager::new().context("Cannot initialize global shortcuts")?);
        }
        let manager = self.manager.as_ref().unwrap();
        let held: Vec<HotKey> = self
            .bindings
            .iter()
            .map(|(_, key)| *key)
            .filter(|key| !self.taken.contains(key))
            .collect();
        // Release old keys first, so a key moving to another action is free to take again.
        let wanted: Vec<HotKey> = next.iter().map(|(_, key)| *key).collect();
        for key in held.iter().filter(|key| !wanted.contains(key)) {
            if let Err(error) = manager.unregister(*key) {
                self.disable();
                bail!(
                    "Cannot release an old shortcut: {error}. Global shortcuts are disabled; try saving again."
                );
            }
        }
        // A key another app holds doesn't stop the others: it's tried again until it's free.
        let mut taken = Vec::new();
        for key in wanted.iter().filter(|key| !held.contains(key)) {
            if let Err(error) = manager.register(*key) {
                tracing::info!(%key, %error, "Global shortcut is taken");
                taken.push(*key);
            }
        }
        self.edges.lock().unwrap().bind(
            next.iter()
                .map(|(binding, key)| (*binding, key.id()))
                .collect(),
        );
        self.ptt.store(false, Ordering::Release);
        self.bindings = next;
        self.taken = taken;
        Ok(())
    }

    /// Tries again for the keys another app held. True when one was freed.
    pub fn retry(&mut self) -> bool {
        let Some(manager) = &self.manager else {
            return false;
        };
        let before = self.taken.len();
        self.taken.retain(|key| manager.register(*key).is_err());
        self.taken.len() != before
    }

    /// The actions whose keys another app holds.
    pub fn taken(&self) -> Vec<Binding> {
        self.bindings
            .iter()
            .filter(|(_, key)| self.taken.contains(key))
            .map(|(binding, _)| *binding)
            .collect()
    }

    /// Releases every shortcut until the next `configure`, so that pressing one while recording
    /// reaches the window instead of being taken by the system.
    pub fn suspend(&mut self) {
        let released = self.manager.as_ref().is_none_or(|manager| {
            self.bindings
                .iter()
                .filter(|(_, key)| !self.taken.contains(key))
                .all(|(_, key)| manager.unregister(*key).is_ok())
        });
        if !released {
            self.disable();
            return;
        }
        self.ptt.store(false, Ordering::Release);
        self.edges.lock().unwrap().bind(Vec::new());
        self.bindings.clear();
        self.taken.clear();
    }

    /// Whether the shortcuts are registered, as far as any are set.
    pub fn is_active(&self) -> bool {
        self.manager.is_some()
    }

    pub fn has_ptt(&self) -> bool {
        self.bindings
            .iter()
            .any(|(binding, _)| *binding == Binding::PushToTalk)
    }

    fn disable(&mut self) {
        self.ptt.store(false, Ordering::Release);
        *self.edges.lock().unwrap() = Edges::default();
        self.bindings.clear();
        self.taken.clear();
        self.manager = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(key: Key, mods: rune::Mods) -> KeyEvent {
        KeyEvent { key, mods }
    }

    #[test]
    fn recording_names_the_physical_key_with_its_modifiers() {
        let ctrl_shift = rune::Mods {
            ctrl: true,
            shift: true,
            ..rune::Mods::default()
        };
        let shortcut = |key, physical, mods| record(&pressed(key, mods), Some(physical));
        assert_eq!(
            shortcut(Key::Text("\r".into()), PhysicalKeyCode::KeyM, ctrl_shift),
            Recorded::Shortcut("Ctrl+Shift+M".into()),
        );
        // Shift+7 types "/" on a German layout; the shortcut is still the 7 key.
        assert_eq!(
            shortcut(Key::Text("/".into()), PhysicalKeyCode::Digit7, ctrl_shift),
            Recorded::Shortcut("Ctrl+Shift+7".into()),
        );
        let Recorded::Shortcut(spoken) =
            shortcut(Key::Text(" ".into()), PhysicalKeyCode::Space, ctrl_shift)
        else {
            panic!("Space is a key");
        };
        assert_eq!(spoken, "Ctrl+Shift+Space");
        assert!(spoken.parse::<HotKey>().is_ok());
    }

    #[test]
    fn recording_takes_a_single_key_and_keeps_its_ways_out() {
        let plain = rune::Mods::default();
        let single = |key, physical| record(&pressed(key, plain), Some(physical));
        assert_eq!(
            single(Key::Text("m".into()), PhysicalKeyCode::KeyM),
            Recorded::Shortcut("M".into()),
        );
        assert_eq!(
            single(Key::Function(8), PhysicalKeyCode::F8),
            Recorded::Shortcut("F8".into()),
        );
        assert_eq!(
            single(Key::Text(" ".into()), PhysicalKeyCode::Space),
            Recorded::Shortcut("Space".into()),
        );
        for shortcut in ["M", "F8", "Space"] {
            assert!(shortcut.parse::<HotKey>().is_ok());
        }
        assert_eq!(
            single(Key::Escape, PhysicalKeyCode::Escape),
            Recorded::Cancel
        );
        assert_eq!(
            single(Key::Backspace, PhysicalKeyCode::Backspace),
            Recorded::Clear,
        );
    }

    #[test]
    fn rejects_invalid_or_equivalent_shortcuts() {
        assert!(Keybinds::default().parse().is_ok());
        assert!(
            Keybinds {
                push_to_talk: "Ctrl+Shift+M".into(),
                ..Keybinds::default()
            }
            .parse()
            .is_err()
        );
        let old: Keybinds =
            serde_json::from_str(r#"{"toggle_mute":"Ctrl+Shift+Space","toggle_deafen":"F9"}"#)
                .unwrap();
        assert!(old.push_to_talk.is_empty());
        assert_eq!(old.parse().unwrap().len(), 2);
        assert!(
            Keybinds {
                toggle_mute: "Ctrl++M".into(),
                ..Keybinds::default()
            }
            .parse()
            .is_err()
        );
        assert!(
            Keybinds {
                toggle_deafen: "Shift+Control+KeyM".into(),
                ..Keybinds::default()
            }
            .parse()
            .is_err()
        );
        assert!(
            Keybinds {
                toggle_mute: "F8".into(),
                toggle_deafen: "F9".into(),
                ..Keybinds::default()
            }
            .parse()
            .is_ok()
        );
        // Any shortcut may be left empty.
        let cleared = Keybinds {
            toggle_mute: String::new(),
            ..Keybinds::default()
        }
        .parse()
        .unwrap();
        assert_eq!(
            cleared
                .iter()
                .map(|(binding, _)| *binding)
                .collect::<Vec<_>>(),
            [Binding::Deafen, Binding::PushToTalk]
        );
    }

    /// Key ids for mute, deafen and push-to-talk, in that order.
    fn ids(keys: &[u32]) -> Vec<(Binding, u32)> {
        [Binding::Mute, Binding::Deafen, Binding::PushToTalk]
            .into_iter()
            .zip(keys.iter().copied())
            .collect()
    }

    #[test]
    fn a_held_key_toggles_once_and_unrelated_keys_are_ignored() {
        let mut edges = Edges {
            ids: ids(&[10, 20, 30]),
            down: [false; 3],
        };
        let event = |id, state| GlobalHotKeyEvent { id, state };
        assert_eq!(
            edges.event(event(10, HotKeyState::Pressed)),
            Some(Action::Mute)
        );
        #[cfg(not(windows))]
        for _ in 0..20 {
            assert_eq!(edges.event(event(10, HotKeyState::Pressed)), None);
        }
        assert_eq!(edges.event(event(99, HotKeyState::Pressed)), None);
        assert_eq!(edges.event(event(10, HotKeyState::Released)), None);
        assert_eq!(
            edges.event(event(10, HotKeyState::Pressed)),
            Some(Action::Mute)
        );
        assert_eq!(
            edges.event(event(20, HotKeyState::Pressed)),
            Some(Action::Deafen)
        );
        edges.bind(ids(&[20, 10, 30]));
        #[cfg(not(windows))]
        assert_eq!(edges.event(event(10, HotKeyState::Pressed)), None);
        assert_eq!(edges.event(event(10, HotKeyState::Released)), None);
        assert_eq!(
            edges.event(event(10, HotKeyState::Pressed)),
            Some(Action::Deafen)
        );
    }

    #[test]
    fn push_to_talk_emits_both_edges_without_repeated_releases() {
        let mut edges = Edges::default();
        edges.bind(ids(&[10, 20, 30]));
        let event = |state| GlobalHotKeyEvent { id: 30, state };
        assert_eq!(edges.event(event(HotKeyState::Released)), None);
        assert_eq!(
            edges.event(event(HotKeyState::Pressed)),
            Some(Action::PushToTalk(true))
        );
        #[cfg(not(windows))]
        assert_eq!(edges.event(event(HotKeyState::Pressed)), None);
        assert_eq!(
            edges.event(event(HotKeyState::Released)),
            Some(Action::PushToTalk(false))
        );
        assert_eq!(edges.event(event(HotKeyState::Released)), None);
        assert_eq!(
            edges.event(event(HotKeyState::Pressed)),
            Some(Action::PushToTalk(true))
        );
        edges.bind(ids(&[10, 20]));
        assert_eq!(edges.event(event(HotKeyState::Released)), None);
    }
}
