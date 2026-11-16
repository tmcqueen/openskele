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

mod scripted {
    //! End to end against `plugins/agent-scripted`, a deterministic ACP agent.
    //! Needs bun, and the bridge built (`cargo build -p skele-mcp-bridge`).

    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use skele_core::agent::{Entry, StopReason};
    use skele_core::{HostEvent, Workbench};

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// A workbench with the scripted agent installed, or `None` when bun or the bridge is missing.
    fn setup() -> Option<Workbench> {
        let bridge = repo().join("target/debug/skele-mcp-bridge");
        if std::process::Command::new("bun").arg("--version").output().is_err() || !bridge.exists() {
            eprintln!("skipping: needs bun and a built skele-mcp-bridge");
            return None;
        }
        // Every test sets the same value.
        std::env::set_var("SKELE_MCP_BRIDGE", bridge);
        let workbench = Workbench::new(Default::default()).unwrap();
        workbench.install_plugin(&repo().join("plugins/agent-scripted")).unwrap();
        Some(workbench)
    }

    fn texts(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::AgentText { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn hello_streams_text() {
        let Some(workbench) = setup() else { return };
        let mut events = workbench.bus().subscribe();
        let session = workbench.new_agent_session("scripted").await.unwrap();
        let stop = workbench.agent_prompt(&session, "hello".into()).await.unwrap();
        assert_eq!(stop, StopReason::EndTurn);
        let transcript = workbench.agent_transcript(&session).unwrap();
        assert_eq!(transcript.entries[0], Entry::UserMessage { text: "hello".into() });
        assert_eq!(texts(&transcript.entries), ["Hello from the scripted agent."]);
        let updates = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|e| matches!(e, HostEvent::AgentUpdate { index: 1, .. }))
            .count();
        assert!(updates >= 3, "streamed in {updates} updates");
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn tool_call_goes_through_bridge() {
        let Some(workbench) = setup() else { return };
        let session = workbench.new_agent_session("scripted").await.unwrap();
        let stop = workbench.agent_prompt(&session, "open settings".into()).await.unwrap();
        assert_eq!(stop, StopReason::EndTurn);
        assert_eq!(workbench.layout().active_panel.as_deref(), Some("settings"));
        let transcript = workbench.agent_transcript(&session).unwrap();
        let tool = transcript.entries.iter().find_map(|e| match e {
            Entry::ToolCall { status, .. } => Some(status.clone()),
            _ => None,
        });
        assert_eq!(tool.as_deref(), Some("completed"));
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn plan_is_recorded() {
        let Some(workbench) = setup() else { return };
        let session = workbench.new_agent_session("scripted").await.unwrap();
        workbench.agent_prompt(&session, "plan".into()).await.unwrap();
        let transcript = workbench.agent_transcript(&session).unwrap();
        let plans: Vec<_> = transcript.entries.iter().filter(|e| matches!(e, Entry::Plan { .. })).collect();
        assert_eq!(plans.len(), 1, "a plan update replaces the plan");
        let Entry::Plan { entries } = plans[0] else { unreachable!() };
        assert_eq!(
            entries.iter().map(|e| (e.content.as_str(), e.status.as_str())).collect::<Vec<_>>(),
            [("Read", "completed"), ("Edit", "completed")]
        );
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn read_sees_unsaved_buffer() {
        let Some(workbench) = setup() else { return };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "on disk\n").unwrap();
        let doc = workbench.documents().open(&path).unwrap();
        workbench.documents().set_text(&doc.uri, "changed\nmore\n").unwrap();
        let session = workbench.new_agent_session("scripted").await.unwrap();
        workbench.agent_prompt(&session, format!("read {}", path.display())).await.unwrap();
        assert_eq!(texts(&workbench.agent_transcript(&session).unwrap().entries), ["changed"]);
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn write_goes_through_document_store() {
        let Some(workbench) = setup() else { return };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.txt");
        std::fs::write(&path, "before\n").unwrap();
        // Open in an editor, no unsaved edits: the editor must see the agent's text.
        let doc = workbench.documents().open(&path).unwrap();
        let mut events = workbench.bus().subscribe();
        let session = workbench.new_agent_session("scripted").await.unwrap();
        workbench.agent_prompt(&session, format!("write {}", path.display())).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "written\n");
        assert_eq!(workbench.documents().text(&doc.uri).unwrap(), "written\n");
        let seen: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
        assert!(seen.iter().any(|e| matches!(e, HostEvent::DocumentChanged { uri, .. } if *uri == doc.uri)));
        assert!(seen.iter().any(|e| matches!(e, HostEvent::DocumentSaved { .. })));
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn write_refuses_unsaved_edits() {
        let Some(workbench) = setup() else { return };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.txt");
        std::fs::write(&path, "disk\n").unwrap();
        let doc = workbench.documents().open(&path).unwrap();
        workbench.documents().set_text(&doc.uri, "my unsaved work\n").unwrap();
        let session = workbench.new_agent_session("scripted").await.unwrap();
        let stop = workbench.agent_prompt(&session, format!("write {}", path.display())).await.unwrap();
        assert_eq!(stop, StopReason::Error, "the agent was told the write failed");
        assert_eq!(workbench.documents().text(&doc.uri).unwrap(), "my unsaved work\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "disk\n");
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn read_after_write_sees_later_disk_changes() {
        let Some(workbench) = setup() else { return };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.txt");
        std::fs::write(&path, "first\n").unwrap();
        let session = workbench.new_agent_session("scripted").await.unwrap();
        workbench.agent_prompt(&session, format!("write {}", path.display())).await.unwrap();
        std::fs::write(&path, "changed outside\n").unwrap(); // e.g. git checkout
        workbench.agent_prompt(&session, format!("read {}", path.display())).await.unwrap();
        assert_eq!(texts(&workbench.agent_transcript(&session).unwrap().entries).last().unwrap(), "changed outside");
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn missing_agent_executable_errors_and_issues_no_grant() {
        let tmp = tempfile::tempdir().unwrap();
        let workbench = Workbench::new(Default::default()).unwrap();
        let dir = tmp.path().join("ghost");
        std::fs::create_dir(&dir).unwrap();
        let manifest = serde_json::json!({
            "id": "test.ghost", "name": "Ghost", "version": "0", "protocol": "0.2",
            "contributes": { "agents": [{ "id": "ghost", "name": "Ghost", "command": "/nonexistent/agent" }] }
        });
        std::fs::write(dir.join("skele-plugin.json"), manifest.to_string()).unwrap();
        workbench.install_plugin(&dir).unwrap();
        let result = tokio::time::timeout(Duration::from_secs(10), workbench.new_agent_session("ghost")).await.unwrap();
        assert!(matches!(&result, Err(skele_core::Error::Agent { .. })), "{result:?}");
        assert_eq!(workbench.grants().len(), 0);
    }

    #[tokio::test]
    async fn unknown_agent_errors() {
        let workbench = Workbench::new(Default::default()).unwrap();
        assert!(matches!(workbench.new_agent_session("nope").await, Err(skele_core::Error::UnknownAgent(_))));
    }

    /// Starts `ask` in the background and returns the permission request's id once it shows up.
    async fn ask(
        workbench: &Workbench,
        session: &str,
    ) -> (tokio::task::JoinHandle<skele_core::Result<StopReason>>, String) {
        let mut events = workbench.bus().subscribe();
        let (w, s) = (workbench.clone(), session.to_owned());
        let prompt = tokio::spawn(async move { w.agent_prompt(&s, "ask".into()).await });
        let id = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(HostEvent::AgentUpdate { entry: Entry::PermissionRequest { id, answer: None, .. }, .. }) =
                    events.recv().await
                {
                    return id;
                }
            }
        })
        .await
        .expect("no permission request");
        (prompt, id)
    }

    /// The prompt's stop reason, failing (not hanging) if the turn never ends.
    async fn finish(prompt: tokio::task::JoinHandle<skele_core::Result<StopReason>>) -> StopReason {
        tokio::time::timeout(Duration::from_secs(10), prompt).await.expect("the turn never ended").unwrap().unwrap()
    }

    fn answer_of(workbench: &Workbench, session: &str, request: &str) -> Option<String> {
        workbench.agent_transcript(session).unwrap().entries.into_iter().find_map(|e| match e {
            Entry::PermissionRequest { id, answer, .. } if id == request => answer,
            _ => None,
        })
    }

    #[tokio::test]
    async fn ask_allow() {
        let Some(workbench) = setup() else { return };
        let session = workbench.new_agent_session("scripted").await.unwrap();
        let (prompt, request) = ask(&workbench, &session).await;
        workbench.agent_answer_permission(&session, &request, Some("allow".into())).unwrap();
        assert_eq!(finish(prompt).await, StopReason::EndTurn);
        assert_eq!(texts(&workbench.agent_transcript(&session).unwrap().entries), ["Allowed."]);
        assert_eq!(answer_of(&workbench, &session, &request).as_deref(), Some("allow"));
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn ask_reject() {
        let Some(workbench) = setup() else { return };
        let session = workbench.new_agent_session("scripted").await.unwrap();
        let (prompt, request) = ask(&workbench, &session).await;
        workbench.agent_answer_permission(&session, &request, Some("reject".into())).unwrap();
        assert_eq!(finish(prompt).await, StopReason::EndTurn);
        assert_eq!(texts(&workbench.agent_transcript(&session).unwrap().entries), ["Rejected."]);
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn cancel_while_permission_pending() {
        let Some(workbench) = setup() else { return };
        let session = workbench.new_agent_session("scripted").await.unwrap();
        let (prompt, request) = ask(&workbench, &session).await;
        workbench.agent_cancel(&session).await.unwrap();
        assert_eq!(finish(prompt).await, StopReason::Cancelled);
        assert_eq!(answer_of(&workbench, &session, &request).as_deref(), Some("cancelled"));
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn answer_twice_or_unknown_is_error() {
        let Some(workbench) = setup() else { return };
        let session = workbench.new_agent_session("scripted").await.unwrap();
        let (prompt, request) = ask(&workbench, &session).await;
        workbench.agent_answer_permission(&session, &request, Some("allow".into())).unwrap();
        finish(prompt).await;
        let before = workbench.agent_transcript(&session).unwrap();
        let again = workbench.agent_answer_permission(&session, &request, Some("reject".into()));
        assert!(matches!(again, Err(skele_core::Error::UnknownPermissionRequest(_))));
        let unknown = workbench.agent_answer_permission(&session, "nope", None);
        assert!(matches!(unknown, Err(skele_core::Error::UnknownPermissionRequest(_))));
        assert_eq!(workbench.agent_transcript(&session).unwrap(), before);
        workbench.close_agent_session(&session).await;
    }

    #[tokio::test]
    async fn agent_crash_mid_prompt() {
        let Some(workbench) = setup() else { return };
        let mut events = workbench.bus().subscribe();
        let session = workbench.new_agent_session("scripted").await.unwrap();
        assert_eq!(workbench.grants().len(), 1);
        let stop = tokio::time::timeout(Duration::from_secs(10), workbench.agent_prompt(&session, "crash".into()))
            .await
            .expect("the prompt never ended")
            .unwrap();
        assert_eq!(stop, StopReason::Error);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(HostEvent::AgentSessionEnded { session: s, .. }) = events.recv().await {
                    if s == session {
                        return;
                    }
                }
            }
        })
        .await
        .expect("no AgentSessionEnded");
        assert_eq!(workbench.grants().len(), 0);
        // The conversation stays readable, with one error saying what happened.
        let transcript = workbench.agent_transcript(&session).unwrap();
        let errors = transcript.entries.iter().filter(|e| matches!(e, Entry::Error { .. })).count();
        assert_eq!(errors, 1, "{:?}", transcript.entries);
        assert!(matches!(workbench.agent_prompt(&session, "hello".into()).await, Err(skele_core::Error::Agent { .. })));
        workbench.close_agent_session(&session).await;
        assert!(workbench.agent_transcript(&session).is_err(), "closing forgets it");
    }

    #[tokio::test]
    async fn close_session_revokes_grant() {
        let Some(workbench) = setup() else { return };
        let session = workbench.new_agent_session("scripted").await.unwrap();
        assert_eq!(workbench.grants().len(), 1);
        workbench.close_agent_session(&session).await;
        assert_eq!(workbench.grants().len(), 0);
        assert!(matches!(
            workbench.agent_prompt(&session, "hello".into()).await,
            Err(skele_core::Error::UnknownAgentSession(_))
        ));
    }
}
