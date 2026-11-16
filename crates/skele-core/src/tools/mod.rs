//! Tools an agent can call: built into the host, or contributed by plugins.
//! Arguments are checked against each tool's schema before it runs.

pub(crate) mod builtin;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use jsonschema::Validator;
use serde::Serialize;
use serde_json::Value;
pub use skele_protocol::methods::{ToolCallResult, ToolContent};

use crate::grants::Grant;
use crate::{Error, Result, Workbench};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
    pub destructive: bool,
    /// Contributing plugin; `None` for built-ins.
    pub source: Option<String>,
}

/// One call: its validated arguments and the grant it came in under.
pub struct ToolCall {
    pub arguments: Value,
    pub grant: Grant,
}

pub type ToolFuture = Pin<Box<dyn Future<Output = Result<ToolCallResult>> + Send>>;
pub type ToolHandler = Arc<dyn Fn(ToolCall) -> ToolFuture + Send + Sync>;

/// Where a tool runs.
#[derive(Clone)]
pub enum ToolTarget {
    Builtin(ToolHandler),
    /// In the sidecar of the plugin with this id, through `tool/call`.
    Plugin(String),
}

struct Entry {
    info: ToolInfo,
    target: ToolTarget,
    validator: Arc<Validator>,
}

/// MCP-safe tool names: a lowercase letter, then lowercase letters, digits or `_`.
pub fn valid_tool_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: RwLock<BTreeMap<String, Entry>>,
}

impl ToolRegistry {
    pub fn register(&self, info: ToolInfo, target: ToolTarget) -> Result<()> {
        if !valid_tool_name(&info.name) {
            return Err(Error::InvalidToolName(info.name));
        }
        let validator = jsonschema::validator_for(&info.input_schema)
            .map_err(|e| Error::InvalidSchema { key: info.name.clone(), message: e.to_string() })?;
        let mut tools = self.tools.write().unwrap();
        if tools.contains_key(&info.name) {
            return Err(Error::DuplicateTool(info.name));
        }
        tools.insert(info.name.clone(), Entry { info, target, validator: Arc::new(validator) });
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<(ToolInfo, ToolTarget)> {
        self.tools.read().unwrap().get(name).map(|e| (e.info.clone(), e.target.clone()))
    }

    /// Every tool, by name.
    pub fn list(&self) -> Vec<ToolInfo> {
        self.tools.read().unwrap().values().map(|e| e.info.clone()).collect()
    }

    pub fn remove_source(&self, source: &str) {
        self.tools.write().unwrap().retain(|_, e| e.info.source.as_deref() != Some(source));
    }

    fn resolve(&self, name: &str) -> Option<(ToolTarget, Arc<Validator>)> {
        self.tools.read().unwrap().get(name).map(|e| (e.target.clone(), e.validator.clone()))
    }
}

impl Workbench {
    pub fn tools(&self) -> &ToolRegistry {
        &self.inner.tools
    }

