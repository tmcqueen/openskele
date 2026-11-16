//! End to end: an MCP client launches the bridge, which connects to the
//! workbench's private socket with a grant token.

use std::path::Path;
use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, JsonObject};
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};
use skele_core::grants::{Grant, GrantKind};
use skele_core::tools::{ToolCall, ToolCallResult, ToolInfo, ToolTarget};
use skele_core::Workbench;

fn on_path(program: &str) -> bool {
    std::process::Command::new(program).arg("--version").output().is_ok()
}

fn plugins() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn grant(workbench: &Workbench, label: &str) -> Grant {
    workbench.grants().issue(GrantKind::AgentSession { session_id: label.into() }, label)
}

/// Starts the bridge the way an agent would: as a child process speaking MCP
/// on stdio, launched as `mcp_launch` describes, presenting `token`.
async fn connect(workbench: &Workbench, grant: &Grant, token: &str) -> Result<RunningService<RoleClient, ()>, String> {
    // Every test sets the same value, so parallel tests do not race on it.
    std::env::set_var("SKELE_MCP_BRIDGE", env!("CARGO_BIN_EXE_skele-mcp-bridge"));
    let launch = workbench.mcp_launch(grant).await.unwrap();
    let mut command = tokio::process::Command::new(&launch.command);
    command.args(&launch.args).envs(launch.env.iter().cloned()).env("SKELE_MCP_TOKEN", token);
    let transport = TokioChildProcess::new(command).map_err(|e| e.to_string())?;
    ().serve(transport).await.map_err(|e| e.to_string())
}

fn object(value: Value) -> JsonObject {
    serde_json::from_value(value).unwrap()
}

async fn text(client: &RunningService<RoleClient, ()>, name: &'static str, args: Value) -> (bool, String) {
    let result = client.call_tool(CallToolRequestParams::new(name).with_arguments(object(args))).await.unwrap();
    (result.is_error == Some(true), result.content[0].as_text().unwrap().text.clone())
}

#[tokio::test]
async fn lists_and_calls_through_bridge() {
    let workbench = Workbench::new(Default::default()).unwrap();
    let grant = grant(&workbench, "a");
    let client = connect(&workbench, &grant, &grant.token).await.unwrap();
    assert!(client.list_all_tools().await.unwrap().iter().any(|t| t.name == "command_execute"));
    let (error, layout) = text(&client, "layout_get", json!({})).await;
    assert!(!error);
    assert!(layout.contains("\"primary\""));
    client.cancel().await.unwrap();
}

#[tokio::test]
async fn command_and_plugin_tools_through_bridge() {
    let has_bun = on_path("bun");
    let has_dotnet = on_path("dotnet") && plugins().join("sample-dotnet/bin/Release").is_dir();
    if !has_bun && !has_dotnet {
        eprintln!("skipping: neither bun nor a built .NET sample");
        return;
    }
    let workbench = Workbench::new(Default::default()).unwrap();
    workbench.load_plugins(&plugins());
    let grant = grant(&workbench, "a");
    let client = connect(&workbench, &grant, &grant.token).await.unwrap();
    if has_bun {
        let (error, reply) =
            text(&client, "command_execute", json!({ "command": "hello.greet", "args": ["Ferris"] })).await;
        assert!(!error, "{reply}");
        assert_eq!(reply, "\"Hello, Ferris!\"");
        assert_eq!(text(&client, "hello_greet", json!({ "name": "Rust" })).await, (false, "Hello, Rust!".to_owned()));
    }
    if has_dotnet {
        let (error, reply) = text(&client, "dotnet_runtime_info", json!({})).await;
        assert!(!error && reply.starts_with(".NET"), "{reply}");
    }
    client.cancel().await.unwrap();
    workbench.shutdown().await;
}

#[tokio::test]
async fn wrong_token_is_refused() {
    let workbench = Workbench::new(Default::default()).unwrap();
    let real = grant(&workbench, "a");
    assert!(connect(&workbench, &real, "00").await.is_err());
}

#[tokio::test]
async fn revoked_grant_closes_connection() {
    let workbench = Workbench::new(Default::default()).unwrap();
    let grant = grant(&workbench, "a");
    let client = connect(&workbench, &grant, &grant.token).await.unwrap();
    assert!(!text(&client, "layout_get", json!({})).await.0);
    workbench.grants().revoke(grant.id);
    // Revoking ends the connection at once, not at the next tool call: the
    // bridge exits, so requests fail at the transport rather than with a reply.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match client.list_all_tools().await {
            Err(rmcp::ServiceError::TransportClosed) => break,
            other => assert!(std::time::Instant::now() < deadline, "still connected after revoke: {other:?}"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn revoked_grant_cannot_list_tools() {
    let workbench = Workbench::new(Default::default()).unwrap();
    let grant = grant(&workbench, "a");
    let client = connect(&workbench, &grant, &grant.token).await.unwrap();
    workbench.grants().revoke(grant.id);
    assert!(client.list_all_tools().await.is_err());
}

#[tokio::test]
async fn two_clients_two_grants() {
    let workbench = Workbench::new(Default::default()).unwrap();
    let info = ToolInfo {
        name: "whoami".into(),
        title: None,
        description: "The grant's label".into(),
        input_schema: json!({ "type": "object" }),
        read_only: true,
        destructive: false,
        source: None,
    };
    let target = ToolTarget::Builtin(Arc::new(|call: ToolCall| {
        Box::pin(async move { Ok(ToolCallResult::text(call.grant.label)) })
    }));
    workbench.tools().register(info, target).unwrap();
    let (a, b) = (grant(&workbench, "a"), grant(&workbench, "b"));
    let (ca, cb) = (connect(&workbench, &a, &a.token).await.unwrap(), connect(&workbench, &b, &b.token).await.unwrap());
    assert_eq!(text(&ca, "whoami", json!({})).await.1, "a");
    assert_eq!(text(&cb, "whoami", json!({})).await.1, "b");
    ca.cancel().await.unwrap();
    cb.cancel().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn socket_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let workbench = Workbench::new(Default::default()).unwrap();
    let listener = workbench.mcp().await.unwrap();
    // The socket lives in a folder of its own that only the owner can enter,
    // which works on every Unix, whether or not the socket's own mode can be set.
    let dir = listener.path().parent().unwrap().to_owned();
    assert!(dir.file_name().unwrap().to_string_lossy().starts_with("openskele-"), "{}", dir.display());
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
    #[cfg(target_os = "linux")]
    assert_eq!(std::fs::metadata(listener.path()).unwrap().permissions().mode() & 0o777, 0o600);
    let path = listener.path().to_owned();
    drop(listener);
    drop(workbench);
    assert!(!dir.exists() && !path.exists(), "socket folder left behind");
}

#[tokio::test]
async fn launch_uses_the_bridge_and_its_env() {
    let workbench = Workbench::new(Default::default()).unwrap();
    let grant = grant(&workbench, "a");
    std::env::set_var("SKELE_MCP_BRIDGE", env!("CARGO_BIN_EXE_skele-mcp-bridge"));
    let launch = workbench.mcp_launch(&grant).await.unwrap();
    assert_eq!(launch.command, Path::new(env!("CARGO_BIN_EXE_skele-mcp-bridge")));
    let env: std::collections::HashMap<_, _> = launch.env.into_iter().collect();
    assert_eq!(env["SKELE_MCP_TOKEN"], grant.token);
    assert_eq!(Path::new(&env["SKELE_MCP_SOCKET"]), workbench.mcp().await.unwrap().path());
}
