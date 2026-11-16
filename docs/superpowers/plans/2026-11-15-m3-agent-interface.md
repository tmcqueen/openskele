# Milestone 3: Agent Interface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An agent pane on the secondary bar where the user talks to any ACP agent, which drives the workbench through the M2 tools.

**Architecture:** `agent/` in `skele-core` is an ACP client: it spawns agents declared in manifests, opens sessions whose `mcpServers` is the M2 bridge with a fresh grant, and keeps each session's transcript as data, emitting `HostEvent::AgentUpdate`. The webview renders transcripts with a generic chat renderer.

**Tech Stack:** Rust `agent-client-protocol` crate; TypeScript `@agentclientprotocol/sdk` (scripted agent); `markdown-it` + `DOMPurify` in the webview.

**Spec:** `docs/superpowers/specs/2026-11-15-app-bars-agents-design.md` (Milestone 3). ADRs: `docs/adr/0002-agents-over-acp.md`, `docs/adr/0003-private-mcp-server-via-bridge.md`, `docs/adr/0006-tool-approval-delegated-to-agent.md`.

**Depends on:** Milestones 1 and 2 merged (`Workbench::mcp_launch`, `GrantStore`, layout containers).

## Global Constraints

- An agent is any ACP executable declared in `contributes.agents`; no vendor code in core.
- `Manifest.main` becomes `Option<Entry>`. A manifest with commands, views or tools and no `main` (containers alone are fine) fails to install (`Error::Plugin { message: "contributes commands, views or tools but has no main" }`).
- `session/new` uses `cwd` = workspace folder (or the process cwd when none) and exactly one MCP server: name `"openskele"`, from `Workbench::mcp_launch(&grant)`, with a newly issued `GrantKind::AgentSession { session_id }` grant labelled `"<agent name> session <n>"`. The grant is revoked when the session ends or its agent process exits.
- Markdown: `markdown-it` with `html: false`, `linkify: true`; output passed through `DOMPurify.sanitize`. Links open nothing inside the webview.
- Built-in container `workbench.agent`, title `"Agent"`, icon `"sparkle"`, secondary bar, order `0`; shown even though it has no views (built-in content).
- The Claude agent manifest uses the ACP adapter package confirmed in Task 1; it relies on the user's existing `claude` login and is not run in automated tests.
- Each task ends with `cargo test --workspace`, clippy `-D warnings`, `bun run typecheck`.

## Review Focus

- The agent executable is missing or exits during `initialize`: `agent_new_session` returns an error message naming the agent; no grant is left issued (test in Task 3).
- The agent exits mid-prompt: the session's in-flight prompt ends with a transcript `Error` entry, pending permission requests are cancelled, the grant is revoked (test in Task 4).
- The user cancels while a permission request is pending: the request is answered `cancelled` and the prompt ends with stop reason `cancelled` (test in Task 4).
- Agent markdown containing `<script>`, `<img onerror>` or `javascript:` links renders as inert text/links (test in Task 6).
- A permission answer for an unknown or already-answered request id: `Err(UnknownPermissionRequest)` and the transcript is unchanged (test in Task 4).

---

### Task 1: Spike the ACP crate and adapters (throwaway)

- [ ] **Step 1:** In a scratch crate, use the latest `agent-client-protocol` crate as a **client** against a minimal agent written with `@agentclientprotocol/sdk` in Bun: `initialize`, `session/new` (with one stdio MCP server entry), `session/prompt`, receive `session/update`s, answer `session/request_permission`, serve `fs/read_text_file`.
- [ ] **Step 2:** Confirm the current npm package and command for Claude Code's ACP adapter (`@zed-industries/claude-code-acp` at last check) and that it accepts `mcpServers` of stdio type.
- [ ] **Step 3:** Record under a "Spike notes" heading at the top of this plan: crate version, the client trait and method names, update variant names (agent message chunk, thought, tool call, tool call update, plan), permission option kinds, the adapter's package and command. Later tasks use these names.
- [ ] **Step 4: Commit** — `git commit -m "docs: M3 spike notes"`

### Task 2: Manifest: agents, optional `main`

**Files:**
- Modify: `crates/skele-protocol/src/manifest.rs`, `crates/skele-core/src/plugin/mod.rs` (`start` uses `main.as_ref()`; install-time check)
- Regenerate: `protocol/*`, SDK generated files
- Test: `crates/skele-protocol/src/lib.rs` tests; `crates/skele-core/tests/agents.rs`

