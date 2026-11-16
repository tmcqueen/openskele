# ADR-0005: App bars are built on Dockview edge groups

**Status:** Accepted (spike confirmed 2026-11-15)
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

## Spike outcome (dockview-core 8.9.0)

Confirmed in a throwaway page served by Vite:

- `api.addEdgeGroup(position, { id, initialSize, collapsedSize })` with
  `position` `'left' | 'right' | 'top'` returns the group API. Panels go in
  with `addPanel({ ..., tabComponent: "icon", position: { referenceGroup: group.id } })`.
- An edge group draws its tab strip on the outer side automatically: vertical
  for left/right, horizontal for top. A `createTabComponent` returning a
  40×40 element with `codicon codicon-<name>` gives an icon-only strip.
- `group.collapse()` / `group.expand()` collapse to `collapsedSize` (44 px),
  leaving the icon strip visible: the "no active container" state.
  `api.setEdgeGroupVisible(position, bool)` hides the whole bar: the
  "bar toggled off" state.
- Moving a bar: close its panels, `api.removeEdgeGroup(old)`, add the group
  and panels at the new position. Works live.
- **Only one edge group per position**: a second `addEdgeGroup` at an
  occupied position throws `edge group already exists at position '<p>'`.
- Icon tabs are ordinary panels and can be dragged out of a bar; the
  workbench sets `disableDnd: true` (the Rust layout does not model
  drag-and-drop rearrangement; ADR-0001).

**Ruling for shared sides.** The settings allow both bars on the same side,
but Dockview allows one edge group per side. When both resolve to the same
position, the secondary bar is drawn on the nearest free edge (left → right,
right → left, top → right) and a one-time warning toast says so. The Rust
layout keeps the requested positions.

## Action Items
1. [x] Spike edge group with icon tabs in `apps/workbench`.
2. [x] Record outcome here.
