//! Built-in tools: thin wrappers over what the workbench already does, so an
//! agent can drive everything a person can.

use std::path::Path;
use std::sync::{Arc, Weak};

use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use skele_protocol::manifest::BarId;
use skele_protocol::methods::MessageLevel;

use super::{ToolCall, ToolCallResult, ToolFuture, ToolInfo, ToolTarget};
use crate::config::Scope;
use crate::layout::Panel;
use crate::workbench::Inner;
use crate::{Error, Result, Workbench};

type Run = fn(Workbench, Value) -> ToolFuture;

/// Reads the (already validated) arguments into a struct.
fn args<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| Error::Other(format!("invalid arguments: {e}")))
}

/// A result whose text is pretty-printed JSON.
fn json_result(value: impl serde::Serialize) -> Result<ToolCallResult> {
    Ok(ToolCallResult::text(serde_json::to_string_pretty(&value)?))
}

fn done() -> Result<ToolCallResult> {
    Ok(ToolCallResult::text("done"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandArgs {
    command: String,
    #[serde(default)]
    args: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewArgs {
    view_id: String,
    parent: Option<String>,
}

#[derive(Deserialize)]
struct KeyArgs {
    key: String,
}

#[derive(Deserialize)]
struct ConfigSetArgs {
    key: String,
    value: Value,
    #[serde(default)]
    scope: Option<Scope>,
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct UriArgs {
    uri: String,
}

#[derive(Deserialize)]
struct EditArgs {
    uri: String,
    start: usize,
    end: usize,
    text: String,
}

#[derive(Deserialize)]
struct TerminalArgs {
    id: u32,
}

#[derive(Deserialize)]
struct TerminalWriteArgs {
    id: u32,
    data: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BarPatch {
    visible: Option<bool>,
    /// Absent: unchanged. `null`: collapse the bar's panel.
    #[serde(default, deserialize_with = "present")]
    active_container: Option<Option<String>>,
}

/// Tells an explicit `null` (Some(None)) from an absent field (None).
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LayoutPatch {
    primary: Option<BarPatch>,
    secondary: Option<BarPatch>,
    active_panel: Option<String>,
}

#[derive(Deserialize)]
struct MessageArgs {
    message: String,
    #[serde(default)]
    level: Option<MessageLevel>,
}

#[derive(Deserialize)]
struct ChannelArgs {
    channel: String,
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false })
}

fn bar_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "visible": { "type": "boolean" },
            "activeContainer": { "type": ["string", "null"], "description": "Container id; null collapses the bar's panel." }
        },
        "additionalProperties": false
    })
}

