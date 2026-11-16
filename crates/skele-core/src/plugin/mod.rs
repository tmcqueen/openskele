//! Sidecar plugin host.
//!
//! A plugin is a directory with a manifest. Installing it registers its
//! contributions without running any of its code; its process is started the
//! first time something needs it and restarted on demand if it dies.

pub mod peer;
pub mod transport;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use skele_protocol::jsonrpc::RpcError;
use skele_protocol::manifest::{Manifest, MANIFEST_FILE};
use skele_protocol::methods::*;
use skele_protocol::{is_compatible, PROTOCOL_VERSION};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use self::peer::{BoxFuture, Handler, Peer};
use crate::commands::{CommandInfo, CommandTarget};
use crate::containers::{ContainerInfo, VIEWS_CONTAINER};
use crate::events::HostEvent;
use crate::keybindings::Keybinding;
use crate::menus::MenuEntry;
use crate::views::ViewInfo;
use crate::workbench::Inner;
use crate::{Error, Result, Workbench};

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Activation event for plugins that want to start with the workbench.
pub const ON_STARTUP: &str = "onStartup";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum PluginState {
    /// Installed; process not started yet.
    Idle,
    Starting,
    Running,
    /// Shut down by the host.
    Stopped,
    /// Failed to start, or exited on its own.
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub state: PluginState,
}

pub(crate) struct Plugin {
    manifest: Manifest,
    dir: PathBuf,
    state: Mutex<PluginState>,
    /// Held across startup so concurrent activations share one process.
    session: tokio::sync::Mutex<Option<Session>>,
}

struct Session {
    peer: Arc<Peer>,
    child: Child,
}

#[derive(Default)]
pub(crate) struct PluginManager {
    plugins: RwLock<BTreeMap<String, Arc<Plugin>>>,
}

impl PluginManager {
    fn get(&self, id: &str) -> Result<Arc<Plugin>> {
        self.plugins.read().unwrap().get(id).cloned().ok_or_else(|| Error::UnknownPlugin(id.to_owned()))
    }

    fn all(&self) -> Vec<Arc<Plugin>> {
        self.plugins.read().unwrap().values().cloned().collect()
    }
}

fn plugin_error(plugin: &str, message: impl std::fmt::Display) -> Error {
    Error::Plugin { plugin: plugin.to_owned(), message: message.to_string() }
}

