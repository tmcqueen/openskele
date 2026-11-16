// The main area against a real Dockview in happy-dom, with the host stubbed:
// what the webview asks of the host, and that it never shows panels the host
// does not have.

import { GlobalRegistrator } from "@happy-dom/global-registrator";
import { beforeEach, expect, mock, test } from "bun:test";

GlobalRegistrator.register();

const calls: string[] = [];
mock.module("./host", () => ({
  commands: {
    layoutClosePanel: async (id: string) => void calls.push(`close ${id}`),
    layoutSetActivePanel: async (id: string) => void calls.push(`activate ${id}`),
  },
}));
mock.module("./toast", () => ({ toast: () => {} }));
// Stands in for Monaco; resolves a turn later, like a real chunk load.
mock.module("./renderers/editor", () => ({ createEditor: () => ({ element: document.createElement("div"), dispose() {} }) }));

const { createDockview } = await import("dockview-core/dist/dockview-core.js");
const { createMainArea, OUTPUT } = await import("./layout");
type Layout = import("./host").Layout;

function setup() {
  const element = document.createElement("div");
  document.body.append(element);
  const dock = createDockview(element, {
    createComponent: () => ({ element: document.createElement("div"), init: () => {} }),
  });
  dock.addPanel({ id: OUTPUT, component: OUTPUT, title: "Output" });
  return { dock, main: createMainArea(dock) };
}

const bar = { position: "left", visible: true, activeContainer: null } as const;
const layout = (paths: string[], active: string | null): Layout => ({
  primary: bar,
  secondary: { ...bar, position: "right" },
  panels: [{ kind: "settings" }, ...paths.map((path) => ({ kind: "editor" as const, path }))],
  activePanel: active,
});

beforeEach(() => void (calls.length = 0));

// First in the file: it needs the editor module not to be loaded yet, so the
// first reconcile really waits for it.
test("a stale reconcile does not bring back a closed panel", async () => {
  const { dock, main } = setup();
  const opened = main.reconcile(layout(["/a"], "editor:/a"));
  const closed = main.reconcile(layout([], "settings"));
  await Promise.all([opened, closed]);
  expect(dock.panels.map((p) => p.id).sort()).toEqual([OUTPUT, "settings"]);
});

test("closing a tab asks the host to close it and nothing else", async () => {
  const { dock, main } = setup();
  await main.reconcile(layout(["/e1", "/e2"], "editor:/e2"));
  // Make /e2 the group's most recently used panel, then the user closes /e1.
  await main.reconcile(layout(["/e1", "/e2"], "editor:/e1"));
  await main.reconcile(layout(["/e1", "/e2"], "editor:/e2"));
  await main.reconcile(layout(["/e1", "/e2"], "editor:/e1"));
  calls.length = 0;
  dock.getPanel("editor:/e1")!.api.close();
  await Promise.resolve();
  expect(calls).toEqual(["close editor:/e1"]);
});

test("a user activating a tab is sent to the host", async () => {
  const { dock, main } = setup();
  await main.reconcile(layout(["/e1"], "editor:/e1"));
  dock.getPanel("settings")!.api.setActive();
  await Promise.resolve();
  expect(calls).toEqual(["activate settings"]);
});
