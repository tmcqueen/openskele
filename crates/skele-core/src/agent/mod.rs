//! Agents: executables speaking the Agent Client Protocol, declared by plugin
//! manifests. The workbench is the ACP client (ADR-0002).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::RwLock;

use serde::Serialize;
use skele_protocol::manifest::AgentContribution;

use crate::{Error, Result, Workbench};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    /// Contributing plugin.
    pub source: String,
}

/// What it takes to start an agent.
#[allow(dead_code)] // read by the ACP client, which comes next
#[derive(Debug, Clone)]
pub(crate) struct AgentEntry {
    pub(crate) info: AgentInfo,
    pub(crate) contribution: AgentContribution,
    /// The contributing plugin's directory: the agent's working directory for
    /// resolving a relative command.
    pub(crate) dir: PathBuf,
}

#[derive(Default)]
pub struct AgentRegistry {
    agents: RwLock<BTreeMap<String, AgentEntry>>,
}

#[allow(dead_code)] // `get` is used by the ACP client, which comes next
impl AgentRegistry {
    pub(crate) fn register(&self, entry: AgentEntry) -> Result<()> {
        let mut agents = self.agents.write().unwrap();
        if agents.contains_key(&entry.info.id) {
            return Err(Error::DuplicateAgent(entry.info.id));
        }
        agents.insert(entry.info.id.clone(), entry);
        Ok(())
    }

    pub(crate) fn get(&self, id: &str) -> Option<AgentEntry> {
        self.agents.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<AgentInfo> {
        self.agents.read().unwrap().values().map(|e| e.info.clone()).collect()
    }

    pub fn remove_source(&self, source: &str) {
        self.agents.write().unwrap().retain(|_, e| e.info.source != source);
    }
}

impl Workbench {
    /// Every installed agent, by id.
    pub fn agents(&self) -> Vec<AgentInfo> {
        self.inner.agents.list()
    }
}
