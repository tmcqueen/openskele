// Makes the main area match the host's layout. Panels are opened, closed and
// activated here only; user gestures on them become requests to the host,
// whose `layoutChanged` event then comes back through `reconcile`.

import type { AddPanelOptions, DockviewApi } from "dockview-core/dist/dockview-core.js";

import { commands, type Layout, type Panel } from "./host";
import { toast } from "./toast";

export const OUTPUT = "output";

type EditorModule = typeof import("./renderers/editor");
let editorModule: EditorModule | undefined;

/** The editor renderer, once some editor panel has loaded it. */
export const loadedEditor = () => editorModule;

const MAIN = /^(output$|settings$|editor:|terminal:)/;

function panelId(panel: Panel): string {
  switch (panel.kind) {
    case "output":
      return OUTPUT;
    case "settings":
      return "settings";
    case "editor":
      return `editor:${panel.path}`;
    case "terminal":
      return `terminal:${panel.id}`;
  }
}

const isDocument = (id: string) => id === "settings" || id.startsWith("editor:");
const isBottom = (id: string) => id === OUTPUT || id.startsWith("terminal:");

// Placement is relative to panels that exist right now: any of them, the
// Output panel included, may have been closed. With nothing to anchor to, a
// panel goes to an edge of the main grid; left to itself Dockview would use
// the active group, which can be an app bar.
const TOP: AddPanelOptions["position"] = { direction: "above" };
const BOTTOM: AddPanelOptions["position"] = { direction: "below" };

/** Where documents and settings open: beside an existing one, else above the output or terminals. */
function mainArea(dock: DockviewApi): AddPanelOptions["position"] {
  const document = dock.panels.find((panel) => isDocument(panel.id));
  if (document) return { referencePanel: document.id, direction: "within" };
  const bottom = dock.panels.find((panel) => isBottom(panel.id));
  return bottom ? { referencePanel: bottom.id, direction: "above" } : TOP;
}

/** Where the output opens: with the terminals, else below the documents. */
function outputArea(dock: DockviewApi): AddPanelOptions["position"] {
  const terminal = dock.panels.find((panel) => panel.id.startsWith("terminal:"));
  if (terminal) return { referencePanel: terminal.id, direction: "within" };
  const document = dock.panels.find((panel) => isDocument(panel.id));
  return document ? { referencePanel: document.id, direction: "below" } : BOTTOM;
}

/** Where terminals open: with the other terminals or the output, else below the documents. */
function bottomArea(dock: DockviewApi): AddPanelOptions["position"] {
  const bottom =
    dock.panels.find((panel) => panel.id.startsWith("terminal:")) ?? dock.panels.find((panel) => panel.id === OUTPUT);
  if (bottom) return { referencePanel: bottom.id, direction: "within" };
  const document = dock.panels.find((panel) => isDocument(panel.id));
  return document ? { referencePanel: document.id, direction: "below" } : BOTTOM;
}

function options(dock: DockviewApi, panel: Panel): AddPanelOptions {
  const id = panelId(panel);
  switch (panel.kind) {
    case "output":
      return { id, component: OUTPUT, title: "Output", position: outputArea(dock) };
    case "settings":
      return { id, component: "settings", title: "Settings", position: mainArea(dock) };
    case "editor":
      return {
        id,
        component: "editor",
        title: panel.path.slice(panel.path.lastIndexOf("/") + 1),
        params: { path: panel.path },
        position: mainArea(dock),
      };
    case "terminal":
      return {
        id,
        component: "terminal",
        title: `Terminal ${panel.id}`,
        params: { id: panel.id },
        position: bottomArea(dock),
      };
  }
}

export interface MainArea {
  reconcile(layout: Layout): Promise<void>;
}

/** Wires Dockview gestures to the host and returns the reconciler. */
export function createMainArea(dock: DockviewApi): MainArea {
  // Set while this module changes Dockview, so its own changes are not sent back.
  let applying = false;
  let current: Layout | undefined;

  dock.onDidRemovePanel((panel) => {
    if (applying || !MAIN.test(panel.id)) return;
    commands.layoutClosePanel(panel.id).catch((error) => toast("error", String(error)));
  });
  dock.onDidActivePanelChange(({ panel }) => {
    if (applying || !panel || !MAIN.test(panel.id) || panel.id === current?.activePanel) return;
    // Closing a tab makes Dockview activate a neighbour of its own choosing;
    // the host picks the next active panel itself. Wait out the rest of the
    // gesture and drop the activation if the host's active panel was closed.
    const id = panel.id;
    queueMicrotask(() => {
      const closed = current?.activePanel && !dock.getPanel(current.activePanel);
      if (closed || id === current?.activePanel) return;
      commands.layoutSetActivePanel(id).catch((error) => toast("error", String(error)));
    });
  });

  return {
    async reconcile(layout) {
      current = layout;
      if (!editorModule && layout.panels.some((panel) => panel.kind === "editor")) {
        try {
          editorModule = await import("./renderers/editor");
        } catch (error) {
          toast("error", `Could not load the editor: ${error}`);
          return;
        }
        // A newer layout arrived while the editor loaded; it has been applied.
        if (current !== layout) return;
      }
      applying = true;
      try {
        const wanted = new Map(layout.panels.map((panel) => [panelId(panel), panel]));
        for (const panel of dock.panels.filter((p) => MAIN.test(p.id) && !wanted.has(p.id))) panel.api.close();
        for (const [id, panel] of wanted) {
          if (!dock.getPanel(id)) dock.addPanel(options(dock, panel));
        }
        if (layout.activePanel) dock.getPanel(layout.activePanel)?.api.setActive();
      } finally {
        applying = false;
      }
    },
  };
}
