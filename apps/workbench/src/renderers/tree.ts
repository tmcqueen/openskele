// Generic tree: renders rows it is given and asks for children by row id.

import { h } from "../dom";
import type { TreeItem } from "../host";

export interface TreeOptions {
  load(parent: string | null): Promise<TreeItem[]>;
  activate(item: TreeItem): void;
}

export interface Tree {
  readonly element: HTMLElement;
  /** Re-queries from the root, keeping expanded rows expanded. */
  refresh(): Promise<void>;
}


export function createTree(options: TreeOptions): Tree {
  const element = h("div", { class: "tree", role: "tree" });
  const expanded = new Set<string>();
  let generation = 0;

  async function level(parent: string | null, depth: number): Promise<HTMLElement[]> {
    const items = await options.load(parent);
    const rows: HTMLElement[] = [];
    for (const item of items) {
      const open = item.collapsible && expanded.has(item.id);
      const row = h(
        "div",
        { class: "tree-row", role: "treeitem", tabIndex: 0, title: item.tooltip ?? "" },
        h("span", { class: "tree-twistie" }, item.collapsible ? (open ? "▾" : "▸") : ""),
        // Icons are codicon names.
        h("span", { class: item.icon ? `tree-icon codicon codicon-${item.icon}` : "tree-icon" }),
        h("span", { class: "tree-label" }, item.label),
        item.description && h("span", { class: "tree-description" }, item.description),
      );
      row.style.paddingLeft = `${8 + depth * 14}px`;
      if (item.collapsible) row.ariaExpanded = String(open);

      const activate = () => {
        if (item.collapsible) {
          if (!expanded.delete(item.id)) expanded.add(item.id);
          void refresh();
        }
        if (item.command) options.activate(item);
      };
      row.addEventListener("click", activate);
      row.addEventListener("keydown", (event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          activate();
        }
      });

      rows.push(row);
      if (open) rows.push(...(await level(item.id, depth + 1)));
    }
    return rows;
  }

  async function refresh(): Promise<void> {
    // Only the latest of overlapping refreshes may paint.
    const mine = ++generation;
    try {
      const rows = await level(null, 0);
      if (mine === generation) element.replaceChildren(...rows);
    } catch (error) {
      if (mine === generation) element.replaceChildren(h("div", { class: "tree-error" }, String(error)));
    }
  }

  return { element, refresh };
}
