//! The private MCP server: the tool registry, offered only to agents the
//! workbench starts. An agent launches `skele-mcp-bridge` as its stdio MCP
//! server; the bridge connects to the workbench's local socket and presents
//! the grant token it was given (ADR-0003).

pub mod service;
pub mod socket;

use std::path::PathBuf;
use std::sync::Arc;

pub use socket::McpListener;

use crate::grants::Grant;
use crate::{Result, Workbench};

/// Environment variables the bridge reads.
pub const SOCKET_ENV: &str = "SKELE_MCP_SOCKET";
pub const TOKEN_ENV: &str = "SKELE_MCP_TOKEN";
/// Overrides where the bridge binary is (tests, unusual installs).
pub const BRIDGE_ENV: &str = "SKELE_MCP_BRIDGE";

/// How to start the bridge for one grant: what an ACP `session/new` lists as
/// a stdio MCP server.
#[derive(Debug, Clone)]
pub struct McpLaunch {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// `SKELE_MCP_BRIDGE`, else `skele-mcp-bridge` beside the running executable.
fn bridge_path() -> PathBuf {
    if let Some(path) = std::env::var_os(BRIDGE_ENV) {
        return PathBuf::from(path);
    }
    let name = if cfg!(windows) { "skele-mcp-bridge.exe" } else { "skele-mcp-bridge" };
    let exe = std::env::current_exe().unwrap_or_default();
    exe.parent().map(|dir| dir.join(name)).unwrap_or_else(|| PathBuf::from(name))
}

impl Workbench {
    /// The socket listener, started on first use.
    pub async fn mcp(&self) -> Result<Arc<McpListener>> {
        self.inner.mcp.get_or_try_init(|| async { McpListener::start(self).map(Arc::new) }).await.cloned()
    }

    /// How an agent should launch the bridge so its calls run under `grant`.
    pub async fn mcp_launch(&self, grant: &Grant) -> Result<McpLaunch> {
        let listener = self.mcp().await?;
        Ok(McpLaunch {
            command: bridge_path(),
            args: Vec::new(),
            env: vec![
                (SOCKET_ENV.to_owned(), listener.path().display().to_string()),
                (TOKEN_ENV.to_owned(), grant.token.clone()),
            ],
        })
    }
}
