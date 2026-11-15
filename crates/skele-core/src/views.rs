use std::collections::BTreeMap;
use std::sync::RwLock;

use serde::Serialize;
use skele_protocol::manifest::{ViewKind, ViewLocation};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ViewInfo {
    pub id: String,
    pub title: String,
    pub kind: ViewKind,
    pub location: ViewLocation,
    /// Plugin that supplies the view's data.
    pub plugin_id: String,
}

#[derive(Default)]
pub struct ViewRegistry {
    views: RwLock<BTreeMap<String, ViewInfo>>,
}

impl ViewRegistry {
    pub fn register(&self, view: ViewInfo) {
        self.views.write().unwrap().insert(view.id.clone(), view);
    }

    pub fn get(&self, id: &str) -> Option<ViewInfo> {
        self.views.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<ViewInfo> {
        self.views.read().unwrap().values().cloned().collect()
    }

    pub fn remove_source(&self, plugin_id: &str) {
        self.views.write().unwrap().retain(|_, v| v.plugin_id != plugin_id);
    }
}
