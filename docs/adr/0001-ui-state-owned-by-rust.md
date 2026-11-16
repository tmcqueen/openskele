# ADR-0001: UI and layout state is owned by the Rust core

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

Today the webview owns the layout. Panels exist only in Dockview, and the
core asks for one by emitting `HostEvent::OpenPanel`
(`crates/skele-core/src/events.rs`), with no way to read back what is open.
Terminals are opened by the webview (`terminal_open` IPC) and die on page
reload. An agent has to be able to read and change the UI (ADR-0002,
ADR-0003). It can only do that through the core, and it has to work in
headless tests where no webview exists.

## Decision

Move the layout into `skele-core`: a `layout.rs` model of both bars
(position, visibility, active container) and the open panels (settings,
editors, terminals), plus a `containers.rs` registry. Every change goes
through core methods and emits `HostEvent::LayoutChanged`. The webview
reconciles Dockview to that model and sends user gestures back as IPC calls.
PTYs are created in the core and the webview attaches to them.

## Options Considered

### Option A: Rust-owned layout (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium: a reconcile loop in `main.ts`, gestures go round-trip |
| Cost | One milestone of work before any agent code |
| Scalability | Any number of front ends and agents see one state |
| Team familiarity | Same pattern as the existing registries and `Snapshot` |

**Pros:** one source of truth; testable with `cargo test`; agent tools are
thin wrappers; terminals survive reloads.
**Cons:** UI gestures take an IPC round trip; Dockview's own state has to
be kept in step.

### Option B: Webview-owned layout, queried over IPC
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low now, high later (request/response into the webview) |
| Cost | Cheap to start |
| Scalability | Breaks with zero or several webviews |
| Team familiarity | Current code |

**Pros:** no reconcile loop.
**Cons:** agent tools fail when no webview is attached; two sources of
truth; untestable headless.

## Trade-off Analysis

The round-trip latency of Option A is local IPC and imperceptible. Option B
puts agent reliability at the mercy of the webview's life cycle and cannot be
covered by the Rust integration tests that already guard plugins.

## Consequences

- Easier: layout tools for the agent, headless tests, page reloads.
- Harder: every UI gesture must go through IPC; Dockview drag-and-drop
  rearrangements need mapping back to the model (or are left as view-only).
- Revisit: layout persistence across restarts (not in this round).

## Action Items
1. [ ] `containers.rs`, `layout.rs`, `LayoutChanged`; remove `OpenPanel`.
2. [ ] Core-owned terminals with scrollback and `terminal_attach`.
3. [ ] `reconcile()` in `apps/workbench/src/main.ts`.
