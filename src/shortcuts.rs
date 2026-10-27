use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, bail};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Keybinds {
    pub toggle_mute: String,
    pub toggle_deafen: String,
}

impl Default for Keybinds {
    fn default() -> Self {
        Self {
            toggle_mute: "Ctrl+Shift+M".into(),
            toggle_deafen: "Ctrl+Shift+D".into(),
        }
    }
}

impl Keybinds {
    pub fn parse(&self) -> Result<[HotKey; 2]> {
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
            bail!("Mute and deafen must use different shortcuts.");
        }
        Ok([mute, deafen])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Mute,
    Deafen,
}

#[derive(Default)]
struct Edges {
    ids: Option<[u32; 2]>,
    down: [bool; 2],
}

impl Edges {
    fn bind(&mut self, ids: [u32; 2]) {
        let down = ids.map(|id| {
            self.ids
                .and_then(|old| old.iter().position(|old| *old == id))
                .is_some_and(|index| self.down[index])
        });
        self.ids = Some(ids);
        self.down = down;
    }

    fn event(&mut self, event: GlobalHotKeyEvent) -> Option<Action> {
        let index = self.ids?.iter().position(|id| *id == event.id)?;
        let pressed = event.state == HotKeyState::Pressed;
        let was_down = std::mem::replace(&mut self.down[index], pressed);
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
}

pub fn supported() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("WAYLAND_DISPLAY").is_none()
}

impl Shortcuts {
    /// Called once on the native event-loop thread, before registering any keys.
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Action>) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let edges = Arc::new(Mutex::new(Edges::default()));
        let callback_edges = edges.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |event| {
            if let Ok(mut edges) = callback_edges.lock()
                && let Some(action) = edges.event(event)
            {
                // Human key presses wake the UI directly; no idle polling or repeat messages.
                let _ = sender.send(action);
            }
        }));
        (
            Self {
                manager: None,
                bindings: Vec::new(),
                edges,
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
            .bind([next[0].id(), next[1].id()]);
        self.bindings = next.to_vec();
        Ok(())
    }

    pub fn is_active(&self) -> bool {
        self.bindings.len() == 2
    }

    fn disable(&mut self) {
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
                toggle_deafen: "F9".into()
            }
            .parse()
            .is_ok()
        );
    }

    #[test]
    fn a_held_key_toggles_once_and_unrelated_keys_are_ignored() {
        let mut edges = Edges {
            ids: Some([10, 20]),
            down: [false; 2],
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
        edges.bind([20, 10]);
        #[cfg(not(windows))]
        assert_eq!(edges.event(event(10, HotKeyState::Pressed)), None);
        assert_eq!(edges.event(event(10, HotKeyState::Released)), None);
        assert_eq!(
            edges.event(event(10, HotKeyState::Pressed)),
            Some(Action::Deafen)
        );
    }
}
