// The agent pane against a stand-in host: per-session state and ended sessions.

import { expect, mock, test } from "bun:test";

const finishPrompt: Record<string, () => void> = {};
const cancelled: string[] = [];
let next = 0;
mock.module("./host", () => ({
  commands: {
    agentList: async () => [{ id: "scripted", name: "Scripted", source: "x" }],
    agentNewSession: async () => `s${++next}`,
    agentTranscript: async () => ({ entries: [] }),
    agentPrompt: (session: string) =>
      new Promise((resolve) => (finishPrompt[session] = () => resolve("endTurn"))),
    agentCancel: async (session: string) => void cancelled.push(session),
    agentCloseSession: async () => null,
    agentAnswerPermission: async () => null,
  },
}));
mock.module("./toast", () => ({ toast: () => {} }));

const { createAgentPane } = await import("./agent");

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

function parts(pane: { element: HTMLElement }) {
  const [send, stop] = pane.element.querySelectorAll<HTMLButtonElement>(".agent-actions button");
  return {
    send,
    stop,
    input: pane.element.querySelector<HTMLTextAreaElement>(".agent-input")!,
    newSession: pane.element.querySelector<HTMLButtonElement>('button[title="New session"]')!,
    sessions: pane.element.querySelector<HTMLSelectElement>(".agent-sessions")!,
  };
}

async function say(pane: { element: HTMLElement }, text: string) {
  const { input, send } = parts(pane);
  input.value = text;
  send.click();
  await tick();
}

test("Stop belongs to the session whose turn is running", async () => {
  const pane = createAgentPane();
  await pane.refresh();
  const ui = parts(pane);
  ui.newSession.click();
  await tick();
  await say(pane, "long task"); // s1 is now busy
  expect(ui.stop.hidden).toBe(false);

  ui.newSession.click();
  await tick(); // s2 is shown and idle
  expect(ui.sessions.value).toBe("s2");
  expect(ui.stop.hidden).toBe(true);
  expect(ui.send.hidden).toBe(false);

  ui.sessions.value = "s1";
  ui.sessions.dispatchEvent(new Event("change"));
  await tick();
  expect(ui.stop.hidden).toBe(false);
  ui.stop.click();
  await tick();
  expect(cancelled).toEqual(["s1"]);
  finishPrompt.s1();
  await tick();
  expect(ui.stop.hidden).toBe(true);
});

test("an ended session stays readable but takes no input", async () => {
  const pane = createAgentPane();
  await pane.refresh();
  const ui = parts(pane);
  ui.newSession.click();
  await tick();
  const id = ui.sessions.value;
  pane.onEvent({ type: "agentSessionEnded", session: id, reason: "the agent exited (exit status: 3)" });
  await tick();
  expect(ui.sessions.value).toBe(id);
  expect([...ui.sessions.options].some((o) => o.value === id && o.textContent!.includes("ended"))).toBe(true);
  expect(ui.input.disabled).toBe(true);
  expect(pane.element.querySelector<HTMLElement>(".chat")!.hidden).toBe(false);
});
