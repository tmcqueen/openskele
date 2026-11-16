//! Agents contributed by plugin manifests.

use std::path::Path;

use serde_json::{json, Value};
use skele_core::{Error, Workbench};

fn plugin(dir: &Path, name: &str, manifest: Value) -> std::path::PathBuf {
    let dir = dir.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let mut full = json!({ "id": format!("test.{name}"), "name": name, "version": "0", "protocol": "0.2" });
    full.as_object_mut().unwrap().extend(manifest.as_object().unwrap().clone());
    std::fs::write(dir.join("skele-plugin.json"), full.to_string()).unwrap();
    dir
}

#[test]
fn agent_only_plugin_installs() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let dir = plugin(
        tmp.path(),
        "agents",
        json!({ "contributes": { "agents": [{ "id": "scripted", "name": "Scripted", "command": "bun" }] } }),
    );
    workbench.install_plugin(&dir).unwrap();
    let agents = workbench.agents();
    assert_eq!(agents.len(), 1);
    assert_eq!((agents[0].id.as_str(), agents[0].name.as_str()), ("scripted", "Scripted"));
    assert_eq!(agents[0].source, "test.agents");
}

#[test]
fn commands_without_main_fail_install() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let dir = plugin(
        tmp.path(),
        "nomain",
        json!({ "contributes": { "commands": [{ "id": "nomain.run", "title": "Run" }] } }),
    );
    let result = workbench.install_plugin(&dir);
    assert!(matches!(&result, Err(Error::Plugin { .. })), "{result:?}");
    assert!(workbench.commands().get("nomain.run").is_none());
}

#[test]
fn containers_alone_need_no_main() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let dir = plugin(
        tmp.path(),
        "box",
        json!({ "contributes": { "viewContainers": [{ "id": "box.c", "title": "Box", "icon": "package" }] } }),
    );
    workbench.install_plugin(&dir).unwrap();
}

#[test]
fn duplicate_agent_id_fails_second_install() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let agent = json!({ "contributes": { "agents": [{ "id": "same", "name": "Same", "command": "x" }] } });
    workbench.install_plugin(&plugin(tmp.path(), "a", agent.clone())).unwrap();
    assert!(workbench.install_plugin(&plugin(tmp.path(), "b", agent)).is_err());
    assert_eq!(workbench.agents().len(), 1);
}
