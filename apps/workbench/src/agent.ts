// The agent pane: pick an agent, start sessions, talk to it. The transcript is
// the host's; this shows it and sends what the user types.

import { h } from "./dom";
import { commands, type AgentInfo, type HostEvent } from "./host";
import { createChat } from "./renderers/chat";
import { toast } from "./toast";

interface Session {
  id: string;
  label: string;
}

export interface AgentPane {
  readonly element: HTMLElement;
  /** Loads the agent list. */
  refresh(): Promise<void>;
  onEvent(event: HostEvent): void;
}

function codicon(name: string): HTMLElement {
  return h("span", { class: `codicon codicon-${name}` });
}

export function createAgentPane(): AgentPane {
  const agentPicker = h("select", { class: "agent-picker", title: "Agent" });
  const sessionPicker = h("select", { class: "agent-sessions", title: "Session" });
  const newSession = h("button", { type: "button", class: "icon-button", title: "New session" }, codicon("add"));
  const closeSession = h("button", { type: "button", class: "icon-button", title: "Close session" }, codicon("close"));
  const header = h("div", { class: "agent-header" }, agentPicker, newSession, sessionPicker, closeSession);

  let current: string | null = null;
  const chat = createChat({
    onAnswer: (request, option) => {
      if (current) commands.agentAnswerPermission(current, request, option).catch((e) => toast("error", String(e)));
    },
  });

  const input = h("textarea", { class: "agent-input", rows: 3, placeholder: "Message the agent (Enter sends, Shift+Enter for a new line)" });
  const send = h("button", { type: "button", class: "primary", title: "Send" }, codicon("send"));
  const stop = h("button", { type: "button", title: "Stop" }, codicon("debug-stop"));
  stop.hidden = true;
  const footer = h("div", { class: "agent-footer" }, input, h("div", { class: "agent-actions" }, send, stop));
  const empty = h("div", { class: "agent-empty" }, "Pick an agent and start a session.");
  const element = h("div", { class: "agent-pane" }, header, empty, chat.element, footer);

  const sessions: Session[] = [];
  let agents: AgentInfo[] = [];
  let running = false;

  function render(): void {
    sessionPicker.replaceChildren(...sessions.map((s) => h("option", { value: s.id }, s.label)));
    if (current) sessionPicker.value = current;
    const has = current !== null;
    empty.hidden = has;
    chat.element.hidden = !has;
    sessionPicker.disabled = closeSession.disabled = !has;
    input.disabled = !has;
    send.hidden = running;
    stop.hidden = !running;
    newSession.disabled = agents.length === 0;
  }

  async function show(id: string | null): Promise<void> {
    current = id;
    render();
    if (id) chat.set(await commands.agentTranscript(id));
  }

  async function start(): Promise<void> {
    const agent = agents.find((a) => a.id === agentPicker.value);
    if (!agent) return;
    newSession.disabled = true;
    try {
      const id = await commands.agentNewSession(agent.id);
      sessions.push({ id, label: `${agent.name} ${sessions.filter((s) => s.label.startsWith(agent.name)).length + 1}` });
      await show(id);
      input.focus();
    } catch (error) {
      toast("error", String(error));
    } finally {
      render();
    }
  }

  async function prompt(): Promise<void> {
    const text = input.value.trim();
    if (!current || !text || running) return;
    input.value = "";
    running = true;
    render();
    try {
      await commands.agentPrompt(current, text);
    } catch (error) {
      toast("error", String(error));
    } finally {
      running = false;
      render();
    }
  }

  newSession.addEventListener("click", () => void start());
  closeSession.addEventListener("click", () => {
    if (current) void commands.agentCloseSession(current);
  });
  sessionPicker.addEventListener("change", () => void show(sessionPicker.value));
  send.addEventListener("click", () => void prompt());
  stop.addEventListener("click", () => {
    if (current) commands.agentCancel(current).catch((e) => toast("error", String(e)));
  });
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void prompt();
    }
  });

  render();
  return {
    element,
    async refresh() {
      agents = await commands.agentList();
      agentPicker.replaceChildren(...agents.map((a) => h("option", { value: a.id }, a.name)));
      render();
    },
    onEvent(event) {
      if (event.type === "agentUpdate" && event.session === current) chat.update(event.index, event.entry);
      if (event.type === "agentSessionEnded") {
        const index = sessions.findIndex((s) => s.id === event.session);
        if (index >= 0) sessions.splice(index, 1);
        if (event.session === current) {
          if (event.reason !== "closed") toast("warning", `The agent session ended: ${event.reason}`);
          void show(sessions.at(-1)?.id ?? null);
        } else render();
      }
    },
  };
}
