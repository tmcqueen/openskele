//! Tools contributed by plugin manifests.

use std::path::Path;

use serde_json::{json, Value};
use skele_core::grants::GrantKind;
use skele_core::plugin::PluginState;
use skele_core::{Error, Workbench};

fn plugin(dir: &Path, name: &str, contributes: Value) -> std::path::PathBuf {
    let dir = dir.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "id": format!("test.{name}"), "name": format!("Plugin {name}"), "version": "0", "protocol": "0.2",
        "main": { "command": "/bin/false" }, "contributes": contributes,
    });
    std::fs::write(dir.join("skele-plugin.json"), manifest.to_string()).unwrap();
    dir
}

fn tool(name: &str) -> Value {
    json!({ "name": name, "description": "A tool", "inputSchema": { "type": "object" } })
}

#[test]
fn plugin_tools_are_listed_without_starting() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let mut look = tool("p_look");
    look["readOnly"] = json!(true);
    workbench.install_plugin(&plugin(tmp.path(), "p", json!({ "tools": [look, tool("p_do")] }))).unwrap();
    let tools = workbench.tools().list();
    let look = tools.iter().find(|t| t.name == "p_look").unwrap();
    let act = tools.iter().find(|t| t.name == "p_do").unwrap();
    assert!(look.read_only && !look.destructive);
    assert!(!act.read_only && act.destructive);
    assert_eq!(look.source.as_deref(), Some("test.p"));
    assert_eq!(workbench.plugins()[0].state, PluginState::Idle);
}

#[test]
fn invalid_tool_name_fails_install_and_rolls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let dir = plugin(
        tmp.path(),
        "bad",
        json!({ "commands": [{ "id": "bad.run", "title": "Run" }], "tools": [tool("hello.greet")] }),
    );
    let result = workbench.install_plugin(&dir);
    assert!(matches!(&result, Err(Error::InvalidToolName(_))), "{result:?}");
    assert!(workbench.commands().get("bad.run").is_none());
    assert!(workbench.plugins().is_empty());
}

#[test]
fn duplicate_tool_across_plugins_fails_second_install() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    workbench.install_plugin(&plugin(tmp.path(), "a", json!({ "tools": [tool("same")] }))).unwrap();
    let second = workbench.install_plugin(&plugin(tmp.path(), "b", json!({ "tools": [tool("same")] })));
    assert!(matches!(&second, Err(Error::DuplicateTool(_))), "{second:?}");
    assert_eq!(workbench.tools().get("same").unwrap().0.source.as_deref(), Some("test.a"));
}

#[tokio::test]
async fn plugin_that_cannot_start_gives_tool_error() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    workbench.install_plugin(&plugin(tmp.path(), "dead", json!({ "tools": [tool("dead_tool")] }))).unwrap();
    let grant = workbench.grants().issue(GrantKind::AgentSession { session_id: "s".into() }, "test");
    let result = workbench.call_tool("dead_tool", json!({}), &grant).await.unwrap();
    assert!(result.is_error);
    assert!(result.content[0].text.starts_with("Plugin dead: "), "{:?}", result.content);
}
