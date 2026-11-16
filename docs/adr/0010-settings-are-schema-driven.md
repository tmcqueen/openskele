# ADR-0010: Settings are schema-driven with user and workspace scopes

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

The core and every plugin need settings, and the settings UI must work for
plugins the workbench has never seen. Values come from hand-editable files,
so they can be wrong. Some settings belong to the person (theme, font), some
to a project (shell, label text), as in VS Code.

## Decision

- Every setting is declared with a JSON Schema: the core registers its own
  sections, and plugins declare `contributes.configuration.properties`. The
  schema's `default` is the default value.
- Values are validated in Rust on write (`jsonschema`); invalid writes are
  rejected with the validation message. Keys must be unique across sections.
- Two scopes: user (`settings.json` in the app config directory) and
  workspace (`.skele/settings.json`). The effective value is workspace, else
  user, else default. Stored values that no longer validate are skipped
  rather than handed to consumers. Files are written atomically.
- The settings pane is generated from the schemas: a control per schema type
  (enum → select, boolean → checkbox, number/integer → number input, string →
  text, anything else → JSON). Changes notify running plugins
  (`config/didChange`) and the webview.

## Options Considered

### Option A: JSON Schema per key, validated in the core (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium |
| Cost | One validator per key, compiled at registration |
| Scalability | Any plugin gets a settings UI for free |
| Team familiarity | Same model as VS Code |

**Pros:** one source for UI, defaults and validation; plugins never see
invalid values.
**Cons:** the generated form only covers common schema shapes well.

### Option B: Plugins render their own settings UI
**Pros:** full control for complex settings.
**Cons:** HTML from plugins (against ADR-0007); no shared validation.

### Option C: Untyped key-value store
**Pros:** trivial.
**Cons:** no defaults, validation or generated UI.

## Trade-off Analysis

Schemas give generated UI, defaults and validation from one declaration,
which is what an extensible workbench needs most.

## Consequences

- Easier: adding a setting is one schema entry.
- Harder: rich editors for objects and arrays (today a JSON text area).
- Revisit: settings search, and per-language or per-folder scopes.

## Action Items
1. [x] `ConfigStore` with scopes, validation and atomic writes.
2. [x] Generated settings pane with user and workspace tabs.
