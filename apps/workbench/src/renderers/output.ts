// The Output panel: a channel picker and Clear button over a read-only
// terminal view. Channels live in the host; this shows one at a time,
// replaying its recent text and then following it.

import { h } from "../dom";

/** What the panel needs from the host. */
export interface OutputHost {
  channels(): Promise<string[]>;
  read(channel: string): Promise<string>;
  clear(channel: string): Promise<void>;
}

/** Where the text goes: a terminal surface in the app, a stand-in in tests. */
export interface OutputView {
  readonly element: HTMLElement;
  write(text: string): void;
  reset(): void;
}

export interface OutputPanel {
  readonly element: HTMLElement;
  /**
   * Fetches the channel list and shows the selected channel. Call it once
   * host events are subscribed, so nothing written meanwhile is missed.
   */
  start(): Promise<void>;
  /** Text written to a channel; shown if it is the selected one. */
  append(channel: string, text: string): void;
  /** A channel was added: refresh the list, keeping the selection. */
  channelsChanged(): Promise<void>;
  /** A channel was emptied. */
  cleared(channel: string): void;
}

const SELECTION_KEY = "skele.output.channel";
const WORKBENCH = "Workbench";

function load(storage: Storage | undefined): string | null {
  try {
    return storage?.getItem(SELECTION_KEY) ?? null;
  } catch {
    return null;
  }
}

function save(storage: Storage | undefined, channel: string): void {
  try {
    storage?.setItem(SELECTION_KEY, channel);
  } catch {
    // Unavailable storage only costs remembering the choice.
  }
}

export function createOutputPanel(host: OutputHost, view: OutputView, storage?: Storage): OutputPanel {
  const picker = h("select", { class: "output-channel", title: "Output channel" });
  const clear = h("button", { class: "output-clear", title: "Clear output", type: "button" }, h("span", { class: "codicon codicon-clear-all" }));
  const header = h("div", { class: "output-header" }, picker, clear);
  const element = h("div", { class: "output-panel" }, header, view.element);

  let selected = WORKBENCH;
  // While a channel's text is being fetched, live text for it is already in
  // what the fetch returns, so it is not written twice.
  let loading = true;

  function fill(channels: string[]): void {
    picker.replaceChildren(...channels.map((channel) => h("option", { value: channel }, channel)));
    picker.value = selected;
  }

  async function show(channel: string): Promise<void> {
    selected = channel;
    picker.value = channel;
    loading = true;
    try {
      const text = await host.read(channel);
      if (selected !== channel) return;
      view.reset();
      view.write(text);
    } finally {
      if (selected === channel) loading = false;
    }
  }

  picker.addEventListener("change", () => {
    save(storage, picker.value);
    void show(picker.value);
  });
  clear.addEventListener("click", () => void host.clear(selected));

  return {
    element,
    async start() {
      const channels = await host.channels();
      const stored = load(storage);
      selected = stored && channels.includes(stored) ? stored : WORKBENCH;
      fill(channels);
      await show(selected);
    },
    append(channel, text) {
      if (channel === selected && !loading) view.write(text);
    },
    async channelsChanged() {
      fill(await host.channels());
    },
    cleared(channel) {
      if (channel === selected) view.reset();
    },
  };
}
