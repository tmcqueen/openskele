//! The layout is owned by the core: built-in commands, settings and calls change it.

#[allow(dead_code)]
mod common;

use std::time::Duration;

use common::expect;
use serde_json::json;
use skele_core::config::Scope;
use skele_core::layout::{BarPosition, Panel};
use skele_core::protocol::manifest::BarId;
use skele_core::{Error, HostEvent, Workbench};

fn workbench() -> Workbench {
    Workbench::new(Default::default()).unwrap()
}

#[tokio::test]
async fn open_settings_command_opens_panel() {
    let workbench = workbench();
    let mut events = workbench.bus().subscribe();
    workbench.execute_command("workbench.openSettings", vec![]).await.unwrap();
    assert_eq!(workbench.layout().active_panel.as_deref(), Some("settings"));
    let layout = expect(&mut events, |e| match e {
        HostEvent::LayoutChanged { layout } => Some(layout.clone()),
        _ => None,
    })
    .await;
    assert_eq!(layout.panels, vec![Panel::Settings]);
}

#[tokio::test]
async fn open_file_twice_one_panel() {
    let workbench = workbench();
    for _ in 0..2 {
        workbench.execute_command("workbench.openFile", vec![json!("/tmp/x")]).await.unwrap();
    }
    assert_eq!(workbench.layout().panels, vec![Panel::Editor { path: "/tmp/x".into() }]);
}

#[tokio::test]
async fn open_file_without_path_errors() {
    assert!(workbench().execute_command("workbench.openFile", vec![]).await.is_err());
}

#[tokio::test]
async fn toggle_primary_bar() {
    let workbench = workbench();
    let bound = workbench.keybindings().list();
    assert!(bound.iter().any(|b| b.key == "Mod+B" && b.command == "workbench.togglePrimaryBar"));
    assert!(bound.iter().any(|b| b.key == "Mod+Alt+B" && b.command == "workbench.toggleSecondaryBar"));
    workbench.execute_command("workbench.togglePrimaryBar", vec![]).await.unwrap();
    assert!(!workbench.layout().primary.visible);
    workbench.execute_command("workbench.togglePrimaryBar", vec![]).await.unwrap();
    assert!(workbench.layout().primary.visible);
}

#[test]
fn position_setting_moves_bar() {
    let workbench = workbench();
    let mut events = workbench.bus().subscribe();
    workbench.set_config("workbench.primaryBar.position", Some(json!("top")), Scope::User).unwrap();
    assert_eq!(workbench.layout().primary.position, BarPosition::Top);
    let mut saw_layout = false;
    while let Ok(event) = events.try_recv() {
        saw_layout |= matches!(event, HostEvent::LayoutChanged { .. });
    }
    assert!(saw_layout);
    let bad = workbench.set_config("workbench.primaryBar.position", Some(json!("middle")), Scope::User);
    assert!(matches!(bad, Err(Error::InvalidSetting { .. })));
    assert_eq!(workbench.layout().primary.position, BarPosition::Top);
}

#[test]
fn position_setting_is_applied_at_startup() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("settings.json");
    std::fs::write(&file, r#"{ "workbench.secondaryBar.position": "top" }"#).unwrap();
    let options = skele_core::WorkbenchOptions { user_settings: Some(file), workspace: None };
    let workbench = Workbench::new(options).unwrap();
    assert_eq!(workbench.layout().secondary.position, BarPosition::Top);
}

#[test]
fn both_bars_may_share_a_side() {
    let workbench = workbench();
    workbench.set_config("workbench.secondaryBar.position", Some(json!("left")), Scope::User).unwrap();
    assert_eq!(workbench.layout().secondary.position, BarPosition::Left);
    assert_eq!(workbench.layout().primary.position, BarPosition::Left);
}

#[test]
fn set_active_container_rejects_wrong_bar() {
    let workbench = workbench();
    let result = workbench.set_active_container(BarId::Secondary, Some("workbench.views".into()));
    assert!(matches!(result, Err(Error::UnknownContainer(_))));
    workbench.set_active_container(BarId::Primary, None).unwrap();
    assert_eq!(workbench.layout().primary.active_container, None);
    workbench.set_active_container(BarId::Primary, Some("workbench.views".into())).unwrap();
    assert_eq!(workbench.layout().primary.active_container.as_deref(), Some("workbench.views"));
}

#[tokio::test]
async fn focus_container_command_reveals_it() {
    let workbench = workbench();
    workbench.set_active_container(BarId::Primary, None).unwrap();
    workbench.execute_command("workbench.focusContainer.workbench.views", vec![]).await.unwrap();
    let layout = workbench.layout();
    assert_eq!(layout.primary.active_container.as_deref(), Some("workbench.views"));
    assert!(layout.primary.visible);
}

