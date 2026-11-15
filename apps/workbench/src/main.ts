// Workbench shell. It lays out panels with Dockview and fills them with the
// generic renderers; what exists and what it does is decided by the host.

import { createDockview, themeDark, themeLight, type DockviewApi, type IContentRenderer } from "dockview-core/dist/dockview-core.js";

import { h } from "./dom";
import { commands, connect, onHostEvent, run, type PluginState, type Snapshot } from "./host";
import { chordOf, normalizeChord } from "./keys";
import { closePalette, togglePalette } from "./palette";
import { createOutput, createPtyTerminal, retheme as rethemeTerminals } from "./renderers/terminal";
import { createTree, type Tree } from "./renderers/tree";
import { createSettings, type SettingsPane } from "./settings";
import { toast } from "./toast";
import "./styles.css";

const OUTPUT = "output";
const SETTINGS = "settings";
const viewPanelId = (viewId: string) => `view:${viewId}`;
const editorPanelId = (path: string) => `editor:${path}`;

let snapshot: Snapshot;
let theme = "dark";
let dock: DockviewApi;
let settings: SettingsPane | undefined;
let terminals = 0;
const trees = new Map<string, Tree>();
const pluginStates = new Map<string, PluginState>();
const output = createOutput();

type EditorModule = typeof import("./renderers/editor");
let editorModule: EditorModule | undefined;

function createComponent({ name }: { name: string }): IContentRenderer {
  switch (name) {
    case "view": {
      let viewId = "";
      const element = h("div", { class: "panel-fill" });
      return {
        element,
        init: ({ params }) => {
          viewId = params.viewId;
          const tree = createTree({
            load: (parent) => commands.viewChildren(viewId, parent),
            activate: (item) => void run(item.command!.command, item.command!.args ?? []),
          });
          trees.set(viewId, tree);
          element.append(tree.element);
          void tree.refresh();
        },
        dispose: () => void trees.delete(viewId),
      };
    }
    case "settings": {
      const pane = createSettings(() => snapshot.hasWorkspace);
      settings = pane;
      void pane.reload();
      return { element: pane.element, init: () => {}, dispose: () => (settings = undefined) };
    }
    case "terminal": {
      const terminal = createPtyTerminal();
      return { element: terminal.element, init: () => {}, dispose: terminal.dispose };
    }
    case "editor": {
      // Monaco is large; it was loaded before this panel was added.
      const element = h("div", { class: "panel-fill" });
      let dispose = () => {};
      return {
        element,
        init: ({ params }) => {
          const editor = editorModule!.createEditor(params.path, theme);
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

/** Where documents and settings open: beside an existing one, else above the output. */
function mainArea() {
  const anchor = dock.panels.find((panel) => panel.id === SETTINGS || panel.id.startsWith("editor:"));
  return anchor
    ? { referencePanel: anchor.id, direction: "within" as const }
    : { referencePanel: OUTPUT, direction: "above" as const };
}

function reveal(id: string, add: () => void): void {
  const existing = dock.getPanel(id);
  if (existing) existing.api.setActive();
  else add();
}

async function openPanel(panel: string, arg: string | null): Promise<void> {
  switch (panel) {
    case "commands":
      return togglePalette(snapshot);
    case "settings":
      return reveal(SETTINGS, () =>
        dock.addPanel({ id: SETTINGS, component: "settings", title: "Settings", position: mainArea() }),
      );
    case "terminal":
      dock.addPanel({
        id: `terminal:${++terminals}`,
        component: "terminal",
        title: `Terminal ${terminals}`,
        position: { referencePanel: OUTPUT, direction: "within" },
      });
      return;
    case "editor": {
      if (!arg) return;
      editorModule ??= await import("./renderers/editor");
      return reveal(editorPanelId(arg), () =>
        dock.addPanel({
          id: editorPanelId(arg),
          component: "editor",
          title: arg.slice(arg.lastIndexOf("/") + 1),
          params: { path: arg },
          position: mainArea(),
        }),
      );
    }
  }
}

/** Makes the sidebar match the contributed views. */
function syncViews(): void {
  const wanted = new Map(snapshot.views.map((view) => [viewPanelId(view.id), view]));
  for (const panel of dock.panels) {
    if (panel.id.startsWith("view:") && !wanted.has(panel.id)) panel.api.close();
  }
  for (const [id, view] of wanted) {
    if (dock.getPanel(id)) continue;
    const sibling = dock.panels.find((panel) => panel.id.startsWith("view:"));
    dock.addPanel({
      id,
      component: "view",
      title: view.title,
      params: { viewId: view.id },
      position: sibling
        ? { referencePanel: sibling.id, direction: "within" }
        : { referencePanel: OUTPUT, direction: view.location === "sidebar" ? "left" : "within" },
      initialWidth: 280,
    });
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
  editorModule?.retheme(theme);
}

async function loadTheme(): Promise<void> {
  const sections = await commands.configSections();
  const property = sections.flatMap((section) => section.properties).find((p) => p.key === "workbench.colorTheme");
  applyTheme(String(property?.effective));
}

async function loadSnapshot(): Promise<void> {
  snapshot = await commands.snapshot();
  syncViews();
  renderStatus();
}

async function start(): Promise<void> {
  dock = createDockview(document.getElementById("dock")!, { createComponent, theme: themeDark });
  dock.addPanel({ id: OUTPUT, component: OUTPUT, title: "Output" });

  onHostEvent((event) => {
    switch (event.type) {
      case "contributionsChanged":
        void loadSnapshot().then(() => settings?.reload());
        break;
      case "message":
        toast(event.level, event.message, event.source);
        break;
      case "terminalData":
        output.write(event.data);
        break;
      case "viewRefresh":
        void trees.get(event.viewId)?.refresh();
        break;
      case "configChanged":
        void settings?.reload();
        if (event.keys.includes("workbench.colorTheme")) void loadTheme();
        break;
      case "openPanel":
        void openPanel(event.panel, event.arg ?? null);
        break;
      case "pluginState":
        pluginStates.set(event.pluginId, event.state);
        renderStatus();
        break;
    }
  });
  await connect();
  await loadSnapshot();
  await loadTheme();
  await openPanel("settings", null);
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
