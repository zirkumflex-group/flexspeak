use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, bail};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
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
    pub fn parse(&self) -> Result<Vec<HotKey>> {
        let mute: HotKey = self
            .toggle_mute
            .trim()
            .parse()
            .context("Invalid mute shortcut")?;
        let deafen: HotKey = self
            .toggle_deafen
            .trim()
            .parse()
            .context("Invalid deafen shortcut")?;
        if mute.id() == deafen.id() {
            bail!("Mute, deafen and push-to-talk must use different shortcuts.");
        }
        let mut keys = vec![mute, deafen];
        if !self.push_to_talk.trim().is_empty() {
            let ptt: HotKey = self
                .push_to_talk
                .trim()
                .parse()
                .context("Invalid push-to-talk shortcut")?;
            if keys.iter().any(|key| key.id() == ptt.id()) {
                bail!("Mute, deafen and push-to-talk must use different shortcuts.");
            }
            keys.push(ptt);
        }
        Ok(keys)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Mute,
    Deafen,
    PushToTalk(bool),
}

#[derive(Default)]
struct Edges {
    ids: Vec<u32>,
    down: [bool; 3],
}

impl Edges {
    fn bind(&mut self, ids: Vec<u32>) {
        let mut down = [false; 3];
        for (index, id) in ids.iter().enumerate() {
            down[index] = self
                .ids
                .iter()
                .position(|old| old == id)
                .is_some_and(|old| self.down[old]);
        }
        self.ids = ids;
        self.down = down;
    }

    fn event(&mut self, event: GlobalHotKeyEvent) -> Option<Action> {
        let index = self.ids.iter().position(|id| *id == event.id)?;
        let pressed = event.state == HotKeyState::Pressed;
        let was_down = std::mem::replace(&mut self.down[index], pressed);
        if index == 2 {
            return (pressed != was_down || (cfg!(windows) && pressed))
                .then_some(Action::PushToTalk(pressed));
        }
        // Windows uses MOD_NOREPEAT; its synthetic release can arrive after a fast second tap.
        (pressed && (cfg!(windows) || !was_down)).then_some(if index == 0 {
            Action::Mute
        } else {
            Action::Deafen
        })
    }
}

pub struct Shortcuts {
    manager: Option<GlobalHotKeyManager>,
    bindings: Vec<HotKey>,
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
                edges,
                ptt,
            },
            receiver,
        )
    }

    pub fn configure(&mut self, keys: &Keybinds) -> Result<()> {
        let next = keys.parse()?;
        if !supported() {
            bail!("Global shortcuts currently support Linux X11, not native Wayland.");
        }
        if self.bindings.as_slice() == next {
            return Ok(());
        }
        if self.manager.is_none() {
            self.manager =
                Some(GlobalHotKeyManager::new().context("Cannot initialize global shortcuts")?);
        }
        let manager = self.manager.as_ref().unwrap();
        let added: Vec<_> = next
            .iter()
            .copied()
            .filter(|key| !self.bindings.contains(key))
            .collect();
        // Register additions before releasing old keys, preserving working bindings on conflicts.
        for (index, key) in added.iter().enumerate() {
            if let Err(error) = manager.register(*key) {
                for registered in &added[..index] {
                    if manager.unregister(*registered).is_err() {
                        self.disable();
                        bail!(
                            "Shortcut registration failed and cleanup failed. Global shortcuts are disabled; try saving again. {error}"
                        );
                    }
                }
                bail!("Cannot register {key}: {error}. Previous shortcuts are unchanged.");
            }
        }
        for key in self.bindings.iter().filter(|key| !next.contains(key)) {
            if let Err(error) = manager.unregister(*key) {
                self.disable();
                bail!(
                    "Cannot release an old shortcut: {error}. Global shortcuts are disabled; try saving again."
                );
            }
        }
        self.edges
            .lock()
            .unwrap()
            .bind(next.iter().map(HotKey::id).collect());
        self.ptt.store(false, Ordering::Release);
        self.bindings = next.to_vec();
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.bindings.len() >= 2
    }

    pub fn has_ptt(&self) -> bool {
        self.bindings.len() == 3
    }

    fn disable(&mut self) {
        self.ptt.store(false, Ordering::Release);
        *self.edges.lock().unwrap() = Edges::default();
        self.bindings.clear();
        self.manager = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn a_held_key_toggles_once_and_unrelated_keys_are_ignored() {
        let mut edges = Edges {
            ids: vec![10, 20, 30],
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
        edges.bind(vec![20, 10, 30]);
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
        edges.bind(vec![10, 20, 30]);
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
        edges.bind(vec![10, 20]);
        assert_eq!(edges.event(event(HotKeyState::Released)), None);
    }
}
