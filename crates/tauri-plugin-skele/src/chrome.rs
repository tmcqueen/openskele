//! Window chrome: whether the title bar is drawn by the webview or the OS.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::json;
use skele_core::Workbench;
use tauri::{Manager, Runtime, WebviewWindowBuilder};

pub const TITLE_BAR_SETTING: &str = "window.titleBarStyle";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum TitleBar {
    /// The webview draws the title bar, menu and window controls.
    Custom,
    /// The OS frame and a native menu bar.
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

/// The chrome in effect. Decided once at startup, so changing the setting
/// takes a restart.
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Chrome {
    pub title_bar: TitleBar,
    pub platform: Platform,
}

pub fn register_setting(workbench: &Workbench) -> skele_core::Result<()> {
    let properties = BTreeMap::from([(
        TITLE_BAR_SETTING.to_owned(),
        json!({
            "type": "string",
            "enum": ["custom", "native"],
            "default": "custom",
            "description": "Draw the title bar and menu in the window, or use the system's. Takes effect after a restart."
        }),
    )]);
    workbench.config().register_section(None, "Window", &properties)
}

impl Chrome {
    pub fn from_settings(workbench: &Workbench) -> Self {
        let title_bar = match workbench.config().get(TITLE_BAR_SETTING).as_str() {
            Some("native") => TitleBar::Native,
            _ => TitleBar::Custom,
        };
        let platform = if cfg!(target_os = "macos") {
            Platform::Macos
        } else if cfg!(target_os = "windows") {
            Platform::Windows
        } else {
            Platform::Linux
        };
        Self { title_bar, platform }
    }

    /// macOS keeps its system menu bar in both styles.
    pub fn native_menu(&self) -> bool {
        self.title_bar == TitleBar::Native || self.platform == Platform::Macos
    }

    /// Applies the title bar style to a window that is about to be created.
    ///
    /// This has to happen at creation: Wayland compositors negotiate
    /// decorations once, so removing them from a live window leaves the
    /// system title bar in place.
    pub fn frame<'a, R: Runtime, M: Manager<R>>(
        &self,
        builder: WebviewWindowBuilder<'a, R, M>,
    ) -> WebviewWindowBuilder<'a, R, M> {
        if self.title_bar == TitleBar::Native {
            return builder;
        }
        // macOS keeps its frame so the traffic lights stay, and lets the
        // webview draw underneath; elsewhere the frame goes entirely.
        #[cfg(target_os = "macos")]
        let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
        #[cfg(not(target_os = "macos"))]
        let builder = builder.decorations(false);
        builder
    }
}
