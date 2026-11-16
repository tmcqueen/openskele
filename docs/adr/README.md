# Architecture decision records

| ADR | Decision | Status |
| --- | --- | --- |
| [0001](0001-ui-state-owned-by-rust.md) | UI and layout state is owned by the Rust core | Accepted |
| [0002](0002-agents-over-acp.md) | Agents connect over the Agent Client Protocol | Accepted |
| [0003](0003-private-mcp-server-via-bridge.md) | Tools reach agents through a private MCP server and a stdio bridge | Accepted |
| [0004](0004-plugin-tools-via-protocol.md) | Plugins contribute tools through the plugin protocol | Accepted |
| [0005](0005-app-bars-on-dockview-edge-groups.md) | App bars are built on Dockview edge groups | Accepted |
| [0006](0006-tool-approval-delegated-to-agent.md) | Tool approval is left to the agent for now | Accepted (known gap) |
| [0007](0007-plugins-are-sidecar-processes.md) | Plugins are sidecar processes speaking JSON-RPC over stdio | Accepted |
| [0008](0008-protocol-is-rust-first-and-generated.md) | The plugin protocol is defined in Rust and everything else is generated | Accepted |
| [0009](0009-core-ships-as-a-tauri-plugin.md) | The core ships as a Tauri plugin with a locked-down webview | Accepted |
| [0010](0010-settings-are-schema-driven.md) | Settings are schema-driven with user and workspace scopes | Accepted |
| [0011](0011-custom-title-bar-chosen-at-window-creation.md) | Custom title bar by default, with the frame chosen at window creation | Accepted |
| [0012](0012-output-channels-in-the-core.md) | Output channels live in the core | Accepted |

New ADRs take the next number. Superseding one: add a new ADR, set the old
one's status to "Superseded by ADR-NNNN".