fn install(workbench: &Workbench, dir: &std::path::Path, name: &str, contributes: serde_json::Value) {
    let dir = dir.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = json!({
        "id": format!("test.{name}"), "name": name, "version": "0", "protocol": "0.1",
        "main": { "command": "/bin/false" }, "contributes": contributes,
    });
    std::fs::write(dir.join("skele-plugin.json"), manifest.to_string()).unwrap();
    workbench.install_plugin(&dir).unwrap();
}

#[test]
fn first_container_with_views_is_chosen_then_kept() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = workbench();
    // The built-in Views container is empty, so nothing is chosen yet.
    assert_eq!(workbench.layout().primary.active_container, None);

    // A container without views is not chosen.
    install(
        &workbench,
        tmp.path(),
        "empty",
        json!({ "viewContainers": [{ "id": "e.box", "title": "E", "icon": "x", "order": -5 }] }),
    );
    assert_eq!(workbench.layout().primary.active_container, None);

    install(
        &workbench,
        tmp.path(),
        "b",
        json!({
            "viewContainers": [{ "id": "b.box", "title": "B", "icon": "x", "order": 2 }],
            "views": [{ "id": "b.v", "title": "V", "container": "b.box" }],
        }),
    );
    assert_eq!(workbench.layout().primary.active_container.as_deref(), Some("b.box"));

    // A lower-order container arriving later does not take over.
    install(
        &workbench,
        tmp.path(),
        "a",
        json!({
            "viewContainers": [{ "id": "a.box", "title": "A", "icon": "x", "order": 1 }],
            "views": [{ "id": "a.v", "title": "V", "container": "a.box" }],
        }),
    );
    assert_eq!(workbench.layout().primary.active_container.as_deref(), Some("b.box"));
}

#[test]
fn collapsed_bar_stays_collapsed_on_install() {
    let tmp = tempfile::tempdir().unwrap();
    let workbench = workbench();
    workbench.set_active_container(BarId::Secondary, None).unwrap();
    install(
        &workbench,
        tmp.path(),
        "s",
        json!({
            "viewContainers": [{ "id": "s.box", "title": "S", "icon": "x", "bar": "secondary" }],
            "views": [{ "id": "s.v", "title": "V", "container": "s.box" }],
        }),
    );
    assert_eq!(workbench.layout().secondary.active_container, None);
}

#[tokio::test]
async fn show_commands_emits_palette_event() {
    let workbench = workbench();
    let mut events = workbench.bus().subscribe();
    workbench.execute_command("workbench.showCommands", vec![]).await.unwrap();
    expect(&mut events, |e| matches!(e, HostEvent::ShowCommandPalette).then_some(())).await;
}

#[test]
fn snapshot_has_layout_and_containers() {
    let snapshot = workbench().snapshot();
    assert_eq!(snapshot.layout, workbench().layout());
    assert!(snapshot.containers.iter().any(|c| c.id == "workbench.views"));
}

#[test]
fn panel_calls_validate_ids() {
    let workbench = workbench();
    assert!(matches!(workbench.close_panel("settings"), Err(Error::UnknownPanel(_))));
    assert!(matches!(workbench.set_active_panel("settings"), Err(Error::UnknownPanel(_))));
    workbench.open_panel(Panel::Settings);
    workbench.open_panel(Panel::Editor { path: "/a".into() });
    workbench.set_active_panel("settings").unwrap();
    assert_eq!(workbench.layout().active_panel.as_deref(), Some("settings"));
    workbench.close_panel("settings").unwrap();
    assert_eq!(workbench.layout().active_panel.as_deref(), Some("editor:/a"));
}

#[test]
fn unchanged_calls_emit_nothing() {
    let workbench = workbench();
    workbench.open_panel(Panel::Settings);
    workbench.set_active_container(BarId::Primary, Some("workbench.views".into())).unwrap();
    let mut events = workbench.bus().subscribe();
    workbench.open_panel(Panel::Settings);
    workbench.set_active_container(BarId::Primary, Some("workbench.views".into())).unwrap();
    std::thread::sleep(Duration::from_millis(10));
    assert!(events.try_recv().is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn new_terminal_adds_panel_and_close_kills_it() {
    let workbench = workbench();
    workbench.execute_command("workbench.newTerminal", vec![]).await.unwrap();
    let layout = workbench.layout();
    let Some(Panel::Terminal { id }) = layout.panels.last().cloned() else { panic!("no terminal panel: {layout:?}") };
    assert_eq!(layout.active_panel, Some(format!("terminal:{id}")));
    assert!(workbench.terminals().attach(id).is_ok());
    workbench.close_panel(&format!("terminal:{id}")).unwrap();
    assert!(matches!(workbench.terminals().attach(id), Err(Error::UnknownTerminal(_))));
}
