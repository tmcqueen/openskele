// Workbench shell. It lays out panels with Dockview and fills them with the
// generic renderers; what exists, where, and what it does is decided by the host.

import "@vscode/codicons/dist/codicon.css";
import { createDockview, themeDark, themeLight, type DockviewApi, type IContentRenderer } from "dockview-core/dist/dockview-core.js";

import { createAgentPane } from "./agent";
import { createBars, type Bars } from "./bars";
import { h } from "./dom";
import { commands, connect, onHostEvent, run, type Layout, type PluginState, type Snapshot } from "./host";
import { chordOf, normalizeChord } from "./keys";
import { createMainArea, loadedEditor, OUTPUT, type MainArea } from "./layout";
import { closePalette, togglePalette } from "./palette";
import { createOutputPanel } from "./renderers/output";
import { createOutput, createPtyTerminal, retheme as rethemeTerminals, setFont } from "./renderers/terminal";
import { createTree, type Tree } from "./renderers/tree";
import { createSettings, type SettingsPane } from "./settings";
import { createTitleBar, type TitleBar } from "./titlebar";
import { toast } from "./toast";
import "./styles.css";

let snapshot: Snapshot;
let layout: Layout;
let theme = "dark";
let dock: DockviewApi;
let bars: Bars;
let main: MainArea;
let settings: SettingsPane | undefined;
let titleBar: TitleBar | undefined;
const trees = new Map<string, Tree>();
const pluginStates = new Map<string, PluginState>();
// Lives for the whole page, so a session survives its container being hidden.
const agentPane = createAgentPane();
// Lives for the whole page, so it follows output while the Output panel is closed.
const output = createOutputPanel(
  { channels: () => commands.outputChannels(), read: (c) => commands.outputRead(c), clear: async (c) => void (await commands.outputClear(c)) },
  createOutput(),
  globalThis.localStorage,
);

/** A container's side panel: each of its views under a heading. */
function createContainer(containerId: string): IContentRenderer {
  const element = h("div", { class: "container-views" });
  const owned: string[] = [];
  return {
    element,
    init: () => {
      for (const view of snapshot.views.filter((v) => v.container === containerId)) {
        const tree = createTree({
          load: (parent) => commands.viewChildren(view.id, parent),
          activate: (item) => void run(item.command!.command, item.command!.args ?? []),
        });
        trees.set(view.id, tree);
        owned.push(view.id);
        element.append(h("section", { class: "view-section" }, h("h3", { class: "view-title" }, view.title), tree.element));
        void tree.refresh();
      }
    },
    dispose: () => owned.forEach((id) => trees.delete(id)),
  };
}

function createComponent({ name }: { name: string }): IContentRenderer {
  switch (name) {
    case "container": {
      let inner: IContentRenderer | undefined;
      const element = h("div", { class: "panel-fill" });
      return {
        element,
        init: (parameters) => {
          if (parameters.params.container === "workbench.agent") {
            element.append(agentPane.element);
            return;
          }
          inner = createContainer(parameters.params.container);
          inner.init(parameters);
          element.append(inner.element);
        },
        dispose: () => inner?.dispose?.(),
      };
    }
    case "settings": {
      const pane = createSettings(() => snapshot.hasWorkspace);
      settings = pane;
      void pane.reload();
      return { element: pane.element, init: () => {}, dispose: () => (settings = undefined) };
    }
    case "terminal": {
      const element = h("div", { class: "panel-fill" });
      let dispose = () => {};
      return {
        element,
        init: ({ params }) => {
          const terminal = createPtyTerminal(params.id);
          element.append(terminal.element);
          dispose = terminal.dispose;
        },
        dispose: () => dispose(),
      };
    }
    case "editor": {
      // Monaco is large; the layout loaded it before adding this panel.
      const element = h("div", { class: "panel-fill" });
      let dispose = () => {};
      return {
        element,
        init: ({ params }) => {
          const editor = loadedEditor()!.createEditor(params.path, theme);
          element.append(editor.element);
          dispose = editor.dispose;
        },
        dispose: () => dispose(),
      };
    }
    default:
      return { element: output.element, init: () => {} };
  }
}

/** Redraws bars and panels. Callers do not wait, so failures are shown here. */
async function render(): Promise<void> {
  try {
    bars.render(snapshot, layout);
    await main.reconcile(layout);
  } catch (error) {
    toast("error", `Could not update the layout: ${error}`);
  }
}

