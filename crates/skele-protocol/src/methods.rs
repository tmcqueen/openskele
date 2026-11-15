//! Protocol methods. Each one is a zero-sized type tying a method name to its
//! params and result, so both ends get checked calls and the OpenRPC document
//! can be generated from the same list.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::view::TreeItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    HostToPlugin,
    PluginToHost,
}

pub trait Request {
    const METHOD: &'static str;
    const DIRECTION: Direction;
    type Params: Serialize + DeserializeOwned + JsonSchema + Send + 'static;
    type Result: Serialize + DeserializeOwned + JsonSchema + Send + 'static;
}

pub trait Notification {
    const METHOD: &'static str;
    const DIRECTION: Direction;
    type Params: Serialize + DeserializeOwned + JsonSchema + Send + 'static;
}

macro_rules! request {
    ($(#[$doc:meta])* $name:ident, $method:literal, $dir:ident, $params:ty => $result:ty) => {
        $(#[$doc])*
        pub enum $name {}
        impl Request for $name {
            const METHOD: &'static str = $method;
            const DIRECTION: Direction = Direction::$dir;
            type Params = $params;
            type Result = $result;
        }
    };
}

macro_rules! notification {
    ($(#[$doc:meta])* $name:ident, $method:literal, $dir:ident, $params:ty) => {
        $(#[$doc])*
        pub enum $name {}
        impl Notification for $name {
            const METHOD: &'static str = $method;
            const DIRECTION: Direction = Direction::$dir;
            type Params = $params;
        }
    };
}

// ---- lifecycle -------------------------------------------------------------

request!(
    /// First message on the wire. The plugin must not send anything before replying.
    Initialize, "initialize", HostToPlugin, InitializeParams => InitializeResult
);
request!(
    /// Ask the plugin to release resources. Followed by `exit`.
    Shutdown, "shutdown", HostToPlugin, Empty => Empty
);
notification!(
    /// The plugin process should terminate.
    Exit, "exit", HostToPlugin, Empty
);

// ---- host -> plugin --------------------------------------------------------

request!(
    /// Run a command the plugin contributed.
    CommandExecute, "command/execute", HostToPlugin, CommandExecuteParams => Value
);
request!(
    /// Fetch the rows of a contributed view, or the children of one row.
    ViewGetChildren, "view/getChildren", HostToPlugin, ViewGetChildrenParams => ViewGetChildrenResult
);
notification!(
    /// Effective configuration changed.
    ConfigDidChange, "config/didChange", HostToPlugin, ConfigDidChangeParams
);
notification!(
    /// A document buffer was written to disk.
    DocumentDidSave, "document/didSave", HostToPlugin, DocumentDidSaveParams
);

// ---- plugin -> host --------------------------------------------------------

notification!(
    /// Show a transient message to the user.
    WindowShowMessage, "window/showMessage", PluginToHost, ShowMessageParams
);
notification!(
    /// Append text to an output terminal.
    TerminalWrite, "terminal/write", PluginToHost, TerminalWriteParams
);
request!(
    /// Read the effective value of a setting.
    ConfigGet, "config/get", PluginToHost, ConfigGetParams => Value
);
notification!(
    /// Tell the host a view's data is stale so it re-queries.
    ViewRefresh, "view/refresh", PluginToHost, ViewRefreshParams
);

// ---- payloads --------------------------------------------------------------

/// No payload. Serializes as `{}`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Empty {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub host: PeerInfo,
    /// The id from the plugin's manifest.
    pub plugin_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub plugin: PeerInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommandExecuteParams {
    pub command: String,
    #[serde(default)]
    pub args: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewGetChildrenParams {
    pub view_id: String,
    /// Id of the item to expand; absent for the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewGetChildrenResult {
    pub items: Vec<TreeItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDidChangeParams {
    /// Changed keys with their new effective values.
    pub changes: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDidSaveParams {
    pub uri: String,
    pub version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum MessageLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShowMessageParams {
    pub level: MessageLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWriteParams {
    /// Output channel name. Defaults to one named after the plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    pub data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigGetParams {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewRefreshParams {
    pub view_id: String,
}
