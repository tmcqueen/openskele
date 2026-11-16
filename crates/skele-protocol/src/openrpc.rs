//! Generates the OpenRPC document and manifest schema from the Rust types.

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::{json, Map, Value};

use crate::manifest::Manifest;
use crate::methods::*;

struct Doc {
    generator: SchemaGenerator,
    methods: Vec<Value>,
}

impl Doc {
    fn schema<T: JsonSchema>(&mut self) -> Value {
        self.generator.subschema_for::<T>().to_value()
    }

    fn request<R: Request>(&mut self) {
        let mut method = self.method::<R::Params>(R::METHOD, R::DIRECTION, "request");
        let result = self.schema::<R::Result>();
        method.insert("result".into(), json!({ "name": "result", "schema": result }));
        self.methods.push(Value::Object(method));
    }

    // OpenRPC models a notification as a method without `result`.
    fn notification<N: Notification>(&mut self) {
        let method = self.method::<N::Params>(N::METHOD, N::DIRECTION, "notification");
        self.methods.push(Value::Object(method));
    }

    /// OpenRPC wants by-name params listed one by one, so the params struct is
    /// flattened here; `x-params` keeps the reference to it for code generators.
    fn method<P: JsonSchema>(&mut self, name: &str, direction: Direction, kind: &str) -> Map<String, Value> {
        let schema = self.schema::<P>();
        let definition = schema
            .get("$ref")
            .and_then(Value::as_str)
            .and_then(|r| r.rsplit('/').next())
            .and_then(|name| self.generator.definitions().get(name))
            .expect("method params are a named struct");
        let required: Vec<&str> = definition
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let params: Vec<Value> = definition
            .get("properties")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(name, schema)| json!({ "name": name, "required": required.contains(&name.as_str()), "schema": schema }))
            .collect();

        let mut method = Map::new();
        method.insert("name".into(), json!(name));
        method.insert("paramStructure".into(), json!("by-name"));
        method.insert("params".into(), json!(params));
        method.insert("x-params".into(), schema);
        method.insert("x-direction".into(), json!(direction));
        method.insert("x-kind".into(), json!(kind));
        method
    }
}

pub fn openrpc() -> Value {
    let mut settings = SchemaSettings::draft07();
    settings.definitions_path = "/components/schemas".into();
    let mut doc = Doc { generator: settings.into_generator(), methods: Vec::new() };

    doc.request::<Initialize>();
    doc.request::<Shutdown>();
    doc.notification::<Exit>();
    doc.request::<CommandExecute>();
    doc.request::<ViewGetChildren>();
    doc.request::<ToolCall>();
    doc.notification::<ConfigDidChange>();
    doc.notification::<DocumentDidSave>();
    doc.notification::<WindowShowMessage>();
    doc.notification::<TerminalWrite>();
    doc.request::<ConfigGet>();
    doc.notification::<ViewRefresh>();

    json!({
        "openrpc": "1.3.2",
        "info": {
            "title": "openskele plugin protocol",
            "version": crate::PROTOCOL_VERSION,
            "description": "JSON-RPC 2.0 over stdio with Content-Length framing. \
                `x-direction` says which side may send a method; methods without `result` are notifications.",
        },
        "methods": doc.methods,
        "components": { "schemas": doc.generator.definitions() },
    })
}

pub fn manifest_schema() -> Value {
    let generator = SchemaSettings::draft07().into_generator();
    generator.into_root_schema_for::<Manifest>().to_value()
}

/// Where the generated documents are checked in, relative to the repo root.
pub const OPENRPC_PATH: &str = "protocol/openrpc.json";
pub const MANIFEST_SCHEMA_PATH: &str = "protocol/plugin-manifest.schema.json";

pub fn render(value: &Value) -> String {
    let mut out = serde_json::to_string_pretty(value).expect("schema serializes");
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The checked-in documents are what SDKs are generated from, so they must
    /// not drift from the Rust types. Fix with `cargo run -p skele-protocol --bin gen-protocol`.
    #[test]
    fn checked_in_documents_are_current() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (path, value) in [(OPENRPC_PATH, openrpc()), (MANIFEST_SCHEMA_PATH, manifest_schema())] {
            let on_disk = std::fs::read_to_string(root.join(path)).unwrap_or_default();
            assert!(on_disk == render(&value), "{path} is stale; regenerate it");
        }
    }
}