**Interfaces:**
- Produces: `AgentContribution { id: String, name: String, command: String, #[serde(default)] args: Vec<String>, #[serde(default)] env: BTreeMap<String, String> }`; `Contributes.agents: Vec<AgentContribution>`; `Manifest.main: Option<Entry>`. `command` resolves like plugin `main` (bare name on PATH, else relative to the manifest dir).

- [ ] **Step 1: Failing tests:** `manifest_parses_agents_without_main`; in `tests/agents.rs` `agent_only_plugin_installs`, `commands_without_main_fail_install`.
- [ ] **Step 2:** Run — FAIL. **Step 3:** Implement. **Step 4:** `cargo test --workspace && bun run gen:protocol` — PASS.
- [ ] **Step 5: Commit** — `git commit -m "protocol: agent contributions; main is optional"`

### Task 3: Scripted agent and the ACP client

**Files:**
- Create: `plugins/agent-scripted/skele-plugin.json`, `plugins/agent-scripted/package.json`, `plugins/agent-scripted/src/main.ts`
- Create: `crates/skele-core/src/agent/mod.rs` (registry, sessions), `agent/client.rs` (ACP connection and client callbacks), `agent/transcript.rs`
- Modify: `crates/skele-core/Cargo.toml`, `lib.rs`, `events.rs`, `error.rs`, `plugin/mod.rs` (register/unregister agents)
- Test: `crates/skele-core/tests/agents.rs`

**Interfaces:**
- Consumes: `Workbench::mcp_launch`, `grants()` (M2); Task 1 names.
- Produces:
  - Scripted agent (`id: "scripted"`, name `"Scripted"`, manifest-only plugin `sample.agent-scripted`, `command: "bun", args: ["run", "src/main.ts"]`). Fixed behavior by prompt text:
    - `"hello"` → streams `"Hello"`, `" from"`, `" the scripted agent."` as three message chunks, ends `end_turn`.
    - `"open settings"` → emits a tool call update, calls MCP tool `command_execute` `{command:"workbench.openSettings"}` through its configured MCP server, marks the tool call `completed`, replies `"Opened settings."`.
    - `"ask"` → sends `session/request_permission` with options `allow` / `reject`; replies `"Allowed."` or `"Rejected."`.
    - `"plan"` → emits a plan with entries `["Read", "Edit"]`, both `pending`, then `completed`.
    - `"read <path>"` → `fs/read_text_file` on the path, replies with its first line.
    - `"crash"` → exits the process with code 3.
  - `agent/transcript.rs`: `#[serde(tag = "type", rename_all = "camelCase")] pub enum Entry { UserMessage { text }, AgentText { text }, Thought { text }, ToolCall { id, title, kind: String, status: String, content: Vec<String> }, Plan { entries: Vec<PlanEntry { content, status }> }, PermissionRequest { id: String, title: String, options: Vec<PermissionOption { id, name, kind }>, answer: Option<String> }, Error { message } }`. `Transcript { entries: Vec<Entry> }` with `apply_text_chunk` (appends to the last `AgentText`/`Thought` or starts one), `upsert_tool_call(id, ..)`, `set_plan`. Specta + Serialize.
  - `agent/mod.rs`: `AgentInfo { id, name, source }`; `Workbench::agents(&self) -> Vec<AgentInfo>`; `Workbench::new_agent_session(&self, agent_id: &str) -> Result<String /* session id */>`; `Workbench::agent_prompt(&self, session: &str, text: String) -> Result<StopReason>` (resolves when the turn ends; `StopReason` = `EndTurn | Cancelled | Error | Other(String)`, Serialize); `agent_cancel(&self, session: &str) -> Result<()>`; `agent_answer_permission(&self, session: &str, request: &str, option: Option<String> /* None = cancelled */) -> Result<()>`; `agent_transcript(&self, session: &str) -> Result<Transcript>`; `close_agent_session(&self, session: &str)`.
  - `HostEvent::AgentUpdate { session: String, index: u32, entry: Entry }` (index = position in transcript; same index = replace) and `HostEvent::AgentSessionEnded { session: String, reason: String }`.
  - Errors: `UnknownAgent`, `UnknownAgentSession`, `UnknownPermissionRequest`, `Agent { agent, message }`.
  - ACP `fs/read_text_file` / `fs/write_text_file` served via `DocumentStore` (open if needed; write = `set_text` then `save_document`).

