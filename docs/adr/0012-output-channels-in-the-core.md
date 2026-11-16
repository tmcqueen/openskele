# ADR-0012: Output channels live in the core

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

Debugging plugins, and later agents, needs their diagnostic output kept per
source, as in VS Code's Output panel. Originally every source was mixed into
one view, the core kept nothing, stray plugin stdout was dropped, and
notifications vanished once dismissed. Agent tools also need to read this
output (ADR-0001, ADR-0003). Design:
`docs/superpowers/specs/2026-11-15-output-channels-design.md`.

## Decision

- `OutputStore` in the core holds one channel per source, each bounded to
  10,000 lines and 4 MiB.
- Each plugin gets a channel fed by its stderr, its stray stdout lines and
  `terminal/write`. A `terminal` name in `terminal/write` selects or creates
  another channel.
- The `Workbench` channel holds the host's own messages.
  `Workbench::notify` is the one way to tell the user something: it shows a
  transient message and appends it to the `Workbench` channel.
- Optional log files, one folder per run, are written when the app starts
  with `--log-dir` / `SKELE_LOG_DIR`, or when `output.logToFile` is on and
  `output.logDirectory` is set.
- The webview's Output panel picks a channel, replays its buffer and follows
  new text.

## Options Considered

### Option A: Channels in the core (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium |
| Cost | Memory bounded per channel |
| Scalability | Readable by the webview, tests and agent tools |
| Team familiarity | Same pattern as core-owned terminals |

**Pros:** survives reloads; agent tools can read it; one notification path.
**Cons:** ANSI escapes are stored as received.

### Option B: Channels in the webview only
**Pros:** no core changes.
**Cons:** history lost on reload; invisible to agent tools and tests.

## Trade-off Analysis

Keeping the buffers in the core costs a bounded amount of memory and makes
the output usable by everything that talks to the core.

## Consequences

- Easier: debugging plugins; agent tools `output_list` and `output_read`.
- Harder: every user-facing message must go through `Workbench::notify` to be
  retained.
- Revisit: log-folder pruning and per-channel levels or filtering.

## Known deviations

- [#7](https://github.com/tmcqueen/openskele/issues/7): failed commands run
  from the native menu use `bus().message` directly, and failures from the
  webview only show a toast, so neither reaches the `Workbench` channel or log
  files. Fixed by `docs/superpowers/plans/2026-11-16-post-m3-review-fixes.md`.

## Action Items
1. [x] `OutputStore`, plugin channels, log files and the Output panel.
2. [ ] Route every command error through `Workbench::notify` (#7).