impl Workbench {
    /// Installs every plugin directory found directly under `dir`. A plugin
    /// that fails to install is reported on the event bus and skipped.
    pub fn load_plugins(&self, dir: &Path) -> Vec<String> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.join(MANIFEST_FILE).is_file())
            .collect();
        dirs.sort();

        let mut installed = Vec::new();
        for dir in dirs {
            match self.install_plugin(&dir) {
                Ok(id) => installed.push(id),
                Err(e) => self.notify(MessageLevel::Error, &format!("{}: {e}", dir.display()), None),
            }
        }
        installed
    }

    /// Reads a plugin's manifest and registers its contributions. Does not start it.
    pub fn install_plugin(&self, dir: &Path) -> Result<String> {
        let manifest: Manifest = serde_json::from_slice(&std::fs::read(dir.join(MANIFEST_FILE))?)?;
        let id = manifest.id.clone();
        if !is_compatible(PROTOCOL_VERSION, &manifest.protocol) {
            return Err(plugin_error(
                &id,
                format!("speaks protocol {}, host speaks {PROTOCOL_VERSION}", manifest.protocol),
            ));
        }
        if self.inner.plugins.plugins.read().unwrap().contains_key(&id) {
            return Err(plugin_error(&id, "already installed"));
        }

        if let Err(e) = self.register_contributions(&manifest) {
            self.unregister_contributions(&id);
            return Err(e);
        }
        let plugin =
            Plugin { manifest, dir: dir.to_owned(), state: Mutex::new(PluginState::Idle), session: Default::default() };
        self.inner.plugins.plugins.write().unwrap().insert(id.clone(), Arc::new(plugin));
        self.bus().emit(HostEvent::ContributionsChanged);
        self.fix_layout_containers();
        Ok(id)
    }

    fn register_contributions(&self, manifest: &Manifest) -> Result<()> {
        let inner = &self.inner;
        let id = &manifest.id;
        let contributes = &manifest.contributes;

        if let Some(configuration) = &contributes.configuration {
            let title = configuration.title.as_deref().unwrap_or(&manifest.name);
            inner.config.register_section(Some(id), title, &configuration.properties)?;
        }
        for command in &contributes.commands {
            let info = CommandInfo {
                id: command.id.clone(),
                title: command.title.clone(),
                category: command.category.clone(),
                source: Some(id.clone()),
            };
            inner.commands.register(info, CommandTarget::Plugin(id.clone()))?;
        }
        for (location, entries) in &contributes.menus {
            for entry in entries {
                let entry = MenuEntry {
                    command: entry.command.clone(),
                    group: entry.group.clone(),
                    order: entry.order,
                    source: Some(id.clone()),
                };
                inner.menus.add(location, entry);
            }
        }
        for binding in &contributes.keybindings {
            inner.keybindings.register(Keybinding {
                key: binding.key.clone(),
                command: binding.command.clone(),
                source: Some(id.clone()),
            });
        }
        for container in &contributes.view_containers {
            self.register_container(ContainerInfo {
                id: container.id.clone(),
                title: container.title.clone(),
                icon: container.icon.clone(),
                bar: container.bar,
                order: container.order,
                source: Some(id.clone()),
            })?;
        }
        for view in &contributes.views {
            let container = view.container.clone().unwrap_or_else(|| VIEWS_CONTAINER.to_owned());
            if inner.containers.get(&container).is_none() {
                return Err(Error::UnknownContainer(container));
            }
            inner.views.register(ViewInfo {
                id: view.id.clone(),
                title: view.title.clone(),
                kind: view.kind,
                container,
                plugin_id: id.clone(),
            });
        }
        Ok(())
    }

    fn unregister_contributions(&self, id: &str) {
        let inner = &self.inner;
        inner.config.remove_source(id);
        inner.commands.remove_source(id);
        inner.menus.remove_source(id);
        inner.keybindings.remove_source(id);
        inner.views.remove_source(id);
        inner.containers.remove_source(id);
        self.fix_layout_containers();
    }

    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.inner
            .plugins
            .all()
            .iter()
            .map(|plugin| PluginInfo {
                id: plugin.manifest.id.clone(),
                name: plugin.manifest.name.clone(),
                version: plugin.manifest.version.clone(),
                state: *plugin.state.lock().unwrap(),
            })
            .collect()
    }

    /// Starts every plugin that listed `event` (or `*`) among its activation events.
    pub fn fire_activation_event(&self, event: &str) {
        for plugin in self.inner.plugins.all() {
            if plugin.manifest.activation_events.iter().any(|e| e == event || e == "*") {
                let workbench = self.clone();
                tokio::spawn(async move {
                    let _ = workbench.activate_plugin(&plugin.manifest.id).await;
                });
            }
        }
    }

    /// Ensures the plugin's process is running and initialized.
    pub async fn activate_plugin(&self, id: &str) -> Result<()> {
        self.plugin_peer(id).await.map(drop)
    }

    pub(crate) async fn plugin_peer(&self, id: &str) -> Result<Arc<Peer>> {
        let plugin = self.inner.plugins.get(id)?;
        let mut session = plugin.session.lock().await;
        if let Some(live) = session.as_ref().filter(|s| !s.peer.is_closed()) {
            return Ok(live.peer.clone());
        }

        self.set_plugin_state(&plugin, PluginState::Starting);
        match self.start(&plugin).await {
            Ok(started) => {
                let peer = started.peer.clone();
                *session = Some(started);
                self.set_plugin_state(&plugin, PluginState::Running);
                Ok(peer)
            }
            Err(e) => {
                *session = None;
                self.set_plugin_state(&plugin, PluginState::Failed);
                self.notify(MessageLevel::Error, &e.to_string(), Some(&plugin.manifest.name));
                Err(e)
            }
        }
    }

    async fn start(&self, plugin: &Arc<Plugin>) -> Result<Session> {
        let manifest = &plugin.manifest;
        let id = &manifest.id;
        let entry = &manifest.main;

        // A bare name is looked up on PATH; anything with a separator is relative to the plugin.
        let program = if Path::new(&entry.command).components().count() > 1 {
            plugin.dir.join(&entry.command)
        } else {
            PathBuf::from(&entry.command)
        };
        let mut child = Command::new(&program)
            .args(&entry.args)
            .envs(&entry.env)
            .current_dir(&plugin.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| plugin_error(id, format!("failed to start `{}`: {e}", entry.command)))?;

        // stdout carries the protocol, so stderr is the plugin's log.
        let stderr = BufReader::new(child.stderr.take().expect("stderr is piped"));
        let (weak, channel) = (Arc::downgrade(&self.inner), manifest.name.clone());
        tokio::spawn(async move {
            let mut lines = stderr.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Some(inner) = weak.upgrade() else { break };
                Workbench { inner }.output(&channel, &format!("{line}\r\n"));
            }
        });

        let handler = PluginHandler { inner: Arc::downgrade(&self.inner), plugin: Arc::downgrade(plugin) };
        let peer = Peer::spawn(
            BufReader::new(child.stdout.take().expect("stdout is piped")),
            child.stdin.take().expect("stdin is piped"),
            Arc::new(handler),
        );

        let params = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            host: PeerInfo { name: "openskele".to_owned(), version: env!("CARGO_PKG_VERSION").to_owned() },
            plugin_id: id.clone(),
            workspace_folder: self.inner.workspace.as_ref().map(|p| p.display().to_string()),
        };
        let reply = peer
            .request::<Initialize>(params, Some(INITIALIZE_TIMEOUT))
            .await
            .map_err(|e| plugin_error(id, format!("initialize failed: {e}")))?;
        if !is_compatible(PROTOCOL_VERSION, &reply.protocol_version) {
            return Err(plugin_error(
                id,
                format!("replied with protocol {}, host speaks {PROTOCOL_VERSION}", reply.protocol_version),
            ));
        }
        Ok(Session { peer, child })
    }

    fn set_plugin_state(&self, plugin: &Plugin, state: PluginState) {
        *plugin.state.lock().unwrap() = state;
        self.bus().emit(HostEvent::PluginState { plugin_id: plugin.manifest.id.clone(), state });
    }

    /// Sends a notification to every plugin that is currently running.
    /// Idle plugins are not started for it.
    pub(crate) fn notify_plugins<N: Notification>(&self, params: N::Params)
    where
        N::Params: Clone,
    {
        for plugin in self.inner.plugins.all() {
            if *plugin.state.lock().unwrap() != PluginState::Running {
                continue;
            }
            if let Ok(session) = plugin.session.try_lock() {
                if let Some(session) = session.as_ref() {
                    let _ = session.peer.notify::<N>(params.clone());
                }
            }
        }
    }

    /// Asks every running plugin to shut down, then makes sure its process is gone.
    pub async fn shutdown_plugins(&self) {
        for plugin in self.inner.plugins.all() {
            let Some(mut session) = plugin.session.lock().await.take() else { continue };
            if !session.peer.is_closed() {
                // Mark first so the connection closing is not reported as a crash.
                self.set_plugin_state(&plugin, PluginState::Stopped);
                let _ = session.peer.request::<Shutdown>(Empty {}, Some(SHUTDOWN_TIMEOUT)).await;
                let _ = session.peer.notify::<Exit>(Empty {});
            }
            if tokio::time::timeout(SHUTDOWN_TIMEOUT, session.child.wait()).await.is_err() {
                let _ = session.child.kill().await;
            }
        }
    }
}

