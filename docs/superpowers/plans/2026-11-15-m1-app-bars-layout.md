# Milestone 1: App Bars and Rust-Owned Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Primary and secondary app bars (icon strip + side panel) whose state, along with every open panel and every terminal, lives in the Rust core.

**Architecture:** New `containers.rs` registry and `layout.rs` model in `skele-core`; the core emits `HostEvent::LayoutChanged` on every change and the webview reconciles Dockview to it. PTYs are created in the core with a scrollback buffer and the webview attaches to them.

**Tech Stack:** Rust (skele-core, skele-protocol, tauri-plugin-skele, tauri-specta), TypeScript + Vite, dockview-core 8.9, @vscode/codicons, xterm.js.

**Spec:** `docs/superpowers/specs/2026-11-15-app-bars-agents-design.md` (Milestone 1). ADRs: `docs/adr/0001-ui-state-owned-by-rust.md`, `docs/adr/0005-app-bars-on-dockview-edge-groups.md`.

## Global Constraints

- `skele-core` has no Tauri dependency; all core behavior is covered by `cargo test -p skele-core`.
- Rust types in `skele-protocol` are the source: after protocol edits run `bun run gen:protocol`; after IPC edits run `bun run gen:bindings`. Both must leave no further diff.
- Protocol version stays `0.1.0` in this milestone (bumped in M2). Unknown manifest fields are already ignored by serde, so old manifests still parse.
- Settings: `workbench.primaryBar.position` default `"left"`, `workbench.secondaryBar.position` default `"right"`, both `enum ["left","right","top"]`.
- Keybindings: `workbench.togglePrimaryBar` = `Mod+B`, `workbench.toggleSecondaryBar` = `Mod+Alt+B`.
- Built-in container id `workbench.views`, title `"Views"`, icon `"list-tree"`, primary bar, order `0`.
- Terminal scrollback: 64 KiB per terminal (`SCROLLBACK_BYTES = 64 * 1024`), trimmed on a UTF-8 char boundary.
- New terminals open at 80×24; the webview resizes on attach.
- Panel ids: `"settings"`, `"editor:<path>"`, `"terminal:<id>"`.
- Linux only is tested. Every task ends with `cargo test --workspace`, `cargo clippy --workspace -- -D warnings` and `bun run typecheck` green.

## Review Focus

- Plugin uninstalled while its container is the active one: the bar's active container falls back to the lowest-order remaining container on that bar, or `None` (test in Task 3).
- A view names a container that does not exist: install fails and none of the plugin's contributions stay registered (test in Task 2).
- Opening a file that is already open: no second panel; the existing one becomes active (test in Task 3).
- Closing the active panel: the panel opened just before it becomes active, or `None` when none remain (test in Task 3).
- A terminal's shell exits: its panel stays; attaching later still yields the scrollback followed by `Exit` (test in Task 5).

---

### Task 1: Manifest: view containers and `container` on views

**Files:**
- Modify: `crates/skele-protocol/src/manifest.rs`
- Modify: `crates/skele-protocol/src/lib.rs` (tests module)
- Regenerate: `protocol/plugin-manifest.schema.json`, `protocol/openrpc.json`, `sdk/*/Generated/*`

**Interfaces:**
- Produces:
  - `pub enum BarId { Primary, Secondary }` (camelCase serde, `JsonSchema`, `cfg_attr(feature="specta", derive(specta::Type))`, `Copy, PartialEq, Eq, Debug`).
  - `pub struct ViewContainerContribution { id: String, title: String, icon: String, #[serde(default = primary)] bar: BarId, #[serde(default)] order: i32 }`.
  - `Contributes.view_containers: Vec<ViewContainerContribution>` (serialized `viewContainers`, default empty).
  - `ViewContribution.container: Option<String>` replaces `location`. `ViewLocation` is deleted.

- [ ] **Step 1: Write the failing test** in `crates/skele-protocol/src/lib.rs` tests:

