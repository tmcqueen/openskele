//! End-to-end: the host drives the .NET sample plugin over JSON-RPC, which
//! exercises the generated C# protocol types and the .NET SDK runtime.
//! Needs the .NET SDK pinned in `global.json` on PATH.

mod common;

use common::{expect, on_path, sample, state};
use serde_json::json;
use skele_core::config::Scope;
use skele_core::plugin::PluginState;
use skele_core::{HostEvent, Workbench, WorkbenchOptions};

const PLUGIN: &str = "sample.dotnet";

/// Builds the sample once per test run: two concurrent `dotnet build`s of the
/// same project fight over the same output files.
fn built() -> std::path::PathBuf {
    static BUILD: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    let plugin_dir = sample("sample-dotnet");
    let result = BUILD.get_or_init(|| {
        let build = std::process::Command::new("dotnet")
            .args(["build", "-c", "Release", "-nologo", "-v", "quiet"])
            .current_dir(sample("sample-dotnet"))
            .output()
            .map_err(|e| e.to_string())?;
        if build.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&build.stdout).into_owned())
        }
    });
    if let Err(output) = result {
        panic!("dotnet build failed:\n{output}");
    }
    plugin_dir
}
const VIEW: &str = "dotnet.runtime";

#[tokio::test]
async fn host_and_dotnet_plugin_round_trip() {
    if !on_path("dotnet") {
        eprintln!("skipping: dotnet is not installed");
        return;
    }
    let plugin_dir = built();

    let workbench = Workbench::new(WorkbenchOptions::default()).unwrap();
    let mut events = workbench.bus().subscribe();
    assert_eq!(workbench.install_plugin(&plugin_dir).unwrap(), PLUGIN);
    assert_eq!(state(&workbench, PLUGIN), PluginState::Idle);

    // Its view lives in its own container on the secondary bar.
    let container = workbench.containers().get("dotnet.container").expect("sample container");
    assert_eq!(container.bar, skele_core::protocol::manifest::BarId::Secondary);
    assert_eq!(workbench.views().get(VIEW).unwrap().container, "dotnet.container");

    // The command reads a setting back through the host before answering.
    let reply = workbench.execute_command("dotnet.describe", vec![]).await.unwrap();
    let described = reply.as_str().unwrap().to_owned();
    assert!(described.starts_with("Running on .NET 10."), "unexpected reply: {described}");
    assert_eq!(state(&workbench, PLUGIN), PluginState::Running);

    let written = expect(&mut events, |e| match e {
        HostEvent::OutputAppended { channel, text } if channel == "Runtime Info" => Some(text.clone()),
        _ => None,
    })
    .await;
    assert_eq!(written, format!("{described}\r\n"));
    let message = expect(&mut events, |e| match e {
        HostEvent::Message { message, .. } => Some(message.clone()),
        _ => None,
    })
    .await;
    assert_eq!(message, described);
    expect(&mut events, |e| matches!(e, HostEvent::ViewRefresh { view_id } if view_id == VIEW).then_some(())).await;

    // config/didChange reaches the plugin, and the new value is what it reads next.
    workbench.set_config("dotnet.label", Some(json!("Powered by")), Scope::User).unwrap();
    expect(&mut events, |e| matches!(e, HostEvent::ViewRefresh { view_id } if view_id == VIEW).then_some(())).await;
    let reply = workbench.execute_command("dotnet.describe", vec![]).await.unwrap();
    assert!(reply.as_str().unwrap().starts_with("Powered by .NET"));

    // Optional fields the plugin left unset are absent, not null, on the wire.
    let root = workbench.view_children(VIEW, None).await.unwrap();
    let labels: Vec<_> = root.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["Describe runtime", "Facts", "Described"]);
    assert_eq!(root[0].command.as_ref().unwrap().command, "dotnet.describe");
    assert!(root[1].collapsible && !root[0].collapsible);
    assert_eq!(root[2].description.as_deref(), Some("2"));
    let facts = workbench.view_children(VIEW, Some("facts".into())).await.unwrap();
    assert_eq!(facts.len(), 3);

    // Saving a buffer is announced to running plugins.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, "draft").unwrap();
    let document = workbench.documents().open(&file).unwrap();
    while events.try_recv().is_ok() {}
    workbench.save_document(&document.uri).unwrap();
    let written = expect(&mut events, |e| match e {
        HostEvent::OutputAppended { text, .. } => Some(text.clone()),
        _ => None,
    })
    .await;
    assert_eq!(written, format!("saved {}\r\n", document.uri));

    workbench.shutdown().await;
    assert_eq!(state(&workbench, PLUGIN), PluginState::Stopped);
}

#[tokio::test]
async fn dotnet_tool_round_trip() {
    if !on_path("dotnet") {
        eprintln!("skipping: dotnet is not installed");
        return;
    }
    let plugin_dir = built();

    let workbench = Workbench::new(WorkbenchOptions::default()).unwrap();
    workbench.install_plugin(&plugin_dir).unwrap();
    let info = workbench.tools().get("dotnet_runtime_info").unwrap().0;
    assert!(info.read_only);
    let grant = workbench.grants().issue(skele_core::grants::GrantKind::AgentSession { session_id: "s".into() }, "t");
    let result = workbench.call_tool("dotnet_runtime_info", json!({}), &grant).await.unwrap();
    assert!(!result.is_error, "{:?}", result.content);
    assert!(result.content[0].text.starts_with(".NET"), "{:?}", result.content);
    workbench.shutdown().await;
}
