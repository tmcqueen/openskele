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
    /// Text was written to an output channel.
    OutputAppended {
        channel: String,
        text: String,
    },
    /// An output channel was created; re-list them.
    OutputChannelsChanged,
    /// An output channel was emptied.
    OutputCleared {
        channel: String,
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
    /// One transcript entry of an agent session changed or was added; same index means replace.
    AgentUpdate {
        session: String,
        index: u32,
        entry: crate::agent::Entry,
    },
    /// An agent session ended: closed, or the agent exited.
    AgentSessionEnded {
        session: String,
        reason: String,
    },
    PluginState {
        plugin_id: String,
        state: PluginState,
    },
    /// A document's text was replaced by someone other than its editor (an
    /// agent); open editors should reload it.
    DocumentChanged {
        uri: String,
        version: u32,
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
