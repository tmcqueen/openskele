//! Broadcast bus for everything a front end may want to react to.

use serde::Serialize;
use skele_protocol::methods::MessageLevel;
use tokio::sync::broadcast;

use crate::layout::Layout;
use crate::plugin::PluginState;

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HostEvent {
    /// Commands, menus, keybindings, views or config schemas changed; re-fetch the snapshot.
    ContributionsChanged,
    Message {
        level: MessageLevel,
        message: String,
        source: Option<String>,
    },
    /// Text for a named output terminal.
    TerminalData {
        terminal: String,
        data: String,
    },
    ViewRefresh {
        view_id: String,
    },
    ConfigChanged {
        keys: Vec<String>,
    },
    /// The bars or open panels changed; this is the whole new layout.
    LayoutChanged {
        layout: Layout,
    },
    /// Open the command palette overlay.
    ShowCommandPalette,
    PluginState {
        plugin_id: String,
        state: PluginState,
    },
    DocumentSaved {
        uri: String,
        version: u32,
    },
}

#[derive(Debug, Clone)]
pub struct EventBus {
    tx: broadcast::Sender<HostEvent>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self { tx: broadcast::channel(1024).0 }
    }
}

impl EventBus {
    pub fn emit(&self, event: HostEvent) {
        // No subscribers is fine: events are not replayed.
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HostEvent> {
        self.tx.subscribe()
    }

    pub fn message(&self, level: MessageLevel, message: impl Into<String>, source: Option<&str>) {
        self.emit(HostEvent::Message { level, message: message.into(), source: source.map(str::to_owned) });
    }
}
