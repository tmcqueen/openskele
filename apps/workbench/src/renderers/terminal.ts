// xterm.js surfaces: a read-only output console fed by host events, and an
// interactive terminal bound to a PTY the host owns.

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

const open = new Set<Terminal>();

/** Re-reads the CSS theme for every live terminal. */
export function retheme(): void {
  open.forEach((terminal) => (terminal.options.theme = theme()));
}

interface Surface {
  element: HTMLElement;
  terminal: Terminal;
  /** Called after each layout change with the new dimensions. */
  onFit(listener: (cols: number, rows: number) => void): void;
  dispose(): void;
}

function surface(options: { readOnly: boolean }): Surface {
  const element = h("div", { class: "term-host" });
  const terminal = new Terminal({
    fontFamily: "ui-monospace, 'Cascadia Mono', Menlo, Consolas, monospace",
    fontSize: 13,
    disableStdin: options.readOnly,
    cursorBlink: !options.readOnly,
    theme: theme(),
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(element);
  open.add(terminal);

  let fitted: ((cols: number, rows: number) => void) | undefined;
  const observer = new ResizeObserver(() => {
    // A hidden tab has no size; fitting then would collapse the grid.
    if (element.clientWidth === 0 || element.clientHeight === 0) return;
    fit.fit();
    fitted?.(terminal.cols, terminal.rows);
  });
  observer.observe(element);

  return {
    element,
    terminal,
    onFit: (listener) => (fitted = listener),
    dispose: () => {
      observer.disconnect();
      open.delete(terminal);
      terminal.dispose();
    },
  };
}

export function createOutput(): { element: HTMLElement; write(data: string): void } {
  const { element, terminal } = surface({ readOnly: true });
  return { element, write: (data) => terminal.write(data) };
}

export function createPtyTerminal(): { element: HTMLElement; dispose(): void } {
  const { element, terminal, onFit, dispose } = surface({ readOnly: false });
  let id: number | undefined;
  let disposed = false;

  const events = new Channel<TerminalEvent>();
  events.onmessage = (event) => {
    if (event.type === "data") terminal.write(event.data);
    else terminal.write("\r\n[process exited]\r\n");
  };

  commands.terminalOpen(terminal.cols, terminal.rows, events).then(
    (opened) => {
      id = opened;
      if (disposed) void commands.terminalClose(opened);
    },
    (error) => terminal.write(`failed to start terminal: ${error}\r\n`),
  );
  terminal.onData((data) => {
    if (id !== undefined) void commands.terminalWrite(id, data);
  });
  onFit((cols, rows) => {
    if (id !== undefined) void commands.terminalResize(id, cols, rows);
  });

  return {
    element,
    dispose: () => {
      disposed = true;
      if (id !== undefined) void commands.terminalClose(id);
      dispose();
    },
  };
}
