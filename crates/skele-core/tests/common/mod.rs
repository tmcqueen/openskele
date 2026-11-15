//! Helpers shared by the sidecar end-to-end tests.

use std::path::{Path, PathBuf};
use std::time::Duration;

use skele_core::plugin::PluginState;
use skele_core::{HostEvent, Workbench};
use tokio::sync::broadcast::Receiver;

pub fn sample(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins").join(name)
}

pub fn on_path(program: &str) -> bool {
    std::process::Command::new(program).arg("--version").output().is_ok()
}

pub fn state(workbench: &Workbench, plugin: &str) -> PluginState {
    workbench.plugins().iter().find(|p| p.id == plugin).unwrap().state
}

/// Waits for an event matching `pick`, failing the test if none arrives.
pub async fn expect<T>(events: &mut Receiver<HostEvent>, mut pick: impl FnMut(&HostEvent) -> Option<T>) -> T {
    let wait = async {
        loop {
            if let Some(found) = pick(&events.recv().await.expect("event bus closed")) {
                return found;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait).await.expect("timed out waiting for event")
}