function renderStatus(): void {
  const items = snapshot.plugins.map((plugin) => {
    const state = pluginStates.get(plugin.id) ?? plugin.state;
    return h("span", { class: `status-item ${state}`, title: `${plugin.id} ${plugin.version}` }, `${plugin.name}: ${state}`);
  });
  document.getElementById("status")!.replaceChildren(...items);
}

function applyTheme(next: string): void {
  theme = next === "light" ? "light" : "dark";
  document.documentElement.dataset.theme = theme;
  dock.updateOptions({ theme: theme === "light" ? themeLight : themeDark });
  rethemeTerminals();
  loadedEditor()?.retheme(theme);
}

/** Settings the shell itself renders from, rather than passing through to a form. */
const APPEARANCE = ["workbench.colorTheme", "terminal.fontFamily", "terminal.fontSize"];

async function loadAppearance(): Promise<void> {
  const sections = await commands.configSections();
  const properties = sections.flatMap((section) => section.properties);
  const [colorTheme, fontFamily, fontSize] = APPEARANCE.map((key) => properties.find((p) => p.key === key)?.effective);
  applyTheme(String(colorTheme));
  setFont(String(fontFamily ?? ""), Number(fontSize) || 13);
}

async function loadSnapshot(): Promise<void> {
  snapshot = await commands.snapshot();
  layout = snapshot.layout;
  titleBar?.setMenus(snapshot.menubar);
  renderStatus();
  await render();
}

async function start(): Promise<void> {
  const chrome = await commands.chrome();
  if (chrome.titleBar === "custom") {
    titleBar = createTitleBar(chrome, (command) => void run(command));
    document.getElementById("titlebar")!.replaceWith(titleBar.element);
  }

  dock = createDockview(document.getElementById("dock")!, {
    createComponent,
    createTabComponent: ({ name }) => (name === "icon" ? bars.createIconTab() : undefined),
    theme: themeDark,
    // The host's layout has no notion of dragged-around panels.
    disableDnd: true,
  });
  bars = createBars(dock, (bar, id) => {
    commands.layoutSetActiveContainer(bar, id).catch((error) => toast("error", String(error)));
  });
  main = createMainArea(dock);

  onHostEvent((event) => {
    switch (event.type) {
      case "contributionsChanged":
        void loadSnapshot().then(() => settings?.reload());
        void agentPane.refresh();
        break;
      case "layoutChanged":
        layout = event.layout;
        void render();
        break;
      case "showCommandPalette":
        togglePalette(snapshot);
        break;
      case "message":
        toast(event.level, event.message, event.source);
        break;
      case "outputAppended":
        output.append(event.channel, event.text);
        break;
      case "outputChannelsChanged":
        void output.channelsChanged();
        break;
      case "outputCleared":
        output.cleared(event.channel);
        break;
      case "viewRefresh":
        void trees.get(event.viewId)?.refresh();
        break;
      case "configChanged":
        void settings?.reload();
        if (event.keys.some((key) => APPEARANCE.includes(key))) void loadAppearance();
        break;
      case "documentChanged":
        loadedEditor()?.reloadDocument(event.uri);
        break;
      case "agentUpdate":
      case "agentSessionEnded":
        agentPane.onEvent(event);
        break;
      case "pluginState":
        pluginStates.set(event.pluginId, event.state);
        renderStatus();
        break;
    }
  });
  await connect();
  await output.start();
  await agentPane.refresh();
  await loadSnapshot();
  await loadAppearance();
  // A reload keeps the host's panels; only a fresh start (just the output) opens Settings.
  if (layout.panels.every((panel) => panel.kind === "output")) await run("workbench.openSettings");
  dock.getPanel(OUTPUT)?.api.setSize({ height: 220 });

  // Capture phase, so chords win over whatever widget has focus.
  window.addEventListener(
    "keydown",
    (event) => {
      const chord = chordOf(event);
      const binding = chord && snapshot.keybindings.findLast((b) => normalizeChord(b.key) === chord);
      if (!binding) return;
      event.preventDefault();
      event.stopPropagation();
      if (binding.command !== "workbench.showCommands") closePalette();
      void run(binding.command);
    },
    true,
  );
}

start().catch((error) => toast("error", `Failed to start: ${error}`));
