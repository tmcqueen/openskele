// Generic filterable list with keyboard selection.

import { h } from "../dom";

export interface ListEntry {
  label: string;
  detail?: string;
}

export interface ListOptions<T extends ListEntry> {
  items: T[];
  placeholder: string;
  pick(item: T): void;
  cancel(): void;
}

export function createList<T extends ListEntry>(options: ListOptions<T>): { element: HTMLElement; focus(): void } {
  const input = h("input", { class: "list-filter", type: "text", placeholder: options.placeholder });
  const rows = h("div", { class: "list-rows", role: "listbox" });
  const element = h("div", { class: "list" }, input, rows);
  let visible: T[] = [];
  let selected = 0;

  function render(): void {
    const query = input.value.trim().toLowerCase();
    visible = options.items.filter((item) => item.label.toLowerCase().includes(query));
    selected = Math.min(selected, Math.max(visible.length - 1, 0));
    rows.replaceChildren(
      ...visible.map((item, index) => {
        const row = h(
          "div",
          { class: "list-row", role: "option", ariaSelected: String(index === selected) },
          h("span", {}, item.label),
          item.detail && h("span", { class: "list-detail" }, item.detail),
        );
        row.addEventListener("click", () => options.pick(item));
        return row;
      }),
    );
    rows.children[selected]?.scrollIntoView({ block: "nearest" });
  }

  input.addEventListener("input", () => {
    selected = 0;
    render();
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      selected = (selected + step + visible.length) % Math.max(visible.length, 1);
      render();
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (visible[selected]) options.pick(visible[selected]);
    } else if (event.key === "Escape") {
      event.preventDefault();
      options.cancel();
    }
  });

  render();
  return { element, focus: () => input.focus() };
}
