//! Menu contributions, resolved against the command and keybinding registries
//! into plain data that a native menu (or any other renderer) can be built from.

use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::Serialize;

use crate::commands::CommandRegistry;
use crate::keybindings::KeybindingRegistry;

/// Prefix of locations that become top-level menus: `menubar/Tools`.
pub const MENUBAR: &str = "menubar/";

#[derive(Debug, Clone)]
pub struct MenuEntry {
    pub command: String,
    pub group: Option<String>,
    pub order: i32,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MenuNode {
    Item { command: String, title: String, accelerator: Option<String> },
    Separator,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Menu {
    pub title: String,
    pub items: Vec<MenuNode>,
}

#[derive(Default)]
pub struct MenuRegistry {
    entries: RwLock<BTreeMap<String, Vec<MenuEntry>>>,
}

impl MenuRegistry {
    pub fn add(&self, location: &str, entry: MenuEntry) {
        self.entries.write().unwrap().entry(location.to_owned()).or_default().push(entry);
    }

    pub fn remove_source(&self, source: &str) {
        let mut entries = self.entries.write().unwrap();
        for list in entries.values_mut() {
            list.retain(|e| e.source.as_deref() != Some(source));
        }
        entries.retain(|_, list| !list.is_empty());
    }

    /// Entries at one location, grouped and ordered, with separators between
    /// groups. Entries whose command is not registered are dropped.
    pub fn resolve(
        &self,
        location: &str,
        commands: &CommandRegistry,
        keybindings: &KeybindingRegistry,
    ) -> Vec<MenuNode> {
        let mut entries: Vec<(MenuEntry, String)> = self
            .entries
            .read()
            .unwrap()
            .get(location)
            .into_iter()
            .flatten()
            .filter_map(|e| Some((e.clone(), commands.get(&e.command)?.0.title)))
            .collect();
        entries.sort_by(|(a, at), (b, bt)| (&a.group, a.order, at).cmp(&(&b.group, b.order, bt)));

        let mut nodes = Vec::new();
        let mut last_group = None;
        for (entry, title) in entries {
            if last_group.as_ref().is_some_and(|g| *g != entry.group) {
                nodes.push(MenuNode::Separator);
            }
            last_group = Some(entry.group);
            nodes.push(MenuNode::Item {
                accelerator: keybindings.for_command(&entry.command),
                command: entry.command,
                title,
            });
        }
        nodes
    }

    /// Top-level menus derived from every `menubar/<Title>` location.
    pub fn menubar(&self, commands: &CommandRegistry, keybindings: &KeybindingRegistry) -> Vec<Menu> {
        let locations: Vec<String> =
            self.entries.read().unwrap().keys().filter(|l| l.starts_with(MENUBAR)).cloned().collect();
        let mut menus: Vec<Menu> = locations
            .iter()
            .map(|location| Menu {
                title: location[MENUBAR.len()..].to_owned(),
                items: self.resolve(location, commands, keybindings),
            })
            .filter(|menu| !menu.items.is_empty())
            .collect();
        menus.sort_by(|a, b| (menubar_rank(&a.title), &a.title).cmp(&(menubar_rank(&b.title), &b.title)));
        menus
    }
}

/// Conventional menus keep their conventional place; anything else sits before Help.
fn menubar_rank(title: &str) -> u8 {
    match title {
        "File" => 0,
        "Edit" => 1,
        "View" => 2,
        "Tools" => 3,
        "Help" => 9,
        _ => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::{CommandInfo, CommandTarget};
    use crate::keybindings::Keybinding;

    fn command(registry: &CommandRegistry, id: &str, title: &str) {
        let info = CommandInfo { id: id.into(), title: title.into(), category: None, source: None };
        registry.register(info, CommandTarget::Plugin("p".into())).unwrap();
    }

    fn entry(command: &str, group: Option<&str>, order: i32) -> MenuEntry {
        MenuEntry { command: command.into(), group: group.map(Into::into), order, source: Some("p".into()) }
    }

    #[test]
    fn menubar_is_grouped_ordered_and_ranked() {
        let commands = CommandRegistry::default();
        let keys = KeybindingRegistry::default();
        let menus = MenuRegistry::default();
        command(&commands, "a", "Alpha");
        command(&commands, "b", "Beta");
        command(&commands, "c", "Gamma");
        keys.register(Keybinding { key: "Mod+B".into(), command: "b".into(), source: None });

        menus.add("menubar/Tools", entry("c", Some("2_more"), 0));
        menus.add("menubar/Tools", entry("b", Some("1_main"), 2));
        menus.add("menubar/Tools", entry("a", Some("1_main"), 1));
        menus.add("menubar/Tools", entry("missing", None, 0));
        menus.add("menubar/Help", entry("a", None, 0));
        menus.add("menubar/File", entry("a", None, 0));
        menus.add("menubar/Ghost", entry("missing", None, 0));
        menus.add("view/title", entry("a", None, 0));

        let bar = menus.menubar(&commands, &keys);
        let titles: Vec<_> = bar.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, ["File", "Tools", "Help"]);

        let item = |command: &str, title: &str, accelerator: Option<&str>| MenuNode::Item {
            command: command.into(),
            title: title.into(),
            accelerator: accelerator.map(Into::into),
        };
        assert_eq!(
            bar[1].items,
            [item("a", "Alpha", None), item("b", "Beta", Some("Mod+B")), MenuNode::Separator, item("c", "Gamma", None)]
        );

        menus.remove_source("p");
        assert!(menus.menubar(&commands, &keys).is_empty());
    }
}
