//! End-to-end: the host drives the Bun sample plugin over JSON-RPC.
//! Needs `bun` on PATH and `bun install` to have been run at the repo root.

mod common;

use common::{expect, on_path, sample};
use serde_json::{json, Value};
use skele_core::config::Scope;
use skele_core::menus::MenuNode;
use skele_core::plugin::PluginState;
use skele_core::{Error, HostEvent, Workbench, WorkbenchOptions};

const PLUGIN: &str = "sample.hello";

#[tokio::test]
async fn host_and_sample_plugin_round_trip() {
    if !on_path("bun") {
        eprintln!("skipping: bun is not installed");
        return;
    }
    let plugin_dir = sample("sample-bun");
    let dir = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(WorkbenchOptions {
        user_settings: Some(dir.path().join("settings.json")),
        workspace: Some(dir.path().join("workspace")),
        log_dir: None,
    })
    .unwrap();
    let mut events = workbench.bus().subscribe();

    // Installing registers contributions without starting the process.
    assert_eq!(workbench.install_plugin(&plugin_dir).unwrap(), PLUGIN);
    let container = workbench.containers().get("hello.container").expect("sample container");
    assert_eq!(container.bar, skele_core::protocol::manifest::BarId::Primary);
    assert_eq!(workbench.views().get("hello.greetings").unwrap().container, "hello.container");
    assert_eq!(workbench.layout().primary.active_container.as_deref(), Some("hello.container"));
    assert_eq!(common::state(&workbench, PLUGIN), PluginState::Idle);
    assert!(workbench.commands().get("hello.greet").is_some());
    let tools = workbench.menubar().into_iter().find(|m| m.title == "Tools").unwrap();
    assert_eq!(
        tools.items[0],
        MenuNode::Item { command: "hello.greet".into(), title: "Greet".into(), accelerator: Some("Mod+Alt+G".into()) }
    );
    assert!(workbench.config().sections().iter().any(|s| s.source.as_deref() == Some(PLUGIN)));
    assert_eq!(workbench.config().get("hello.name"), json!("World"));

    // First use starts the sidecar; the command reads config back through the host.
    let reply = workbench.execute_command("hello.greet", vec![]).await.unwrap();
    assert_eq!(reply, json!("Hello, World!"));
    assert_eq!(common::state(&workbench, PLUGIN), PluginState::Running);

    // The sample prints one line to stdout on purpose; it lands in its channel.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !workbench.output_read("Hello").unwrap_or_default().contains("sample.hello stdout check") {
        assert!(std::time::Instant::now() < deadline, "stray stdout never reached the Hello channel");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let message = expect(&mut events, |e| match e {
        HostEvent::Message { message, source, .. } => Some((message.clone(), source.clone())),
        _ => None,
    })
    .await;
    assert_eq!(message, ("Hello, World!".to_owned(), Some("Hello".to_owned())));

    // Settings changes reach the running plugin, which refreshes its view.
    workbench.set_config("hello.name", Some(json!("Rust")), Scope::Workspace).unwrap();
    workbench.set_config("hello.shout", Some(json!(true)), Scope::User).unwrap();
    workbench.set_config("hello.repeat", Some(json!(2)), Scope::User).unwrap();
    let rejected = workbench.set_config("hello.repeat", Some(json!(9)), Scope::User);
    assert!(matches!(rejected, Err(Error::InvalidSetting { .. })));
    while events.try_recv().is_ok() {}
    workbench.set_config("hello.greeting", Some(json!("Ahoy")), Scope::User).unwrap();
    expect(&mut events, |e| {
        matches!(e, HostEvent::ViewRefresh { view_id } if view_id == "hello.greetings").then_some(())
    })
    .await;

    let reply = workbench.execute_command("hello.greet", vec![]).await.unwrap();
    assert_eq!(reply, json!("AHOY, RUST!"));
    let reply = workbench.execute_command("hello.greet", vec![json!("args")]).await.unwrap();
    assert_eq!(reply, json!("AHOY, ARGS!"));
    let written = expect(&mut events, |e| match e {
        HostEvent::OutputAppended { channel, text } if channel == "Hello" => Some(text.clone()),
        _ => None,
    })
    .await;
    assert_eq!(written, "AHOY, RUST!\r\n");

    // Views are data: rows from the plugin, children fetched by row id.
    let root = workbench.view_children("hello.greetings", None).await.unwrap();
    let labels: Vec<_> = root.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["Greet", "Open manifest", "History", "Effective settings"]);
    assert_eq!(root[0].command.as_ref().unwrap().command, "hello.greet");
    let history = workbench.view_children("hello.greetings", Some("history".into())).await.unwrap();
    assert_eq!(history.len(), 5); // 1 + 2 + 2 with hello.repeat = 2

    // Unknown commands are an error; a command returning nothing yields null.
    assert!(workbench.execute_command("nope", vec![]).await.is_err());
    assert_eq!(workbench.execute_command("hello.clear", vec![]).await.unwrap(), Value::Null);

    // Shutdown stops the process; the next use starts a fresh one.
    workbench.shutdown().await;
    assert_eq!(common::state(&workbench, PLUGIN), PluginState::Stopped);
    workbench.execute_command("hello.greet", vec![]).await.unwrap();
    assert_eq!(common::state(&workbench, PLUGIN), PluginState::Running);
    let history = workbench.view_children("hello.greetings", Some("history".into())).await.unwrap();
    assert_eq!(history.len(), 2);
    workbench.shutdown().await;
}

