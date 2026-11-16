# Post-M3 Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the five peer-review bugs (#3–#7) so the webview, IPC and native menu change layout and report errors only through the Rust core, and fix the nine CodeRabbit findings on PR #1 (M2's tools and MCP server).

**Architecture:** Tasks 1–5 each move a decision the webview or an IPC command makes on its own back into `skele-core`: panel closes wait for the core (#3), views load when the core's layout shows them (#4), initial panels are set by the host once (#5), terminals close only through the layout (#6), and failed commands are reported through `Workbench::notify` (#7). Tasks 6–9 fix the MCP server's connection lifecycle, keep open editors in step with agent edits, reject tool schemas MCP cannot call, and make the bridge and tests portable to Windows. Task 10 accepts integral numbers such as `1.0` wherever a tool schema says `integer`.

**Tech Stack:** Rust (`skele-core`, `tauri-plugin-skele`), TypeScript webview on Dockview 8.9, `bun test` with happy-dom.

**Spec:** GitHub issues [#3](https://github.com/tmcqueen/openskele/issues/3), [#4](https://github.com/tmcqueen/openskele/issues/4), [#5](https://github.com/tmcqueen/openskele/issues/5), [#6](https://github.com/tmcqueen/openskele/issues/6), [#7](https://github.com/tmcqueen/openskele/issues/7) (each has acceptance criteria), plus `docs/adr/0001-ui-state-owned-by-rust.md`, `0007-plugins-are-sidecar-processes.md` and `0012-output-channels-in-the-core.md`. All five were confirmed still present at `origin/m3-agent-interface` (`4643df9`). CodeRabbit's review of [PR #1](https://github.com/tmcqueen/openskele/pull/1) at `f6cb166`: nine inline comments, all verified as valid; M3 changes none of the flagged files except `workbench.rs` (shutdown now revokes agent grants first, which softens finding 3).

**Base:** `main` after `m3-agent-interface` (which contains `m2-tools-mcp`) is merged. Line numbers below are at `4643df9`.

## Global Constraints

- ADR-0001: the Rust layout is the only source of truth. The webview never adds, removes or activates a main-area panel except inside `reconcile` (`apps/workbench/src/layout.ts`).
- ADR-0007: a plugin starts only when one of its commands runs, one of its views is shown, or it declares `onStartup`.
- ADR-0012: every user-facing notification goes through `Workbench::notify`, which emits one `HostEvent::Message` and appends one line `[<level>] <message>\r\n` (or `[<level>] <source>: <message>\r\n`) to the `Workbench` channel.
- Agent tools keep calling `Workbench::execute_command` directly: a failed `command_execute` tool call returns its error to the agent and does not notify the user.
- No Dockview tab context menu: do not set `getTabContextMenuItems` (absent means no menu in 8.9).
- Each task ends green on: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `bun run typecheck`, `bun run --cwd apps/workbench test`, and `bun run gen:bindings && git diff --exit-code apps/workbench/src/bindings.ts` after the task's own binding change is committed.
- Each task's commit message ends with `(closes #N)`; Tasks 6–9 instead name the CodeRabbit findings they fix (`CR1`–`CR9`, numbered as in each task).
- MCP requires a tool's `inputSchema` to be a JSON Schema whose `type` is `"object"`.
- The bridge (`crates/skele-mcp-bridge`) keeps no dependency on `skele-core` outside `[dev-dependencies]`.

## Review Focus

- A close request for a panel the core no longer has (closed meanwhile by an agent tool): the IPC call fails, a toast says so, and the next `layoutChanged` removes the tab; the webview must not keep a tab the core does not list (test in Task 5).
- A `viewRefresh` arriving for a view whose container has never been shown: nothing is fetched and the plugin is not asked for anything; the view loads fresh when first shown (test in Task 4).
- `initial_panels` listing the same panel twice, or not listing Output: one panel each, the last one active; Output is simply not open (test in Task 3).
- A command that fails because the IPC call itself is rejected (permission or serialization error, never reaching the core): no toast, but `console.error` records it; nothing is lost silently (test in Task 2).
- Hiding a bar or collapsing its container after views were shown: already-loaded trees keep their content and still refresh on `viewRefresh`; nothing is fetched for containers that were never shown (test in Task 4).

---

### Task 1: Remove the `terminal_close` IPC command (#6)

**Files:**
- Modify: `crates/tauri-plugin-skele/src/commands.rs:158-162` (delete `terminal_close`)
- Modify: `crates/tauri-plugin-skele/build.rs` (`COMMANDS`), `crates/tauri-plugin-skele/permissions/default.toml:13`, `crates/tauri-plugin-skele/src/lib.rs:41`
- Regenerate: `apps/workbench/src/bindings.ts`, `crates/tauri-plugin-skele/permissions/autogenerated/` (the build removes `commands/terminal_close.toml`; delete it if it remains)
- Test: `crates/skele-core/tests/layout.rs` (`new_terminal_adds_panel_and_close_kills_it`, line 225), new `apps/workbench/src/bindings.test.ts`

**Interfaces:**
- Produces: `layout_close_panel("terminal:<id>")` (→ `Workbench::close_panel`) is the only IPC path that ends a terminal.

- [ ] **Step 1: Failing tests**
  - Extend `new_terminal_adds_panel_and_close_kills_it`: after `close_panel`, assert `!workbench.snapshot().layout.panels.contains(&Panel::Terminal { id })` and `workbench.terminals().read(id)` is `Err(Error::UnknownTerminal(_))`. (Passes today; it pins the remaining path.)
  - `bindings.test.ts`, test `"no IPC command closes a terminal outside the layout"`: `expect("terminalClose" in commands).toBe(false)` with `commands` imported from `./bindings`.
- [ ] **Step 2:** `bun run --cwd apps/workbench test bindings` — expected FAIL (`terminalClose` exists).
- [ ] **Step 3:** Delete the command from the four places; `bun run gen:bindings`.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "tauri: drop terminal_close; terminals close through the layout (closes #6)"`

### Task 2: Report failed UI commands through `Workbench::notify` (#7)

**Files:**
- Modify: `crates/skele-core/src/workbench.rs` (beside `execute_command`, line 169)
- Modify: `crates/tauri-plugin-skele/src/commands.rs:80-82`, `crates/tauri-plugin-skele/src/menu.rs:20-23`
- Modify: `apps/workbench/src/host.ts:27-33`
- Test: `crates/skele-core/tests/output.rs` (top level and `mod log_files`), new `apps/workbench/src/host.test.ts`

**Interfaces:**
- Produces: `pub async fn execute_command_reported(&self, id: &str, args: Vec<Value>) -> Result<Value>` on `Workbench`: calls `execute_command`; on `Err(e)` calls `self.notify(MessageLevel::Error, &e.to_string(), None)` once, then returns `Err(e)`.
- Produces: webview `run(command, args)` no longer toasts; on rejection it calls `console.error("command failed", command, error)`.

- [ ] **Step 1: Failing tests**
  - `output.rs` `reported_command_failure_is_notified_once`: subscribe, `execute_command_reported("no.such", vec![])` is `Err`; exactly one `HostEvent::Message { level: Error, .. }` is received (then `try_recv` is empty of messages); `output_read(WORKBENCH_CHANNEL)` equals `"[error] unknown command: no.such\r\n"`.
  - `output.rs` `reported_command_success_is_silent`: `execute_command_reported("workbench.openSettings", vec![])` is `Ok`; no `Message` event; the Workbench channel is empty.
  - `log_files::reported_failure_reaches_log_file`: `with(None, Some(tmp))`, failing call as above, `file(&run_dir(&w), WORKBENCH_CHANNEL)` contains `"unknown command: no.such"`.
  - `host.test.ts` (mock `./bindings` so `commands.executeCommand` rejects with `"boom"`, mock `./toast` with a spy): `"a failed command is not toasted twice"` — `await run("x")` resolves, the toast spy was not called, a `console.error` spy was called once.
- [ ] **Step 2:** `cargo test -p skele-core --test output reported` and `bun run --cwd apps/workbench test host` — expected FAIL.
- [ ] **Step 3:** Implement `execute_command_reported`; the `execute_command` IPC calls it; `menu.rs` calls it and drops `bus().message` (and the now-unused `MessageLevel` import); `host.ts` `run()` drops the toast.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "core: failed UI commands go through notify (closes #7)"`

### Task 3: Initial panels set by the host once (#5)

**Files:**
- Modify: `crates/skele-core/src/workbench.rs:31-39` (`WorkbenchOptions`) and `Workbench::new`
- Modify: `crates/tauri-plugin-skele/src/lib.rs:112-116`
- Modify: `apps/workbench/src/main.ts:234-235` (delete the comment and the `openSettings` heuristic; keep the Output `setSize` line)
- Test: `crates/skele-core/tests/layout.rs`

**Interfaces:**
- Produces: `WorkbenchOptions.initial_panels: Vec<Panel>` (`#[derive(Default)]` gives empty). Empty keeps the default layout (`[Output]`, no active panel). Non-empty: the layout starts with no panels and `Layout::open` is applied to each in order, so duplicates collapse and the last one is active. No `LayoutChanged` is emitted for it.
- Produces: the Tauri plugin passes `initial_panels: vec![Panel::Output, Panel::Settings]`.

- [ ] **Step 1: Failing tests** in `tests/layout.rs`:
  - `initial_panels_replace_the_default`: options with `[Output, Settings]` → `layout().panels == [Output, Settings]`, `active_panel == Some("settings")`.
  - `initial_panels_dedupe_and_may_omit_output`: `[Settings, Settings]` → `panels == [Settings]`, active `"settings"`.
  - `closing_initial_settings_sticks`: from `[Output, Settings]`, `close_panel("settings")`, then `snapshot().layout.panels == [Output]` (this snapshot is what a reloaded webview reads).
- [ ] **Step 2:** `cargo test -p skele-core --test layout initial` — expected FAIL (no field).
- [ ] **Step 3:** Implement; update the plugin setup; delete the webview heuristic.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "core: initial panels come from the host, not the webview (closes #5)"`

### Task 4: Load views only when their container is shown (#4)

**Files:**
- Create: `apps/workbench/src/containers.ts`
- Modify: `apps/workbench/src/main.ts` (replace `createContainer`, lines 42-60, and the `trees` map, line 30; `viewRefresh` handler, lines 209-210; `render`, lines 121-128)
- Test: new `apps/workbench/src/containers.test.ts`

**Interfaces:**
- Consumes: `createTree(options: TreeOptions): Tree` from `renderers/tree.ts` (unchanged); `Layout`, `ViewInfo`, `TreeItem` from `./host`.
- Produces, in `containers.ts`:
  - `export interface ContainerViews { readonly element: HTMLElement; show(): void; refresh(viewId: string): void; dispose(): void }`
  - `export function createContainerViews(views: ViewInfo[], deps: { load(viewId: string, parent: string | null): Promise<TreeItem[]>; activate(item: TreeItem): void }): ContainerViews` — builds one titled section and tree per view without fetching. The first `show()` refreshes every tree; later calls do nothing. `refresh(viewId)` refreshes that tree only if `show()` has run.
  - `export function shownContainers(layout: Layout): string[]` — for `primary` then `secondary`, the `activeContainer` of each bar with `visible === true` and a non-null `activeContainer`.
- Produces, in `main.ts`: `const containerViews = new Map<string, ContainerViews>()` keyed by container id. The `container` component registers on `init` and calls `show()` at once if `shownContainers(layout)` includes it; `dispose` unregisters. `render()` calls `show()` for each id in `shownContainers(layout)` after `bars.render`. `viewRefresh` calls `refresh(event.viewId)` on every entry. The `workbench.agent` container is unchanged.

- [ ] **Step 1: Failing tests** in `containers.test.ts` (no host mock needed; `deps.load` is a spy returning `[]`):
  - `"building a container fetches nothing"`: two views → `load` not called.
  - `"show fetches each view once"`: `show(); show();` → `load` called exactly with `("v1", null)` and `("v2", null)`.
  - `"refresh before show fetches nothing"`: `refresh("v1")` → no call; after `show()`, `refresh("v1")` → one more `("v1", null)` call.
  - `"shown containers ignore hidden and collapsed bars"`: primary `{visible: true, activeContainer: "a"}`, secondary `{visible: false, activeContainer: "b"}` → `["a"]`; primary `activeContainer: null` → `[]`.
- [ ] **Step 2:** `bun run --cwd apps/workbench test containers` — expected FAIL (module missing).
- [ ] **Step 3:** Implement `containers.ts`; rewire `main.ts`.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "workbench: views load when their container is shown (closes #4)"`

### Task 5: Closing a tab asks the host first (#3)

**Files:**
- Modify: `apps/workbench/src/layout.ts` (`options()` sets `tabComponent: "main"` on every main-area panel; `createMainArea` drops the `onDidRemovePanel` handler at lines 104-107 and gains `createTab`)
- Modify: `apps/workbench/src/main.ts:174` (`createTabComponent`: `"icon"` → `bars.createIconTab()`, `"main"` → `main.createTab()`)
- Modify: `apps/workbench/src/styles.css` (tab title and close button; keep the existing outlined-tab look)
- Test: `apps/workbench/src/layout.test.ts`

**Interfaces:**
- Produces: `MainArea.createTab(): ITabRenderer`. The element holds a `.main-tab-title` span and a `button.main-tab-close` (`codicon codicon-close`, `aria-label="Close"`). `init({ api, title })` keeps `api.id`. Clicking the button, or a middle click (`auxclick`, `button === 1`) on the tab, calls `commands.layoutClosePanel(id)` with the event's propagation stopped; a rejection is shown with `toast("error", …)`. It never calls `api.close()`. Panels leave Dockview only through `reconcile`.

- [ ] **Step 1: Failing tests** in `layout.test.ts` (extend the `./host` mock so `layoutClosePanel` can be made to reject; render tabs through `createTabComponent: ({ name }) => name === "main" ? main.createTab() : undefined` in `setup()`):
  - Replace `"closing a tab asks the host to close it and nothing else"` with `"the close button asks the host and keeps the tab until the host's layout drops it"`: click `.main-tab-close` on `editor:/e1` → `calls` equals `["close editor:/e1"]` and `dock.getPanel("editor:/e1")` still exists; `reconcile` without `/e1` → the panel is gone and `calls` is unchanged.
  - `"a rejected close leaves the tab and says why"`: `layoutClosePanel` rejects → the panel still exists and the toast spy received `"error"`.
  - `"middle click closes through the host"`: dispatch `auxclick` with `button: 1` on the tab element → one `close` call.
- [ ] **Step 2:** `bun run --cwd apps/workbench test layout` — expected FAIL.
- [ ] **Step 3:** Implement `createTab`, wire it, remove the `onDidRemovePanel` handler, add styles.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "workbench: tab closes wait for the host's layout (closes #3)"`

### Task 6: MCP connection lifecycle (CR1, CR2, CR3, CR6, CR9)

Findings: [CR1](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542589) watcher tasks outlive their connection; [CR2](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542610) revocation is not seen during initialization; [CR3](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542631) shutdown leaves connection handlers running; [CR6](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542689) a closed listener stays cached; [CR9](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542729) the revocation test passes without the watcher.

**Files:**
- Modify: `crates/skele-core/src/mcp/service.rs:90-128` (`serve`), `crates/skele-core/src/mcp/socket.rs` (`McpListener`, `handle`), `crates/skele-core/src/mcp/mod.rs:44-47` (`Workbench::mcp`), `crates/skele-core/src/workbench.rs` (`Inner.mcp`, `shutdown`)
- Test: `crates/skele-core/src/mcp/service.rs` tests, `crates/skele-mcp-bridge/tests/bridge.rs` (`revoked_grant_closes_connection`, line ~96)

**Interfaces:**
- Produces: `serve(workbench, grant, reader, writer, stop: CancellationToken) -> Result<()>` (`tokio_util::sync::CancellationToken`, already a dependency via `tokio-util`). One token per connection, created before `serve_with_ct`, is cancelled by the revocation watcher, by `stop`, and when the connection ends; the watcher is a single task that `select!`s on revocations and the token, so it ends with the connection. Initialization runs under the token, so revoking during it ends the connection.
- Produces: `McpListener` owns a listener-wide `CancellationToken` (each handler gets a child) and a `tokio::task::JoinSet` of handlers. `pub async fn close(&self)` cancels the token, aborts accepting, waits for every handler to finish, then removes the socket and folder. `Drop` still cancels and removes files without waiting.
- Produces: `Inner.mcp: tokio::sync::Mutex<Option<Arc<McpListener>>>`. `Workbench::mcp()` starts and stores a listener when none is stored. `shutdown()` takes the stored listener and awaits `close()` after `close_agent_sessions()` and before `shutdown_plugins()`.

- [ ] **Step 1: Failing tests**
  - `service.rs` `disconnect_ends_the_watcher` (a unit test, so it can see `inner`): serve over `tokio::io::duplex`, drop the client end, await `serve`; then poll for up to 1 s until `Arc::strong_count(&workbench.inner) == 1`, which holds only once the watcher has dropped its `Workbench` clone.
  - `service.rs` `revoke_during_initialize_ends_serve`: start `serve` on a duplex whose client never sends `initialize`; revoke the grant; `serve` returns within 2 s.
  - `service.rs` `stop_token_ends_serve`: cancel `stop` on an initialized connection; `serve` returns within 2 s.
  - `tests/mcp.rs` (new, in `skele-core`) `shutdown_waits_for_connections`: connect a client through `workbench.mcp()` (raw `interprocess` stream, token line, rmcp client over it), call `workbench.shutdown().await`; afterwards the client's `waiting()` completes within 2 s and the socket file is gone.
  - `tests/mcp.rs` `mcp_after_shutdown_is_a_new_listener`: `mcp()`, `shutdown()`, `mcp()` again → a different path that exists.
  - `bridge.rs` `revoked_grant_closes_connection`: after `revoke`, send nothing; `tokio::time::timeout(Duration::from_secs(5), client.waiting())` completes.
- [ ] **Step 2:** `cargo test -p skele-core mcp && cargo test -p skele-core --test mcp && cargo test -p skele-mcp-bridge` — expected FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "mcp: connections end with their grant, their peer or shutdown (CR1 CR2 CR3 CR6 CR9)"`

### Task 7: Agent edits reach open editors; stale editor writes are refused (CR4)

Finding: [CR4](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542641). M3 made ACP file writes emit `DocumentChanged`, but the MCP `document_edit` tool does not, and the editor's next debounced push sends its stale full buffer through `documentSetText`, erasing the tool's edit.

**Files:**
- Modify: `crates/skele-core/src/document.rs` (`set_text`), `crates/skele-core/src/workbench.rs` (new `edit_document`), `crates/skele-core/src/tools/builtin.rs:240-243`, `crates/skele-core/src/agent/session.rs:362-368` (`write_for_agent` uses the new method), `crates/skele-core/src/error.rs`
- Modify: `crates/tauri-plugin-skele/src/commands.rs:172-174` (`document_set_text`), `apps/workbench/src/renderers/editor.ts`
- Test: `crates/skele-core/src/document.rs` tests, `crates/skele-core/src/tools/builtin.rs` tests, `apps/workbench/src/renderers/editor.test.ts` (new; mock `monaco-editor` as `layout.test.ts` mocks the editor module)

**Interfaces:**
- Produces: `DocumentStore::set_text(&self, uri: &str, text: &str, base: Option<u32>) -> Result<u32>` — with `Some(base)`, `Err(Error::StaleDocument { uri: String, current: u32 })` when `base` is not the current version, and nothing changes. `None` replaces unconditionally; only `write_for_agent` passes `None`, after its own unsaved-edits check.
- Produces: `Workbench::edit_document(&self, uri: &str, start: usize, end: usize, text: &str) -> Result<u32>` — `documents().edit(..)`, then emits `HostEvent::DocumentChanged { uri, version }`. The `document_edit` tool calls it.
- Produces: IPC `document_set_text(uri, text, base: u32) -> Result<u32>`. The editor tracks the version it last synced (from `documentOpen`, each successful `documentSetText`, and each reload). A `StaleDocument` rejection reloads the editor from the host and shows `toast("warning", "<file> changed elsewhere; your last edit was not applied")`.

- [ ] **Step 1: Failing tests**
  - `document.rs` `stale_set_text_is_refused`: open (v1), `edit` (v2), `set_text(uri, "x", Some(1))` → `StaleDocument { current: 2, .. }`, text unchanged; `set_text(uri, "x", Some(2))` → `Ok(3)`; `set_text(uri, "y", None)` → `Ok(4)`.
  - `builtin.rs` `document_edit_tells_editors`: subscribe, call `document_edit` → one `HostEvent::DocumentChanged` with the returned version.
  - `editor.test.ts` `"a stale push reloads instead of overwriting"`: stub `documentSetText` to reject with a `StaleDocument` message once; type into the model, advance the debounce; expect `documentOpen` called again (reload) and the warning toast.
- [ ] **Step 2:** `cargo test -p skele-core stale document_edit_tells && bun run --cwd apps/workbench test editor` — expected FAIL.
- [ ] **Step 3:** Implement; `bun run gen:bindings`.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "documents: agent edits reach editors; stale editor writes refused (CR4)"`

### Task 8: Tool schemas must accept MCP arguments (CR5)

Finding: [CR5](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542672). `{"type":"string"}` compiles, is advertised, and every call fails validation because MCP arguments are always an object.

**Files:**
- Modify: `crates/skele-core/src/tools/mod.rs:68-80` (`ToolRegistry::register`)
- Test: `crates/skele-core/src/tools/mod.rs` tests, `crates/skele-core/tests/tools.rs`

**Interfaces:**
- Produces: `register` returns `Err(Error::InvalidSchema { key: <tool name>, message: "a tool's inputSchema must have \"type\": \"object\"" })` unless `input_schema["type"] == "object"`, checked before compiling. A plugin manifest with such a tool fails to install and leaves none of its contributions (existing rollback).

- [ ] **Step 1: Failing tests**
  - `tools/mod.rs` `non_object_schema_rejected`: `{"type":"string"}`, `{}` and `{"type":["object","null"]}` → `InvalidSchema`; `{"type":"object"}` → `Ok`.
  - `tests/tools.rs` `plugin_with_non_object_tool_fails_install`: manifest tool with `{"type":"string"}` → install `Err`, plugin's commands absent.
- [ ] **Step 2:** `cargo test -p skele-core non_object` — expected FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "tools: inputSchema must be an object schema (CR5)"`

### Task 9: Windows: one pipe name for listener and bridge; portable tests (CR7, CR8)

Findings: [CR7](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542698) `sample_plugin.rs` uses `std::os::unix` unconditionally; [CR8](https://github.com/tmcqueen/openskele/pull/1#discussion_r4586542715) the listener names its pipe after the socket's folder and the bridge after the file (`mcp.sock`).

**Files:**
- Modify: `crates/skele-core/src/mcp/socket.rs` (`listen` on Windows; new `endpoint`), `crates/skele-core/src/mcp/mod.rs` (`mcp_launch`), `crates/skele-mcp-bridge/src/main.rs:60-64`
- Modify: `crates/skele-core/tests/sample_plugin.rs:~130-145`
- Test: `crates/skele-core/src/mcp/socket.rs` tests

**Interfaces:**
- Produces: `McpListener::endpoint(&self) -> String` — what the bridge connects to: the socket path on Unix, the pipe name (the socket folder's name) on Windows. `mcp_launch` puts `endpoint()` in `SKELE_MCP_SOCKET`. The Windows `listen` and the bridge both use that string unchanged (`to_ns_name::<GenericNamespaced>()`), so the name is computed in one place.
- Produces: the sample-plugin test that copies the fixture is `#[cfg(unix)]`.

- [ ] **Step 1: Failing test** — `socket.rs` `launch_points_at_the_endpoint`: `mcp_launch(&grant)`'s `SKELE_MCP_SOCKET` equals `listener.endpoint()`; on Unix that equals `listener.path().display().to_string()`.
- [ ] **Step 2:** `cargo test -p skele-core launch_points` — expected FAIL (no `endpoint`).
- [ ] **Step 3:** Implement; gate the test; `cargo check --workspace --tests --target x86_64-pc-windows-msvc` if that target is installed (`rustup target list --installed`), otherwise state in the commit message that Windows was not compiled.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "mcp: listener and bridge share one Windows pipe name; unix-only test gated (CR7 CR8)"`

### Task 10: Integral numbers are accepted where a tool schema says `integer` (PR #1 P3)

Finding: [P3 on `builtin.rs:76`](https://github.com/tmcqueen/openskele/pull/1#discussion_r4588772659). JSON Schema's `integer` includes numbers with no fractional part, so `{"start": 0.0, "end": 1.0}` passes validation (checked against jsonschema 0.61.2) and then fails ``invalid type: floating point `1.0`, expected usize`` in `serde_json::from_value`. The same holds for `id` in `terminal_write` and `terminal_read` (`u32`).

**Files:**
- Modify: `crates/skele-core/src/tools/mod.rs:107-117` (`Workbench::call_tool`)
- Test: `crates/skele-core/src/tools/mod.rs` tests, `crates/skele-core/src/tools/builtin.rs` tests

**Interfaces:**
- Produces: `fn integral_numbers(value: Value) -> Value` in `tools/mod.rs` — walks objects and arrays and replaces each number that is finite, has no fractional part and fits `i64` (negative) or `u64` (non-negative) with that integer; every other value is unchanged (fractional numbers, strings such as `"1.0"`, numbers beyond `u64`).
- Produces: `call_tool` applies `integral_numbers` after schema validation and before dispatch, so built-in handlers and plugin tools (`tool/call`) both receive `1` rather than `1.0`. Fractional and negative offsets are still refused by the schema; an integral value beyond the target type (e.g. `1e20` for `usize`, `2^32` for `u32`) is still refused when the handler deserializes it. Every refusal stays a `ToolCallResult` with `is_error: true`.

- [ ] **Step 1: Failing tests**
  - `tools/mod.rs` `integral_numbers_become_integers`: `{"a": 1.0, "b": [2.0, 0.5, -3.0], "c": "1.0", "d": 1e20}` → `{"a": 1, "b": [2, 0.5, -3], "c": "1.0", "d": 1e20}`.
  - `builtin.rs` `document_edit_accepts_integral_float_offsets`: on an open document with text `hello\n`, `document_edit` with `start: 0.0, end: 1.0, text: "j"` succeeds and `document_read` returns `jello\n`.
  - `builtin.rs` `document_edit_refuses_fractional_negative_and_huge_offsets`: `start: 0.5`, `start: -1` and `start: 1e20` (each with `end: 1`) each give `is_error: true` and leave the text unchanged.
  - `builtin.rs` `terminal_read_accepts_integral_float_id` (unix): `terminal_open`, then `terminal_read` with `id` as `<id>.0` succeeds.
- [ ] **Step 2:** `cargo test -p skele-core integral document_edit_ terminal_read_accepts` — expected FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run the Global Constraints checks — PASS.
- [ ] **Step 5: Commit** — `git commit -m "tools: integral numbers are integers after validation (PR #1 P3)"`

### Task 11: End-to-end check and docs

**Files:**
- Modify: `docs/adr/0001-ui-state-owned-by-rust.md`, `docs/adr/0007-plugins-are-sidecar-processes.md`, `docs/adr/0012-output-channels-in-the-core.md` (tick the action items that name #3–#7; remove each "Known deviations" entry that is now fixed)

- [ ] **Step 1:** `bun run build:plugins`, then run the app on a private display with throwaway settings (`Xvfb :99`, `DISPLAY=:99 GDK_BACKEND=x11`, `XDG_CONFIG_HOME`/`XDG_DATA_HOME` in the scratchpad, `bun run dev`) and drive it with `xdotool`:
  - Fresh start shows Output and Settings with Settings active. Close Settings, reload the webview (touch `apps/workbench/src/main.ts`): only Output remains.
  - The status bar shows plugins whose containers were never opened as `idle`; opening a container's icon starts its plugin.
  - Open a terminal, close its tab: the shell process is gone (`ps --ppid <app pid>`), and a reload shows no terminal tab.
  - Set `terminal.shell` to `/nonexistent`, run New Terminal from the title bar menu and from `Ctrl+Shift+T`: one toast each, and the Workbench channel in the Output panel shows each failure once.
  - Open a file in the editor, have the scripted agent call `document_edit` on it: the editor shows the edit at once, and typing afterwards keeps it.
  - Start and close three agent sessions, then quit: no `skele-mcp-bridge` process remains and the socket folder under `$XDG_RUNTIME_DIR` is gone.
- [ ] **Step 2:** Update the three ADRs.
- [ ] **Step 3: Commit** — `git commit -m "docs: review fixes recorded in ADRs"`
