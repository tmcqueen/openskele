# Output Channels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Per-source output channels kept in the core, a channel dropdown in the Output panel, and optional log files.

**Architecture:** `output/` in `skele-core` holds an in-memory store of bounded line buffers and an optional log sink; every write goes through `Workbench::output`, which appends, emits `OutputAppended`, and mirrors to the log sink. Plugin stderr, stray stdout and `terminal/write` all land there. The webview's Output panel lists channels, replays one and follows it.

**Tech Stack:** Rust (skele-core, tauri-plugin-skele), `chrono` (local timestamps, already in Cargo.lock), TypeScript, xterm.js, bun test + happy-dom.

**Spec:** `docs/superpowers/specs/2026-11-15-output-channels-design.md`

## Global Constraints

- Channel buffer: last `OUTPUT_LINES = 10_000` lines; a trailing partial line counts as one.
- Host channel name: `WORKBENCH_CHANNEL = "Workbench"`; `channels()` lists it first, the rest sorted by name.
- Settings: `output.logToFile` (boolean, default `false`), `output.logDirectory` (string, default `""`).
- Log files only when `--log-dir <path>` / `SKELE_LOG_DIR` is given (wins over settings) **or** `output.logToFile && output.logDirectory != ""`.
- Log layout: `<dir>/<YYYYMMDD-HHMMSS>/<channel>.log`, local time; channel file names keep `[A-Za-z0-9._ -]`, anything else becomes `_`. Flushed on every write. Text written as received.
- Relative `output.logDirectory` resolves against the workspace; no workspace → ignored with one warning in the Workbench channel. Unwritable directory → one warning, no files.
- `HostEvent::TerminalData` is replaced by `OutputAppended { channel, text }`; new `OutputChannelsChanged` and `OutputCleared { channel }`.
- Each task ends with `bun run test`, `cargo clippy --workspace --all-targets -- -D warnings`, `bun run typecheck`; format only touched Rust files with `rustfmt --edition 2021`.

## Review Focus

- A chunk without a trailing newline followed by its continuation: one line in the buffer, not two (test in Task 1).
- Turning logging on mid-run: the file starts with the channel's existing buffer, then new text, no duplicates (test in Task 3).
- `output.logToFile` true with an empty directory: no files, no warning (test in Task 3).
- Clearing the selected channel in one webview: the view empties; text appended afterwards appears (test in Task 5).
- Stored selection names a channel that no longer exists: falls back to Workbench (test in Task 5).

---

### Task 0: Branch and commit the pending Output-panel work

- [ ] `git switch -c output-channels`; commit the uncommitted Output panel / terminal placement changes already on disk as `workbench: Output is a layout panel; View → Show Output` (paths: `README.md apps/workbench/src crates/skele-core`).

### Task 1: Output store and `Workbench::output`

**Files:** Create `crates/skele-core/src/output/mod.rs`; modify `lib.rs`, `events.rs`, `workbench.rs`, `plugin/mod.rs`; tests inline + `crates/skele-core/tests/output.rs`; update `tests/sample_plugin.rs`, `tests/sample_dotnet.rs` to the new event.

**Interfaces (produces):**
- `pub const OUTPUT_LINES: usize = 10_000; pub const WORKBENCH_CHANNEL: &str = "Workbench";`
- `OutputStore { append(&self, channel: &str, text: &str) -> bool /* channel was new */, channels(&self) -> Vec<String>, read(&self, channel: &str) -> Option<String>, clear(&self, channel: &str) -> bool }`
- `Workbench::output(&self, channel: &str, text: &str)` — append, emit `OutputAppended`, emit `OutputChannelsChanged` when new, write to the log sink (Task 3 adds the sink).
- `Workbench::output_channels() -> Vec<String>`, `output_read(&str) -> Result<String>` (`Error::UnknownChannel`), `output_clear(&str) -> Result<()>` (emits `OutputCleared`).
- `Workbench::notify(&self, level: MessageLevel, message: &str, source: Option<&str>)` — toast (`bus.message`) **and** a line `"[<level>] <source>: <message>\r\n"` in Workbench. Replaces `bus().message` in `plugin/mod.rs` and is used for `window/showMessage`.
- The Workbench channel exists from startup (empty).

- [ ] **Step 1: Failing unit tests** in `output/mod.rs`: `partial_lines_join` (append `"ab"`, `"c\r\n"`, `"d"` → read `"abc\r\nd"`, 2 lines), `buffer_keeps_last_lines` (10_005 lines → read starts with line `5`), `channels_workbench_first` (`b`, `Workbench`, `a` → `[Workbench, a, b]`), `append_reports_new_channel`, `clear_empties_but_keeps_channel`.
- [ ] **Step 2: Failing integration tests** in `tests/output.rs`: `workbench_channel_exists_at_startup`; `output_emits_events` (new channel → `OutputChannelsChanged` then `OutputAppended`); `read_unknown_channel_errors`; `clear_emits_cleared`; `notify_logs_to_workbench`.
- [ ] **Step 3:** Run `cargo test -p skele-core output` — FAIL.
- [ ] **Step 4:** Implement; plugin stderr and `terminal/write` call `workbench.output(...)`; `[host]` protocol errors too.
- [ ] **Step 5:** `cargo test` — PASS (sample tests match `OutputAppended { channel, text }`).
- [ ] **Step 6: Commit** `core: output channels with bounded line buffers`.

### Task 2: Stray stdout reaches the plugin's channel

**Files:** `crates/skele-core/src/plugin/transport.rs`, `peer.rs`, `plugin/mod.rs`; tests inline and in `tests/sample_plugin.rs`.

