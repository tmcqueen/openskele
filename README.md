# openskele

A skeleton for an extensible workbench shell: Rust owns state and behavior, a
Tauri 2 webview renders it, and plugins are sidecar processes speaking
JSON-RPC 2.0 over stdio.

## Layout

| Path | What it is |
| --- | --- |
| `crates/skele-protocol` | Wire types: JSON-RPC envelope, protocol methods, plugin manifest, view models. Source of the schema documents. |
| `crates/skele-core` | Headless core, no Tauri: command/menu/keybinding/view registries, config store, event bus, plugin host, PTY terminals (portable-pty), document buffers (ropey). |
| `crates/tauri-plugin-skele` | The core packaged as a Tauri plugin: typed commands (tauri-specta), `Channel` streaming, permissions, native menu built from the menu registry. |
| `apps/workbench` | The app. `src-tauri` is a few lines of wiring; `src` is the generic renderer (Dockview layout; tree, list, form, xterm.js terminal, Monaco editor). |
| `protocol/` | Generated `openrpc.json` and `plugin-manifest.schema.json`. |
| `sdk/typescript` | Thin plugin SDK; its protocol types are generated from `protocol/openrpc.json`. |
| `plugins/sample-bun` | Sample plugin in Bun/TypeScript. |

## Running

Needs Rust, Bun, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/)
(on Debian/Ubuntu: `libwebkit2gtk-4.1-dev`).

```sh
bun install
bun run dev        # the workbench, with plugins/ loaded and the repo as workspace
bun run test       # cargo test --workspace (the end-to-end test needs bun on PATH)
bun run typecheck
```

Without a webview, the core runs headless:

```sh
cargo run -p skele-core --example headless -- plugins hello.greet Ferris
```

Plugins are loaded from `plugins/` in development builds, from `plugins/` under
the app data directory, and from `SKELE_PLUGINS_DIR`. The workspace folder is
the first CLI argument or `SKELE_WORKSPACE`.

## Plugins

A plugin is a directory with a `skele-plugin.json` manifest. Installing it
registers what it `contributes` (commands, menus, keybindings, views,
configuration schema) without starting it. Its process starts the first time
one of its commands runs or one of its views is shown, or at startup if it
lists the `onStartup` activation event, and is restarted on demand if it exits.

The protocol is JSON-RPC 2.0 with LSP-style `Content-Length` framing on
stdin/stdout; stderr is the plugin's log. It opens with a versioned
`initialize` handshake.

| Direction | Methods |
| --- | --- |
| host → plugin | `initialize`, `shutdown`, `exit`, `command/execute`, `view/getChildren`, `config/didChange`, `document/didSave` |
| plugin → host | `window/showMessage`, `terminal/write`, `config/get`, `view/refresh` |

Plugins return view models (`TreeItem`s), never markup; the webview builds DOM
from them without parsing HTML.

### Changing the protocol

The Rust types in `skele-protocol` are the source. After editing them:

```sh
bun run gen:protocol   # protocol/*.json, then sdk/typescript/src/generated
bun run gen:bindings   # apps/workbench/src/bindings.ts, after changing IPC commands
```

A test fails if `protocol/` is stale.

## Settings

Each `contributes.configuration.properties` entry is a JSON Schema. The
settings pane is generated from those schemas. Values are validated in Rust on
write and stored per scope: user (`settings.json` in the app config directory)
and workspace (`.skele/settings.json`). The effective value is workspace, then
user, then the schema's `default`.

## Not built yet

- Webview panels as a plugin escape hatch, and `list`/`form` view kinds from plugins.
- `when` clauses for menus and keybindings; nested submenus.
- Plugin-to-host command execution, document edits from plugins.
- WASM component plugins (wasmtime + WIT) as a sandboxed in-process alternative
  to sidecars. `CommandTarget` and the view provider lookup are the seams.
- SDKs for other languages; any JSON-RPC client can implement `protocol/openrpc.json`.
