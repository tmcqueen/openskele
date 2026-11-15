//! Tauri plugin that hosts the openskele workbench core and exposes it to a
//! webview. The webview only renders: every command here forwards to
//! [`skele_core::Workbench`], and state changes flow back over a
//! [`tauri::ipc::Channel`].

mod commands;
mod menu;

use std::path::PathBuf;

use skele_core::plugin::ON_STARTUP;
use skele_core::{HostEvent, Workbench, WorkbenchOptions};
use tauri::plugin::TauriPlugin;
use tauri::{Manager, RunEvent, Wry};

pub use skele_core as core;

const PLUGIN_NAME: &str = "skele";

/// The typed command surface. Also used to export the TypeScript bindings.
///
/// Fixed to the Wry runtime: commands that take the calling `Webview` cannot
/// be generic over the runtime and still be collected here.
pub fn specta_builder() -> tauri_specta::Builder<Wry> {
    tauri_specta::Builder::<Wry>::new()
        .plugin_name(PLUGIN_NAME)
        .commands(tauri_specta::collect_commands![
            commands::snapshot,
            commands::subscribe,
            commands::execute_command,
            commands::view_children,
            commands::config_sections,
            commands::config_set,
            commands::terminal_open,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_close,
            commands::document_open,
            commands::document_set_text,
            commands::document_save,
        ])
        // `serde_json::Value` carries 64-bit integers, which Specta refuses to
        // type by default. They cross the IPC boundary as JSON numbers anyway.
        .dangerously_cast_bigints_to_number()
}

#[derive(Default)]
pub struct Builder {
    plugin_dirs: Vec<PathBuf>,
    workspace: Option<PathBuf>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a directory whose subdirectories are installed as plugins, in
    /// addition to `plugins/` under the app's data directory.
    pub fn plugin_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.plugin_dirs.push(dir.into());
        self
    }

    /// Opens a workspace folder, enabling workspace-scope settings.
    pub fn workspace(mut self, dir: impl Into<PathBuf>) -> Self {
        self.workspace = Some(dir.into());
        self
    }

    pub fn build(self) -> TauriPlugin<Wry> {
        let Self { mut plugin_dirs, workspace } = self;
        let specta = specta_builder();

        tauri::plugin::Builder::new(PLUGIN_NAME)
            .invoke_handler(specta.invoke_handler())
            .setup(move |app, _api| {
                let workbench = Workbench::new(WorkbenchOptions {
                    user_settings: Some(app.path().app_config_dir()?.join("settings.json")),
                    workspace,
                })?;
                plugin_dirs.push(app.path().app_data_dir()?.join("plugins"));
                for dir in &plugin_dirs {
                    workbench.load_plugins(dir);
                }
                app.manage(workbench.clone());
                app.manage(commands::Sessions::default());

                menu::install(app, &workbench)?;

                // Contributions can change after startup; keep the native menu in step.
                let (handle, events_of) = (app.clone(), workbench.clone());
                tauri::async_runtime::spawn(async move {
                    let mut events = events_of.bus().subscribe();
                    loop {
                        match events.recv().await {
                            Ok(HostEvent::ContributionsChanged) => menu::refresh(&handle, &events_of),
                            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });

                tauri::async_runtime::spawn(async move { workbench.fire_activation_event(ON_STARTUP) });
                Ok(())
            })
            .on_event(|app, event| {
                if let RunEvent::Exit = event {
                    let workbench = app.state::<Workbench>().inner().clone();
                    tauri::async_runtime::block_on(workbench.shutdown());
                }
            })
            .build()
    }
}
