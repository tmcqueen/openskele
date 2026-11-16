# App bars, agent interface and agent-driven UI: design

Date: 2026-11-15. Source: `plan2.md` (previous session), confirmed and extended
in this one. Decisions are recorded as ADRs in `docs/adr/` and referenced
below as ADR-000N.

## Intent

openskele is a VS Code–style workbench skeleton: Rust owns state and
behavior, a Tauri webview renders it, and plugins in any language extend it
over JSON-RPC. The next step makes it a place where a person and an AI agent
work side by side:

1. **App bars.** A primary and a secondary bar, each an icon strip plus the
   side panel it opens, like VS Code's activity bar and secondary side bar.
2. **An agent pane** where the user talks to an agent directly, like the
   Copilot or Claude agent panes. Any agent works, not one vendor's.
3. **Agent-driven UI.** The agent can drive everything a user can (commands,
   settings, documents, terminals, layout), and end users can add to what it
   can do by writing plugins.

## Goals

- Bars: primary defaults left, secondary right; each can be docked left,
  right or top by a setting, applied live; each toggles with a keybinding.
- Plugins contribute view containers (icon + title) and put views in them.
- One tool registry that the agent sees through MCP. Built-in tools cover the
  workbench; plugins add tools in any language via the existing protocol.
- Agents are any ACP executable, added by a manifest entry. Two ship: a
  deterministic scripted agent for tests and Claude Code via its ACP adapter.
- The transcript (messages, tool calls, plans, permission requests) is data in
  Rust, rendered by a generic chat renderer.

## Non-goals (this round)

- MCP access for clients the workbench did not start (Claude Desktop, a
  terminal `claude`, Cursor). Deferred, with seams kept (ADR-0003).
- An HTTP MCP transport or any TCP listener.
- A host-side "confirm mutating tools" gate (ADR-0006). Needed before
  external MCP access ships.
- Persisting layout across restarts. Bar positions persist because they are
  settings; which container and panels were open does not.
- Packaging the bridge binary for `tauri build`. Development builds only.
- macOS and Windows verification.

## Anti-goals

- The agent seeing a different workbench than the user: every tool goes
  through the same `Workbench` methods the UI uses, and UI state the agent
  can change is the state the UI renders.
- Any process outside the ones the workbench started being able to drive it.
- Plugins or agents sending markup that the webview renders as HTML. Plugins
  return view models; agent markdown is rendered with raw HTML disabled and
  sanitized.
- Hard-wiring one agent vendor.

## Constraints

- Core stays headless (no Tauri in `skele-core`); everything is testable from
  `cargo test`.
- `skele-protocol` Rust types stay the source of truth; `gen:protocol` and
  `gen:bindings` must leave no diff.
- The protocol is versioned; adding methods or manifest fields bumps the minor
  version while major is 0 (`is_compatible` rejects a minor mismatch).
- Linux is the only platform tested here.

## Approaches

The previous session weighed these and the choices stand:

- **Where UI state lives.** Rust (chosen, ADR-0001) vs. the webview keeping
  its own layout and the agent asking it over IPC. The second leaves the
  agent depending on a live webview and on a second source of truth.
- **Agent connection.** ACP (chosen, ADR-0002) vs. a bespoke protocol vs.
  embedding one vendor's SDK. ACP is spoken by Claude Code (adapter), Gemini
  CLI and Codex, so agents become manifest entries.
- **How the agent gets tools.** A private MCP server handed to the agent in
  `session/new` (chosen, ADR-0003) vs. a public MCP endpoint vs. ACP alone.
  ACP has no tool channel; clients pass MCP servers. Public access is
  deferred.
- **Plugin tools.** A new `tool/call` protocol method (chosen, ADR-0004) vs.
  plugins running their own MCP servers. One registry keeps one approval and
  naming story and reuses the plugin lifecycle.
- **Bars in the webview.** Dockview edge groups (chosen, ADR-0005) vs.
  hand-built flex regions around `#dock`, which remain the fallback.

## Design

### Milestone 1: app bars and Rust-owned layout

**View containers** (`crates/skele-core/src/containers.rs`), shaped like
`views.rs`: `ContainerInfo { id, title, icon, bar: Primary | Secondary, order, source }`.
Manifest gains `contributes.viewContainers`. `ViewContribution.location`
(`sidebar | panel`) is replaced by `container: Option<String>`; absent means
the built-in `workbench.views` container on the primary bar. Unknown
container ids are an install error. Registration and removal go in
`plugin/mod.rs` `register_contributions` / `unregister_contributions`.

**Layout** (`crates/skele-core/src/layout.rs`):

