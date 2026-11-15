// Custom title bar: menu, drag region and window controls in one strip. Used
// when the host says the webview owns the chrome; the window itself is still
// moved, minimized and closed by the host.

import { h } from "./dom";
import { commands, type Chrome, type Menu } from "./host";
import { createMenuBar } from "./renderers/menu";
import { toast } from "./toast";

export interface TitleBar {
  readonly element: HTMLElement;
  setMenus(menus: Menu[]): void;
}

const report = (error: unknown) => toast("error", String(error));

function control(label: string, glyph: string, act: () => Promise<unknown>): HTMLButtonElement {
  const button = h("button", { class: "window-control", type: "button", ariaLabel: label, title: label }, glyph);
  button.addEventListener("click", () => void act().catch(report));
  return button;
}

export function createTitleBar(chrome: Chrome, pick: (command: string) => void): TitleBar {
  // macOS keeps its system menu bar and its own window buttons.
  const mac = chrome.platform === "macos";
  const menu = mac ? undefined : createMenuBar({ pick });

  const drag = h("div", { class: "titlebar-drag" }, document.title);
  drag.addEventListener("mousedown", (event) => {
    if (event.button !== 0) return;
    // The second press of a double-click toggles; a single press starts a move.
    const act = event.detail === 2 ? commands.windowToggleMaximize() : commands.windowStartDrag();
    void act.catch(report);
  });

  const controls =
    !mac &&
    h(
      "div",
      { class: "window-controls" },
      control("Minimize", "─", commands.windowMinimize),
      control("Maximize", "□", commands.windowToggleMaximize),
      control("Close", "✕", commands.windowClose),
    );

  const element = h("header", { id: "titlebar", class: mac ? "titlebar mac" : "titlebar" }, menu?.element, drag, controls);
  return { element, setMenus: (menus) => menu?.update(menus) };
}
