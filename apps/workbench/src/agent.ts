// The agent pane: pick an agent, start sessions, talk to it. The transcript is
// the host's; this shows it and sends what the user types.

import { h } from "./dom";
import { commands, type AgentInfo, type HostEvent } from "./host";
import { createChat } from "./renderers/chat";
import { toast } from "./toast";

interface Session {
  id: string;
  label: string;
  /** A turn is in progress. */
  running: boolean;
  /** The agent is gone; the transcript can still be read. */
  ended: boolean;
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
  let labels = 0;
  const find = (id: string | null) => sessions.find((s) => s.id === id);

  function render(): void {
    sessionPicker.replaceChildren(
      ...sessions.map((s) => h("option", { value: s.id }, s.ended ? `${s.label} (ended)` : s.label)),
    );
    if (current) sessionPicker.value = current;
    const session = find(current);
    empty.hidden = session !== undefined;
    chat.element.hidden = session === undefined;
    sessionPicker.disabled = closeSession.disabled = session === undefined;
    input.disabled = send.disabled = !session || session.ended;
    send.hidden = session?.running === true;
    stop.hidden = session?.running !== true;
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
      sessions.push({ id, label: `${agent.name} ${++labels}`, running: false, ended: false });
      await show(id);
      input.focus();
    } catch (error) {
      toast("error", String(error));
    } finally {
      render();
    }
  }

  async function prompt(): Promise<void> {
    const session = find(current);
    const text = input.value.trim();
    if (!session || !text || session.running || session.ended) return;
    input.value = "";
    session.running = true;
    render();
    try {
      await commands.agentPrompt(session.id, text);
    } catch (error) {
      toast("error", String(error));
    } finally {
      session.running = false;
      render();
    }
  }

  function forget(id: string): void {
    const index = sessions.findIndex((s) => s.id === id);
    if (index >= 0) sessions.splice(index, 1);
    if (id === current) void show(sessions.at(-1)?.id ?? null);
    else render();
  }

  newSession.addEventListener("click", () => void start());
  closeSession.addEventListener("click", () => {
    const id = current;
    if (id) commands.agentCloseSession(id).then(() => forget(id), (e) => toast("error", String(e)));
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
        const session = find(event.session);
        if (!session) return;
        if (event.reason === "closed") return forget(session.id);
        // The agent went away: keep the conversation, and what went wrong, on screen.
        session.ended = true;
        session.running = false;
        toast("warning", `${session.label} ended: ${event.reason}`);
        render();
      }
    },
  };
}
