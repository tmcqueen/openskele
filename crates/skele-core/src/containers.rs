//! View containers: the icons on the app bars, each opening a side panel of views.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde_json::Value;
use skele_protocol::manifest::BarId;

use crate::commands::{CommandInfo, CommandTarget};
use crate::{Error, Result, Workbench};

/// The built-in container that holds views which name no container.
pub const VIEWS_CONTAINER: &str = "workbench.views";

/// Prefix of the per-container command that reveals it.
pub const FOCUS_COMMAND_PREFIX: &str = "workbench.focusContainer.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ContainerInfo {
    pub id: String,
    pub title: String,
    /// Codicon name.
    pub icon: String,
    pub bar: BarId,
    pub order: i32,
    /// Contributing plugin; `None` for built-ins.
    pub source: Option<String>,
}

#[derive(Default)]
pub struct ContainerRegistry {
    containers: RwLock<BTreeMap<String, ContainerInfo>>,
}

impl ContainerRegistry {
    pub fn register(&self, info: ContainerInfo) -> Result<()> {
        let mut containers = self.containers.write().unwrap();
        if containers.contains_key(&info.id) {
            return Err(Error::DuplicateContainer(info.id));
        }
        containers.insert(info.id.clone(), info);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<ContainerInfo> {
        self.containers.read().unwrap().get(id).cloned()
    }

    /// Every container, primary bar first, then by `order`, then by id.
    pub fn list(&self) -> Vec<ContainerInfo> {
        let mut list: Vec<_> = self.containers.read().unwrap().values().cloned().collect();
        list.sort_by(|a, b| (a.bar, a.order, &a.id).cmp(&(b.bar, b.order, &b.id)));
        list
    }

    pub fn on_bar(&self, bar: BarId) -> Vec<ContainerInfo> {
        self.list().into_iter().filter(|c| c.bar == bar).collect()
    }

    pub fn remove_source(&self, source: &str) {
        self.containers.write().unwrap().retain(|_, c| c.source.as_deref() != Some(source));
    }
}

impl Workbench {
    pub fn containers(&self) -> &ContainerRegistry {
        &self.inner.containers
    }

    /// Registers a container and the command that focuses it. Both carry the
    /// container's source, so removing the source removes both.
    pub(crate) fn register_container(&self, info: ContainerInfo) -> Result<()> {
        let command = CommandInfo {
            id: format!("{FOCUS_COMMAND_PREFIX}{}", info.id),
            title: format!("Focus {}", info.title),
            category: Some("View".to_owned()),
            source: info.source.clone(),
        };
        let weak = Arc::downgrade(&self.inner);
        let (bar, id) = (info.bar, info.id.clone());
        let focus = CommandTarget::Builtin(Arc::new(move |_args| {
            let inner = weak.upgrade();
            let id = id.clone();
            Box::pin(async move {
                if let Some(inner) = inner {
                    Workbench { inner }.focus_container(bar, &id)?;
                }
                Ok(Value::Null)
            })
        }));
        self.inner.containers.register(info)?;
        self.inner.commands.register(command, focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skele_protocol::manifest::BarId;

    fn info(id: &str, bar: BarId, order: i32, source: Option<&str>) -> ContainerInfo {
        ContainerInfo {
            id: id.to_owned(),
            title: id.to_owned(),
            icon: "package".to_owned(),
            bar,
            order,
            source: source.map(str::to_owned),
        }
    }

    #[test]
    fn list_orders_by_bar_then_order() {
        let registry = ContainerRegistry::default();
        registry.register(info("s", BarId::Secondary, 0, None)).unwrap();
        registry.register(info("p5", BarId::Primary, 5, None)).unwrap();
        registry.register(info("p1", BarId::Primary, 1, None)).unwrap();
        let ids: Vec<_> = registry.list().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["p1", "p5", "s"]);
        let primary: Vec<_> = registry.on_bar(BarId::Primary).into_iter().map(|c| c.id).collect();
        assert_eq!(primary, ["p1", "p5"]);
    }

    #[test]
    fn duplicate_container_is_rejected() {
        let registry = ContainerRegistry::default();
        registry.register(info("a", BarId::Primary, 0, None)).unwrap();
        let again = registry.register(info("a", BarId::Secondary, 0, Some("x")));
        assert!(matches!(again, Err(Error::DuplicateContainer(id)) if id == "a"));
        assert_eq!(registry.get("a").unwrap().bar, BarId::Primary);
    }

    #[test]
    fn remove_source_drops_only_that_plugin() {
        let registry = ContainerRegistry::default();
        registry.register(info("builtin", BarId::Primary, 0, None)).unwrap();
        registry.register(info("mine", BarId::Primary, 1, Some("me"))).unwrap();
        registry.register(info("theirs", BarId::Primary, 2, Some("them"))).unwrap();
        registry.remove_source("me");
        let ids: Vec<_> = registry.list().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["builtin", "theirs"]);
    }
}
