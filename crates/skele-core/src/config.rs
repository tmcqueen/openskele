//! Schema-driven configuration with user and workspace scopes.
//!
//! Contributors register JSON Schemas per setting key. Values are validated
//! against them on write, and the effective value of a key is the first of
//! workspace value, user value, schema default.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    User,
    Workspace,
}

/// A group of settings as the settings UI needs it: schema plus current values.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ConfigSection {
    /// Contributing plugin; `None` for the core.
    pub source: Option<String>,
    pub title: String,
    pub properties: Vec<ConfigProperty>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ConfigProperty {
    pub key: String,
    /// The setting's JSON Schema, verbatim from the contributor.
    pub schema: Value,
    pub user: Option<Value>,
    pub workspace: Option<Value>,
    pub effective: Value,
}

struct Section {
    source: Option<String>,
    title: String,
    properties: BTreeMap<String, Property>,
}

struct Property {
    schema: Value,
    validator: Validator,
}

impl Property {
    fn check(&self, key: &str, value: &Value) -> Result<()> {
        let errors: Vec<String> = self.validator.iter_errors(value).map(|e| e.to_string()).collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::InvalidSetting { key: key.to_owned(), message: errors.join("; ") })
        }
    }
}

#[derive(Default)]
struct Layer {
    path: Option<PathBuf>,
    values: Map<String, Value>,
}

impl Layer {
    /// A missing file is an empty layer; an unreadable one is an error rather
    /// than something to silently overwrite on the next save.
    fn load(path: Option<PathBuf>) -> Result<Self> {
        let values = match path.as_deref().map(std::fs::read) {
            Some(Ok(bytes)) => serde_json::from_slice(&bytes)?,
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => Map::new(),
        };
        Ok(Self { path, values })
    }

