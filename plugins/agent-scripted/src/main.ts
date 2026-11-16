// A deterministic ACP agent for testing the workbench's agent pane without an
// API key. It answers a few fixed prompts:
//
//   hello          streams three message chunks
//   open settings  calls the workbench's command_execute tool through its MCP server
//   ask            asks permission, then says what was chosen
//   plan           reports a two-step plan, then completes it
//   read <path>    reads a file through the client (fs/read_text_file), replies with its first line
//   write <path>   writes "written\n" through the client (fs/write_text_file)
//   crash          exits with code 3
//
// Anything else is echoed back.

import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { Readable, Writable } from "node:stream";

import { agent, methods, ndJsonStream, PROTOCOL_VERSION, type AgentContext, type McpServer } from "@agentclientprotocol/sdk";

/** What session/new gave us: where the workbench's tools are. */
const sessions = new Map<string, { mcpServers: McpServer[]; cancelled: boolean }>();
let nextSession = 1;

/** Calls one MCP tool over a stdio server, with a hand-rolled minimal client. */
async function callTool(server: McpServer, name: string, args: object): Promise<string> {
  if (!("command" in server)) throw new Error("only stdio MCP servers are supported");
  const env = Object.fromEntries(server.env.map((v) => [v.name, v.value]));
  const child = spawn(server.command, server.args, { env: { ...process.env, ...env }, stdio: ["pipe", "pipe", "inherit"] });
  const lines = createInterface({ input: child.stdout });
  const waiting = new Map<number, (message: any) => void>();
  lines.on("line", (line) => {
    const message = JSON.parse(line);
    waiting.get(message.id)?.(message);
  });
  let id = 0;
  const request = (method: string, params: object) =>
    new Promise<any>((resolve, reject) => {
      const mine = ++id;
      waiting.set(mine, (m) => (m.error ? reject(new Error(m.error.message)) : resolve(m.result)));
      child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: mine, method, params }) + "\n");
    });
  try {
    await request("initialize", {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "agent-scripted", version: "0.1.0" },
    });
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" }) + "\n");
    const result = await request("tools/call", { name, arguments: args });
    const text = result.content.map((c: { text?: string }) => c.text ?? "").join("");
    if (result.isError) throw new Error(text);
    return text;
  } finally {
    child.kill();
  }
}

async function say(client: AgentContext, sessionId: string, text: string): Promise<void> {
  await client.notify(methods.client.session.update, {
    sessionId,
    update: { sessionUpdate: "agent_message_chunk", content: { type: "text", text } },
  });
}

const app = agent({ name: "agent-scripted" })
  .onRequest(methods.agent.initialize, () => ({
    protocolVersion: PROTOCOL_VERSION,
    agentCapabilities: {},
    authMethods: [],
  }))
  .onRequest(methods.agent.session.new, ({ params }) => {
    const sessionId = `scripted-${nextSession++}`;
    sessions.set(sessionId, { mcpServers: params.mcpServers, cancelled: false });
    return { sessionId };
  })
  .onNotification(methods.agent.session.cancel, ({ params }) => {
    const session = sessions.get(params.sessionId);
    if (session) session.cancelled = true;
  })
  .onRequest(methods.agent.session.prompt, async ({ params, client }) => {
    const { sessionId } = params;
    const session = sessions.get(sessionId)!;
    session.cancelled = false;
    const prompt = params.prompt
      .map((block) => (block.type === "text" ? block.text : ""))
      .join("")
      .trim();
    const [verb, ...rest] = prompt.split(" ");
    const arg = rest.join(" ");

    switch (prompt === "open settings" ? prompt : verb) {
      case "hello":
        for (const chunk of ["Hello", " from", " the scripted agent."]) await say(client, sessionId, chunk);
        break;
      case "open settings": {
        const toolCallId = "open-settings";
        await client.notify(methods.client.session.update, {
          sessionId,
          update: { sessionUpdate: "tool_call", toolCallId, title: "Open settings", kind: "other", status: "in_progress" },
        });
        const server = session.mcpServers[0];
        if (!server) throw new Error("no MCP server was given to this session");
        const result = await callTool(server, "command_execute", { command: "workbench.openSettings" });
        await client.notify(methods.client.session.update, {
          sessionId,
          update: {
            sessionUpdate: "tool_call_update",
            toolCallId,
            status: "completed",
            content: [{ type: "content", content: { type: "text", text: result } }],
          },
        });
        await say(client, sessionId, "Opened settings.");
        break;
      }
      case "ask": {
        const answer = await client.request(methods.client.session.requestPermission, {
          sessionId,
          toolCall: { toolCallId: "ask", title: "Do the thing" },
          options: [
            { optionId: "allow", name: "Allow", kind: "allow_once" },
            { optionId: "reject", name: "Reject", kind: "reject_once" },
          ],
        });
        if (answer.outcome.outcome === "cancelled" || session.cancelled) return { stopReason: "cancelled" };
        await say(client, sessionId, answer.outcome.optionId === "allow" ? "Allowed." : "Rejected.");
        break;
      }
      case "plan": {
        const entries = (status: "pending" | "completed") =>
          ["Read", "Edit"].map((content) => ({ content, priority: "medium" as const, status }));
        await client.notify(methods.client.session.update, { sessionId, update: { sessionUpdate: "plan", entries: entries("pending") } });
        await client.notify(methods.client.session.update, { sessionId, update: { sessionUpdate: "plan", entries: entries("completed") } });
        await say(client, sessionId, "Planned.");
        break;
      }
      case "read": {
        const { content } = await client.request(methods.client.fs.readTextFile, { sessionId, path: arg });
        await say(client, sessionId, content.split("\n")[0]);
        break;
      }
      case "write":
        await client.request(methods.client.fs.writeTextFile, { sessionId, path: arg, content: "written\n" });
        await say(client, sessionId, "Wrote.");
        break;
      case "crash":
        process.exit(3);
      default:
        await say(client, sessionId, `You said: ${prompt}`);
    }
    return { stopReason: "end_turn" };
  });

const stream = ndJsonStream(
  Writable.toWeb(process.stdout) as WritableStream<Uint8Array>,
  Readable.toWeb(process.stdin) as unknown as ReadableStream<Uint8Array>,
);
app.connect(stream);