```text
Layout {
  primary:   Bar { position: Left|Right|Top, visible, active_container: Option<id> }
  secondary: Bar { ... }
  panels:    Vec<Panel>        // in open order
  active_panel: Option<panel id>
}
Panel = Settings | Editor { path } | Terminal { id: u32 }
panel id = "settings" | "editor:<path>" | "terminal:<n>"
```

Every mutation emits `HostEvent::LayoutChanged { layout }` (the whole layout;
it is small). `HostEvent::OpenPanel` is removed. Built-ins that opened panels
now mutate the layout: `workbench.openSettings`, `workbench.openFile`,
`workbench.newTerminal`. The command palette is an overlay, not a panel:
`workbench.showCommands` emits `HostEvent::ShowCommandPalette`.

*My call:* the Output panel stays fixed at the bottom and outside the layout
model; it is a log, not a document.

**Terminals become core-owned** (*my call*, needed so a terminal panel in the
Rust layout means something without a webview, and so M2's `terminal_open`
tool can reuse it). `workbench.newTerminal` opens the PTY in
`TerminalManager` at 80×24 with a scrollback ring buffer (64 KiB per
terminal) and adds a `Terminal { id }` panel. The webview attaches with a new
`terminal_attach(id, channel)` IPC, receives the scrollback first, then live
data, and resizes. `terminal_open` IPC goes away. Closing the panel closes
the PTY; a page reload re-attaches instead of killing shells.

**Settings** in the built-in section: `workbench.primaryBar.position`
(default `left`), `workbench.secondaryBar.position` (default `right`), enum
`left | right | top`. On `ConfigChanged` the layout updates the bar and emits
`LayoutChanged`.

**Commands:** `workbench.togglePrimaryBar` (`Mod+B`),
`workbench.toggleSecondaryBar` (`Mod+Alt+B`), and one
`workbench.focusContainer.<id>` per registered container, registered and
removed with the container.

**Snapshot** gains `containers` and `layout`.

**IPC:** `layout_set_active_container(bar, id | null)`,
`layout_set_active_panel(id)`, `layout_close_panel(id)`, `terminal_attach`.
Add to `build.rs`, `permissions/default.toml`, `collect_commands!`;
regenerate bindings.

**Webview:** one `reconcile(layout, snapshot)` in `main.ts` replaces
`syncViews()` and `openPanel()`; it runs on `layoutChanged` and
`contributionsChanged` and makes Dockview match. User gestures (icon click,
tab close, tab focus) call the IPC and wait for `layoutChanged` instead of
changing Dockview directly. Bars are Dockview edge groups with a custom
icon-only tab renderer; a spike confirms this first (ADR-0005). Icons are
`@vscode/codicons` names; the glyph map in `renderers/tree.ts` goes. Both
sample plugins contribute a container and move their view into it.

### Milestone 2: tools and the private MCP server

**Tool registry** (`crates/skele-core/src/tools.rs`):
`ToolInfo { name, title, description, input_schema, read_only, destructive, source }`
with `ToolTarget::Builtin(handler) | Plugin(id)`, mirroring `CommandTarget`.
Names match `^[a-z][a-z0-9_]*$` (MCP-safe); duplicates are rejected like
duplicate commands. Handlers take `(ToolCall { arguments, grant })` and return
`ToolResult { content: Vec<Content>, is_error }`. Arguments are validated
against `input_schema` with the `jsonschema` crate already in use.

**Built-in tools:** `workbench_snapshot`, `command_execute`,
`view_get_children`, `config_list`, `config_get`, `config_set`,
`document_open`, `document_read`, `document_edit`, `document_save`,
`terminal_open`, `terminal_write`, `terminal_read` (reads the M1 scrollback),
`layout_get`, `layout_set`, `message_show`. Each is a thin wrapper over the
existing `Workbench` method.

**Plugin tools:** `contributes.tools: [{ name, title?, description,
inputSchema, readOnly? }]`. New host→plugin request `tool/call { name,
arguments } → { content, isError }` in `methods.rs` and `openrpc.rs`.
Calling a plugin tool activates the plugin like a command does. SDKs:
`gen:protocol` regenerates types; hand-written `host.tools.register(name,
handler)` in `sdk/typescript/src/index.ts` and `Tools.Register` in
`sdk/dotnet/Skele.Sdk/PluginRuntime.cs`. Each sample contributes one tool.
`PROTOCOL_VERSION` becomes `0.2.0`; sample manifests say `"protocol": "0.2"`.

**Grants** (the seam for later external access, ADR-0003):

```text
Grant { id, label, token (32 random bytes, hex), kind: AgentSession { session_id } | External,
        confirm_mutations: bool, expires: Option<Instant> }
GrantStore: issue(kind, label) -> Grant, check(token) -> Option<Grant>, revoke(id)
```

Only `AgentSession` grants are issued in this round. Tools receive the grant,
never an ACP session; anything a tool needs about its caller comes from it.
`confirm_mutations` exists and is `false` for agent grants; nothing reads it
yet (ADR-0006).