```rust
#[test]
fn manifest_parses_view_containers() {
    let m: manifest::Manifest = serde_json::from_value(serde_json::json!({
        "id": "a.b", "name": "B", "version": "0", "protocol": "0.1",
        "main": { "command": "x" },
        "contributes": {
            "viewContainers": [{ "id": "b.box", "title": "Box", "icon": "package" },
                               { "id": "b.side", "title": "Side", "icon": "info", "bar": "secondary", "order": 3 }],
            "views": [{ "id": "b.v", "title": "V", "container": "b.box" },
                      { "id": "b.w", "title": "W", "location": "sidebar" }]
        }
    })).unwrap();
    let c = &m.contributes;
    assert_eq!(c.view_containers[0].bar, manifest::BarId::Primary);
    assert_eq!(c.view_containers[1].bar, manifest::BarId::Secondary);
    assert_eq!(c.view_containers[1].order, 3);
    assert_eq!(c.views[0].container.as_deref(), Some("b.box"));
    assert_eq!(c.views[1].container, None); // old `location` is ignored
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p skele-protocol manifest_parses_view_containers`
Expected: compile error, no field `view_containers`.

- [ ] **Step 3: Implement the types above in `manifest.rs`**; remove `ViewLocation` and `default_view_location`.

- [ ] **Step 4: Run the test**

Run: `cargo test -p skele-protocol`
Expected: PASS. (`skele-core` will not compile until Task 2; that is expected.)

- [ ] **Step 5: Regenerate protocol documents**

Run: `bun run gen:protocol && git status --short protocol sdk`
Expected: `plugin-manifest.schema.json` shows `viewContainers` and `container`; no `location`.

- [ ] **Step 6: Commit** (together with Task 2 if `skele-core` must compile first; otherwise commit now with `git commit -m "protocol: view containers in the manifest"`).

### Task 2: Container registry

**Files:**
- Create: `crates/skele-core/src/containers.rs`
- Modify: `crates/skele-core/src/lib.rs`, `crates/skele-core/src/views.rs`, `crates/skele-core/src/plugin/mod.rs:140-200`, `crates/skele-core/src/workbench.rs`, `crates/skele-core/src/error.rs`
- Test: inline `#[cfg(test)]` in `containers.rs`; `crates/skele-core/tests/containers.rs`

**Interfaces:**
- Consumes: `BarId`, `ViewContainerContribution` (Task 1).
- Produces:
  - `pub struct ContainerInfo { id: String, title: String, icon: String, bar: BarId, order: i32, source: Option<String> }` (Serialize camelCase, specta).
  - `ContainerRegistry { register(&self, ContainerInfo) -> Result<()>` (duplicate → `Error::DuplicateContainer(id)`), `get(&self, &str) -> Option<ContainerInfo>`, `list(&self) -> Vec<ContainerInfo>` sorted by `(bar, order, id)`, `on_bar(&self, BarId) -> Vec<ContainerInfo>`, `remove_source(&self, &str)` `}`.
  - `pub const VIEWS_CONTAINER: &str = "workbench.views";`
  - `ViewInfo.container: String` replaces `location` (absent in manifest → `VIEWS_CONTAINER`).
  - `Workbench::containers(&self) -> &ContainerRegistry`; `Snapshot.containers: Vec<ContainerInfo>`.
  - `Error::UnknownContainer(String)`, `Error::DuplicateContainer(String)`.
  - Built-in command per container: id `workbench.focusContainer.<container id>`, title `"Focus <title>"`, category `"View"`; registered with the container, removed with its source. Its handler calls `Workbench::set_active_container(bar, Some(id))` (defined in Task 4; until then it may be a no-op stub, replaced in Task 4).

- [ ] **Step 1: Write failing unit tests** in `containers.rs`: `list_orders_by_bar_then_order` (register secondary/order 0, primary/order 5, primary/order 1 → primary order1, primary order5, secondary), `duplicate_container_is_rejected`, `remove_source_drops_only_that_plugin`.

- [ ] **Step 2: Write failing integration tests** in `tests/containers.rs`, installing manifests written to a `tempfile::tempdir()`:
  - `plugin_containers_and_views_are_registered`: manifest with one container and one view in it → `snapshot().containers` contains `workbench.views` and the plugin's; the view's `container` is the plugin's; command `workbench.focusContainer.<id>` exists.
  - `view_without_container_goes_to_views`: view with no `container` → `container == "workbench.views"`.
  - `unknown_container_fails_install_and_rolls_back`: view with `"container": "nope"` → `install_plugin` returns `Err(Error::UnknownContainer("nope"))`; afterwards no command, view, container or config section from that plugin is registered.
  - (There is no public uninstall; removal by source is covered by the `remove_source_drops_only_that_plugin` unit test and by the rollback test above.)

