# ADR-0005: App bars are built on Dockview edge groups

**Status:** Accepted (pending spike)
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

The workbench lays out panels with `dockview-core` 8.9. The new primary and
secondary bars need an icon strip plus a collapsible, resizable side panel,
dockable left, right or top. Dockview 8.9 exposes `addEdgeGroup(position, …)`
(`dist/cjs/dockview/dockviewShell.d.ts`) for groups pinned to an edge.

## Decision

Render each bar as a Dockview edge group with a custom icon-only tab
renderer (one tab per view container, codicon icons). Start M1 with a spike;
if edge groups cannot draw an icon-only strip cleanly, fall back to two
hand-built flex regions around `#dock` with a plain icon strip.

## Options Considered

### Option A: Dockview edge groups (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low if the API fits |
| Cost | Spike to confirm |
| Scalability | Same docking as other panels |
| Team familiarity | Already using Dockview |

**Pros:** docking, collapse and resize come free; one layout system.
**Cons:** new, little-documented API; tab rendering may fight us.

### Option B: Hand-built flex regions
**Pros:** full control of the strip.
**Cons:** own resize/collapse code; two layout systems side by side.

## Trade-off Analysis

Try the cheaper path first, behind a short spike with a clear fallback.

## Consequences

- Easier: moving bars between edges is a Dockview call.
- Harder: keeping edge groups in step with the Rust layout (ADR-0001).
- Revisit: after the spike; update this ADR's status with the result.

## Action Items
1. [ ] Spike edge group with icon tabs in `apps/workbench`.
2. [ ] Record outcome here (Accepted or Superseded by fallback).
