# ADR-0004: Plugins contribute tools through the plugin protocol

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

End users extend what the agent can do by writing plugins, in any language.
Plugins already speak JSON-RPC to the host with generated SDKs for TypeScript
and .NET, are activated lazily, and declare contributions in a manifest.

## Decision

Add `contributes.tools` (`name`, `title?`, `description`, `inputSchema`,
`readOnly?`) to the manifest and a host→plugin request
`tool/call { name, arguments } → { content, isError }`. Plugin tools join the
same registry as built-ins (`ToolTarget::Plugin(id)`); calling one activates
the plugin. Names match `^[a-z][a-z0-9_]*$` and must be unique. The protocol
goes to `0.2.0`.

## Options Considered

### Option A: `tool/call` in the plugin protocol (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low: one method, one manifest field, SDK helpers |
| Cost | Protocol minor bump; samples updated |
| Scalability | One registry for listing, validation and approval |
| Team familiarity | Same as `command/execute` |

**Pros:** reuses activation, lifecycle and SDKs; tools visible without
starting the plugin; one place to validate arguments.
**Cons:** plugins cannot use MCP-only features (resources, prompts).

### Option B: Plugins run their own MCP servers, listed in `session/new`
**Pros:** standard MCP, nothing new in the protocol.
**Cons:** every plugin needs an MCP library; no shared validation, naming or
future host-side approval; tools cannot be listed without starting plugins.

## Trade-off Analysis

Option A keeps the host in charge of every tool call, which ADR-0006 and
external access will need.

## Consequences

- Easier: writing a tool in Bun or .NET is a handler registration.
- Harder: 0.1 plugins stop loading until their manifest says 0.2.
- Revisit: MCP resources and prompts, if wanted.

## Action Items
1. [ ] `ToolCall` request and manifest field in `skele-protocol`; `gen:protocol`.
2. [ ] `host.tools.register` (TS) and `Tools.Register` (.NET).
3. [ ] One sample tool per sample plugin.