- [ ] **Step 3: Run to verify failures**

Run: `cargo test -p skele-core containers`
Expected: compile errors / FAIL.

- [ ] **Step 4: Implement.** Register `workbench.views` in `register_builtins`. In `register_contributions`, register containers before views; check each view's container exists (built-in or this manifest's) and return `Error::UnknownContainer` otherwise (the caller already rolls back). Add `containers.remove_source(id)` and focus-command removal to `unregister_contributions` (focus commands carry `source = Some(plugin id)` so `commands.remove_source` already drops them).

- [ ] **Step 5: Run tests**

Run: `cargo test -p skele-core`
Expected: PASS, including existing `sample_plugin` / `sample_dotnet` tests.

- [ ] **Step 6: Commit**

```bash
git add crates/skele-protocol crates/skele-core protocol sdk
git commit -m "core: view container registry"
```

### Task 3: Layout model

**Files:**
- Create: `crates/skele-core/src/layout.rs` (pure model + unit tests; Workbench methods come in Task 4)
- Modify: `crates/skele-core/src/lib.rs`

**Interfaces:**
- Consumes: `BarId`, `ContainerInfo` (Task 2).
- Produces (all Serialize/Deserialize camelCase, specta, Clone, Debug, PartialEq):
  - `pub enum BarPosition { Left, Right, Top }`
  - `pub struct Bar { position: BarPosition, visible: bool, active_container: Option<String> }`
  - `#[serde(tag = "kind")] pub enum Panel { Settings, Editor { path: String }, Terminal { id: u32 } }` with `pub fn id(&self) -> String`.
  - `pub struct Layout { primary: Bar, secondary: Bar, panels: Vec<Panel>, active_panel: Option<String> }`; `Default` = primary Left visible, secondary Right visible, no panels.
  - `impl Layout`:
    - `bar(&self, BarId) -> &Bar`, `bar_mut(&mut self, BarId) -> &mut Bar`
    - `open(&mut self, panel: Panel)` — appends if its id is new; always makes it active.
    - `close(&mut self, id: &str) -> Option<Panel>` — if it was active, the panel before it in `panels` becomes active, else the new last one, else `None`.
    - `activate(&mut self, id: &str) -> bool` — false if no such panel.
    - `fix_containers(&mut self, containers: &[ContainerInfo])` — for each bar, if `active_container` is not a container on that bar, set it to the lowest-order container on that bar, or `None`; if it is `None` and the bar has containers, pick the lowest-order one.

