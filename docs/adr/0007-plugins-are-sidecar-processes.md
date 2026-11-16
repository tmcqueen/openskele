# ADR-0007: Plugins are sidecar processes speaking JSON-RPC over stdio

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

openskele is meant to be extended by people who write in whatever language
they like. A plugin has to be able to crash, hang or leak without taking the
workbench with it, and the host has to know what a plugin offers (commands,
menus, keybindings, views, settings) without running any of its code, so the
UI can be drawn at startup and plugins started only when needed. Language
Server Protocol and Debug Adapter Protocol show that a process boundary plus
JSON-RPC scales to many implementations in many languages.

## Decision

- A plugin is a directory with a `skele-plugin.json` manifest and an
  executable. The host starts it as a child process and speaks JSON-RPC 2.0
  on its stdin/stdout with LSP-style `Content-Length` framing. stderr and any
  stray stdout lines are the plugin's log (ADR-0012).
- The manifest declares everything the plugin `contributes`. Installing a
  plugin registers those contributions and starts nothing.
- Activation is lazy: the process starts the first time one of its commands
  runs or one of its views is shown, or at startup if it lists `onStartup`.
  A plugin that exits is marked failed and restarted the next time it is needed.
- The connection opens with a versioned `initialize` handshake; host and
  plugin must agree on the protocol version (ADR-0008).
- Plugins describe UI as data (view models such as `TreeItem`), never markup.
  The webview builds DOM from them without parsing HTML.

## Options Considered

### Option A: Sidecar processes over stdio JSON-RPC (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium: process lifecycle, framing, request correlation |
| Cost | One process per active plugin |
| Scalability | Any language with a JSON library; crashes are contained |
| Team familiarity | Same model as LSP and DAP |

**Pros:** language-neutral; isolated failures; contributions visible without
starting code; easy to test with a real process.
**Cons:** per-call IPC latency; per-process memory; a slow plugin is still
slow, just not fatal.

### Option B: In-process native plugins (dynamic libraries)
| Dimension | Assessment |
|-----------|------------|
| Complexity | High: unstable Rust ABI, unloading, versioning |
| Cost | Low runtime cost |
| Scalability | Rust (or C ABI) only |
| Team familiarity | Low |

**Pros:** fastest calls; shared memory.
**Cons:** a plugin crash takes down the host; no language freedom.

### Option C: WASM components now (wasmtime + WIT)
| Dimension | Assessment |
|-----------|------------|
| Complexity | High: WIT interfaces, host bindings, toolchains per language |
| Cost | Large up-front investment |
| Scalability | Sandboxed, in-process, good isolation |
| Team familiarity | Low |

**Pros:** real sandboxing; one process.
**Cons:** immature toolchains for several target languages; no access to
the ecosystems plugins usually need (process spawning, native libraries).

## Trade-off Analysis

Process isolation and language freedom matter more for an extensible
workbench than call latency, which is local IPC and small next to what
plugins do. WASM components stay attractive for sandboxed, untrusted
plugins and remain a later option alongside sidecars, not a replacement.

## Consequences

- Easier: writing a plugin in TypeScript, C# or anything else; testing the
  host against real plugin processes.
- Harder: every plugin capability needs a protocol method; processes must be
  shut down cleanly on exit.
- Revisit: WASM components for sandboxed in-process plugins.

## Known deviations

- [#4](https://github.com/tmcqueen/openskele/issues/4): the webview fetches
  every container's views when it builds them, which starts plugins whose
  views were never shown. Fixed by
  `docs/superpowers/plans/2026-11-16-post-m3-review-fixes.md`.

## Action Items
1. [x] Plugin host with framing, request correlation and lazy activation
   (`crates/skele-core/src/plugin/`).
2. [x] Sample plugins in Bun and .NET with end-to-end tests.
3. [ ] Load views only when their container is shown (#4).
