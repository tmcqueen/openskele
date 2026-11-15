// Command palette: the list renderer over the command registry.

import { h } from "./dom";
import { run, type Snapshot } from "./host";
import { displayChord } from "./keys";
import { createList } from "./renderers/list";

let current: HTMLElement | undefined;

export function closePalette(): void {
  current?.remove();
  current = undefined;
}

export function togglePalette(snapshot: Snapshot): void {
  if (current) return closePalette();

  const chords = new Map(snapshot.keybindings.map((binding) => [binding.command, binding.key]));
  const list = createList({
    placeholder: "Type a command",
    items: snapshot.commands.map((command) => ({
      id: command.id,
      label: command.category ? `${command.category}: ${command.title}` : command.title,
      detail: chords.has(command.id) ? displayChord(chords.get(command.id)!) : undefined,
    })),
    pick: (item) => {
      closePalette();
      void run(item.id);
    },
    cancel: closePalette,
  });

  const backdrop = h("div", { class: "palette-backdrop" }, h("div", { class: "palette" }, list.element));
  backdrop.addEventListener("mousedown", (event) => {
    if (event.target === backdrop) closePalette();
  });
  document.body.append(backdrop);
  current = backdrop;
  list.focus();
}
