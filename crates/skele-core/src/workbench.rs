use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{json, Value};
use skele_protocol::manifest::BarId;
use skele_protocol::methods::{
    CommandExecute, CommandExecuteParams, ConfigDidChange, ConfigDidChangeParams, DocumentDidSave,
    DocumentDidSaveParams, ViewGetChildren, ViewGetChildrenParams,
};
use skele_protocol::view::TreeItem;

use crate::commands::{CommandInfo, CommandRegistry, CommandTarget};
use crate::config::{ConfigStore, Scope};
use crate::containers::{ContainerInfo, ContainerRegistry, VIEWS_CONTAINER};
use crate::document::DocumentStore;
use crate::events::{EventBus, HostEvent};
use crate::keybindings::{Keybinding, KeybindingRegistry};
use crate::layout::{Layout, Panel, PRIMARY_POSITION, SECONDARY_POSITION};
use crate::menus::{Menu, MenuEntry, MenuRegistry};
use crate::plugin::{PluginInfo, PluginManager};
use crate::pty::TerminalManager;
use crate::views::{ViewInfo, ViewRegistry};
use crate::{Error, Result};

#[derive(Debug, Clone, Default)]
pub struct WorkbenchOptions {
    /// Where user-scope settings are persisted; in memory only when `None`.
    pub user_settings: Option<PathBuf>,
    /// The open workspace folder, enabling workspace-scope settings.
    pub workspace: Option<PathBuf>,
}

/// Everything a front end needs to draw its chrome, in one round trip.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub commands: Vec<CommandInfo>,
    pub menubar: Vec<Menu>,
    pub keybindings: Vec<Keybinding>,
    pub containers: Vec<ContainerInfo>,
    pub views: Vec<ViewInfo>,
    pub layout: Layout,
    pub plugins: Vec<PluginInfo>,
    pub has_workspace: bool,
}

pub(crate) struct Inner {
    pub(crate) commands: CommandRegistry,
    pub(crate) menus: MenuRegistry,
    pub(crate) keybindings: KeybindingRegistry,
    pub(crate) views: ViewRegistry,
    pub(crate) containers: ContainerRegistry,
    pub(crate) layout: Mutex<Layout>,
    pub(crate) config: ConfigStore,
    pub(crate) bus: EventBus,
    pub(crate) plugins: PluginManager,
    pub(crate) terminals: TerminalManager,
    pub(crate) documents: DocumentStore,
    pub(crate) workspace: Option<PathBuf>,
}

/// Handle to the workbench state. Cheap to clone.
#[derive(Clone)]
pub struct Workbench {
    pub(crate) inner: Arc<Inner>,
}

impl Workbench {
    pub fn new(options: WorkbenchOptions) -> Result<Self> {
        let workspace_settings = options.workspace.as_deref().map(ConfigStore::workspace_file);
        let workbench = Self {
            inner: Arc::new(Inner {
                commands: Default::default(),
                menus: Default::default(),
                keybindings: Default::default(),
                views: Default::default(),
                containers: Default::default(),
                layout: Default::default(),
                config: ConfigStore::open(options.user_settings, workspace_settings)?,
                bus: Default::default(),
                plugins: Default::default(),
                terminals: Default::default(),
                documents: Default::default(),
                workspace: options.workspace,
            }),
        };
        workbench.register_builtins()?;
        workbench.apply_bar_settings();
        Ok(workbench)
    }

    pub fn commands(&self) -> &CommandRegistry {
        &self.inner.commands
    }
    pub fn menus(&self) -> &MenuRegistry {
        &self.inner.menus
    }
    pub fn keybindings(&self) -> &KeybindingRegistry {
        &self.inner.keybindings
    }
    pub fn views(&self) -> &ViewRegistry {
        &self.inner.views
    }
    pub fn config(&self) -> &ConfigStore {
        &self.inner.config
    }
    pub fn bus(&self) -> &EventBus {
        &self.inner.bus
    }
    pub fn terminals(&self) -> &TerminalManager {
        &self.inner.terminals
    }
    pub fn documents(&self) -> &DocumentStore {
        &self.inner.documents
    }
    pub fn workspace(&self) -> Option<&Path> {
        self.inner.workspace.as_deref()
    }

