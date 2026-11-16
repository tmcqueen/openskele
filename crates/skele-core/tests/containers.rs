//! View containers contributed by plugin manifests.

use std::path::Path;

use serde_json::{json, Value};
use skele_core::containers::VIEWS_CONTAINER;
use skele_core::protocol::manifest::BarId;
use skele_core::{Error, Workbench};

fn plugin(dir: &Path, name: &str, contributes: Value) -> std::path::PathBuf {
    let dir = dir.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "id": format!("test.{name}"), "name": name, "version": "0.1.0", "protocol": "0.2",
        "main": { "command": "/bin/false" },
        "contributes": contributes,
    });
    std::fs::write(dir.join("skele-plugin.json"), manifest.to_string()).unwrap();
    dir
}

#[test]
fn plugin_containers_and_views_are_registered() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let dir = plugin(
        tmp.path(),
        "boxes",
        json!({
            "viewContainers": [{ "id": "boxes.side", "title": "Boxes", "icon": "package", "bar": "secondary" }],
            "views": [{ "id": "boxes.list", "title": "List", "container": "boxes.side" }],
        }),
    );
    workbench.install_plugin(&dir).unwrap();

    let snapshot = workbench.snapshot();
    let ids: Vec<_> = snapshot.containers.iter().map(|c| c.id.as_str()).collect();
    assert!(ids.contains(&VIEWS_CONTAINER));
    let mine = snapshot.containers.iter().find(|c| c.id == "boxes.side").unwrap();
    assert_eq!(mine.bar, BarId::Secondary);
    assert_eq!(mine.source.as_deref(), Some("test.boxes"));
    assert_eq!(workbench.views().get("boxes.list").unwrap().container, "boxes.side");
    let focus = workbench.commands().get("workbench.focusContainer.boxes.side").expect("focus command");
    assert_eq!(focus.0.title, "Focus Boxes");
}

#[test]
fn view_without_container_goes_to_views() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let dir = plugin(tmp.path(), "plain", json!({ "views": [{ "id": "plain.v", "title": "V" }] }));
    workbench.install_plugin(&dir).unwrap();
    assert_eq!(workbench.views().get("plain.v").unwrap().container, VIEWS_CONTAINER);
}

#[test]
fn unknown_container_fails_install_and_rolls_back() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = Workbench::new(Default::default()).unwrap();
    let dir = plugin(
        tmp.path(),
        "broken",
        json!({
            "commands": [{ "id": "broken.run", "title": "Run" }],
            "viewContainers": [{ "id": "broken.box", "title": "Box", "icon": "package" }],
            "views": [{ "id": "broken.v", "title": "V", "container": "nope" }],
            "configuration": { "properties": { "broken.x": { "type": "string", "default": "" } } },
        }),
    );
    let result = workbench.install_plugin(&dir);
    assert!(matches!(&result, Err(Error::UnknownContainer(id)) if id == "nope"), "got {result:?}");
    assert!(workbench.commands().get("broken.run").is_none());
    assert!(workbench.commands().get("workbench.focusContainer.broken.box").is_none());
    assert!(workbench.containers().get("broken.box").is_none());
    assert!(workbench.views().get("broken.v").is_none());
    assert!(workbench.config().sections().iter().all(|s| s.source.as_deref() != Some("test.broken")));
    assert!(workbench.plugins().is_empty());
}