**Interfaces:** `read_frame(reader, on_stray: &mut (dyn FnMut(&str) + Send))` passes each non-header, non-blank line (without line ending); `Handler::stray_output(&self, line: String)` with a default no-op; `PluginHandler` forwards to `workbench.output(name, line + "\r\n")`.

- [ ] **Step 1: Failing test:** rename `frames_round_trip_and_stray_output_is_skipped` → `frames_round_trip_and_stray_output_is_reported`, asserting the callback saw `["stray console.log output"]`.
- [ ] **Step 2:** `cargo test -p skele-core transport` — FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5:** Sample Bun plugin: add `console.log("sample.hello stdout check")` once at activation (a deliberate stray line, commented as such); `sample_plugin.rs` asserts the Hello channel contains it.
- [ ] **Step 6: Commit** `core: stray plugin stdout goes to its output channel`.

### Task 3: Log files

**Files:** Create `crates/skele-core/src/output/log.rs`; modify `output/mod.rs`, `workbench.rs` (`WorkbenchOptions.log_dir: Option<PathBuf>`, settings, live apply), `Cargo.toml` (`chrono` with `default-features = false, features = ["clock"]`); tests `tests/output.rs`.

**Interfaces:**
- `LogSink::open(dir: &Path) -> io::Result<LogSink>` (creates `<dir>/<stamp>/`), `LogSink::write(&mut self, channel: &str, text: &str) -> io::Result<()>`, `LogSink::run_dir(&self) -> &Path`.
- `pub fn log_file_name(channel: &str) -> String`.
- `Workbench::log_dir(&self) -> Option<PathBuf>` (current run folder, for tests and the README).
- `Workbench::apply_log_settings(&self)`, called from `new()` and from `set_config` for both keys; ignored when `WorkbenchOptions.log_dir` is set.

- [ ] **Step 1: Failing tests** (`tempfile`): `startup_dir_writes_files` (options `log_dir`; output to `Hello` → `<run>/Hello.log` has the text); `settings_need_both` (only `logToFile` → no `log_dir()`, no warning in Workbench; then set directory → files); `enabling_midrun_writes_buffer_first` (output `a\r\n`, enable, output `b\r\n` → file is exactly `a\r\nb\r\n`); `disable_stops_writing`; `relative_dir_without_workspace_warns`; `relative_dir_uses_workspace`; `flag_wins_over_settings` (settings point elsewhere → files only under the flag dir); `log_file_name_sanitises` (`"a/b:c"` → `"a_b_c.log"`).
- [ ] **Step 2:** FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5: Commit** `core: optional output log files`.

### Task 4: IPC and the startup flag

**Files:** `crates/tauri-plugin-skele/src/commands.rs`, `lib.rs` (`Builder::log_dir`), `build.rs`, `permissions/default.toml`; `apps/workbench/src-tauri/src/main.rs`; regenerate `bindings.ts`.

**Interfaces:** IPC `output_channels() -> Vec<String>`, `output_read(channel) -> Result<String>`, `output_clear(channel) -> Result<()>`. `main.rs`: the workspace is the first argument that is not `--log-dir` or its value; `--log-dir <path>` else `SKELE_LOG_DIR` → `Builder::log_dir`.

- [ ] **Step 1:** Implement; `cargo build -p tauri-plugin-skele && bun run gen:bindings` — bindings gain the three commands and the new events, no `terminalData`.
- [ ] **Step 2:** `bun run gen:bindings` again → no diff.
- [ ] **Step 3: Commit** `tauri: output channel IPC; --log-dir flag`.

### Task 5: Output panel with channel picker

**Files:** Create `apps/workbench/src/renderers/output.ts`, `output.test.ts`; modify `renderers/terminal.ts` (`createOutput` returns `{ element, write, reset }`), `main.ts`, `styles.css`.

**Interfaces:** `createOutputPanel(host: { channels(): Promise<string[]>; read(c: string): Promise<string>; clear(c: string): Promise<void> }, view: { element: HTMLElement; write(s: string): void; reset(): void }, storage?: Storage): OutputPanel` with `element`, `append(channel, text)`, `channelsChanged(): Promise<void>`, `cleared(channel)`. Selection key `skele.output.channel` in `localStorage`.

- [ ] **Step 1: Failing tests** (fake `host`, `view`, in-memory `Storage`): `lists_channels_and_shows_workbench`; `switching_replays_then_follows` (select `Hello` → `view.reset` then `write(read("Hello"))`; `append("Hello", "x")` writes, `append("Other", "y")` doesn't); `selection_survives_reload` (stored `Hello` → initial read of Hello); `missing_stored_channel_falls_back`; `clear_button_clears_selected` (calls `host.clear("Hello")`; `cleared("Hello")` → `view.reset()`; later append shows).
- [ ] **Step 2:** `bun test` — FAIL. **Step 3:** Implement. **Step 4:** PASS; `bun run typecheck` clean.
- [ ] **Step 5:** Wire `main.ts`: `outputAppended` → `panel.append`, `outputChannelsChanged` → `panel.channelsChanged()`, `outputCleared` → `panel.cleared`.
- [ ] **Step 6: Commit** `workbench: Output panel channel picker`.

### Task 6: Docs and end-to-end

- [ ] README: replace the stderr note in Plugins with "Output channels" (sources, 10,000 lines, picker, log files: flag, env, both settings, layout).
- [ ] Xvfb (`env -u WAYLAND_DISPLAY GDK_BACKEND=x11 DISPLAY=:99`): picker lists Workbench, Hello, Runtime Info; Greet writes to Hello only; switching replays; Clear empties; reload keeps selection; `SKELE_LOG_DIR=<tmp>` produces `<tmp>/<stamp>/Hello.log` that grows on Greet.
- [ ] Full checks; commit `docs: output channels`.
