//! Tauri plugin that hosts the openskele workbench core and exposes it to a
//! webview. The webview only renders: every command here forwards to
//! [`skele_core::Workbench`], and state changes flow back over a
//! [`tauri::ipc::Channel`].

mod chrome;
mod commands;
mod menu;

use std::path::PathBuf;

use skele_core::plugin::ON_STARTUP;
use skele_core::{HostEvent, Workbench, WorkbenchOptions};
use tauri::plugin::TauriPlugin;
use tauri::{Manager, RunEvent, WebviewWindowBuilder, Wry};
use tokio::sync::broadcast::error::RecvError;

use crate::chrome::Chrome;

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
            commands::terminal_attach,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_close,
            commands::document_open,
            commands::document_set_text,
            commands::document_save,
            commands::output_channels,
            commands::output_read,
            commands::output_clear,
            commands::layout_set_active_container,
            commands::layout_set_active_panel,
            commands::layout_close_panel,
            commands::chrome,
            commands::window_start_drag,
            commands::window_minimize,
            commands::window_toggle_maximize,
            commands::window_close,
        ])
        // `serde_json::Value` carries 64-bit integers, which Specta refuses to
        // type by default. They cross the IPC boundary as JSON numbers anyway.
        .dangerously_cast_bigints_to_number()
}

#[derive(Default)]
pub struct Builder {
    plugin_dirs: Vec<PathBuf>,
    workspace: Option<PathBuf>,
    log_dir: Option<PathBuf>,
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

    /// Writes output channels to files under this directory, whatever the
    /// `output.*` settings say.
    pub fn log_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = Some(dir.into());
        self
    }

    /// Builds the plugin. It creates every window that `tauri.conf.json`
    /// declares with `"create": false`, framed according to the
    /// `window.titleBarStyle` setting; windows Tauri creates itself keep the
    /// system frame.
    pub fn build(self) -> TauriPlugin<Wry> {
        let Self { mut plugin_dirs, workspace, log_dir } = self;
        let specta = specta_builder();

        tauri::plugin::Builder::new(PLUGIN_NAME)
            .invoke_handler(specta.invoke_handler())
            .setup(move |app, _api| {
                let workbench = Workbench::new(WorkbenchOptions {
                    user_settings: Some(app.path().app_config_dir()?.join("settings.json")),
                    workspace,
                    log_dir,
                })?;
                plugin_dirs.push(app.path().app_data_dir()?.join("plugins"));
                for dir in &plugin_dirs {
                    workbench.load_plugins(dir);
                }
                app.manage(workbench.clone());
                app.manage(commands::Sessions::default());

                chrome::register_setting(&workbench)?;
                let chrome = Chrome::from_settings(&workbench);
                app.manage(chrome);

                if chrome.native_menu() {
                    menu::install(app, &workbench)?;

                    // Contributions can change after startup; keep the native menu in step.
                    let (handle, events_of) = (app.clone(), workbench.clone());
                    tauri::async_runtime::spawn(async move {
                        let mut events = events_of.bus().subscribe();
                        loop {
                            match events.recv().await {
                                Ok(HostEvent::ContributionsChanged) => menu::refresh(&handle, &events_of),
                                Ok(_) | Err(RecvError::Lagged(_)) => {}
                                Err(RecvError::Closed) => break,
                            }
                        }
                    });
                }

                // Windows marked `"create": false` in tauri.conf.json are ours to
                // create, so the title bar style can be part of their creation.
                // Tauri holds its plugin lock while this hook runs and needs it
                // again to create a window, so creation happens from a task.
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    for config in handle.config().app.windows.iter().filter(|window| !window.create) {
                        let created = WebviewWindowBuilder::from_config(&handle, config)
                            .and_then(|builder| chrome.frame(builder).build());
                        if let Err(e) = created {
                            eprintln!("could not create window {}: {e}", config.label);
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