    pub fn menubar(&self) -> Vec<Menu> {
        self.inner.menus.menubar(&self.inner.commands, &self.inner.keybindings)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            commands: self.inner.commands.list(),
            menubar: self.menubar(),
            keybindings: self.inner.keybindings.list(),
            containers: self.inner.containers.list(),
            views: self.inner.views.list(),
            layout: self.layout(),
            plugins: self.plugins(),
            has_workspace: self.inner.config.has_workspace(),
        }
    }

    /// Runs a command wherever it lives, starting its plugin if necessary.
    pub async fn execute_command(&self, id: &str, args: Vec<Value>) -> Result<Value> {
        let (_, target) = self.inner.commands.get(id).ok_or_else(|| Error::UnknownCommand(id.to_owned()))?;
        match target {
            CommandTarget::Builtin(handler) => handler(args).await,
            CommandTarget::Plugin(plugin) => {
                let peer = self.plugin_peer(&plugin).await?;
                let params = CommandExecuteParams { command: id.to_owned(), args };
                peer.request::<CommandExecute>(params, None).await
            }
        }
    }

    /// Fetches the rows of a contributed view from its plugin.
    pub async fn view_children(&self, view_id: &str, parent: Option<String>) -> Result<Vec<TreeItem>> {
        let view = self.inner.views.get(view_id).ok_or_else(|| Error::UnknownView(view_id.to_owned()))?;
        let peer = self.plugin_peer(&view.plugin_id).await?;
        let params = ViewGetChildrenParams { view_id: view_id.to_owned(), parent };
        Ok(peer.request::<ViewGetChildren>(params, None).await?.items)
    }

    /// Validates and stores a setting, then tells the front end and running plugins.
    pub fn set_config(&self, key: &str, value: Option<Value>, scope: Scope) -> Result<()> {
        self.inner.config.set(key, value, scope)?;
        let changes = BTreeMap::from([(key.to_owned(), self.inner.config.get(key))]);
        self.notify_plugins::<ConfigDidChange>(ConfigDidChangeParams { changes });
        self.inner.bus.emit(HostEvent::ConfigChanged { keys: vec![key.to_owned()] });
        if key == PRIMARY_POSITION || key == SECONDARY_POSITION {
            self.apply_bar_settings();
        }
        Ok(())
    }

    /// Writes a document buffer to disk, then tells the front end and running plugins.
    pub fn save_document(&self, uri: &str) -> Result<u32> {
        let version = self.inner.documents.save(uri)?;
        self.notify_plugins::<DocumentDidSave>(DocumentDidSaveParams { uri: uri.to_owned(), version });
        self.inner.bus.emit(HostEvent::DocumentSaved { uri: uri.to_owned(), version });
        Ok(version)
    }

    pub async fn shutdown(&self) {
        self.shutdown_plugins().await;
        self.inner.terminals.close_all();
    }

    fn register_builtins(&self) -> Result<()> {
        self.register_container(ContainerInfo {
            id: VIEWS_CONTAINER.to_owned(),
            title: "Views".to_owned(),
            icon: "list-tree".to_owned(),
            bar: BarId::Primary,
            order: 0,
            source: None,
        })?;

        // Each built-in holds the workbench weakly so the registry does not keep it alive.
        let weak = Arc::downgrade(&self.inner);
        let builtin = |run: fn(&Workbench, Vec<Value>) -> Result<Value>| -> CommandTarget {
            let weak = weak.clone();
            CommandTarget::Builtin(Arc::new(move |args| {
                let inner = weak.upgrade();
                Box::pin(async move {
                    match inner {
                        Some(inner) => run(&Workbench { inner }, args),
                        None => Ok(Value::Null),
                    }
                })
            }))
        };
        type Run = fn(&Workbench, Vec<Value>) -> Result<Value>;
        let menu_builtins: [(&str, &str, &str, &str, Run); 6] = [
            ("workbench.showCommands", "Show All Commands", "menubar/View", "Mod+Shift+P", |w, _| {
                w.inner.bus.emit(HostEvent::ShowCommandPalette);
                Ok(Value::Null)
            }),
            ("workbench.openSettings", "Open Settings", "menubar/File", "Mod+,", |w, _| {
                w.open_panel(Panel::Settings);
                Ok(Value::Null)
            }),
            ("workbench.showOutput", "Show Output", "menubar/View", "Mod+Shift+U", |w, _| {
                w.open_panel(Panel::Output);
                Ok(Value::Null)
            }),
            ("workbench.newTerminal", "New Terminal", "menubar/View", "Mod+Shift+T", |w, _| {
                Ok(json!(w.new_terminal()?))
            }),
            ("workbench.togglePrimaryBar", "Toggle Primary Side Bar", "menubar/View", "Mod+B", |w, _| {
                w.toggle_bar(BarId::Primary);
                Ok(Value::Null)
            }),
            ("workbench.toggleSecondaryBar", "Toggle Secondary Side Bar", "menubar/View", "Mod+Alt+B", |w, _| {
                w.toggle_bar(BarId::Secondary);
                Ok(Value::Null)
            }),
        ];
        for (id, title, menu, key, run) in menu_builtins {
            let info = CommandInfo { id: id.to_owned(), title: title.to_owned(), category: None, source: None };
            self.inner.commands.register(info, builtin(run))?;
            self.inner.menus.add(menu, MenuEntry { command: id.to_owned(), group: None, order: 0, source: None });
            self.inner.keybindings.register(Keybinding { key: key.to_owned(), command: id.to_owned(), source: None });
        }

        // Not in any menu: it needs a path, so it is for view items and other callers with one.
        let open_file: Run = |w, args| {
            let Some(path) = args.first().and_then(Value::as_str) else {
                return Err(Error::Other("workbench.openFile expects a file path".to_owned()));
            };
            w.open_panel(Panel::Editor { path: path.to_owned() });
            Ok(Value::Null)
        };
        let info = CommandInfo {
            id: "workbench.openFile".to_owned(),
            title: "Open File".to_owned(),
            category: None,
            source: None,
        };
        self.inner.commands.register(info, builtin(open_file))?;

        let bar_position = |default: &str, which: &str| {
            json!({
                "type": "string",
                "enum": ["left", "right", "top"],
                "default": default,
                "description": format!("Where the {which} side bar is docked.")
            })
        };
        let properties = BTreeMap::from([
            (PRIMARY_POSITION.to_owned(), bar_position("left", "primary")),
            (SECONDARY_POSITION.to_owned(), bar_position("right", "secondary")),
            (
                "workbench.colorTheme".to_owned(),
                json!({
                    "type": "string",
                    "enum": ["dark", "light"],
                    "default": "dark",
                    "description": "Color theme of the workbench."
                }),
            ),
            (
                "terminal.shell".to_owned(),
                json!({
                    "type": "string",
                    "default": "",
                    "description": "Program to run in new terminals. Empty uses your default shell."
                }),
            ),
            (
                "terminal.fontFamily".to_owned(),
                json!({
                    "type": "string",
                    "default": "",
                    "description": "Font for terminals and the output panel, as a CSS font-family list. Empty uses the default monospace font."
                }),
            ),
            (
                "terminal.fontSize".to_owned(),
                json!({
                    "type": "number",
                    "minimum": 6,
                    "maximum": 40,
                    "default": 13,
                    "description": "Font size for terminals and the output panel, in pixels."
                }),
            ),
        ]);
        self.inner.config.register_section(None, "Workbench", &properties)
    }
}
