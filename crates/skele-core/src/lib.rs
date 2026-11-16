//! Headless core of the openskele workbench.
//!
//! Everything stateful lives here and nothing in this crate knows about Tauri
//! or a webview: registries for contributions, the event bus, configuration,
//! the sidecar plugin host, PTY terminals and document buffers. A front end
//! (the Tauri plugin, a test, a CLI) drives it through [`Workbench`].

pub mod commands;
pub mod config;
pub mod containers;
pub mod document;
pub mod error;
pub mod events;
pub mod grants;
pub mod keybindings;
pub mod layout;
pub mod mcp;
pub mod menus;
pub mod output;
pub mod plugin;
pub mod pty;
pub mod tools;
pub mod views;
mod workbench;

pub use error::{Error, Result};
pub use events::{EventBus, HostEvent};
pub use workbench::{Snapshot, Workbench, WorkbenchOptions};

pub use skele_protocol as protocol;
