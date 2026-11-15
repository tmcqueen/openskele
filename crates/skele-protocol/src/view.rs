//! View models. Plugins describe UI as data; the webview owns rendering.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A command invocation attached to a piece of UI.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CommandRef {
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<Value>,
}

/// One row of a tree or list view.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TreeItem {
    /// Stable within the view; passed back as `parent` to fetch children.
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Name from the host's icon set; never a URL or markup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Whether the item can be expanded to request children.
    #[serde(default)]
    pub collapsible: bool,
    /// Run when the item is activated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandRef>,
}