**MCP service** (`crates/skele-core/src/mcp/`): `service.rs` implements the
`rmcp` server handler over the tool registry (`tools/list`, `tools/call`,
read-only / destructive annotations) and takes any `AsyncRead + AsyncWrite`
plus a `Grant`. It knows nothing about sockets. `socket.rs` listens on a
local socket (`interprocess`: Unix socket in the runtime dir, mode 0600;
named pipe on Windows), reads a one-line token handshake, looks up the grant,
and hands the stream to the service; a bad token closes the connection.

**Bridge** (`crates/skele-mcp-bridge`): a small binary that reads
`SKELE_MCP_SOCKET` and `SKELE_MCP_TOKEN` from its environment, connects,
sends the token, then pipes stdin/stdout. It is what the agent launches as
its stdio MCP server. Found next to the current executable, overridable with
`SKELE_MCP_BRIDGE` (tests use the cargo-built path).

### Milestone 3: agent interface

**Manifest:** `contributes.agents: [{ id, name, command, args, env }]`;
`main` becomes optional so a plugin can contribute only agents.
Contributions with commands, views or tools but no `main` are an install
error.

**ACP client** (`crates/skele-core/src/agent/`) on the
`agent-client-protocol` crate: spawn as in `plugin/mod.rs` `start()`;
`initialize`; `session/new` with `cwd` = workspace and `mcpServers` = one
stdio entry for the bridge with a freshly issued `AgentSession` grant in its
env; `session/prompt`; `session/cancel`. The grant is revoked when the
session ends. *My call:* several sessions may exist in core at once; the pane
shows one, with a picker.

**Transcript** owned in Rust per session: `UserMessage`, `AgentText
(markdown, streamed)`, `Thought`, `ToolCall { id, title, kind, status,
content }`, `Plan { entries }`, `PermissionRequest { id, options, answer }`.
`session/update` appends or updates entries and emits
`HostEvent::AgentUpdate { session, entry }`;
`session/request_permission` becomes a pending `PermissionRequest` that the
UI answers through IPC.

**ACP file methods** `fs/read_text_file` and `fs/write_text_file` go through
`DocumentStore`, so the agent sees unsaved buffers and its writes show in
open editors.

**IPC:** `agent_list`, `agent_new_session`, `agent_prompt`, `agent_cancel`,
`agent_answer_permission`, `agent_transcript`. Streaming uses the existing
`subscribe` channel.

**Webview:** `renderers/chat.ts`, a generic renderer for the transcript
(messages, collapsible tool calls, plan checklist, inline permission
buttons); markdown via `markdown-it` with `html: false`, then `DOMPurify`.
`agent.ts` is the pane (agent picker, transcript, input, stop, new session),
registered as the built-in `workbench.agent` container on the secondary bar.

**Agents shipped:** `plugins/agent-scripted`, a deterministic ACP agent built
on `@agentclientprotocol/sdk` that, on fixed prompts, streams text, calls a
workbench tool through the bridge and asks permission; and
`plugins/agent-claude`, a manifest-only plugin for Claude Code's ACP adapter
using the user's existing `claude` login.

## Verification

- **M1:** unit tests for `containers.rs`, `layout.rs`, terminal scrollback;
  on the private Xvfb display with `xdotool`: both bars render, icons switch
  containers, `Ctrl+B` toggles, each position setting moves its bar live,
  terminals survive a page reload.
- **M2:** unit tests for the tool registry, schema validation and grants; an
  integration test that starts the socket, launches `skele-mcp-bridge`,
  connects an `rmcp` client and checks `tools/list`, `command_execute` →
  `hello.greet`, a Bun plugin tool, a .NET plugin tool, and that a wrong
  token is refused.
- **M3:** integration test driving `agent-scripted` end to end (prompt →
  streamed text → tool call through the bridge → permission → transcript);
  on Xvfb, a prompt in the pane makes the scripted agent open Settings via a
  tool call. One manual pass with Claude.
- Throughout: `cargo test --workspace`, clippy, `bun run typecheck`,
  `dotnet build`, no diff from `gen:protocol` / `gen:bindings`.

## Left to the builder

- Exact Dockview reconcile algorithm and placement of new panels.
- Codicon choices for built-in containers.
- Scrollback size tuning, socket file naming, error message wording.
- Whether `layout_set` takes a full layout or a patch (prefer a patch of
  bars and active panel; panels are opened through their own tools).

## Risks

- `rmcp`, `agent-client-protocol` and Dockview edge groups are newer than
  anything used here: each milestone starts with a spike against the real
  API, and the plan adjusts to what it finds.
- Claude ACP adapter package name must be checked when M3 starts.
- Tool approval depends on the agent asking (ADR-0006).
