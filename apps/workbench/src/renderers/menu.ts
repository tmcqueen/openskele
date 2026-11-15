// Generic menu bar: top-level buttons with dropdowns, built from menu data.

import { h } from "../dom";
import type { Menu } from "../host";
import { displayChord } from "../keys";

export interface MenuBarOptions {
  /** Called with the command id of the chosen item. */
  pick(command: string): void;
}

export interface MenuBar {
  readonly element: HTMLElement;
  update(menus: Menu[]): void;
}

export function createMenuBar(options: MenuBarOptions): MenuBar {
  const element = h("div", { class: "menubar", role: "menubar" });
  let buttons: HTMLButtonElement[] = [];
  let dropdowns: HTMLElement[] = [];
  let open: number | null = null;

  const items = (index: number) => Array.from(dropdowns[index].querySelectorAll<HTMLButtonElement>(".menu-item"));

  function show(index: number | null, focus: "first" | "last" | "button" | "none" = "none"): void {
    open = index;
    buttons.forEach((button, i) => {
      button.ariaExpanded = String(i === index);
      dropdowns[i].hidden = i !== index;
    });
    if (index === null) return;
    if (focus === "first") items(index).at(0)?.focus();
    else if (focus === "last") items(index).at(-1)?.focus();
    else if (focus === "button") buttons[index].focus();
  }

  function update(menus: Menu[]): void {
    open = null;
    buttons = [];
    dropdowns = [];
    element.replaceChildren(
      ...menus.map((menu, index) => {
        const button = h(
          "button",
          { class: "menubar-button", type: "button", role: "menuitem", ariaHasPopup: "menu", ariaExpanded: "false" },
          menu.title,
        );
        // mousedown rather than click, so press-drag-release onto an item feels native.
        button.addEventListener("mousedown", (event) => {
          event.preventDefault();
          show(open === index ? null : index, "button");
        });
        button.addEventListener("mouseenter", () => {
          if (open !== null && open !== index) show(index, "button");
        });

        const dropdown = h(
          "div",
          { class: "menu", role: "menu", hidden: true },
          ...menu.items.map((node) => {
            if (node.type === "separator") return h("div", { class: "menu-separator", role: "separator" });
            const item = h(
              "button",
              { class: "menu-item", type: "button", role: "menuitem", tabIndex: -1 },
              h("span", {}, node.title),
              node.accelerator && h("span", { class: "menu-accelerator" }, displayChord(node.accelerator)),
            );
            item.addEventListener("mouseenter", () => item.focus());
            item.addEventListener("click", () => {
              show(null);
              options.pick(node.command);
            });
            return item;
          }),
        );

        buttons.push(button);
        dropdowns.push(dropdown);
        return h("div", { class: "menubar-entry" }, button, dropdown);
      }),
    );
  }

  element.addEventListener("keydown", (event) => {
    const count = buttons.length;
    const current = open ?? buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (current < 0 || count === 0) return;
    const entries = items(current);
    const focused = entries.indexOf(document.activeElement as HTMLButtonElement);

    switch (event.key) {
      case "ArrowRight":
      case "ArrowLeft": {
        const next = (current + (event.key === "ArrowRight" ? 1 : -1) + count) % count;
        if (open === null) buttons[next].focus();
        else show(next, "first");
        break;
      }
      case "ArrowDown":
        if (open === null) show(current, "first");
        else entries[(focused + 1) % entries.length]?.focus();
        break;
      case "ArrowUp":
        if (open === null) show(current, "last");
        else entries[(focused - 1 + entries.length) % entries.length]?.focus();
        break;
      case "Enter":
      case " ":
        // On an item the button's own click handles it.
        if (focused >= 0) return;
        show(open === null ? current : null, "first");
        break;
      case "Escape":
        if (open === null) return;
        show(null);
        buttons[current].focus();
        break;
      default:
        return;
    }
    event.preventDefault();
  });

  document.addEventListener("mousedown", (event) => {
    if (open !== null && !element.contains(event.target as Node)) show(null);
  });
  window.addEventListener("blur", () => show(null));

  return { element, update };
}