/// Serves the plugin-to-host half of the protocol for one plugin.
struct PluginHandler {
    inner: Weak<Inner>,
    plugin: Weak<Plugin>,
}

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(RpcError::invalid_params)
}

impl PluginHandler {
    fn context(&self) -> Option<(Workbench, Arc<Plugin>)> {
        Some((Workbench { inner: self.inner.upgrade()? }, self.plugin.upgrade()?))
    }
}

impl Handler for PluginHandler {
    fn request(&self, method: String, params: Value) -> BoxFuture<Result<Value, RpcError>> {
        let context = self.context();
        Box::pin(async move {
            let (workbench, _) = context.ok_or_else(|| RpcError::internal("host is shutting down"))?;
            match method.as_str() {
                ConfigGet::METHOD => {
                    let params: ConfigGetParams = parse(params)?;
                    Ok(workbench.config().get(&params.key))
                }
                other => Err(RpcError::method_not_found(other)),
            }
        })
    }

    fn notification(&self, method: String, params: Value) {
        let Some((workbench, plugin)) = self.context() else { return };
        let name = &plugin.manifest.name;
        let bus = workbench.bus();
        // A notification has no reply channel, so malformed ones are logged to the plugin's terminal.
        let outcome: Result<(), RpcError> = (|| {
            match method.as_str() {
                WindowShowMessage::METHOD => {
                    let params: ShowMessageParams = parse(params)?;
                    workbench.notify(params.level, &params.message, Some(name));
                }
                TerminalWrite::METHOD => {
                    let params: TerminalWriteParams = parse(params)?;
                    let channel = params.terminal.unwrap_or_else(|| name.clone());
                    workbench.output(&channel, &params.data);
                }
                ViewRefresh::METHOD => {
                    let params: ViewRefreshParams = parse(params)?;
                    let owned =
                        workbench.views().get(&params.view_id).is_some_and(|v| v.plugin_id == plugin.manifest.id);
                    if !owned {
                        return Err(RpcError::invalid_params(format!("not this plugin's view: {}", params.view_id)));
                    }
                    bus.emit(HostEvent::ViewRefresh { view_id: params.view_id });
                }
                other => return Err(RpcError::method_not_found(other)),
            }
            Ok(())
        })();
        if let Err(e) = outcome {
            workbench.output(name, &format!("[host] {method}: {e}\r\n"));
        }
    }

    fn closed(&self) {
        let Some((workbench, plugin)) = self.context() else { return };
        let state = *plugin.state.lock().unwrap();
        if state == PluginState::Running {
            workbench.set_plugin_state(&plugin, PluginState::Failed);
            workbench.notify(
                MessageLevel::Warning,
                "plugin exited unexpectedly; it will restart when next needed",
                Some(&plugin.manifest.name),
            );
        }
    }
}
