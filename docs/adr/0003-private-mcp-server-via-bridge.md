# ADR-0003: Tools reach agents through a private MCP server and a stdio bridge

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

ACP has no tool channel of its own: a client gives an agent tools by listing
MCP servers in `session/new`, and every ACP agent must accept stdio MCP
servers (HTTP is optional). The workbench must expose its tool registry to
agents it starts, and to nothing else for now. A later wish is that outside
MCP clients (a `claude` started in a terminal, Claude Desktop, Cursor) can
drive the workbench too; that should not need a rework.

## Decision

- An MCP service in `skele-core` (`rmcp` crate) serves the tool registry. It
  accepts any `AsyncRead + AsyncWrite` and a `Grant`; it has no transport.
- The host listens on a local socket (`interprocess`: Unix socket, mode 0600;
  named pipe on Windows). No TCP port.
- A small binary, `skele-mcp-bridge`, is what the agent launches as its stdio
  MCP server. It reads the socket path and a token from its environment,
  sends the token, then pipes stdio.
- Tokens are **grants**, not ACP sessions: `Grant { id, label, token, kind:
  AgentSession | External, confirm_mutations, expires }`. Only
  `AgentSession` grants are issued now. Tools receive the grant and never
  reach for an ACP session.

## Options Considered

### Option A: Private socket + bridge + grants (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium: one extra binary, a grant store |
| Cost | Bridge must be packaged beside the app for release builds |
| Scalability | Several agents, each with its own grant |
| Team familiarity | Same stdio model as plugins |

**Pros:** no network exposure; works with every ACP agent; outside access
later is a discovery file plus `External` grants.
**Cons:** extra binary to ship; local-socket differences per OS.

### Option B: Public MCP endpoint now (stdio bridge or HTTP)
**Pros:** outside clients work from day one.
**Cons:** needs a host-side approval gate first (ADR-0006); HTTP adds a port
and its auth questions.

### Option C: Each agent gets an in-process stdio MCP server per session
**Pros:** no socket.
**Cons:** the agent launches the MCP server, so it cannot be the host
process; a bridge of some kind is needed anyway.

## Trade-off Analysis

Option A costs one tiny binary and keeps the door open: the three seams
(transport-free service, grants instead of sessions, tools that only see
grants) make external access an additive change rather than a rework.

## Consequences

- Easier: adding external access later: write a 0600 discovery file with
  socket path and an `External` token, add a "Copy MCP config" command.
- Harder: release packaging (Tauri external binary).
- Revisit: external access, together with ADR-0006; HTTP only if a specific
  client needs it.

## Action Items
1. [x] Spike `rmcp` server over a duplex stream.
2. [x] `mcp/service.rs`, `mcp/socket.rs`, `GrantStore`.
3. [x] `crates/skele-mcp-bridge` and its integration test (incl. bad token).
