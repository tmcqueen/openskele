# ADR-0009: The core ships as a Tauri plugin with a locked-down webview

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

The core (`skele-core`) is headless and must stay testable with
`cargo test`. A desktop shell needs it inside a Tauri 2 app, with a webview
that only renders (ADR-0001). The webview runs third-party content indirectly
(plugin view models, agent markdown, documents), so it should be able to do
as little as possible on its own. The IPC between webview and core should be
typed on both sides.

## Decision

- `crates/tauri-plugin-skele` wraps the core as a Tauri plugin. The app
  (`apps/workbench/src-tauri`) is a few lines of wiring.
- IPC commands are declared with `#[tauri::command]` + `#[specta::specta]`
  and collected by `tauri-specta`, which exports
  `apps/workbench/src/bindings.ts` (`bun run gen:bindings`). Streams (host
  events, PTY output) use `tauri::ipc::Channel`.
- The webview's only capability is `skele:default`, which lists the plugin's
  own commands. Window controls for the custom title bar (ADR-0011) are
  plugin commands acting on the calling window, not `core:window`
  permissions.
- The specta builder is fixed to the Wry runtime, because commands that take
  the calling `Webview` cannot be collected generically.

## Options Considered

### Option A: Tauri plugin + tauri-specta + one capability (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low |
| Cost | Each command listed in `build.rs`, permissions and the builder |
| Scalability | Any Tauri app can embed the workbench |
| Team familiarity | Standard Tauri pattern |

**Pros:** typed bindings; the core stays Tauri-free; minimal webview rights.
**Cons:** three places to register a command; specta pinned to an RC.

### Option B: Commands in the app crate, untyped `invoke`
**Pros:** fewer files.
**Cons:** no typed bindings; the core's surface is tied to one app.

### Option C: Grant Tauri's core permissions (window, fs, shell) to the webview
**Pros:** less Rust code for window controls.
**Cons:** the webview gains abilities it does not need; every new ability is
a capability change rather than a reviewed command.

## Trade-off Analysis

The extra registration step is mechanical; typed IPC and a webview that can
only call reviewed workbench commands are worth it.

## Consequences

- Easier: front-end changes against generated types; auditing what the
  webview can do.
- Harder: adding a command touches `build.rs`, `permissions/default.toml`
  and `collect_commands!`.
- Revisit: when tauri-specta and specta leave release candidate status.

## Action Items
1. [x] Plugin crate, bindings export and `skele:default` capability.
2. [x] Window-control commands for the custom title bar.