- [ ] **Step 1: Write failing tests** in `layout.rs`:
  - `open_same_file_twice_keeps_one_panel` — open `Editor{"/a"}` twice → `panels.len() == 1`, active `"editor:/a"`.
  - `open_activates` — open settings then editor → active `"editor:/a"`; open settings again → active `"settings"`, len 2.
  - `closing_active_activates_previous` — open s, e1, e2; activate e1; close e1 → active `"settings"`; close settings → active `"editor:/e2"`; close it → `None`.
  - `closing_inactive_keeps_active`.
  - `activate_unknown_is_false`.
  - `fix_containers_falls_back_by_order` — primary active `"gone"`, containers primary `b`(order 2), `a`(order 1) → active `Some("a")`; no primary containers → `None`.
  - `fix_containers_ignores_other_bar` — primary active set to a secondary container id → replaced.
  - `panel_ids` — `Panel::Terminal{id:3}.id() == "terminal:3"`.
  - `panel_serializes_tagged` — `serde_json::to_value(Panel::Editor{path:"/a".into()})` == `{"kind":"editor","path":"/a"}`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p skele-core layout::`
Expected: FAIL / compile error.

- [ ] **Step 3: Implement** the types and methods above.

- [ ] **Step 4: Run tests**

Run: `cargo test -p skele-core layout::`
Expected: PASS.

- [ ] **Step 5: Commit** — `git commit -m "core: layout model"`

### Task 4: Workbench owns the layout

**Files:**
- Modify: `crates/skele-core/src/layout.rs` (add `impl Workbench` block), `crates/skele-core/src/workbench.rs`, `crates/skele-core/src/events.rs`, `crates/skele-core/src/plugin/mod.rs` (call `fix_containers` after install/uninstall)
- Test: `crates/skele-core/tests/layout.rs`

**Interfaces:**
- Consumes: Tasks 2–3.
- Produces:
  - `HostEvent::LayoutChanged { layout: Layout }`, `HostEvent::ShowCommandPalette`. `HostEvent::OpenPanel` removed.
  - `Inner.layout: Mutex<Layout>`; `Snapshot.layout: Layout`.
  - On `Workbench`: `layout(&self) -> Layout`, `open_panel(&self, Panel)`, `close_panel(&self, id: &str) -> Result<()>` (`Error::UnknownPanel`; closing a `Terminal{id}` also calls `terminals().close(id)`), `set_active_panel(&self, id: &str) -> Result<()>`, `set_active_container(&self, bar: BarId, id: Option<String>) -> Result<()>` (container must exist and be on `bar`, else `Error::UnknownContainer`), `toggle_bar(&self, bar: BarId)`. Each mutation emits exactly one `LayoutChanged` (none if nothing changed).
  - `Error::UnknownPanel(String)`.
  - Built-ins: `workbench.openSettings` → `open_panel(Settings)`; `workbench.openFile(path)` → `open_panel(Editor{path})`; `workbench.showCommands` → emits `ShowCommandPalette`; `workbench.togglePrimaryBar` (`Mod+B`, menu `menubar/View`), `workbench.toggleSecondaryBar` (`Mod+Alt+B`, menu `menubar/View`); focus-container commands from Task 2 now call `set_active_container`. (`workbench.newTerminal` moves in Task 5.)
  - Settings `workbench.primaryBar.position` / `workbench.secondaryBar.position` registered in the built-in section; applied to the layout in `Workbench::new` and in `set_config` when either key changes (emit `LayoutChanged` after `ConfigChanged`).

- [ ] **Step 1: Write failing integration tests** in `tests/layout.rs` (subscribe to the bus first; use `common::expect` where an event is awaited — add `mod common;`):
  - `open_settings_command_opens_panel` — `execute_command("workbench.openSettings")` → `layout().active_panel == Some("settings")` and a `LayoutChanged` event arrives.
  - `open_file_twice_one_panel` — `workbench.openFile` with `"/tmp/x"` twice → one `Editor` panel.
  - `open_file_without_path_errors`.
  - `toggle_primary_bar` — `Mod+B` keybinding is registered for `workbench.togglePrimaryBar`; executing it flips `layout().primary.visible`.
  - `position_setting_moves_bar` — `set_config("workbench.primaryBar.position", Some(json!("top")), Scope::User)` → `layout().primary.position == BarPosition::Top`; `"middle"` → `Err(InvalidSetting)` and position unchanged.
  - `both_bars_may_share_a_side` — set secondary to `"left"` → accepted.
  - `set_active_container_rejects_wrong_bar` — `set_active_container(BarId::Secondary, Some("workbench.views"))` → `Err(UnknownContainer)`.
  - `install_picks_lowest_order_container` — install a plugin with primary container `p.box` (order -1) → `layout().primary.active_container == Some("p.box")` only if none was active before; with `workbench.views` already active it stays `workbench.views`. (Fallback after removal is pinned by Task 3's `fix_containers_falls_back_by_order`.)
  - `show_commands_emits_palette_event`.
  - `snapshot_has_layout_and_containers`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p skele-core --test layout`
Expected: FAIL.

