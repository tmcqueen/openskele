//! A session's transcript as data: what the pane renders and what tests read.
//! Built from ACP `session/update` notifications by the client.

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub content: String,
    /// `pending`, `in_progress` or `completed`.
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub id: String,
    pub name: String,
    /// `allow_once`, `allow_always`, `reject_once` or `reject_always`.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Entry {
    UserMessage { text: String },
    /// Markdown, streamed: consecutive chunks extend one entry.
    AgentText { text: String },
    Thought { text: String },
    ToolCall { id: String, title: String, kind: String, status: String, content: Vec<String> },
    /// The agent's current plan; a new plan replaces it in place.
    Plan { entries: Vec<PlanEntry> },
    /// `answer` is the chosen option id, `"cancelled"`, or `None` while waiting.
    PermissionRequest { id: String, title: String, options: Vec<PermissionOption>, answer: Option<String> },
    Error { message: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub entries: Vec<Entry>,
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Object(o) if o.get("type").and_then(Value::as_str) == Some("text") => {
            o.get("text").and_then(Value::as_str).unwrap_or_default().to_owned()
        }
        // A tool call's content item wraps a content block.
        Value::Object(o) => o.get("content").map(text_of).unwrap_or_default(),
        _ => String::new(),
    }
}

fn str_of(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

impl Transcript {
    /// Appends an entry and returns its index.
    pub fn push(&mut self, entry: Entry) -> usize {
        self.entries.push(entry);
        self.entries.len() - 1
    }

    /// Applies one ACP session update, given as its wire JSON. Returns the
    /// index of the entry that changed, if any.
    pub fn apply(&mut self, update: &Value) -> Option<usize> {
        match update.get("sessionUpdate")?.as_str()? {
            "agent_message_chunk" => Some(self.chunk(false, &text_of(update.get("content")?))),
            "agent_thought_chunk" => Some(self.chunk(true, &text_of(update.get("content")?))),
            "tool_call" | "tool_call_update" => {
                let id = str_of(update, "toolCallId")?;
                Some(self.tool_call(&id, update))
            }
            "plan" => {
                let entries = update
                    .get("entries")?
                    .as_array()?
                    .iter()
                    .map(|e| PlanEntry {
                        content: str_of(e, "content").unwrap_or_default(),
                        status: str_of(e, "status").unwrap_or_default(),
                    })
                    .collect();
                Some(self.plan(entries))
            }
            _ => None,
        }
    }

    fn chunk(&mut self, thought: bool, text: &str) -> usize {
        match self.entries.last_mut() {
            Some(Entry::AgentText { text: last }) if !thought => last.push_str(text),
            Some(Entry::Thought { text: last }) if thought => last.push_str(text),
            _ => {
                let text = text.to_owned();
                return self.push(if thought { Entry::Thought { text } } else { Entry::AgentText { text } });
            }
        }
        self.entries.len() - 1
    }

    fn tool_call(&mut self, id: &str, update: &Value) -> usize {
        let existing = self.entries.iter().position(|e| matches!(e, Entry::ToolCall { id: i, .. } if i == id));
        let index = existing.unwrap_or_else(|| {
            self.push(Entry::ToolCall {
                id: id.to_owned(),
                title: String::new(),
                kind: "other".into(),
                status: "pending".into(),
                content: Vec::new(),
            })
        });
        if let Entry::ToolCall { title, kind, status, content, .. } = &mut self.entries[index] {
            if let Some(t) = str_of(update, "title") {
                *title = t;
            }
            if let Some(k) = str_of(update, "kind") {
                *kind = k;
            }
            if let Some(s) = str_of(update, "status") {
                *status = s;
            }
            if let Some(items) = update.get("content").and_then(Value::as_array) {
                *content = items.iter().map(text_of).collect();
            }
        }
        index
    }

    fn plan(&mut self, entries: Vec<PlanEntry>) -> usize {
        match self.entries.iter().rposition(|e| matches!(e, Entry::Plan { .. })) {
            Some(index) => {
                self.entries[index] = Entry::Plan { entries };
                index
            }
            None => self.push(Entry::Plan { entries }),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn chunks_join_until_something_else_comes() {
        let mut t = Transcript::default();
        let chunk = |text: &str| json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } });
        assert_eq!(t.apply(&chunk("a")), Some(0));
        assert_eq!(t.apply(&chunk("b")), Some(0));
        t.apply(&json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": "hm" } }));
        assert_eq!(t.apply(&chunk("c")), Some(2));
        assert_eq!(t.entries[0], Entry::AgentText { text: "ab".into() });
        assert_eq!(t.entries[1], Entry::Thought { text: "hm".into() });
    }

    #[test]
    fn tool_call_updates_in_place() {
        let mut t = Transcript::default();
        t.apply(&json!({ "sessionUpdate": "tool_call", "toolCallId": "x", "title": "Do", "kind": "edit", "status": "in_progress" }));
        let i = t.apply(&json!({
            "sessionUpdate": "tool_call_update", "toolCallId": "x", "status": "completed",
            "content": [{ "type": "content", "content": { "type": "text", "text": "ok" } }]
        }));
        assert_eq!(i, Some(0));
        assert_eq!(t.entries[0], Entry::ToolCall {
            id: "x".into(),
            title: "Do".into(),
            kind: "edit".into(),
            status: "completed".into(),
            content: vec!["ok".into()]
        });
    }

    #[test]
    fn unknown_updates_are_ignored() {
        let mut t = Transcript::default();
        assert_eq!(t.apply(&json!({ "sessionUpdate": "current_mode_update" })), None);
        assert!(t.entries.is_empty());
    }
}
