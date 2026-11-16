//! Plugin manifest (`skele-plugin.json`). Everything the host needs to show a
//! plugin's contributions without starting its process.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MANIFEST_FILE: &str = "skele-plugin.json";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Unique id, conventionally `publisher.name`.
    pub id: String,
    pub name: String,
    pub version: String,
    /// Protocol version the plugin speaks, e.g. `"0.1"`.
    pub protocol: String,
    /// How to start the sidecar. Relative paths resolve against the manifest's directory.
    pub main: Entry,
    /// When to start the process. Executing a contributed command or opening a
    /// contributed view always activates the plugin; list `onStartup` to start eagerly.
    #[serde(default)]
    pub activation_events: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<CommandContribution>,
    /// Menu location (e.g. `menubar/Tools`, `view/title`) to entries.
    #[serde(default)]
    pub menus: BTreeMap<String, Vec<MenuContribution>>,
    #[serde(default)]
    pub keybindings: Vec<KeybindingContribution>,
    /// Icon-strip entries on the app bars; views are placed in them by id.
    #[serde(default)]
    pub view_containers: Vec<ViewContainerContribution>,
    #[serde(default)]
    pub views: Vec<ViewContribution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ConfigurationContribution>,
    /// Tools an agent can call, served by the plugin through `tool/call`.
    #[serde(default)]
    pub tools: Vec<ToolContribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolContribution {
    /// Unique among all tools: lowercase letters, digits and `_`, starting with a letter.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// What the tool does, written for the agent that decides whether to call it.
    pub description: String,
    /// JSON Schema of the `arguments` object.
    pub input_schema: Value,
    /// The tool only reads. Tools that are not read-only are reported to agents as destructive.
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandContribution {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MenuContribution {
    pub command: String,
    /// Entries sharing a group are kept together; groups are separated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default)]
    pub order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeybindingContribution {
    pub command: String,
    /// Chord such as `Ctrl+Shift+P`. `Mod` is Cmd on macOS and Ctrl elsewhere.
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ViewKind {
    Tree,
    List,
}

/// Which app bar a view container sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum BarId {
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewContainerContribution {
    pub id: String,
    pub title: String,
    /// Codicon name, e.g. `package`.
    pub icon: String,
    #[serde(default = "default_bar")]
    pub bar: BarId,
    /// Lower sorts first within a bar.
    #[serde(default)]
    pub order: i32,
}

fn default_bar() -> BarId {
    BarId::Primary
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewContribution {
    pub id: String,
    pub title: String,
    #[serde(default = "default_view_kind")]
    pub kind: ViewKind,
    /// Id of the view container to show the view in; the built-in
    /// `workbench.views` container when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

fn default_view_kind() -> ViewKind {
    ViewKind::Tree
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationContribution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Setting key to its JSON Schema. `default` supplies the default value.
    pub properties: BTreeMap<String, Value>,
}
