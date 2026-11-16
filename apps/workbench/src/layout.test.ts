// The main area against a real Dockview in happy-dom, with the host stubbed:
// what the webview asks of the host, and that it never shows panels the host
// does not have.

import { beforeEach, expect, mock, test } from "bun:test";

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
  return { dock, main: createMainArea(dock) };
}

const bar = { position: "left", visible: true, activeContainer: null } as const;
const layout = (paths: string[], active: string | null): Layout => ({
  primary: bar,
  secondary: { ...bar, position: "right" },
  panels: [{ kind: "output" }, { kind: "settings" }, ...paths.map((path) => ({ kind: "editor" as const, path }))],
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

/** Output closed: settings plus terminals. */
const withTerminals = (ids: number[], active: string | null): Layout => ({
  ...layout([], active),
  panels: [{ kind: "settings" }, ...ids.map((id) => ({ kind: "terminal" as const, id }))],
});

const without = (l: Layout, kind: string): Layout => ({ ...l, panels: l.panels.filter((p) => p.kind !== kind) });

test("terminals open after the Output panel was closed", async () => {
  const { dock, main } = setup();
  await main.reconcile(layout([], "settings"));
  dock.getPanel(OUTPUT)!.api.close();
  await main.reconcile(without(layout([], "settings"), "output"));
  await main.reconcile(withTerminals([1], "terminal:1"));
  await main.reconcile(withTerminals([1, 2], "terminal:2"));
  expect(dock.getPanel("terminal:1")).toBeDefined();
  expect(dock.getPanel("terminal:2")).toBeDefined();
  // Terminals share a group rather than each opening a new split.
  expect(dock.getPanel("terminal:2")!.group).toBe(dock.getPanel("terminal:1")!.group);
});

test("settings opens when neither Output nor a document is open", async () => {
  const { dock, main } = setup();
  await main.reconcile(without(layout([], "settings"), "output"));
  expect(dock.getPanel("settings")).toBeDefined();
});

test("output reopens below the documents after being closed", async () => {
  const { dock, main } = setup();
  await main.reconcile(layout(["/a"], "editor:/a"));
  dock.getPanel(OUTPUT)!.api.close();
  expect(calls).toContain("close output");
  await main.reconcile(without(layout(["/a"], "editor:/a"), "output"));
  await main.reconcile(layout(["/a"], OUTPUT));
  const output = dock.getPanel(OUTPUT);
  expect(output).toBeDefined();
  expect(output!.group).not.toBe(dock.getPanel("editor:/a")!.group);
  expect(dock.activePanel?.id).toBe(OUTPUT);
});

test("with nothing to anchor to, panels open in the main grid, not in a bar", async () => {
  const { dock, main } = setup();
  // An app bar exists and holds the active panel, as at startup.
  const bar = dock.addEdgeGroup("right", { id: "bar:secondary", initialSize: 200, collapsedSize: 44 });
  dock.addPanel({ id: "container:x", component: "container", title: "X", position: { referenceGroup: bar.id } });
  dock.getPanel("container:x")!.api.setActive();
  await main.reconcile(layout([], "settings"));
  for (const id of [OUTPUT, "settings"]) {
    expect(dock.getPanel(id)!.group.id).not.toBe(bar.id);
  }
  expect(dock.getPanel("settings")!.group.id).not.toBe(dock.getPanel(OUTPUT)!.group.id);
});
