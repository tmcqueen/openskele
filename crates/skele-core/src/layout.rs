//! The workbench layout: both app bars and the open panels. The core owns it;
//! front ends render it and send gestures back as calls.

use serde::{Deserialize, Serialize};
use skele_protocol::manifest::BarId;

use crate::containers::ContainerInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub position: BarPosition,
    pub visible: bool,
    /// The container whose side panel is open; `None` collapses the panel.
    pub active_container: Option<String>,
}

/// Something open in the main area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Panel {
    Settings,
    Editor { path: String },
    Terminal { id: u32 },
}

impl Panel {
    /// Stable id: `settings`, `editor:<path>` or `terminal:<id>`.
    pub fn id(&self) -> String {
        match self {
            Panel::Settings => "settings".to_owned(),
            Panel::Editor { path } => format!("editor:{path}"),
            Panel::Terminal { id } => format!("terminal:{id}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub primary: Bar,
    pub secondary: Bar,
    /// In the order they were opened.
    pub panels: Vec<Panel>,
    pub active_panel: Option<String>,
}

impl Default for Layout {
    fn default() -> Self {
        let bar = |position| Bar { position, visible: true, active_container: None };
        Self {
            primary: bar(BarPosition::Left),
            secondary: bar(BarPosition::Right),
            panels: Vec::new(),
            active_panel: None,
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

    /// Keeps each bar's active container pointing at a container on that bar:
    /// one that vanished or sits on the other bar is replaced by the bar's
    /// lowest-order container, or `None`. A bar with nothing chosen is
    /// collapsed and stays so. Returns whether anything changed.
    pub fn fix_containers(&mut self, containers: &[ContainerInfo]) -> bool {
        let mut changed = false;
        for bar_id in [BarId::Primary, BarId::Secondary] {
            let mut on_bar: Vec<_> = containers.iter().filter(|c| c.bar == bar_id).collect();
            on_bar.sort_by(|a, b| (a.order, &a.id).cmp(&(b.order, &b.id)));
            let bar = self.bar_mut(bar_id);
            let dangling = bar.active_container.as_ref().is_some_and(|id| !on_bar.iter().any(|c| &c.id == id));
            if dangling {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::ContainerInfo;
    use skele_protocol::manifest::BarId;

    fn editor(path: &str) -> Panel {
        Panel::Editor { path: path.to_owned() }
    }

    fn container(id: &str, bar: BarId, order: i32) -> ContainerInfo {
        ContainerInfo { id: id.into(), title: id.into(), icon: "x".into(), bar, order, source: None }
    }

    #[test]
    fn open_same_file_twice_keeps_one_panel() {
        let mut layout = Layout::default();
        layout.open(editor("/a"));
        layout.open(editor("/a"));
        assert_eq!(layout.panels.len(), 1);
        assert_eq!(layout.active_panel.as_deref(), Some("editor:/a"));
    }

    #[test]
    fn open_activates() {
        let mut layout = Layout::default();
        layout.open(Panel::Settings);
        layout.open(editor("/a"));
        assert_eq!(layout.active_panel.as_deref(), Some("editor:/a"));
        layout.open(Panel::Settings);
        assert_eq!(layout.active_panel.as_deref(), Some("settings"));
        assert_eq!(layout.panels.len(), 2);
    }

    #[test]
    fn closing_active_activates_previous() {
        let mut layout = Layout::default();
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
        let mut layout = Layout::default();
        layout.open(Panel::Settings);
        layout.open(editor("/a"));
        layout.close("settings");
        assert_eq!(layout.active_panel.as_deref(), Some("editor:/a"));
        assert_eq!(layout.close("nope"), None);
    }

    #[test]
    fn activate_unknown_is_false() {
        let mut layout = Layout::default();
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
    fn fix_containers_keeps_collapsed() {
        let mut layout = Layout::default();
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
        assert_eq!(layout.secondary.active_container, None); // collapsed stays collapsed
    }

    #[test]
    fn panel_ids() {
        assert_eq!(Panel::Terminal { id: 3 }.id(), "terminal:3");
        assert_eq!(Panel::Settings.id(), "settings");
        assert_eq!(editor("/a").id(), "editor:/a");
    }

    #[test]
    fn panel_serializes_tagged() {
        let value = serde_json::to_value(editor("/a")).unwrap();
        assert_eq!(value, serde_json::json!({ "kind": "editor", "path": "/a" }));
    }

    #[test]
    fn default_bars() {
        let layout = Layout::default();
        assert_eq!(layout.bar(BarId::Primary).position, BarPosition::Left);
        assert_eq!(layout.bar(BarId::Secondary).position, BarPosition::Right);
        assert!(layout.primary.visible && layout.secondary.visible);
    }
}
