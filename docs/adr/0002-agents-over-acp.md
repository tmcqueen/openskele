# ADR-0002: Agents connect over the Agent Client Protocol

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

The workbench needs an agent pane that is not tied to one vendor and that end
users can extend. Agents that already exist (Claude Code, Gemini CLI, Codex)
run as their own processes. The Agent Client Protocol (ACP) is JSON-RPC over
stdio between an editor (client) and an agent; Gemini CLI speaks it natively,
and Claude Code and Codex speak it through adapters. It covers sessions,
streamed updates (text, thoughts, tool calls, plans), permission requests and
client-side file access.

## Decision

The workbench is an ACP client, built on the `agent-client-protocol` crate.
An agent is any ACP executable, declared by `contributes.agents` in a plugin
manifest (`id`, `name`, `command`, `args`, `env`); `main` becomes optional.
The transcript is kept as data in Rust and rendered by a generic chat
renderer.

## Options Considered

### Option A: ACP client (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium: new crate, transcript model |
| Cost | No per-vendor code |
| Scalability | Any ACP agent is a manifest entry |
| Team familiarity | Same shape as the sidecar plugin host (JSON-RPC over stdio) |

**Pros:** vendor neutral; matches the plugin model; agents bring their own
models, auth and approval policies.
**Cons:** depends on a young protocol and crate; behavior varies by agent.

### Option B: Embed one vendor's SDK (e.g. Claude Agent SDK)
**Pros:** richest control of one agent.
**Cons:** locks in a vendor and a language runtime; every other agent needs
new code.

### Option C: Bespoke agent protocol
**Pros:** exactly fits the workbench.
**Cons:** no existing agent speaks it.

## Trade-off Analysis

Option A trades some control and protocol maturity for zero per-agent code,
which is the whole point of an extensible workbench.

## Consequences

- Easier: adding agents; testing with a scripted ACP agent.
- Harder: tools reach the agent only through MCP servers listed in
  `session/new` (ADR-0003); approval is the agent's job (ADR-0006).
- Revisit: crate API when 5.x changes; Claude adapter package name.

## Action Items
1. [ ] Spike `agent-client-protocol` against `plugins/agent-scripted`.
2. [ ] `agent/` module, transcript, IPC, chat renderer.
3. [ ] `plugins/agent-claude` manifest.
