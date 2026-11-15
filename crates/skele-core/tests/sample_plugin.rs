//! End-to-end: the host drives the Bun sample plugin over JSON-RPC.
//! Needs `bun` on PATH and `bun install` to have been run at the repo root.

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use skele_core::config::Scope;
use skele_core::menus::MenuNode;
use skele_core::plugin::PluginState;
use skele_core::{Error, HostEvent, Workbench, WorkbenchOptions};
use tokio::sync::broadcast::Receiver;

const PLUGIN: &str = "sample.hello";

fn state(workbench: &Workbench) -> PluginState {
    workbench.plugins().iter().find(|p| p.id == PLUGIN).unwrap().state
}

/// Waits for an event matching `pick`, failing the test if none arrives.
async fn expect<T>(events: &mut Receiver<HostEvent>, mut pick: impl FnMut(&HostEvent) -> Option<T>) -> T {
    let wait = async {
        loop {
            if let Some(found) = pick(&events.recv().await.expect("event bus closed")) {
                return found;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait).await.expect("timed out waiting for event")
}

#[tokio::test]
async fn host_and_sample_plugin_round_trip() {
    if std::process::Command::new("bun").arg("--version").output().is_err() {
        eprintln!("skipping: bun is not installed");
        return;
    }
    let plugin_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-bun");
    let dir = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(WorkbenchOptions {
        user_settings: Some(dir.path().join("settings.json")),
        workspace: Some(dir.path().join("workspace")),
    })
    .unwrap();
    let mut events = workbench.bus().subscribe();

    // Installing registers contributions without starting the process.
    assert_eq!(workbench.install_plugin(&plugin_dir).unwrap(), PLUGIN);
    assert_eq!(state(&workbench), PluginState::Idle);
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
    assert_eq!(state(&workbench), PluginState::Running);
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
        HostEvent::TerminalData { terminal, data } if terminal == "Hello" => Some(data.clone()),
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
    assert_eq!(state(&workbench), PluginState::Stopped);
    workbench.execute_command("hello.greet", vec![]).await.unwrap();
    assert_eq!(state(&workbench), PluginState::Running);
    let history = workbench.view_children("hello.greetings", Some("history".into())).await.unwrap();
    assert_eq!(history.len(), 2);
    workbench.shutdown().await;
}