- [ ] **Step 3: Implement.** Remove the `open_panel` closure in `register_builtins`; give each built-in its own handler holding `Weak<Inner>`. Call `layout.fix_containers(&containers.list())` at the end of `register_builtins`, `install_plugin` and `unregister_contributions`, emitting `LayoutChanged` if it changed. Add the two settings to the `properties` map in `register_builtins`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p skele-core`
Expected: PASS.

- [ ] **Step 5: Commit** — `git commit -m "core: workbench owns the layout; OpenPanel removed"`

### Task 5: Core-owned terminals with scrollback

**Files:**
- Modify: `crates/skele-core/src/pty.rs`, `crates/skele-core/src/layout.rs`, `crates/skele-core/src/workbench.rs`
- Test: inline tests in `pty.rs`; `crates/skele-core/tests/layout.rs`

**Interfaces:**
- Produces:
  - `pub const SCROLLBACK_BYTES: usize = 64 * 1024;`
  - `TerminalManager::open(&self, options: TerminalOptions) -> Result<u32>` (no callback any more).
  - `pub struct Attachment { pub scrollback: String, pub exited: bool, pub events: tokio::sync::broadcast::Receiver<TerminalEvent> }`
  - `TerminalManager::attach(&self, id: u32) -> Result<Attachment>` — scrollback snapshot and subscription taken under the same lock the reader thread holds when it appends and broadcasts, so nothing is missed or duplicated.
  - `TerminalManager::read(&self, id: u32) -> Result<String>` — current scrollback (M2's `terminal_read` uses it).
  - `write`, `resize`, `close`, `close_all` unchanged.
  - `Workbench::new_terminal(&self) -> Result<u32>` — opens with `terminal.shell`, workspace cwd, 80×24, then `open_panel(Terminal{id})`. `workbench.newTerminal` (`Mod+Shift+T`) calls it.

- [ ] **Step 1: Write failing unit tests** in `pty.rs`:
  - Replace `shell_output_reaches_the_callback` with `attach_sees_scrollback_then_live` (unix): open `/bin/sh`, write `echo sk''ele-one\n`, poll `read(id)` until it contains `skele-one` (10 s deadline), then `attach` → `scrollback` contains `skele-one`; write `echo sk''ele-two; exit\n`; receive from `events` until `Exit`, collected text contains `skele-two` and not `skele-one`.
  - `attach_after_exit_reports_exited` (unix): run `exit\n`, wait for `read`/exit, `attach` → `exited == true`.
  - `scrollback_is_bounded` — unit-test the ring helper directly: push 100 KiB of `"é"` → length ≤ `SCROLLBACK_BYTES` and still valid UTF-8 starting on a char boundary.
  - `unknown_terminal_errors` — `attach(999)` → `Err(UnknownTerminal(999))`.

- [ ] **Step 2: Write failing integration test** in `tests/layout.rs`: `new_terminal_adds_panel_and_close_kills_it` (unix) — `execute_command("workbench.newTerminal")` → layout has `Terminal{id}` active; `close_panel("terminal:<id>")` → `terminals().attach(id)` is `Err(UnknownTerminal)`.

- [ ] **Step 3: Run to verify failure**

Run: `cargo test -p skele-core pty:: && cargo test -p skele-core --test layout`
Expected: FAIL.

- [ ] **Step 4: Implement.** Per terminal keep `Arc<Mutex<Shared { scrollback: String, exited: bool, tx: broadcast::Sender<TerminalEvent> }>>` (channel capacity 256); the reader thread appends, trims to `SCROLLBACK_BYTES` (drop from the front up to the next char boundary) and sends, all under that lock.

- [ ] **Step 5: Run tests**

Run: `cargo test -p skele-core`
Expected: PASS (Tauri crate breaks until Task 6).

- [ ] **Step 6: Commit** — `git commit -m "core: terminals owned by the core with scrollback and attach"`

### Task 6: IPC for layout and terminal attach

**Files:**
- Modify: `crates/tauri-plugin-skele/src/commands.rs`, `src/lib.rs` (`collect_commands!`), `build.rs` (`COMMANDS`), `permissions/default.toml`
- Regenerate: `crates/tauri-plugin-skele/permissions/autogenerated/**`, `apps/workbench/src/bindings.ts`

**Interfaces:**
- Consumes: Task 4 Workbench methods, Task 5 `attach`.
- Produces (TypeScript names via tauri-specta): `layoutSetActiveContainer(bar: BarId, id: string | null)`, `layoutSetActivePanel(id: string)`, `layoutClosePanel(id: string)`, `terminalAttach(id: number, onEvent: Channel<TerminalEvent>)`. `terminalOpen` is removed (permission `allow-terminal-open` too). `Session.terminals: Vec<u32>` becomes `Session.attachments: Vec<JoinHandle<()>>`; a reload aborts the forwarders and **does not** close terminals. `terminal_attach` sends the scrollback as one `Data` event, then `Exit` if `exited`, then forwards live events until the channel or terminal closes.

- [ ] **Step 1: Implement the four commands** and update the three command lists and permissions.

- [ ] **Step 2: Build and regenerate**

Run: `cargo build -p tauri-plugin-skele && bun run gen:bindings && git diff --stat apps/workbench/src/bindings.ts`
Expected: builds; bindings gain the new commands, `HostEvent` has `layoutChanged` and `showCommandPalette`, no `openPanel`.

- [ ] **Step 3: Verify no stray diff**

Run: `bun run gen:bindings && git diff --exit-code apps/workbench/src/bindings.ts crates/tauri-plugin-skele/permissions` (after staging step 2's output)
Expected: exit 0.

- [ ] **Step 4: Commit** — `git commit -m "tauri: layout and terminal attach IPC"` (the webview typechecks again after Task 8; note this in the message body).

### Task 7: Spike: Dockview edge groups as icon bars (throwaway)

**Files:**
- Scratch only: a branch-local `apps/workbench/src/spike-edge.ts`, not committed.
- Modify: `docs/adr/0005-app-bars-on-dockview-edge-groups.md` (record the outcome)

- [ ] **Step 1:** Read `node_modules/dockview-core/dist/cjs/dockview/dockviewShell.d.ts` and the `addEdgeGroup` signature; note how tabs are rendered (`createTabComponent` / header actions), collapse and resize APIs, and whether `position` can change on an existing group.
- [ ] **Step 2:** Build a throwaway page: one edge group left with two icon-only tabs (codicon `list-tree`, `package`), each with a tree placeholder; one on the right; move the left one to the top at runtime.
- [ ] **Step 3:** Run under Xvfb (see Task 10 for the commands); screenshot.
- [ ] **Step 4:** Record in ADR-0005: **Accepted** with the API calls to use (group creation, tab renderer, move, collapse), or **Superseded by fallback** with the reason. Delete the spike file.
- [ ] **Step 5: Commit** — `git commit -m "docs: ADR-0005 spike outcome"`

### Task 8: Webview: codicons, bars and reconcile

**Files:**
- Create: `apps/workbench/src/bars.ts`, `apps/workbench/src/layout.ts`
- Modify: `apps/workbench/src/main.ts`, `apps/workbench/src/renderers/tree.ts`, `apps/workbench/src/styles.css`, `apps/workbench/package.json`

**Interfaces:**
- Consumes: Task 6 bindings, Task 7 outcome.
- Produces:
  - `bars.ts`: `createBars(dock: DockviewApi, onSelect: (bar: BarId, id: string | null) => void): Bars` with `Bars.render(snapshot: Snapshot, layout: Layout): void`. Clicking the active icon of a bar sends `null` (collapses), another icon sends its id. Containers with no views are not shown (built-in content containers arrive in M3).
  - `layout.ts`: `reconcile(dock: DockviewApi, layout: Layout, snapshot: Snapshot): void` — closes Dockview panels absent from `layout.panels`, adds missing ones (settings/editor beside an existing document panel else above Output, terminals within Output's group), and activates `layout.activePanel`. It must not call any layout IPC itself (no feedback loops); it ignores Dockview close events it caused.
  - Gestures: Dockview tab close → `commands.layoutClosePanel(id)` (and the panel stays until `layoutChanged` removes it); tab activation by the user → `commands.layoutSetActivePanel(id)`.
  - Tree icons: `item.icon` is a codicon name rendered as `<span class="codicon codicon-${icon}">`; `ICONS` map deleted. Import `@vscode/codicons/dist/codicon.css` in `main.ts`.

- [ ] **Step 1:** `cd apps/workbench && bun add @vscode/codicons` (pin the resolved version in `package.json`).
- [ ] **Step 2:** Implement `bars.ts` per Task 7's outcome, `layout.ts`, and in `main.ts` delete `openPanel()`, `syncViews()`, `reveal()` and the `openPanel` event case; add cases `layoutChanged` → `reconcile` + `bars.render`, `showCommandPalette` → `togglePalette(snapshot)`; `contributionsChanged` → reload snapshot then both. On start, call `commands.executeCommand("workbench.openSettings")` instead of `openPanel("settings")` only if `snapshot.layout.panels` is empty (keeps reload state).
- [ ] **Step 3:** Apply `layout.primary.position` / `secondary.position` and `visible` in `bars.render`.
- [ ] **Step 4:** Check the icon names the samples send (`plugins/sample-bun/src/main.ts`, `plugins/sample-dotnet/Program.cs`: `play`, `gear`, `comment`, `file`, `folder`) all exist in `node_modules/@vscode/codicons/dist/codicon.css`; rename any that do not.
- [ ] **Step 5: Typecheck**

Run: `bun run typecheck`
Expected: no errors.

- [ ] **Step 6: Commit** — `git commit -m "workbench: app bars, codicons and layout reconcile"`

### Task 9: Webview terminals attach

**Files:**
- Modify: `apps/workbench/src/renderers/terminal.ts`, `apps/workbench/src/main.ts` (`createComponent` `terminal` case)

**Interfaces:**
- Produces: `createPtyTerminal(id: number): { element: HTMLElement; dispose(): void }` — calls `commands.terminalAttach(id, channel)`, then `terminalResize` on fit; `dispose` only tears down xterm (closing the PTY is the layout's job via `layoutClosePanel`). Panel params carry `{ id }` from `Panel.Terminal`.

- [ ] **Step 1:** Implement; remove every `terminalOpen` / `terminalClose` call from the webview.
- [ ] **Step 2: Typecheck** — `bun run typecheck`, expected clean.
- [ ] **Step 3: Commit** — `git commit -m "workbench: terminals attach to core-owned PTYs"`

### Task 10: Sample containers, docs and end-to-end check

**Files:**
- Modify: `plugins/sample-bun/skele-plugin.json`, `plugins/sample-dotnet/skele-plugin.json`, `README.md`, `crates/skele-core/tests/sample_plugin.rs`, `crates/skele-core/tests/sample_dotnet.rs`

- [ ] **Step 1:** Sample manifests: `sample-bun` adds `viewContainers: [{ "id": "hello.container", "title": "Hello", "icon": "comment-discussion" }]` and its view gets `"container": "hello.container"`; `sample-dotnet` adds `{ "id": "dotnet.container", "title": "Runtime", "icon": "server-environment", "bar": "secondary" }` and its view moves there. Remove `location` from both.
- [ ] **Step 2:** Extend the two sample tests: after install, `snapshot().containers` contains the sample's container on the expected bar.

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 3:** README: Layout table row for `containers.rs`/`layout.rs` under `skele-core`; a "View containers and app bars" section (manifest example, the two settings, keybindings); remove `location` from any example.
- [ ] **Step 4: End-to-end on a private X display**

```bash
Xvfb :99 -screen 0 1600x1000x24 &
DISPLAY=:99 bun run dev &
# wait for the window, then:
DISPLAY=:99 xdotool search --name openskele windowactivate
DISPLAY=:99 import -window root /tmp/m1-start.png
```

Check by screenshot after each action:
- both bars visible: Views/Hello on the left strip, Runtime on the right;
- clicking an icon switches the container; clicking the active one collapses it;
- `DISPLAY=:99 xdotool key ctrl+b` hides/shows the primary bar; `ctrl+alt+b` the secondary;
- setting primary position to `top` in Settings moves it live; `right` too;
- open a terminal (`ctrl+shift+t`), type `echo hi`, reload the page (`ctrl+r` in dev) → the terminal panel is back with `hi` in it.

- [ ] **Step 5: Full checks**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings && bun run typecheck && dotnet build plugins/sample-dotnet -c Release -nologo && bun run gen:protocol && bun run gen:bindings && git diff --exit-code`
Expected: all succeed; no diff.

- [ ] **Step 6: Commit** — `git commit -m "samples: view containers; docs for app bars"`