    /// Runs a tool. Only an unknown name is an `Err`: bad arguments and
    /// failures inside the tool are a result with `is_error`, for the agent to read.
    pub async fn call_tool(&self, name: &str, arguments: Value, grant: &Grant) -> Result<ToolCallResult> {
        let (target, validator) = self.inner.tools.resolve(name).ok_or_else(|| Error::UnknownTool(name.to_owned()))?;
        if let Err(e) = validator.validate(&arguments) {
            return Ok(ToolCallResult::error(format!("invalid arguments: {e}")));
        }
        let outcome = match target {
            ToolTarget::Builtin(handler) => handler(ToolCall { arguments, grant: grant.clone() }).await,
            ToolTarget::Plugin(plugin) => self.call_plugin_tool(&plugin, name, arguments).await,
        };
        Ok(outcome.unwrap_or_else(|e| ToolCallResult::error(e.to_string())))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::grants::GrantKind;

    fn echo(called: Arc<AtomicBool>) -> (ToolInfo, ToolTarget) {
        let info = ToolInfo {
            name: "echo".into(),
            title: None,
            description: "Echoes text".into(),
            input_schema: json!({ "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] }),
            read_only: true,
            destructive: false,
            source: None,
        };
        let target = ToolTarget::Builtin(Arc::new(move |call: ToolCall| {
            called.store(true, Ordering::SeqCst);
            Box::pin(async move {
                let text = call.arguments["text"].as_str().unwrap_or_default().to_owned();
                Ok(ToolCallResult::text(text))
            })
        }));
        (info, target)
    }

    fn setup() -> (Workbench, Grant, Arc<AtomicBool>) {
        let workbench = Workbench::new(Default::default()).unwrap();
        let grant = workbench.grants().issue(GrantKind::AgentSession { session_id: "s".into() }, "test");
        let called = Arc::new(AtomicBool::new(false));
        let (info, target) = echo(called.clone());
        workbench.tools().register(info, target).unwrap();
        (workbench, grant, called)
    }

    #[tokio::test]
    async fn call_echo_returns_text() {
        let (workbench, grant, _) = setup();
        let result = workbench.call_tool("echo", json!({ "text": "hi" }), &grant).await.unwrap();
        assert!(!result.is_error);
        assert_eq!(result.content[0].text, "hi");
    }

    #[tokio::test]
    async fn missing_required_arg_is_tool_error_and_handler_not_called() {
        let (workbench, grant, called) = setup();
        let result = workbench.call_tool("echo", json!({}), &grant).await.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("text"), "{:?}", result.content);
        assert!(!called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn wrong_type_is_tool_error() {
        let (workbench, grant, _) = setup();
        assert!(workbench.call_tool("echo", json!({ "text": 3 }), &grant).await.unwrap().is_error);
    }

    #[tokio::test]
    async fn handler_error_is_tool_error() {
        let (workbench, grant, _) = setup();
        let info = ToolInfo {
            name: "fails".into(),
            input_schema: json!({ "type": "object" }),
            ..workbench.tools().get("echo").unwrap().0
        };
        let target = ToolTarget::Builtin(Arc::new(|_| Box::pin(async { Err(crate::Error::Other("broken".into())) })));
        workbench.tools().register(info, target).unwrap();
        let result = workbench.call_tool("fails", json!({}), &grant).await.unwrap();
        assert!(result.is_error);
        assert_eq!(result.content[0].text, "broken");
    }

    #[tokio::test]
    async fn unknown_tool_is_err() {
        let (workbench, grant, _) = setup();
        assert!(matches!(workbench.call_tool("nope", json!({}), &grant).await, Err(crate::Error::UnknownTool(_))));
    }

    #[test]
    fn duplicate_tool_rejected() {
        let (workbench, _, called) = setup();
        let (info, target) = echo(called);
        assert!(matches!(workbench.tools().register(info, target), Err(crate::Error::DuplicateTool(_))));
    }

    #[test]
    fn bad_names_rejected() {
        for name in ["hello.greet", "Hello", "1x", "", "a-b"] {
            assert!(!valid_tool_name(name), "{name:?} should be invalid");
        }
        for name in ["a", "hello_greet", "x1"] {
            assert!(valid_tool_name(name), "{name:?} should be valid");
        }
        let (workbench, _, called) = setup();
        let (mut info, target) = echo(called);
        info.name = "Bad.Name".into();
        assert!(matches!(workbench.tools().register(info, target), Err(crate::Error::InvalidToolName(_))));
    }

    #[test]
    fn invalid_schema_rejected() {
        let (workbench, _, called) = setup();
        let (mut info, target) = echo(called);
        info.name = "bad_schema".into();
        info.input_schema = json!({ "type": 12 });
        assert!(matches!(workbench.tools().register(info, target), Err(crate::Error::InvalidSchema { .. })));
    }

    #[tokio::test]
    async fn handler_sees_grant() {
        let (workbench, grant, called) = setup();
        let (mut info, _) = echo(called);
        info.name = "whoami".into();
        info.input_schema = json!({ "type": "object" });
        let target = ToolTarget::Builtin(Arc::new(|call: ToolCall| {
            Box::pin(async move { Ok(ToolCallResult::text(call.grant.label)) })
        }));
        workbench.tools().register(info, target).unwrap();
        assert_eq!(workbench.call_tool("whoami", json!({}), &grant).await.unwrap().content[0].text, "test");
    }

    #[test]
    fn list_is_sorted_by_name() {
        let (workbench, _, _) = setup();
        let names: Vec<_> = workbench.tools().list().into_iter().map(|t| t.name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}