/// (name, description, input schema, read_only, destructive, handler)
fn table() -> Vec<(&'static str, &'static str, Value, bool, bool, Run)> {
    vec![
        (
            "workbench_snapshot",
            "Everything the workbench offers right now: commands, menus, keybindings, view containers, views, the layout, and plugins with their state.",
            object(json!({}), &[]),
            true,
            false,
            |w, _| Box::pin(async move { json_result(w.snapshot()) }),
        ),
        (
            "command_execute",
            "Runs any workbench or plugin command by id (see workbench_snapshot for the list) and returns its result.",
            object(json!({ "command": { "type": "string" }, "args": { "type": "array" } }), &["command"]),
            false,
            true,
            |w, a| Box::pin(async move {
                let a: CommandArgs = args(a)?;
                json_result(w.execute_command(&a.command, a.args).await?)
            }),
        ),
        (
            "view_get_children",
            "Rows of a contributed tree view, or the children of one row (by its id).",
            object(json!({ "viewId": { "type": "string" }, "parent": { "type": "string" } }), &["viewId"]),
            true,
            false,
            |w, a| Box::pin(async move {
                let a: ViewArgs = args(a)?;
                json_result(w.view_children(&a.view_id, a.parent).await?)
            }),
        ),
        (
            "config_list",
            "All settings, by section: each key with its schema, user and workspace values, and effective value.",
            object(json!({}), &[]),
            true,
            false,
            |w, _| Box::pin(async move { json_result(w.config().sections()) }),
        ),
        (
            "config_get",
            "The effective value of a setting: workspace, then user, then its default.",
            object(json!({ "key": { "type": "string" } }), &["key"]),
            true,
            false,
            |w, a| Box::pin(async move { json_result(w.config().get(&args::<KeyArgs>(a)?.key)) }),
        ),
        (
            "config_set",
            "Sets a setting, validated against its schema. Scope is user (default) or workspace; a null value clears it in that scope.",
            object(
                json!({ "key": { "type": "string" }, "value": {}, "scope": { "enum": ["user", "workspace"] } }),
                &["key", "value"],
            ),
            false,
            false,
            |w, a| Box::pin(async move {
                let a: ConfigSetArgs = args(a)?;
                let value = (!a.value.is_null()).then_some(a.value);
                w.set_config(&a.key, value, a.scope.unwrap_or(Scope::User))?;
                done()
            }),
        ),
        (
            "document_open",
            "Opens a file in an editor panel and returns its uri and version. Edits go to the open buffer until document_save.",
            object(json!({ "path": { "type": "string" } }), &["path"]),
            false,
            false,
            |w, a| Box::pin(async move {
                let snapshot = w.documents().open(Path::new(&args::<PathArgs>(a)?.path))?;
                w.open_panel(Panel::Editor { path: snapshot.path.clone() });
                json_result(json!({ "uri": snapshot.uri, "version": snapshot.version }))
            }),
        ),
        (
            "document_read",
            "The current text of an open document, unsaved edits included.",
            object(json!({ "uri": { "type": "string" } }), &["uri"]),
            true,
            false,
            |w, a| Box::pin(async move { Ok(ToolCallResult::text(w.documents().text(&args::<UriArgs>(a)?.uri)?)) }),
        ),
        (
            "document_edit",
            "Replaces the characters from start to end (character offsets, end exclusive) of an open document with text. Returns the new version.",
            object(
                json!({
                    "uri": { "type": "string" },
                    "start": { "type": "integer", "minimum": 0 },
                    "end": { "type": "integer", "minimum": 0 },
                    "text": { "type": "string" }
                }),
                &["uri", "start", "end", "text"],
            ),
            false,
            false,
            |w, a| Box::pin(async move {
                let a: EditArgs = args(a)?;
                json_result(json!({ "version": w.documents().edit(&a.uri, a.start, a.end, &a.text)? }))
            }),
        ),
        (
            "document_save",
            "Writes an open document to disk.",
            object(json!({ "uri": { "type": "string" } }), &["uri"]),
            false,
            true,
            |w, a| Box::pin(async move { json_result(json!({ "version": w.save_document(&args::<UriArgs>(a)?.uri)? })) }),
        ),
        (
            "terminal_open",
            "Starts a shell in the workspace in a new terminal panel and returns its id.",
            object(json!({}), &[]),
            false,
            false,
            |w, _| Box::pin(async move { json_result(json!({ "id": w.new_terminal()? })) }),
        ),
        (
            "terminal_write",
            "Types data into a terminal, as if at the keyboard. End a command with \\n to run it.",
            object(json!({ "id": { "type": "integer", "minimum": 0 }, "data": { "type": "string" } }), &["id", "data"]),
            false,
            true,
            |w, a| Box::pin(async move {
                let a: TerminalWriteArgs = args(a)?;
                w.terminals().write(a.id, &a.data)?;
                done()
            }),
        ),
        (
            "terminal_read",
            "The recent output of a terminal (up to 64 KiB), escape sequences included.",
            object(json!({ "id": { "type": "integer", "minimum": 0 } }), &["id"]),
            true,
            false,
            |w, a| Box::pin(async move { Ok(ToolCallResult::text(w.terminals().read(args::<TerminalArgs>(a)?.id)?)) }),
        ),
        (
            "layout_get",
            "Both side bars (position, visibility, open container) and the open panels with the active one.",
            object(json!({}), &[]),
            true,
            false,
            |w, _| Box::pin(async move { json_result(w.layout()) }),
        ),
        (
            "layout_set",
            "Changes the layout: show or hide a side bar, open one of its containers (null collapses it), or activate an open panel by id. Omitted fields stay as they are.",
            object(
                json!({ "primary": bar_schema(), "secondary": bar_schema(), "activePanel": { "type": "string" } }),
                &[],
            ),
            false,
            false,
            |w, a| Box::pin(async move {
                let a: LayoutPatch = args(a)?;
                for (bar, patch) in [(BarId::Primary, a.primary), (BarId::Secondary, a.secondary)] {
                    let Some(patch) = patch else { continue };
                    if let Some(visible) = patch.visible {
                        w.set_bar_visible(bar, visible);
                    }
                    if let Some(container) = patch.active_container {
                        w.set_active_container(bar, container)?;
                    }
                }
                if let Some(panel) = a.active_panel {
                    w.set_active_panel(&panel)?;
                }
                json_result(w.layout())
            }),
        ),
        (
            "message_show",
            "Shows the user a short message and logs it in the Workbench output channel.",
            object(json!({ "message": { "type": "string" }, "level": { "enum": ["info", "warning", "error"] } }), &["message"]),
            false,
            false,
            |w, a| Box::pin(async move {
                let a: MessageArgs = args(a)?;
                w.notify(a.level.unwrap_or(MessageLevel::Info), &a.message, Some("agent"));
                done()
            }),
        ),
        (
            "output_list",
            "Names of the output channels: Workbench, then one per plugin and any others.",
            object(json!({}), &[]),
            true,
            false,
            |w, _| Box::pin(async move { json_result(w.output_channels()) }),
        ),
        (
            "output_read",
            "The recent text of an output channel, such as a plugin's stderr; useful when something failed.",
            object(json!({ "channel": { "type": "string" } }), &["channel"]),
            true,
            false,
            |w, a| Box::pin(async move { Ok(ToolCallResult::text(w.output_read(&args::<ChannelArgs>(a)?.channel)?)) }),
        ),
    ]
}

