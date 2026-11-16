// xterm.js surfaces: a read-only output console fed by host events, and an
// interactive view of a PTY the host owns.

import { Channel } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

import { h } from "../dom";
import { commands, type TerminalEvent } from "../host";

function theme() {
  const style = getComputedStyle(document.documentElement);
  return {
    background: style.getPropertyValue("--panel").trim(),
    foreground: style.getPropertyValue("--text").trim(),
    cursor: style.getPropertyValue("--text").trim(),
  };
}

const DEFAULT_FONT = "ui-monospace, 'Cascadia Mono', Menlo, Consolas, monospace";
let font = { family: DEFAULT_FONT, size: 13 };

interface Surface {
  element: HTMLElement;
  terminal: Terminal;
  /** Recomputes the grid for the element's size; a no-op while hidden. */
  refit(): void;
  /** Called after each layout change with the new dimensions. */
  onFit(listener: (cols: number, rows: number) => void): void;
  dispose(): void;
}

const open = new Set<Surface>();

/** Re-reads the CSS theme for every live terminal. */
export function retheme(): void {
  open.forEach(({ terminal }) => (terminal.options.theme = theme()));
}

/** Sets the font of every terminal, live and future. An empty family means the default. */
export function setFont(family: string, size: number): void {
  // The default stays as the fallback so a misspelled font still renders monospaced.
  font = { family: family.trim() ? `${family}, ${DEFAULT_FONT}` : DEFAULT_FONT, size };
  open.forEach(({ terminal, refit }) => {
    terminal.options.fontFamily = font.family;
    terminal.options.fontSize = font.size;
    refit();
  });
}

function surface(options: { readOnly: boolean }): Surface {
  const element = h("div", { class: "term-host" });
  const terminal = new Terminal({
    fontFamily: font.family,
    fontSize: font.size,
    disableStdin: options.readOnly,
    cursorBlink: !options.readOnly,
    theme: theme(),
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(element);

  let fitted: ((cols: number, rows: number) => void) | undefined;
  const refit = () => {
    // A hidden tab has no size; fitting then would collapse the grid.
    if (element.clientWidth === 0 || element.clientHeight === 0) return;
    fit.fit();
    fitted?.(terminal.cols, terminal.rows);
  };
  const observer = new ResizeObserver(refit);
  observer.observe(element);

  const self: Surface = {
    element,
    terminal,
    refit,
    onFit: (listener) => (fitted = listener),
    dispose: () => {
      observer.disconnect();
      open.delete(self);
      terminal.dispose();
    },
  };
  open.add(self);
  return self;
}

export function createOutput(): { element: HTMLElement; write(data: string): void } {
  const { element, terminal } = surface({ readOnly: true });
  return { element, write: (data) => terminal.write(data) };
}

/**
 * Shows a terminal the host owns. Closing the view leaves the shell running;
 * ending it is the layout's job.
 */
export function createPtyTerminal(id: number): { element: HTMLElement; dispose(): void } {
  const { element, terminal, onFit, refit, dispose } = surface({ readOnly: false });

  const events = new Channel<TerminalEvent>();
  events.onmessage = (event) => {
    if (event.type === "data") terminal.write(event.data);
    else terminal.write("\r\n[process exited]\r\n");
  };
  commands.terminalAttach(id, events).then(
    () => refit(),
    (error) => terminal.write(`failed to attach to terminal ${id}: ${error}\r\n`),
  );
  terminal.onData((data) => void commands.terminalWrite(id, data));
  onFit((cols, rows) => void commands.terminalResize(id, cols, rows));

  return { element, dispose };
}
