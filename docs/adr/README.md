# Architecture decision records

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-ui-state-owned-by-rust.md) | UI and layout state is owned by the Rust core | Accepted |
| [0002](0002-agents-over-acp.md) | Agents connect over the Agent Client Protocol | Accepted |
| [0003](0003-private-mcp-server-via-bridge.md) | Tools reach agents through a private MCP server and a stdio bridge | Accepted |
| [0004](0004-plugin-tools-via-protocol.md) | Plugins contribute tools through the plugin protocol | Accepted |
| [0005](0005-app-bars-on-dockview-edge-groups.md) | App bars are built on Dockview edge groups | Accepted (pending spike) |
| [0006](0006-tool-approval-delegated-to-agent.md) | Tool approval is left to the agent for now | Accepted (known gap) |

New ADRs take the next number. Superseding one: add a new ADR, set the old
one's status to "Superseded by ADR-NNNN".
