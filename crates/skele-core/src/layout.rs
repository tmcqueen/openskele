//! The workbench layout: both app bars and the open panels. The core owns it;
//! front ends render it and send gestures back as calls.

use serde::Serialize;
use skele_protocol::manifest::BarId;

use crate::containers::ContainerInfo;
use crate::events::HostEvent;
use crate::pty::TerminalOptions;
use crate::{Error, Result, Workbench};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum BarPosition {
    Left,
    Right,
    Top,
}

impl BarPosition {
    /// Parses a `workbench.*Bar.position` setting value.
    pub fn from_setting(value: &str) -> Option<Self> {
        match value {
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub position: BarPosition,
    pub visible: bool,
    /// The container whose side panel is open; `None` collapses the panel.
    pub active_container: Option<String>,
}

/// Something open in the main area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Panel {
    /// The log of plugin output and host messages.
    Output,
    Settings,
    Editor {
        path: String,
    },
    Terminal {
        id: u32,
    },
}

impl Panel {
    /// Stable id: `settings`, `editor:<path>` or `terminal:<id>`.
    pub fn id(&self) -> String {
        match self {
            Panel::Output => "output".to_owned(),
            Panel::Settings => "settings".to_owned(),
            Panel::Editor { path } => format!("editor:{path}"),
            Panel::Terminal { id } => format!("terminal:{id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub primary: Bar,
    pub secondary: Bar,
    /// In the order they were opened.
    pub panels: Vec<Panel>,
    pub active_panel: Option<String>,
    /// Per bar (primary, secondary): whether a container was ever chosen for
    /// it, collapsed included. Until then the bar takes the first container
    /// that can show something.
    #[serde(skip)]
    chosen: [bool; 2],
}

impl Default for Layout {
    fn default() -> Self {
        let bar = |position| Bar { position, visible: true, active_container: None };
        Self {
            primary: bar(BarPosition::Left),
            secondary: bar(BarPosition::Right),
            // The output is open from the start; it can be closed and reopened like any panel.
            panels: vec![Panel::Output],
            active_panel: None,
            chosen: [false; 2],
        }
    }
}

impl Layout {
    pub fn bar(&self, bar: BarId) -> &Bar {
        match bar {
            BarId::Primary => &self.primary,
            BarId::Secondary => &self.secondary,
        }
    }

    pub fn bar_mut(&mut self, bar: BarId) -> &mut Bar {
        match bar {
            BarId::Primary => &mut self.primary,
            BarId::Secondary => &mut self.secondary,
        }
    }

    /// Sets a bar's active container (`None` collapses it) and remembers
    /// that a choice was made.
    pub fn choose(&mut self, bar: BarId, id: Option<String>) {
        self.chosen[bar as usize] = true;
        self.bar_mut(bar).active_container = id;
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.panels.iter().position(|panel| panel.id() == id)
    }

    /// Opens a panel, or reveals it if one with the same id is open, and makes it active.
    pub fn open(&mut self, panel: Panel) {
        let id = panel.id();
        if self.position(&id).is_none() {
            self.panels.push(panel);
        }
        self.active_panel = Some(id);
    }

    /// Closes a panel. If it was active, the one opened before it becomes
    /// active, else the last remaining one.
    pub fn close(&mut self, id: &str) -> Option<Panel> {
        let index = self.position(id)?;
        let panel = self.panels.remove(index);
        if self.active_panel.as_deref() == Some(id) {
            let next = index.checked_sub(1).or(self.panels.len().checked_sub(1));
            self.active_panel = next.map(|i| self.panels[i].id());
        }
        Some(panel)
    }

    /// Makes an open panel active; false if no such panel is open.
    pub fn activate(&mut self, id: &str) -> bool {
        let open = self.position(id).is_some();
        if open {
            self.active_panel = Some(id.to_owned());
        }
        open
    }

    /// Keeps each bar's active container pointing at one of `containers` on
    /// that bar. One that vanished or sits on the other bar is replaced by the
    /// bar's lowest-order container, or `None`. A bar no one has chosen for
    /// yet takes that container too; a chosen `None` (collapsed) is kept.
    /// Returns whether anything changed.
    pub fn fix_containers(&mut self, containers: &[ContainerInfo]) -> bool {
        let mut changed = false;
        for bar_id in [BarId::Primary, BarId::Secondary] {
            let mut on_bar: Vec<_> = containers.iter().filter(|c| c.bar == bar_id).collect();
            on_bar.sort_by(|a, b| (a.order, &a.id).cmp(&(b.order, &b.id)));
            let chosen = self.chosen[bar_id as usize];
            let bar = self.bar_mut(bar_id);
            let replace = match &bar.active_container {
                Some(id) => !on_bar.iter().any(|c| &c.id == id),
                None => !chosen,
            };
            if replace {
                let fallback = on_bar.first().map(|c| c.id.clone());
                if bar.active_container != fallback {
                    bar.active_container = fallback;
                    changed = true;
                }
            }
        }
        changed
    }
}

/// Settings that place the bars.
pub const PRIMARY_POSITION: &str = "workbench.primaryBar.position";
pub const SECONDARY_POSITION: &str = "workbench.secondaryBar.position";

impl Workbench {
    pub fn layout(&self) -> Layout {
        self.inner.layout.lock().unwrap().clone()
    }

    /// Applies `change` and emits `LayoutChanged` if the layout differs afterwards.
    fn update_layout<R>(&self, change: impl FnOnce(&mut Layout) -> R) -> R {
        let (result, changed) = {
            let mut layout = self.inner.layout.lock().unwrap();
            let before = layout.clone();
            let result = change(&mut layout);
            (result, (*layout != before).then(|| layout.clone()))
        };
        if let Some(layout) = changed {
            self.inner.bus.emit(HostEvent::LayoutChanged { layout });
        }
        result
    }

    /// Opens a panel, or reveals the open one with the same id.
    pub fn open_panel(&self, panel: Panel) {
        self.update_layout(|layout| layout.open(panel));
    }

    /// Closes a panel; closing a terminal panel also ends its shell.
    pub fn close_panel(&self, id: &str) -> Result<()> {
        let closed = self.update_layout(|layout| layout.close(id));
        match closed {
            Some(Panel::Terminal { id }) => self.inner.terminals.close(id),
            Some(_) => {}
            None => return Err(Error::UnknownPanel(id.to_owned())),
        }
        Ok(())
    }

    pub fn set_active_panel(&self, id: &str) -> Result<()> {
        if self.update_layout(|layout| layout.activate(id)) {
            Ok(())
        } else {
            Err(Error::UnknownPanel(id.to_owned()))
        }
    }

    /// Opens a container's side panel on `bar`, or collapses the bar's panel with `None`.
    pub fn set_active_container(&self, bar: BarId, id: Option<String>) -> Result<()> {
        if let Some(id) = &id {
            if self.inner.containers.get(id).is_none_or(|c| c.bar != bar) {
                return Err(Error::UnknownContainer(id.clone()));
            }
        }
        self.update_layout(|layout| layout.choose(bar, id));
        Ok(())
    }

    /// Shows a bar if hidden and opens the container's side panel.
    pub(crate) fn focus_container(&self, bar: BarId, id: &str) -> Result<()> {
        if self.inner.containers.get(id).is_none_or(|c| c.bar != bar) {
            return Err(Error::UnknownContainer(id.to_owned()));
        }
        self.update_layout(|layout| {
            layout.bar_mut(bar).visible = true;
            layout.choose(bar, Some(id.to_owned()));
        });
        Ok(())
    }

    /// Starts a shell in the workspace and opens a panel for it.
    pub fn new_terminal(&self) -> Result<u32> {
        let shell = self.inner.config.get("terminal.shell").as_str().map(str::to_owned);
        let cwd = self.inner.workspace.clone();
        let id = self.inner.terminals.open(TerminalOptions { shell, cwd, cols: 80, rows: 24 })?;
        self.open_panel(Panel::Terminal { id });
        Ok(id)
    }

    pub fn toggle_bar(&self, bar: BarId) {
        self.update_layout(|layout| {
            let bar = layout.bar_mut(bar);
            bar.visible = !bar.visible;
        });
    }

    /// Re-reads both position settings into the layout.
    pub(crate) fn apply_bar_settings(&self) {
        let read = |key| self.inner.config.get(key).as_str().and_then(BarPosition::from_setting);
        let (primary, secondary) = (read(PRIMARY_POSITION), read(SECONDARY_POSITION));
        self.update_layout(|layout| {
            if let Some(position) = primary {
                layout.primary.position = position;
            }
            if let Some(position) = secondary {
                layout.secondary.position = position;
            }
        });
    }

    /// Repairs active containers after containers or views were added or
    /// removed. Only containers with at least one view can be active.
    pub(crate) fn fix_layout_containers(&self) {
        let views = self.inner.views.list();
        let mut containers = self.inner.containers.list();
        containers.retain(|c| views.iter().any(|v| v.container == c.id));
        self.update_layout(|layout| layout.fix_containers(&containers));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::ContainerInfo;
    use skele_protocol::manifest::BarId;

    fn editor(path: &str) -> Panel {
        Panel::Editor { path: path.to_owned() }
    }

    /// A layout with nothing open, so tests see only the panels they open.
    fn empty() -> Layout {
        Layout { panels: Vec::new(), ..Layout::default() }
    }

    fn container(id: &str, bar: BarId, order: i32) -> ContainerInfo {
        ContainerInfo { id: id.into(), title: id.into(), icon: "x".into(), bar, order, source: None }
    }

    #[test]
    fn open_same_file_twice_keeps_one_panel() {
        let mut layout = empty();
        layout.open(editor("/a"));
        layout.open(editor("/a"));
        assert_eq!(layout.panels.len(), 1);
        assert_eq!(layout.active_panel.as_deref(), Some("editor:/a"));
    }

    #[test]
    fn open_activates() {
        let mut layout = empty();
        layout.open(Panel::Settings);
        layout.open(editor("/a"));
        assert_eq!(layout.active_panel.as_deref(), Some("editor:/a"));
        layout.open(Panel::Settings);
        assert_eq!(layout.active_panel.as_deref(), Some("settings"));
        assert_eq!(layout.panels.len(), 2);
    }

    #[test]
    fn closing_active_activates_previous() {
        let mut layout = empty();
        layout.open(Panel::Settings);
        layout.open(editor("/e1"));
        layout.open(editor("/e2"));
        assert!(layout.activate("editor:/e1"));
        assert_eq!(layout.close("editor:/e1"), Some(editor("/e1")));
        assert_eq!(layout.active_panel.as_deref(), Some("settings"));
        layout.close("settings");
        assert_eq!(layout.active_panel.as_deref(), Some("editor:/e2"));
        layout.close("editor:/e2");
        assert_eq!(layout.active_panel, None);
    }

    #[test]
    fn closing_inactive_keeps_active() {
        let mut layout = empty();
        layout.open(Panel::Settings);
        layout.open(editor("/a"));
        layout.close("settings");
        assert_eq!(layout.active_panel.as_deref(), Some("editor:/a"));
        assert_eq!(layout.close("nope"), None);
    }

    #[test]
    fn activate_unknown_is_false() {
        let mut layout = empty();
        assert!(!layout.activate("settings"));
        assert_eq!(layout.active_panel, None);
    }

    #[test]
    fn fix_containers_falls_back_by_order() {
        let mut layout = Layout::default();
        layout.primary.active_container = Some("gone".into());
        let containers = [container("b", BarId::Primary, 2), container("a", BarId::Primary, 1)];
        assert!(layout.fix_containers(&containers));
        assert_eq!(layout.primary.active_container.as_deref(), Some("a"));
        assert!(!layout.fix_containers(&containers));

        assert!(layout.fix_containers(&[]));
        assert_eq!(layout.primary.active_container, None);
    }

    #[test]
    fn fix_containers_fills_unchosen_bar() {
        let mut layout = Layout::default();
        assert!(layout.fix_containers(&[container("b", BarId::Primary, 2), container("a", BarId::Primary, 1)]));
        assert_eq!(layout.primary.active_container.as_deref(), Some("a"));
    }

    #[test]
    fn fix_containers_keeps_collapsed() {
        let mut layout = Layout::default();
        layout.choose(BarId::Primary, None);
        assert!(!layout.fix_containers(&[container("a", BarId::Primary, 1)]));
        assert_eq!(layout.primary.active_container, None);
    }

    #[test]
    fn fix_containers_keeps_a_valid_choice() {
        let mut layout = Layout::default();
        let containers = [container("a", BarId::Primary, 1), container("b", BarId::Primary, 2)];
        layout.primary.active_container = Some("b".into());
        assert!(!layout.fix_containers(&containers));
        assert_eq!(layout.primary.active_container.as_deref(), Some("b"));
    }

    #[test]
    fn fix_containers_ignores_other_bar() {
        let mut layout = Layout::default();
        let containers = [container("p", BarId::Primary, 0), container("s", BarId::Secondary, 0)];
        layout.primary.active_container = Some("s".into());
        layout.fix_containers(&containers);
        assert_eq!(layout.primary.active_container.as_deref(), Some("p"));
        assert_eq!(layout.secondary.active_container.as_deref(), Some("s"));
    }

    #[test]
    fn panel_ids() {
        assert_eq!(Panel::Terminal { id: 3 }.id(), "terminal:3");
        assert_eq!(Panel::Settings.id(), "settings");
        assert_eq!(Panel::Output.id(), "output");
        assert_eq!(editor("/a").id(), "editor:/a");
    }

    #[test]
    fn panel_serializes_tagged() {
        let value = serde_json::to_value(editor("/a")).unwrap();
        assert_eq!(value, serde_json::json!({ "kind": "editor", "path": "/a" }));
    }

    #[test]
    fn output_is_open_by_default() {
        assert_eq!(Layout::default().panels, vec![Panel::Output]);
    }

    #[test]
    fn default_bars() {
        let layout = Layout::default();
        assert_eq!(layout.bar(BarId::Primary).position, BarPosition::Left);
        assert_eq!(layout.bar(BarId::Secondary).position, BarPosition::Right);
        assert!(layout.primary.visible && layout.secondary.visible);
    }
}
