// The two app bars, drawn as Dockview edge groups: an icon tab per view
// container, and the active container's views beside it. The bars only show
// what the host's layout says; clicks go back to the host as requests.

import type { DockviewApi, ITabRenderer } from "dockview-core/dist/dockview-core.js";

import { h } from "./dom";
import type { Bar, BarId, BarPosition, ContainerInfo, Layout, Snapshot } from "./host";
import { toast } from "./toast";

/** Width (or height, for a top bar) of the icon strip a collapsed bar keeps. */
const STRIP = 44;
const PANEL_SIZE = 280;

export const containerPanelId = (id: string) => `container:${id}`;

export interface Bars {
  /** Makes both edge groups match the layout. Never calls the host. */
  render(snapshot: Snapshot, layout: Layout): void;
  /** Tab renderer for the icons; Dockview asks for it by the name `icon`. */
  createIconTab(): ITabRenderer;
}

interface Drawn {
  position: BarPosition;
  /** Container ids and their view ids; a change means the group is rebuilt. */
  signature: string;
}

/** Edge a bar is drawn at: Dockview allows one edge group per side. */
function placement(primary: BarPosition, secondary: BarPosition): Record<BarId, BarPosition> {
  if (primary !== secondary) return { primary, secondary };
  const free: Record<BarPosition, BarPosition> = { left: "right", right: "left", top: "right" };
  return { primary, secondary: free[secondary] };
}

export function createBars(dock: DockviewApi, onSelect: (bar: BarId, id: string | null) => void): Bars {
  const drawn = new Map<BarId, Drawn>();
  let current: Layout | undefined;
  let warnedShared = false;

  function containersOf(snapshot: Snapshot, bar: BarId): ContainerInfo[] {
    // Only containers with something in them get an icon.
    return snapshot.containers.filter(
      (c) => c.bar === bar && snapshot.views.some((v) => v.container === c.id),
    );
  }

  function signature(snapshot: Snapshot, containers: ContainerInfo[]): string {
    return JSON.stringify(
      containers.map((c) => [c.id, c.icon, c.title, snapshot.views.filter((v) => v.container === c.id).map((v) => v.id)]),
    );
  }

  function remove(bar: BarId): void {
    const was = drawn.get(bar);
    if (!was) return;
    for (const panel of dock.panels.filter((p) => p.params?.bar === bar)) panel.api.close();
    if (dock.getEdgeGroup(was.position)) dock.removeEdgeGroup(was.position);
    drawn.delete(bar);
  }

  function build(bar: BarId, position: BarPosition, containers: ContainerInfo[], sig: string): void {
    const group = dock.addEdgeGroup(position, { id: `bar:${bar}`, initialSize: PANEL_SIZE, collapsedSize: STRIP });
    for (const container of containers) {
      dock.addPanel({
        id: containerPanelId(container.id),
        component: "container",
        tabComponent: "icon",
        title: container.title,
        params: { bar, container: container.id, icon: container.icon, side: position },
        position: { referenceGroup: group.id },
      });
    }
    drawn.set(bar, { position, signature: sig });
  }

  function apply(bar: BarId, state: Bar, position: BarPosition): void {
    if (!drawn.has(bar)) return;
    dock.setEdgeGroupVisible(position, state.visible);
    const group = dock.getEdgeGroup(position);
    const panel = state.activeContainer ? dock.getPanel(containerPanelId(state.activeContainer)) : undefined;
    if (!panel) {
      group?.collapse();
      return;
    }
    group?.expand();
    panel.api.setActive();
  }

  return {
    render(snapshot, layout) {
      current = layout;
      const where = placement(layout.primary.position, layout.secondary.position);
      if (where.secondary !== layout.secondary.position && !warnedShared) {
        warnedShared = true;
        toast("warning", `Both side bars are set to ${layout.primary.position}; the secondary bar is shown on the ${where.secondary}.`);
      }

      const wanted = (["primary", "secondary"] as const).map((bar) => {
        const containers = containersOf(snapshot, bar);
        return { bar, containers, position: where[bar], sig: signature(snapshot, containers) };
      });
      // Moving one bar can free or take the other's edge, so any move rebuilds both.
      const moved = wanted.some(({ bar, position }) => drawn.has(bar) && drawn.get(bar)!.position !== position);
      // Remove everything that changes first: a bar can only be added at an
      // edge once the other bar has left it.
      for (const { bar, containers, sig } of wanted) {
        const was = drawn.get(bar);
        if (moved || !containers.length || (was && was.signature !== sig)) remove(bar);
      }
      const built: BarPosition[] = [];
      for (const { bar, containers, position, sig } of wanted) {
        if (containers.length && !drawn.has(bar)) {
          build(bar, position, containers, sig);
          built.push(position);
        }
      }
      // Adding an edge group squeezes the ones already there; restore their size.
      for (const position of built) {
        dock.getEdgeGroup(position)?.setSize(position === "top" ? { height: PANEL_SIZE } : { width: PANEL_SIZE });
      }
      for (const { bar, position } of wanted) apply(bar, layout[bar], position);
    },

    createIconTab() {
      const element = h("div", { class: "bar-icon" });
      let bar: BarId = "primary";
      let container = "";
      element.addEventListener("click", () => {
        // Clicking the open container collapses the bar's panel.
        const active = current?.[bar].activeContainer ?? null;
        onSelect(bar, active === container ? null : container);
      });
      return {
        element,
        init: ({ params, title }) => {
          ({ bar, container } = params as { bar: BarId; container: string });
          element.title = title ?? "";
          const { icon, side } = params as { icon: string; side: BarPosition };
          element.className = `bar-icon side-${side} codicon codicon-${icon}`;
        },
      };
    },
  };
}
