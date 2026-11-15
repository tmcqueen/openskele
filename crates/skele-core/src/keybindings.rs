use std::sync::RwLock;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Keybinding {
    /// Chord such as `Mod+Shift+P`. `Mod` is Cmd on macOS and Ctrl elsewhere.
    pub key: String,
    pub command: String,
    pub source: Option<String>,
}

#[derive(Default)]
pub struct KeybindingRegistry {
    bindings: RwLock<Vec<Keybinding>>,
}

impl KeybindingRegistry {
    pub fn register(&self, binding: Keybinding) {
        self.bindings.write().unwrap().push(binding);
    }

    pub fn list(&self) -> Vec<Keybinding> {
        self.bindings.read().unwrap().clone()
    }

    /// The most recently registered chord for a command, if any.
    pub fn for_command(&self, command: &str) -> Option<String> {
        let bindings = self.bindings.read().unwrap();
        bindings.iter().rev().find(|b| b.command == command).map(|b| b.key.clone())
    }

    pub fn remove_source(&self, source: &str) {
        self.bindings.write().unwrap().retain(|b| b.source.as_deref() != Some(source));
    }
}
