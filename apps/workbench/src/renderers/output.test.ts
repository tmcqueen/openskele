// The Output panel's channel picker against a stand-in host and view.

import { beforeEach, expect, test } from "bun:test";

const { createOutputPanel } = await import("./output");

let buffers: Record<string, string>;
let shown: string;
let cleared: string[];
const host = {
  channels: async () => Object.keys(buffers),
  read: async (channel: string) => buffers[channel] ?? "",
  clear: async (channel: string) => void cleared.push(channel),
};
const view = {
  element: document.createElement("div"),
  write: (text: string) => void (shown += text),
  reset: () => void (shown = ""),
};

function storage(initial: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: () => null,
    get length() {
      return map.size;
    },
  };
}

const select = (panel: { element: HTMLElement }) => panel.element.querySelector("select")!;

async function choose(panel: { element: HTMLElement }, channel: string) {
  select(panel).value = channel;
  select(panel).dispatchEvent(new Event("change"));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  buffers = { Workbench: "started\r\n", Hello: "hi\r\n", "Runtime Info": "rt\r\n" };
  shown = "";
  cleared = [];
});

test("lists channels and shows Workbench", async () => {
  const panel = createOutputPanel(host, view, storage());
  await panel.ready;
  const options = [...select(panel).options].map((o) => o.value);
  expect(options).toEqual(["Workbench", "Hello", "Runtime Info"]);
  expect(shown).toBe("started\r\n");
});

test("switching replays the channel, then follows only it", async () => {
  const panel = createOutputPanel(host, view, storage());
  await panel.ready;
  await choose(panel, "Hello");
  expect(shown).toBe("hi\r\n");
  panel.append("Hello", "x");
  panel.append("Runtime Info", "y");
  expect(shown).toBe("hi\r\nx");
});

test("the selection survives a reload", async () => {
  const store = storage();
  const first = createOutputPanel(host, view, store);
  await first.ready;
  await choose(first, "Hello");
  const second = createOutputPanel(host, view, store);
  await second.ready;
  expect(select(second).value).toBe("Hello");
  expect(shown).toBe("hi\r\n");
});

test("a stored channel that no longer exists falls back to Workbench", async () => {
  const panel = createOutputPanel(host, view, storage({ "skele.output.channel": "Gone" }));
  await panel.ready;
  expect(select(panel).value).toBe("Workbench");
  expect(shown).toBe("started\r\n");
});

test("Clear clears the selected channel; later text still shows", async () => {
  const panel = createOutputPanel(host, view, storage());
  await panel.ready;
  await choose(panel, "Hello");
  panel.element.querySelector("button")!.click();
  expect(cleared).toEqual(["Hello"]);
  panel.cleared("Hello");
  expect(shown).toBe("");
  panel.append("Hello", "after");
  expect(shown).toBe("after");
  panel.cleared("Runtime Info");
  expect(shown).toBe("after");
});

test("a new channel appears in the list without changing the selection", async () => {
  const panel = createOutputPanel(host, view, storage());
  await panel.ready;
  await choose(panel, "Hello");
  buffers["New"] = "";
  await panel.channelsChanged();
  expect([...select(panel).options].map((o) => o.value)).toContain("New");
  expect(select(panel).value).toBe("Hello");
});
