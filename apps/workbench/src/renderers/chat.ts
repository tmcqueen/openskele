// Generic chat renderer over a transcript: plain data in, DOM out. Agent text
// is markdown (rendered inert); everything else is built as text.

import { h } from "../dom";
import type { Entry, Transcript } from "../host";
import { renderMarkdown } from "./markdown";

export interface Chat {
  readonly element: HTMLElement;
  /** Shows a whole transcript, replacing what was there. */
  set(transcript: Transcript): void;
  /** Adds the entry at `index`, or replaces the one already there. */
  update(index: number, entry: Entry): void;
}

const STATUS_ICON: Record<string, string> = {
  pending: "circle-large-outline",
  in_progress: "loading codicon-modifier-spin",
  completed: "check",
  failed: "error",
};

function icon(name: string): HTMLElement {
  return h("span", { class: `codicon codicon-${name}` });
}

function render(entry: Entry, onAnswer: (request: string, option: string | null) => void): HTMLElement {
  switch (entry.type) {
    case "userMessage":
      return h("div", { class: "chat-entry chat-user" }, entry.text);
    case "agentText":
      return h("div", { class: "chat-entry chat-agent" }, renderMarkdown(entry.text));
    case "thought": {
      const details = h("details", { class: "chat-entry chat-thought" }, h("summary", {}, "Thinking"));
      details.append(h("div", {}, entry.text));
      return details;
    }
    case "toolCall": {
      const details = h(
        "details",
        { class: "chat-entry chat-tool" },
        h("summary", {}, icon(STATUS_ICON[entry.status] ?? "tools"), ` ${entry.title || entry.id}`),
      );
      for (const text of entry.content) details.append(h("pre", {}, text));
      return details;
    }
    case "plan":
      return h(
        "ul",
        { class: "chat-entry chat-plan" },
        ...entry.entries.map((item) =>
          h("li", { class: item.status }, icon(item.status === "completed" ? "pass-filled" : "circle-large-outline"), ` ${item.content}`),
        ),
      );
    case "permissionRequest": {
      const box = h("div", { class: "chat-entry chat-permission" }, h("div", { class: "chat-permission-title" }, entry.title));
      if (entry.answer === null || entry.answer === undefined) {
        const buttons = entry.options.map((option) => {
          const button = h("button", { type: "button", class: option.kind.startsWith("allow") ? "primary" : "" }, option.name);
          button.addEventListener("click", () => onAnswer(entry.id, option.id));
          return button;
        });
        box.append(h("div", { class: "chat-permission-options" }, ...buttons));
      } else {
        const chosen = entry.options.find((option) => option.id === entry.answer)?.name ?? "Cancelled";
        box.append(h("div", { class: "chat-permission-answer" }, chosen));
      }
      return box;
    }
    case "error":
      return h("div", { class: "chat-entry chat-error" }, icon("error"), ` ${entry.message}`);
  }
}

export function createChat(options: { onAnswer(request: string, option: string | null): void }): Chat {
  const element = h("div", { class: "chat", role: "log" });
  const rows: HTMLElement[] = [];

  function place(index: number, entry: Entry): void {
    const row = render(entry, options.onAnswer);
    if (rows[index]) rows[index].replaceWith(row);
    else element.append(row);
    rows[index] = row;
  }

  return {
    element,
    set(transcript) {
      rows.length = 0;
      element.replaceChildren();
      transcript.entries.forEach((entry, index) => place(index, entry));
    },
    update(index, entry) {
      // Keep the view pinned to the bottom if it was already there.
      const atBottom = element.scrollHeight - element.scrollTop - element.clientHeight < 8;
      place(index, entry);
      if (atBottom) element.scrollTop = element.scrollHeight;
    },
  };
}
