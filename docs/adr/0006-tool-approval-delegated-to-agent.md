# ADR-0006: Tool approval is left to the agent for now

**Status:** Accepted (known gap)
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

Agent tools can change settings, edit and save files, and type into
terminals. ACP agents ask the user before acting through
`session/request_permission`, and MCP tool annotations (`readOnlyHint`,
`destructiveHint`) let an agent's policy tell safe tools from risky ones. An
agent that ignores its own policy, or any future external MCP client
(ADR-0003), has no such prompt.

## Decision

In this round the host does not gate tool calls itself. It annotates every
tool as read-only or destructive and relies on the agent's ACP permission
flow, shown inline in the transcript. `Grant.confirm_mutations` exists but is
unread. A host-side confirm dialog is a required precondition for external
MCP access.

## Options Considered

### Option A: Rely on the agent (chosen)
**Pros:** no duplicate prompts for well-behaved agents; less UI now.
**Cons:** a misbehaving agent can mutate without asking.

### Option B: Host-side confirmation for mutating tools now
**Pros:** a second, agent-independent gate.
**Cons:** double prompts with agents that already ask; needs a modal path
that works while the agent pane streams.

## Trade-off Analysis

Agents started by the workbench are chosen by the user and run with the
user's own rights anyway (they can edit files directly). The gate matters
most when the caller is not one the user picked inside the workbench, which
is exactly the deferred external case.

## Consequences

- Easier: M2 and M3 ship sooner.
- Harder: nothing stops a non-asking agent.
- Revisit: before external MCP access; implement by reading
  `confirm_mutations` in the MCP service.

## Action Items
1. [x] Annotate every built-in tool correctly; test the annotations
   (`annotations_match_table` on `m2-tools-mcp`, `a05a35d`; lands with M2).
2. [ ] Follow-up: host-side confirmation, gated by `confirm_mutations`.