fn agent_grant(workbench: &Workbench) -> skele_core::grants::Grant {
    workbench.grants().issue(skele_core::grants::GrantKind::AgentSession { session_id: "s".into() }, "test")
}

#[tokio::test]
async fn bun_tool_round_trip() {
    if !on_path("bun") {
        eprintln!("skipping: bun is not installed");
        return;
    }
    let workbench = Workbench::new(WorkbenchOptions::default()).unwrap();
    workbench.install_plugin(&sample("sample-bun")).unwrap();
    let grant = agent_grant(&workbench);
    let result = workbench.call_tool("hello_greet", json!({ "name": "Ferris" }), &grant).await.unwrap();
    assert!(!result.is_error, "{:?}", result.content);
    assert_eq!(result.content[0].text, "Hello, Ferris!");
    workbench.shutdown().await;
}

#[tokio::test]
async fn bun_unhandled_tool_is_tool_error() {
    if !on_path("bun") {
        eprintln!("skipping: bun is not installed");
        return;
    }
    // A copy of the sample whose manifest declares a tool the code never registers.
    let tmp = tempfile::tempdir().unwrap();
    let copy = tmp.path().join("sample-bun");
    std::fs::create_dir(&copy).unwrap();
    for entry in ["src", "package.json"] {
        std::os::unix::fs::symlink(sample("sample-bun").join(entry), copy.join(entry)).unwrap();
    }
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(sample("sample-bun").join("skele-plugin.json")).unwrap()).unwrap();
    manifest["contributes"]["tools"].as_array_mut().unwrap().push(
        json!({ "name": "hello_unhandled", "description": "Never registered", "inputSchema": { "type": "object" } }),
    );
    std::fs::write(copy.join("skele-plugin.json"), manifest.to_string()).unwrap();

    let workbench = Workbench::new(WorkbenchOptions::default()).unwrap();
    workbench.install_plugin(&copy).unwrap();
    let grant = agent_grant(&workbench);
    let result = workbench.call_tool("hello_unhandled", json!({}), &grant).await.unwrap();
    assert!(result.is_error);
    assert!(result.content[0].text.starts_with("Hello: "), "{:?}", result.content);
    let after = workbench.call_tool("hello_greet", json!({}), &grant).await.unwrap();
    assert!(!after.is_error, "{:?}", after.content);
    workbench.shutdown().await;
}