    fn save(&self) -> Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut text = serde_json::to_string_pretty(&self.values)?;
        text.push('\n');
        // Write-then-rename so a crash never leaves a truncated settings file.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

pub struct ConfigStore {
    sections: RwLock<Vec<Section>>,
    user: RwLock<Layer>,
    /// `None` when no workspace is open.
    workspace: Option<RwLock<Layer>>,
}

impl ConfigStore {
    /// `user_file` of `None` keeps user settings in memory only.
    pub fn open(user_file: Option<PathBuf>, workspace_file: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            sections: RwLock::default(),
            user: RwLock::new(Layer::load(user_file)?),
            workspace: match workspace_file {
                Some(path) => Some(RwLock::new(Layer::load(Some(path))?)),
                None => None,
            },
        })
    }

    pub fn workspace_file(workspace: &Path) -> PathBuf {
        workspace.join(".skele").join("settings.json")
    }

    /// Registers the schemas for a group of settings. Fails without
    /// registering anything if a schema is malformed or a key is taken.
    pub fn register_section(
        &self,
        source: Option<&str>,
        title: &str,
        properties: &BTreeMap<String, Value>,
    ) -> Result<()> {
        let mut sections = self.sections.write().unwrap();
        let mut compiled = BTreeMap::new();
        for (key, schema) in properties {
            if sections.iter().any(|s| s.properties.contains_key(key)) {
                return Err(Error::InvalidSchema { key: key.clone(), message: "setting already defined".into() });
            }
            let validator = jsonschema::validator_for(schema)
                .map_err(|e| Error::InvalidSchema { key: key.clone(), message: e.to_string() })?;
            compiled.insert(key.clone(), Property { schema: schema.clone(), validator });
        }
        sections.push(Section { source: source.map(str::to_owned), title: title.to_owned(), properties: compiled });
        Ok(())
    }

    pub fn remove_source(&self, source: &str) {
        self.sections.write().unwrap().retain(|s| s.source.as_deref() != Some(source));
    }

    /// Effective value of a setting; `Null` if it is unknown or has no default.
    pub fn get(&self, key: &str) -> Value {
        let sections = self.sections.read().unwrap();
        let property = sections.iter().find_map(|s| s.properties.get(key));
        self.effective(key, property)
    }

    fn effective(&self, key: &str, property: Option<&Property>) -> Value {
        // Files can be edited by hand, so stored values that no longer satisfy
        // the schema are skipped instead of being handed to consumers.
        let valid = |v: &Value| property.is_none_or(|p| p.check(key, v).is_ok());
        let workspace = self.workspace.as_ref().and_then(|l| l.read().unwrap().values.get(key).cloned());
        let user = self.user.read().unwrap().values.get(key).cloned();
        workspace
            .filter(|v| valid(v))
            .or(user.filter(|v| valid(v)))
            .or_else(|| property.and_then(|p| p.schema.get("default").cloned()))
            .unwrap_or(Value::Null)
    }

    /// Sets a value in one scope, or clears it with `None`, and persists the scope.
    pub fn set(&self, key: &str, value: Option<Value>, scope: Scope) -> Result<()> {
        {
            let sections = self.sections.read().unwrap();
            let property = sections
                .iter()
                .find_map(|s| s.properties.get(key))
                .ok_or_else(|| Error::UnknownSetting(key.to_owned()))?;
            if let Some(value) = &value {
                property.check(key, value)?;
            }
        }
        let layer = match scope {
            Scope::User => &self.user,
            Scope::Workspace => self.workspace.as_ref().ok_or(Error::NoWorkspace)?,
        };
        let mut layer = layer.write().unwrap();
        match value {
            Some(value) => layer.values.insert(key.to_owned(), value),
            None => layer.values.remove(key),
        };
        layer.save()
    }

    pub fn has_workspace(&self) -> bool {
        self.workspace.is_some()
    }

    pub fn sections(&self) -> Vec<ConfigSection> {
        let sections = self.sections.read().unwrap();
        sections
            .iter()
            .map(|section| ConfigSection {
                source: section.source.clone(),
                title: section.title.clone(),
                properties: section
                    .properties
                    .iter()
                    .map(|(key, property)| ConfigProperty {
                        key: key.clone(),
                        schema: property.schema.clone(),
                        user: self.user.read().unwrap().values.get(key).cloned(),
                        workspace: self.workspace.as_ref().and_then(|l| l.read().unwrap().values.get(key).cloned()),
                        effective: self.effective(key, Some(property)),
                    })
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn store(dir: &Path) -> ConfigStore {
        let store =
            ConfigStore::open(Some(dir.join("user.json")), Some(ConfigStore::workspace_file(&dir.join("ws")))).unwrap();
        let properties = BTreeMap::from([
            ("a.name".to_owned(), json!({"type": "string", "default": "World"})),
            ("a.count".to_owned(), json!({"type": "integer", "minimum": 1, "maximum": 5, "default": 1})),
        ]);
        store.register_section(Some("a"), "A", &properties).unwrap();
        store
    }

    #[test]
    fn scopes_layer_over_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert_eq!(store.get("a.name"), json!("World"));

        store.set("a.name", Some(json!("User")), Scope::User).unwrap();
        assert_eq!(store.get("a.name"), json!("User"));
        store.set("a.name", Some(json!("Workspace")), Scope::Workspace).unwrap();
        assert_eq!(store.get("a.name"), json!("Workspace"));

        store.set("a.name", None, Scope::Workspace).unwrap();
        assert_eq!(store.get("a.name"), json!("User"));
        store.set("a.name", None, Scope::User).unwrap();
        assert_eq!(store.get("a.name"), json!("World"));
    }

    #[test]
    fn writes_are_validated() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        assert!(matches!(store.set("a.count", Some(json!(9)), Scope::User), Err(Error::InvalidSetting { .. })));
        assert!(matches!(store.set("a.count", Some(json!("two")), Scope::User), Err(Error::InvalidSetting { .. })));
        assert!(matches!(store.set("nope", Some(json!(1)), Scope::User), Err(Error::UnknownSetting(_))));
        assert_eq!(store.get("a.count"), json!(1));
    }

    #[test]
    fn values_persist_and_invalid_stored_values_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        store(dir.path()).set("a.count", Some(json!(3)), Scope::Workspace).unwrap();
        assert_eq!(store(dir.path()).get("a.count"), json!(3));

        std::fs::write(dir.path().join("user.json"), r#"{"a.name": 42}"#).unwrap();
        let store = store(dir.path());
        assert_eq!(store.get("a.name"), json!("World"));
        let sections = store.sections();
        let name = sections[0].properties.iter().find(|p| p.key == "a.name").unwrap();
        assert_eq!(name.user, Some(json!(42)));
        assert_eq!(name.effective, json!("World"));
    }

    #[test]
    fn bad_schemas_and_duplicate_keys_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let bad = BTreeMap::from([("b.x".to_owned(), json!({"type": "nonsense"}))]);
        assert!(matches!(store.register_section(None, "B", &bad), Err(Error::InvalidSchema { .. })));
        let dup = BTreeMap::from([("a.name".to_owned(), json!({"type": "string"}))]);
        assert!(matches!(store.register_section(None, "B", &dup), Err(Error::InvalidSchema { .. })));
        assert_eq!(store.sections().len(), 1);
    }
}
