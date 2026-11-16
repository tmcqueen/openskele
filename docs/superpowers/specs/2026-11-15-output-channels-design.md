# Output channels: design

Date: 2026-11-15. Small change; short design.

## Intent

Debugging plugins (and, from Milestone 3, agents) needs their diagnostic
output kept per source, like VS Code's Output panel. Today every source is
mixed into one view, the core keeps nothing, and stray stdout is thrown away.

## Goals

- One output channel per source, each a bounded in-memory buffer of lines kept
  in the core:
  - one per plugin, fed by its stderr, its stray (non-protocol) stdout lines and
    `terminal/write`; a `terminal` name in `terminal/write` selects or creates
    another channel;
  - `Workbench` for the host's own messages (`window/showMessage` and host
    errors are also logged there).
- The Output panel has a dropdown of channels and a Clear button. Switching
  replays the channel's buffer, then follows new lines. The selected channel
  survives a page reload.
- Optional log files, written only when **either**:
  - the app is started with `--log-dir <path>` (or `SKELE_LOG_DIR`), or
  - settings `output.logToFile` is `true` **and** `output.logDirectory` is not
    empty.

## Non-goals

- Pruning old log folders (the directory is the user's).
- Log levels or filtering inside a channel.

## Approaches

- **Channels in the core (chosen).** Same pattern as core-owned terminals:
  events for new text, a call to read a buffer. Survives reloads; M2 agent
  tools can read it (`output_read`).
- Channels in the webview only: loses history on reload and is invisible to
  the agent tools (ADR-0001). Rejected.

## Design

- `crates/skele-core/src/output.rs`: `OutputStore` with `append(channel, text)`,
  `channels() -> Vec<String>` (Workbench first, then by name), `read(channel)
  -> String`, `clear(channel)`. Buffer: last **10,000 lines** per channel.
- `HostEvent::TerminalData` is renamed `OutputAppended { channel, text }`;
  `HostEvent::OutputChannelsChanged` when a channel is created.
- Stray stdout: `plugin/transport.rs` hands non-protocol lines to a callback
  instead of dropping them; they go to the plugin's channel.
- Log files: `<dir>/<YYYYMMDD-HHMMSS>/<channel>.log` per run, appended and
  flushed per write so `tail -f` works; the channel's existing buffer is
  written when a file is first opened, so turning logging on mid-run loses
  nothing. Text is written as received (ANSI escapes included).
  - The startup flag wins over settings. Settings apply live: turning
    `output.logToFile` on or changing `output.logDirectory` opens files in a
    new run folder; turning it off closes them.
  - A relative `output.logDirectory` resolves against the workspace folder; with
    no workspace it is ignored and a warning goes to the Workbench channel. A
    directory that cannot be created gives one warning and no files.
- IPC: `output_channels()`, `output_read(channel)`, `output_clear(channel)`.
- Webview: the Output panel gets a header row (channel `<select>`, Clear
  button) above its xterm; the selection is kept in `localStorage`.

## Left to the builder

Flag parsing location (`apps/workbench/src-tauri/src/main.rs` beside the
workspace argument), file-name sanitising for channel names, exact header styling.