- [ ] **Step 1: Failing tests** in `tests/agents.rs` (skip without bun; `load_plugins(plugins/)`):
  - `hello_streams_text` — new session, prompt `"hello"` → `EndTurn`; transcript is `[UserMessage "hello", AgentText "Hello from the scripted agent."]`; at least three `AgentUpdate` events for index 1.
  - `tool_call_goes_through_bridge` — prompt `"open settings"` (set `SKELE_MCP_BRIDGE` to the cargo-built bridge: build it first with `cargo build -p skele-mcp-bridge` and use `target/debug/skele-mcp-bridge`) → `layout().active_panel == Some("settings")`; transcript has a `ToolCall` with status `completed`.
  - `plan_is_recorded`.
  - `write_goes_through_document_store` — scripted prompt `"write <path>"` (add to the agent: `fs/write_text_file` with `"written\n"`) → file on disk is `"written\n"` and `DocumentSaved` is emitted.
  - `read_sees_unsaved_buffer` — open a temp file in `DocumentStore`, `set_text("changed\n…")` without saving, prompt `"read <path>"` → reply `"changed"`.
  - `missing_agent_executable_errors_and_issues_no_grant` — install an agent manifest with `command: "/nonexistent"` → `new_agent_session` is `Err(Agent{..})`; the grant store holds no grants (add `GrantStore::len()` for tests).
- [ ] **Step 2:** Run `cargo build -p skele-mcp-bridge && cargo test -p skele-core --test agents` — FAIL.
- [ ] **Step 3:** Implement the agent, transcript and client; spawn as in `plugin/mod.rs` `start()` (stderr to an output terminal named after the agent, `kill_on_drop`).
- [ ] **Step 4:** Run — PASS.
- [ ] **Step 5: Commit** — `git commit -m "core: ACP client, transcripts; scripted agent"`

### Task 4: Permissions, cancel and failure paths

**Files:** `crates/skele-core/src/agent/*`, `crates/skele-core/tests/agents.rs`

- [ ] **Step 1: Failing tests:**
  - `ask_allow` — prompt `"ask"` in a task; wait for an `AgentUpdate` with a `PermissionRequest`; `agent_answer_permission(.., Some("allow"))` → prompt ends `EndTurn`, reply `"Allowed."`, request's `answer == Some("allow")`.
  - `ask_reject` — same with `"reject"` → `"Rejected."`.
  - `cancel_while_permission_pending` — `agent_cancel` → request `answer == Some("cancelled")`, stop reason `Cancelled`.
  - `answer_twice_is_error` / `answer_unknown_is_error` — `Err(UnknownPermissionRequest)`, transcript unchanged.
  - `agent_crash_mid_prompt` — prompt `"crash"` → `Err` or `StopReason::Error`; transcript ends with `Error`; `AgentSessionEnded` emitted; grant revoked (`GrantStore::len() == 0`).
  - `close_session_revokes_grant`.
- [ ] **Step 2:** Run — FAIL. **Step 3:** Implement. **Step 4:** Run — PASS.
- [ ] **Step 5: Commit** — `git commit -m "core: agent permissions, cancel and failure handling"`

### Task 5: Agent IPC and the built-in container

**Files:**
- Modify: `crates/skele-core/src/workbench.rs` (built-in `workbench.agent` container), `crates/tauri-plugin-skele/src/commands.rs`, `src/lib.rs`, `build.rs`, `permissions/default.toml`
- Regenerate: `apps/workbench/src/bindings.ts`, permissions

**Interfaces:**
- Produces IPC (TS names): `agentList()`, `agentNewSession(agentId)`, `agentPrompt(session, text)` → `StopReason`, `agentCancel(session)`, `agentAnswerPermission(session, request, option | null)`, `agentTranscript(session)`, `agentCloseSession(session)`. Streaming via the existing `subscribe` channel (`agentUpdate`, `agentSessionEnded`). On app exit, all sessions close.
- Containers: `ContainerInfo` gains `builtin_content: bool` (serialized `builtinContent`); `true` for `workbench.agent`, so the webview shows it without views.

- [ ] **Step 1:** Implement; add a core test `agent_container_is_on_secondary_bar`.
- [ ] **Step 2:** `cargo test --workspace && cargo build -p tauri-plugin-skele && bun run gen:bindings` — PASS; bindings contain the new commands and events.
- [ ] **Step 3: Commit** — `git commit -m "tauri: agent IPC; built-in agent container"`

