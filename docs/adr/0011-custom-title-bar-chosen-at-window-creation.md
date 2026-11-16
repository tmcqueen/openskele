# ADR-0011: Custom title bar by default, with the frame chosen at window creation

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

The menu and window controls should sit in the title bar, as in VS Code, with
the system frame and native menu available for people who prefer them. The
first implementation removed decorations from an already-created window.
That works on X11 but not under Wayland (verified on KDE Plasma): the
compositor negotiates decorations once, so the system title bar stayed.

## Decision

- Setting `window.titleBarStyle`: `custom` (default) or `native`. It is read
  once at startup; changing it takes a restart.
- `custom`: the webview draws the menu (rendered from the same menu registry
  as the native one), a drag region and the window buttons. On macOS the
  system menu bar and traffic lights are kept and the window uses the
  overlay title bar style.
- `native`: the system frame and a native menu bar built from the registry.
- The frame is decided at creation: windows declared with `"create": false`
  in `tauri.conf.json` are created by the workbench plugin with the right
  decorations. Creation runs from a task spawned in plugin setup, because
  Tauri holds its plugin lock while setup runs and needs it to create a
  window.

## Options Considered

### Option A: Create the window with the right frame (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low |
| Cost | Apps must mark their windows `"create": false` |
| Scalability | Works on X11, Wayland, Windows, macOS |
| Team familiarity | Standard `WebviewWindowBuilder` |

**Pros:** correct on Wayland; one place decides the frame.
**Cons:** the setting needs a restart.

### Option B: Toggle decorations on a live window
**Pros:** could apply without restart.
**Cons:** ignored by Wayland compositors once negotiated.

### Option C: Native frame only
**Pros:** no custom chrome to maintain.
**Cons:** no menu or controls in the title bar.

## Trade-off Analysis

A restart for a rarely changed setting is a small cost against a title bar
that actually works on Wayland.

## Consequences

- Easier: menu and window controls share one strip; on Linux and Windows in
  custom mode the webview is the only thing bound to keyboard chords.
- Harder: three platform paths for the bar; Windows snap layouts on the
  hand-drawn maximize button are not handled.
- Revisit: dragging and edge resizing on Wayland, and macOS and Windows
  behavior, all untested.

## Action Items
1. [x] `window.titleBarStyle` setting and the plugin-created window.
2. [ ] Verify dragging and resizing on Wayland, macOS and Windows.
