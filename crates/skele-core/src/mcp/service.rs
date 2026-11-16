//! The tool registry as an MCP server, over any byte stream. Knows nothing
//! about sockets, pipes or ports: the transport is the caller's business.

use std::sync::Arc;

use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, InitializeResult,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Notify;

use crate::grants::Grant;
use crate::tools::ToolInfo;
use crate::{Error, Result, Workbench};

#[derive(Clone)]
struct Service {
    workbench: Workbench,
    grant: Grant,
    /// Signalled when the grant turns out to be revoked, to end the connection.
    revoked: Arc<Notify>,
}

fn to_mcp(info: ToolInfo) -> Result<Tool> {
    let schema = match info.input_schema {
        Value::Object(map) => map,
        other => return Err(Error::Other(format!("tool {} has a non-object schema: {other}", info.name))),
    };
    let mut tool = Tool::new(info.name, info.description, Arc::new(schema))
        .annotate(ToolAnnotations::new().read_only(info.read_only).destructive(info.destructive));
    if let Some(title) = info.title {
        tool = tool.with_title(title);
    }
    Ok(tool)
}

impl Service {
    /// Revocation normally ends the connection at once; this covers a request
    /// that arrives in between.
    fn check_grant(&self) -> std::result::Result<(), ErrorData> {
        if self.workbench.grants().check(&self.grant.token).is_some() {
            return Ok(());
        }
        self.revoked.notify_one();
        Err(ErrorData::invalid_request("this connection's grant was revoked", None))
    }
}

impl ServerHandler for Service {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, ErrorData> {
        self.check_grant()?;
        let tools = self.workbench.tools().list().into_iter().filter_map(|info| to_mcp(info).ok()).collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        self.check_grant()?;
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        match self.workbench.call_tool(&request.name, arguments, &self.grant).await {
            Ok(result) => {
                let content = result.content.into_iter().map(|c| ContentBlock::text(c.text)).collect();
                let result =
                    if result.is_error { CallToolResult::error(content) } else { CallToolResult::success(content) };
                Ok(result.into())
            }
            Err(Error::UnknownTool(_)) => Err(ErrorData::method_not_found::<CallToolRequestMethod>()),
            Err(e) => Err(ErrorData::internal_error(e.to_string(), None)),
        }
    }
}

/// Serves the tool registry over one connection, under `grant`, until the
/// peer disconnects or the grant is found to be revoked.
pub async fn serve<R, W>(workbench: Workbench, grant: Grant, reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    let revoked = Arc::new(Notify::new());
    // Revoking the grant ends this connection without waiting for its next request.
    let mut revocations = workbench.grants().revocations();
    let (watch, id, token) = (workbench.clone(), grant.id, grant.token.clone());
    let signal = revoked.clone();
    tokio::spawn(async move {
        loop {
            match revocations.recv().await {
                Ok(revoked) if revoked == id => break,
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    if watch.grants().check(&token).is_none() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
        signal.notify_one();
    });
    let service = Service { workbench, grant, revoked: revoked.clone() };
    let running = service.serve((reader, writer)).await.map_err(|e| Error::Other(format!("mcp: {e}")))?;
    let cancel = running.cancellation_token();
    tokio::spawn(async move {
        revoked.notified().await;
        cancel.cancel();
    });
    running.waiting().await.map_err(|e| Error::Other(format!("mcp: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rmcp::model::CallToolRequestParams;
    use rmcp::ServiceExt;

    use crate::grants::GrantKind;
    use crate::Workbench;

    #[tokio::test]
    async fn revoking_ends_serve() {
        let workbench = Workbench::new(Default::default()).unwrap();
        let grant = workbench.grants().issue(GrantKind::AgentSession { session_id: "s".into() }, "test");
        let (server_end, client_end) = tokio::io::duplex(64 * 1024);
        let (r, w) = tokio::io::split(server_end);
        let server = tokio::spawn(super::serve(workbench.clone(), grant.clone(), r, w));
        let (r, w) = tokio::io::split(client_end);
        let client = ().serve((r, w)).await.unwrap();
        client.list_all_tools().await.unwrap();
        workbench.grants().revoke(grant.id);
        let ended = tokio::time::timeout(std::time::Duration::from_secs(3), server).await;
        assert!(ended.is_ok(), "serve kept running after revoke");
        // The connection is gone, not merely refusing: requests fail at the transport.
        let after = client.list_all_tools().await;
        assert!(matches!(after, Err(rmcp::ServiceError::TransportClosed)), "{after:?}");
    }

    #[tokio::test]
    async fn serves_over_duplex() {
        let workbench = Workbench::new(Default::default()).unwrap();
        let grant = workbench.grants().issue(GrantKind::AgentSession { session_id: "s".into() }, "test");
        let (server_end, client_end) = tokio::io::duplex(64 * 1024);
        let (r, w) = tokio::io::split(server_end);
        let server = tokio::spawn(super::serve(workbench.clone(), grant, r, w));

        let (r, w) = tokio::io::split(client_end);
        let client = ().serve((r, w)).await.unwrap();
        let tools = client.list_all_tools().await.unwrap();
        let snapshot = tools.iter().find(|t| t.name == "workbench_snapshot").unwrap();
        let hints = snapshot.annotations.as_ref().unwrap();
        assert_eq!((hints.read_only_hint, hints.destructive_hint), (Some(true), Some(false)));
        let save = tools.iter().find(|t| t.name == "document_save").unwrap().annotations.clone().unwrap();
        assert_eq!(save.destructive_hint, Some(true));

        let mut args = rmcp::model::JsonObject::new();
        args.insert("key".into(), "workbench.colorTheme".into());
        let result = client.call_tool(CallToolRequestParams::new("config_get").with_arguments(args)).await.unwrap();
        assert_eq!(result.is_error, Some(false));
        assert_eq!(result.content[0].as_text().unwrap().text, "\"dark\"");

        let bad = client.call_tool(CallToolRequestParams::new("config_get")).await.unwrap();
        assert_eq!(bad.is_error, Some(true), "missing key is a tool error the agent can read");
        assert!(client.call_tool(CallToolRequestParams::new("no_such_tool")).await.is_err());

        client.cancel().await.unwrap();
        server.await.unwrap().unwrap();
        let _ = Arc::new(());
    }
}