### Task 6: Chat renderer and agent pane

**Files:**
- Create: `apps/workbench/src/renderers/chat.ts`, `apps/workbench/src/renderers/markdown.ts`, `apps/workbench/src/agent.ts`, `apps/workbench/src/renderers/markdown.test.ts`
- Modify: `apps/workbench/src/bars.ts` (render `builtinContent` containers: `workbench.agent` → `agent.ts`), `main.ts` (route `agentUpdate` / `agentSessionEnded`), `styles.css`, `package.json` (`markdown-it`, `dompurify`, `@types/markdown-it`; `"test": "bun test"`)

**Interfaces:**
- Produces:
  - `markdown.ts`: `renderMarkdown(source: string): DocumentFragment` (markdown-it `html: false, linkify: true` → `DOMPurify.sanitize(html, { RETURN_DOM_FRAGMENT: true })`; every `<a>` gets `rel="noopener noreferrer"` and a click handler that prevents navigation).
  - `chat.ts`: `createChat(options: { onAnswer(request: string, option: string | null): void }): Chat` with `Chat.element`, `Chat.set(transcript: Transcript)`, `Chat.update(index: number, entry: Entry)`. Renders: user bubble; agent text as markdown; thought collapsed; tool call as a collapsible row with codicon by status (`loading~spin` in progress, `check` completed, `error` failed); plan as a checklist; permission request as buttons per option until answered, then the chosen option's name.
  - `agent.ts`: `createAgentPane(): { element: HTMLElement; onEvent(event: HostEvent): void }` — agent picker (`agentList`), "New session" (codicon `add`), session picker, transcript (`createChat`), multiline input (Enter sends, Shift+Enter newline), Stop button visible while a prompt runs (calls `agentCancel`).
- [ ] **Step 1: Failing test** `markdown.test.ts` (`bun test`, with `happy-dom` registered as the DOM for DOMPurify): `renderMarkdown("<script>x()</script>")` contains no `script` element; `"![a](x onerror=alert(1))"` has no `onerror` attribute; `"[c](javascript:alert(1))"` has no `href` starting `javascript:`; `"**b**"` yields a `strong`.
- [ ] **Step 2:** `cd apps/workbench && bun test` — FAIL. **Step 3:** Implement `markdown.ts`. **Step 4:** `bun test` — PASS.
- [ ] **Step 5:** Implement `chat.ts`, `agent.ts`, wiring; `bun run typecheck` — clean.
- [ ] **Step 6: Commit** — `git commit -m "workbench: agent pane and chat renderer"`

### Task 7: Claude agent manifest, docs, end-to-end

**Files:**
- Create: `plugins/agent-claude/skele-plugin.json` (manifest only; `protocol: "0.2"`; one agent `id: "claude"`, `name: "Claude"`, command/args from Task 1 notes, e.g. `bunx` + the adapter package)
- Modify: `README.md`; `docs/adr/0002-agents-over-acp.md` (tick action items)

- [ ] **Step 1:** Write the manifest; `cargo test --workspace` still passes (installing it must not start anything).
- [ ] **Step 2:** README: "Agents" section (manifest `contributes.agents` example, how tools reach the agent, that approval is the agent's — ADR-0006), Layout table row for `agent/` and the two plugins.
- [ ] **Step 3: End-to-end on Xvfb** (commands as in M1 Task 10): open the Agent container on the secondary bar, pick "Scripted", new session, type `hello` ⏎ → streamed reply appears; `open settings` ⏎ → Settings panel opens and the tool call row shows completed; `ask` ⏎ → buttons appear, click Allow → `Allowed.`; `plan` ⏎ → checklist; Stop during `ask` → request shows cancelled. Screenshot each.
- [ ] **Step 4: Manual pass with Claude** (the user's login): pick "Claude", ask it to "open the settings panel using the workbench tools" → it calls `command_execute` (or `layout_set`) and the panel opens; its permission prompt appears inline.
- [ ] **Step 5: Full checks** — `cargo test --workspace && cargo clippy --workspace -- -D warnings && bun run typecheck && (cd apps/workbench && bun test) && dotnet build plugins/sample-dotnet -c Release -nologo && bun run gen:protocol && bun run gen:bindings && git diff --exit-code`.
- [ ] **Step 6: Commit** — `git commit -m "agents: Claude manifest; docs"`
