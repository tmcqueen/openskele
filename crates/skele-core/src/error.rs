use skele_protocol::jsonrpc::RpcError;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("command already registered: {0}")]
    DuplicateCommand(String),
    #[error("unknown view: {0}")]
    UnknownView(String),
    #[error("unknown view container: {0}")]
    UnknownContainer(String),
    #[error("view container already registered: {0}")]
    DuplicateContainer(String),
    #[error("no such panel is open: {0}")]
    UnknownPanel(String),
    #[error("unknown output channel: {0}")]
    UnknownChannel(String),
    #[error("unknown tool: {0}")]
    UnknownTool(String),
    #[error("tool already registered: {0}")]
    DuplicateTool(String),
    #[error("invalid tool name {0:?}: use lowercase letters, digits and _, starting with a letter")]
    InvalidToolName(String),
    #[error("unknown plugin: {0}")]
    UnknownPlugin(String),
    #[error("unknown setting: {0}")]
    UnknownSetting(String),
    #[error("invalid value for {key}: {message}")]
    InvalidSetting { key: String, message: String },
    #[error("invalid schema for {key}: {message}")]
    InvalidSchema { key: String, message: String },
    #[error("no workspace is open")]
    NoWorkspace,
    #[error("plugin {plugin}: {message}")]
    Plugin { plugin: String, message: String },
    #[error("unknown terminal: {0}")]
    UnknownTerminal(u32),
    #[error("document is not open: {0}")]
    UnknownDocument(String),
    #[error("{0}")]
    Other(String),
    #[error(transparent)]
    Rpc(#[from] RpcError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
