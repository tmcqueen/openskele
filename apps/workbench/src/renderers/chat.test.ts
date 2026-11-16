// The transcript renderer over plain data.

import { expect, test } from "bun:test";

import type { Entry } from "../host";
import { createChat } from "./chat";

const ask: Entry = {
  type: "permissionRequest",
  id: "p1",
  title: "Do the thing",
  options: [
    { id: "allow", name: "Allow", kind: "allow_once" },
    { id: "reject", name: "Reject", kind: "reject_once" },
  ],
  answer: null,
};

test("renders each kind of entry", () => {
  const chat = createChat({ onAnswer: () => {} });
  chat.set({
    entries: [
      { type: "userMessage", text: "hi" },
      { type: "agentText", text: "**bold**" },
      { type: "toolCall", id: "t", title: "Open settings", kind: "other", status: "completed", content: ["done"] },
      { type: "plan", entries: [{ content: "Read", status: "completed" }, { content: "Edit", status: "pending" }] },
      { type: "error", message: "boom" },
    ],
  });
  const el = chat.element;
  expect(el.querySelector(".chat-user")?.textContent).toBe("hi");
  expect(el.querySelector(".chat-agent strong")?.textContent).toBe("bold");
  expect(el.querySelector(".chat-tool")?.textContent).toContain("Open settings");
  expect(el.querySelectorAll(".chat-plan li").length).toBe(2);
  expect(el.querySelector(".chat-error")?.textContent).toContain("boom");
});

test("a waiting permission request offers its options", () => {
  const answers: [string, string | null][] = [];
  const chat = createChat({ onAnswer: (request, option) => void answers.push([request, option]) });
  chat.set({ entries: [ask] });
  const buttons = [...chat.element.querySelectorAll<HTMLButtonElement>(".chat-permission button")];
  expect(buttons.map((b) => b.textContent)).toEqual(["Allow", "Reject"]);
  buttons[1].click();
  expect(answers).toEqual([["p1", "reject"]]);
});

test("an update replaces its entry in place", () => {
  const chat = createChat({ onAnswer: () => {} });
  chat.set({ entries: [{ type: "agentText", text: "Hel" }] });
  chat.update(0, { type: "agentText", text: "Hello" });
  chat.update(1, { ...ask, answer: "allow" });
  expect(chat.element.querySelectorAll(".chat-entry").length).toBe(2);
  expect(chat.element.querySelector(".chat-agent")?.textContent?.trim()).toBe("Hello");
  const answered = chat.element.querySelector(".chat-permission")!;
  expect(answered.querySelector("button")).toBeNull();
  expect(answered.textContent).toContain("Allow");
});
