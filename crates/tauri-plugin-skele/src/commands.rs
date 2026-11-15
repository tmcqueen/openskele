//! IPC surface for the webview. Thin by design: argument plumbing only.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;
use skele_core::config::{ConfigSection, Scope};
use skele_core::document::DocumentSnapshot;
use skele_core::protocol::view::TreeItem;
use skele_core::pty::{TerminalEvent, TerminalOptions};
use skele_core::{HostEvent, Snapshot, Workbench};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::{State, Webview, Window};
use tokio::sync::broadcast::error::RecvError;

use crate::chrome::Chrome;

type Result<T> = std::result::Result<T, String>;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// What each webview holds open, so a page reload does not leak the previous
/// page's event stream and shells.
#[derive(Default)]
pub struct Sessions(Mutex<HashMap<String, Session>>);

#[derive(Default)]
pub struct Session {
    events: Option<JoinHandle<()>>,
    terminals: Vec<u32>,
}

#[tauri::command]
#[specta::specta]
pub fn snapshot(workbench: State<'_, Workbench>) -> Snapshot {
    workbench.snapshot()
}

/// Streams host events to the calling webview. A page calls this once when it
/// loads, which also releases whatever that webview's previous page held.
#[tauri::command]
#[specta::specta]
pub fn subscribe(
    webview: Webview,
    workbench: State<'_, Workbench>,
    sessions: State<'_, Sessions>,
    on_event: Channel<HostEvent>,
) {
    let mut events = workbench.bus().subscribe();
    let forward = tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    if on_event.send(event).is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });

    let session = Session { events: Some(forward), terminals: Vec::new() };
    let previous = sessions.0.lock().unwrap().insert(webview.label().to_owned(), session);
    if let Some(previous) = previous {
        if let Some(events) = previous.events {
            events.abort();
        }
        for id in previous.terminals {
            workbench.terminals().close(id);
        }
    }
}

#[tauri::command]
#[specta::specta]
pub async fn execute_command(workbench: State<'_, Workbench>, id: String, args: Vec<Value>) -> Result<Value> {
    workbench.execute_command(&id, args).await.map_err(message)
}

#[tauri::command]
#[specta::specta]
pub async fn view_children(
    workbench: State<'_, Workbench>,
    view_id: String,
    parent: Option<String>,
) -> Result<Vec<TreeItem>> {
    workbench.view_children(&view_id, parent).await.map_err(message)
}

#[tauri::command]
#[specta::specta]
pub fn config_sections(workbench: State<'_, Workbench>) -> Vec<ConfigSection> {
    workbench.config().sections()
}

/// Sets a setting in one scope; a `null` value clears it there.
#[tauri::command]
#[specta::specta]
pub fn config_set(workbench: State<'_, Workbench>, key: String, value: Option<Value>, scope: Scope) -> Result<()> {
    workbench.set_config(&key, value, scope).map_err(message)
}

/// Starts a shell on a PTY and streams its output over `on_event`.
#[tauri::command]
#[specta::specta]
pub fn terminal_open(
    webview: Webview,
    workbench: State<'_, Workbench>,
    sessions: State<'_, Sessions>,
    cols: u16,
    rows: u16,
    on_event: Channel<TerminalEvent>,
) -> Result<u32> {
    let shell = workbench.config().get("terminal.shell").as_str().map(str::to_owned);
    let options = TerminalOptions { shell, cwd: workbench.workspace().map(Path::to_owned), cols, rows };
    let id = workbench.terminals().open(options, move |event| drop(on_event.send(event))).map_err(message)?;
    sessions.0.lock().unwrap().entry(webview.label().to_owned()).or_default().terminals.push(id);
    Ok(id)
}

#[tauri::command]
#[specta::specta]
pub fn terminal_write(workbench: State<'_, Workbench>, id: u32, data: String) -> Result<()> {
    workbench.terminals().write(id, &data).map_err(message)
}

#[tauri::command]
#[specta::specta]
pub fn terminal_resize(workbench: State<'_, Workbench>, id: u32, cols: u16, rows: u16) -> Result<()> {
    workbench.terminals().resize(id, cols, rows).map_err(message)
}

#[tauri::command]
#[specta::specta]
pub fn terminal_close(workbench: State<'_, Workbench>, id: u32) {
    workbench.terminals().close(id);
}

#[tauri::command]
#[specta::specta]
pub fn document_open(workbench: State<'_, Workbench>, path: String) -> Result<DocumentSnapshot> {
    workbench.documents().open(Path::new(&path)).map_err(message)
}

#[tauri::command]
#[specta::specta]
pub fn document_set_text(workbench: State<'_, Workbench>, uri: String, text: String) -> Result<u32> {
    workbench.documents().set_text(&uri, &text).map_err(message)
}

#[tauri::command]
#[specta::specta]
pub fn document_save(workbench: State<'_, Workbench>, uri: String) -> Result<u32> {
    workbench.save_document(&uri).map_err(message)
}

/// The title bar style in effect for this run.
#[tauri::command]
#[specta::specta]
pub fn chrome(chrome: State<'_, Chrome>) -> Chrome {
    *chrome
}

// The custom title bar's controls. They act on the calling window, and going
// through the workbench plugin keeps the webview from needing window permissions.

#[tauri::command]
#[specta::specta]
pub fn window_start_drag(window: Window) -> Result<()> {
    window.start_dragging().map_err(message)
}

#[tauri::command]
#[specta::specta]
pub fn window_minimize(window: Window) -> Result<()> {
    window.minimize().map_err(message)
}

#[tauri::command]
#[specta::specta]
pub fn window_toggle_maximize(window: Window) -> Result<()> {
    if window.is_maximized().map_err(message)? {
        window.unmaximize().map_err(message)
    } else {
        window.maximize().map_err(message)
    }
}

#[tauri::command]
#[specta::specta]
pub fn window_close(window: Window) -> Result<()> {
    window.close().map_err(message)
}