/// Registers every built-in tool. Each holds the workbench weakly.
pub(crate) fn register(workbench: &Workbench) -> Result<()> {
    let weak: Weak<Inner> = Arc::downgrade(&workbench.inner);
    for (name, description, input_schema, read_only, destructive, run) in table() {
        let info = ToolInfo {
            name: name.to_owned(),
            title: None,
            description: description.to_owned(),
            input_schema,
            read_only,
            destructive,
            source: None,
        };
        let weak = weak.clone();
        let target = ToolTarget::Builtin(Arc::new(move |call: ToolCall| match weak.upgrade() {
            Some(inner) => run(Workbench { inner }, call.arguments),
            None => Box::pin(async { Err(Error::Other("the workbench is shutting down".into())) }),
        }));
        workbench.tools().register(info, target)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use crate::grants::{Grant, GrantKind};
    use crate::layout::{BarPosition, Panel};
    use crate::output::WORKBENCH_CHANNEL;
    use crate::tools::ToolCallResult;
    use crate::{Workbench, WorkbenchOptions};

    fn setup() -> (Workbench, Grant) {
        let workbench = Workbench::new(WorkbenchOptions::default()).unwrap();
        let grant = workbench.grants().issue(GrantKind::AgentSession { session_id: "s".into() }, "test");
        (workbench, grant)
    }

    async fn call(workbench: &Workbench, grant: &Grant, name: &str, args: Value) -> ToolCallResult {
        workbench.call_tool(name, args, grant).await.unwrap()
    }

    fn ok(result: &ToolCallResult) -> &str {
        assert!(!result.is_error, "tool error: {:?}", result.content);
        &result.content[0].text
    }

    fn json_of(result: &ToolCallResult) -> Value {
        serde_json::from_str(ok(result)).unwrap()
    }

    #[test]
    fn annotations_match_table() {
        let (workbench, _) = setup();
        // (name, read_only, destructive)
        let expected = [
            ("command_execute", false, true),
            ("config_get", true, false),
            ("config_list", true, false),
            ("config_set", false, false),
            ("document_edit", false, false),
            ("document_open", false, false),
            ("document_read", true, false),
            ("document_save", false, true),
            ("layout_get", true, false),
            ("layout_set", false, false),
            ("message_show", false, false),
            ("output_list", true, false),
            ("output_read", true, false),
            ("terminal_open", false, false),
            ("terminal_read", true, false),
            ("terminal_write", false, true),
            ("view_get_children", true, false),
            ("workbench_snapshot", true, false),
        ];
        let tools = workbench.tools().list();
        let actual: Vec<_> = tools.iter().map(|t| (t.name.as_str(), t.read_only, t.destructive)).collect();
        assert_eq!(actual, expected);
        assert!(tools.iter().all(|t| t.source.is_none() && !t.description.is_empty()));
    }

    #[tokio::test]
    async fn workbench_snapshot_lists_commands() {
        let (workbench, grant) = setup();
        let snapshot = json_of(&call(&workbench, &grant, "workbench_snapshot", json!({})).await);
        assert!(snapshot["commands"].as_array().unwrap().iter().any(|c| c["id"] == "workbench.openSettings"));
        assert!(snapshot["layout"]["primary"].is_object());
    }

    #[tokio::test]
    async fn command_execute_runs_builtin() {
        let (workbench, grant) = setup();
        ok(&call(&workbench, &grant, "command_execute", json!({ "command": "workbench.openSettings" })).await);
        assert_eq!(workbench.layout().active_panel.as_deref(), Some("settings"));
    }

    #[tokio::test]
    async fn command_execute_unknown_is_tool_error() {
        let (workbench, grant) = setup();
        assert!(call(&workbench, &grant, "command_execute", json!({ "command": "nope" })).await.is_error);
    }

    #[tokio::test]
    async fn config_set_then_get() {
        let (workbench, grant) = setup();
        let args = json!({ "key": "workbench.colorTheme", "value": "light" });
        ok(&call(&workbench, &grant, "config_set", args).await);
        let value = json_of(&call(&workbench, &grant, "config_get", json!({ "key": "workbench.colorTheme" })).await);
        assert_eq!(value, json!("light"));
    }

    #[tokio::test]
    async fn config_set_invalid_is_tool_error() {
        let (workbench, grant) = setup();
        let args = json!({ "key": "workbench.colorTheme", "value": "purple" });
        assert!(call(&workbench, &grant, "config_set", args).await.is_error);
    }

    #[tokio::test]
    async fn config_set_workspace_scope_without_workspace_is_tool_error() {
        let (workbench, grant) = setup();
        let args = json!({ "key": "workbench.colorTheme", "value": "light", "scope": "workspace" });
        assert!(call(&workbench, &grant, "config_set", args).await.is_error);
    }

    #[tokio::test]
    async fn config_list_has_workbench_section() {
        let (workbench, grant) = setup();
        let sections = json_of(&call(&workbench, &grant, "config_list", json!({})).await);
        assert!(sections.as_array().unwrap().iter().any(|s| s["title"] == "Workbench"));
    }

    #[tokio::test]
    async fn document_open_read_edit_save() {
        let (workbench, grant) = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hello\n").unwrap();

        let opened = json_of(&call(&workbench, &grant, "document_open", json!({ "path": path })).await);
        let uri = opened["uri"].as_str().unwrap().to_owned();
        let canonical = std::fs::canonicalize(&path).unwrap().display().to_string();
        assert!(workbench.layout().panels.contains(&Panel::Editor { path: canonical }));

        assert_eq!(ok(&call(&workbench, &grant, "document_read", json!({ "uri": uri })).await), "hello\n");
        let edit = json!({ "uri": uri, "start": 0, "end": 5, "text": "howdy" });
        ok(&call(&workbench, &grant, "document_edit", edit).await);
        assert_eq!(ok(&call(&workbench, &grant, "document_read", json!({ "uri": uri })).await), "howdy\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\n");
        ok(&call(&workbench, &grant, "document_save", json!({ "uri": uri })).await);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "howdy\n");
    }

    #[tokio::test]
    async fn document_read_unknown_is_tool_error() {
        let (workbench, grant) = setup();
        assert!(call(&workbench, &grant, "document_read", json!({ "uri": "file:///nope" })).await.is_error);
    }

    #[tokio::test]
    async fn document_edit_out_of_range_is_tool_error() {
        let (workbench, grant) = setup();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "hi").unwrap();
        let opened = json_of(&call(&workbench, &grant, "document_open", json!({ "path": path })).await);
        let edit = json!({ "uri": opened["uri"], "start": 0, "end": 99, "text": "x" });
        assert!(call(&workbench, &grant, "document_edit", edit).await.is_error);
    }

    #[tokio::test]
    async fn layout_get_and_set() {
        let (workbench, grant) = setup();
        let layout = json_of(&call(&workbench, &grant, "layout_get", json!({})).await);
        assert_eq!(layout["primary"]["visible"], json!(true));

        ok(&call(&workbench, &grant, "layout_set", json!({ "primary": { "visible": false } })).await);
        assert!(!workbench.layout().primary.visible);
        ok(&call(&workbench, &grant, "layout_set", json!({ "primary": { "visible": false } })).await);
        assert!(!workbench.layout().primary.visible, "setting, not toggling");

        ok(&call(&workbench, &grant, "layout_set", json!({ "activePanel": "output" })).await);
        assert_eq!(workbench.layout().active_panel.as_deref(), Some("output"));
        ok(&call(&workbench, &grant, "layout_set", json!({ "secondary": { "activeContainer": null } })).await);
        assert_eq!(workbench.layout().secondary.active_container, None);
        assert_eq!(workbench.layout().primary.position, BarPosition::Left);
    }

    #[tokio::test]
    async fn layout_set_unknown_panel_is_tool_error() {
        let (workbench, grant) = setup();
        assert!(call(&workbench, &grant, "layout_set", json!({ "activePanel": "editor:/nope" })).await.is_error);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_open_write_read() {
        let (workbench, grant) = setup();
        workbench.set_config("terminal.shell", Some(json!("/bin/sh")), crate::config::Scope::User).unwrap();
        let opened = json_of(&call(&workbench, &grant, "terminal_open", json!({})).await);
        let id = opened["id"].as_u64().unwrap();
        ok(&call(&workbench, &grant, "terminal_write", json!({ "id": id, "data": "echo sk''ele-tool\n" })).await);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let text = ok(&call(&workbench, &grant, "terminal_read", json!({ "id": id })).await).to_owned();
            if text.contains("skele-tool") {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "no output: {text:?}");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        workbench.terminals().close_all();
    }

    #[tokio::test]
    async fn terminal_read_unknown_is_tool_error() {
        let (workbench, grant) = setup();
        assert!(call(&workbench, &grant, "terminal_read", json!({ "id": 999 })).await.is_error);
    }

    #[tokio::test]
    async fn message_show_logs_to_workbench() {
        let (workbench, grant) = setup();
        ok(&call(&workbench, &grant, "message_show", json!({ "message": "hi", "level": "warning" })).await);
        assert_eq!(workbench.output_read(WORKBENCH_CHANNEL).unwrap(), "[warning] agent: hi\r\n");
    }

    #[tokio::test]
    async fn output_list_and_read() {
        let (workbench, grant) = setup();
        workbench.output("Hello", "hi\r\n");
        let channels = json_of(&call(&workbench, &grant, "output_list", json!({})).await);
        assert_eq!(channels, json!([WORKBENCH_CHANNEL, "Hello"]));
        assert_eq!(ok(&call(&workbench, &grant, "output_read", json!({ "channel": "Hello" })).await), "hi\r\n");
        assert!(call(&workbench, &grant, "output_read", json!({ "channel": "nope" })).await.is_error);
    }

    #[tokio::test]
    async fn view_get_children_unknown_is_tool_error() {
        let (workbench, grant) = setup();
        assert!(call(&workbench, &grant, "view_get_children", json!({ "viewId": "nope" })).await.is_error);
    }
}
