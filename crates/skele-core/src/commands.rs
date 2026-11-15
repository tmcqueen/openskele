use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde_json::Value;

use crate::{Error, Result};

pub type CommandFuture = Pin<Box<dyn Future<Output = Result<Value>> + Send>>;
pub type CommandHandler = Arc<dyn Fn(Vec<Value>) -> CommandFuture + Send + Sync>;

/// Where a command runs.
#[derive(Clone)]
pub enum CommandTarget {
    /// In the host process.
    Builtin(CommandHandler),
    /// In the sidecar of the plugin with this id, started on demand.
    Plugin(String),
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CommandInfo {
    pub id: String,
    pub title: String,
    pub category: Option<String>,
    /// Contributing plugin; `None` for built-ins.
    pub source: Option<String>,
}

#[derive(Default)]
pub struct CommandRegistry {
    commands: RwLock<BTreeMap<String, (CommandInfo, CommandTarget)>>,
}

impl CommandRegistry {
    pub fn register(&self, info: CommandInfo, target: CommandTarget) -> Result<()> {
        let mut commands = self.commands.write().unwrap();
        if commands.contains_key(&info.id) {
            return Err(Error::DuplicateCommand(info.id));
        }
        commands.insert(info.id.clone(), (info, target));
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<(CommandInfo, CommandTarget)> {
        self.commands.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<CommandInfo> {
        self.commands.read().unwrap().values().map(|(info, _)| info.clone()).collect()
    }

    pub fn remove_source(&self, source: &str) {
        self.commands.write().unwrap().retain(|_, (info, _)| info.source.as_deref() != Some(source));
    }
}
