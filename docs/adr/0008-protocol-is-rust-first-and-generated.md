# ADR-0008: The plugin protocol is defined in Rust and everything else is generated

**Status:** Accepted
**Date:** 2026-11-15
**Deciders:** Tim McQueen

## Context

The plugin protocol (ADR-0007) is used by the Rust host and by SDKs in
several languages. It has to be schema-first in the sense that there is one
machine-readable description of every method and payload, and versioned so a
host can refuse a plugin it cannot talk to. The SDKs should be thin and cheap
to add for a new language. Off-the-shelf client generators target
request/response HTTP APIs; this protocol is bidirectional (both sides send
requests and notifications on one stdio pipe).

## Decision

- The Rust types in `skele-protocol` are the source of truth. Each method is
  a type tying its name, direction, params and result together.
- `cargo run -p skele-protocol --bin gen-protocol` writes
  `protocol/openrpc.json` (with `x-direction` and `x-kind` extensions) and
  `protocol/plugin-manifest.schema.json`. A test fails if the checked-in
  documents are stale.
- `scripts/gen-sdk.ts` reads the OpenRPC document through a shared model
  (`scripts/gen/model.ts`) and runs one emitter per language. Only the
  protocol types and method tables are generated.
- Each SDK adds a small hand-written runtime (framing, dispatch, id
  correlation): TypeScript in `sdk/typescript`, .NET 10 in `sdk/dotnet` using
  System.Text.Json source generation and no package dependencies.
- The protocol version follows `0.minor`: adding methods or manifest fields
  bumps the minor version, and while the major is 0 host and plugin must
  match on the minor.

## Options Considered

### Option A: Rust-first types, generated documents and SDK types (chosen)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low: derives plus a small emitter per language |
| Cost | About 150 lines of runtime per SDK |
| Scalability | New language = one emitter + one runtime |
| Team familiarity | High: the host is Rust |

**Pros:** host types cannot drift from the schema; generated output is
checked by tests; SDKs have no runtime dependencies.
**Cons:** the schema is shaped by what `schemars` emits; emitters handle
only the JSON Schema subset the protocol uses.

### Option B: Hand-written OpenRPC/JSON Schema, Rust generated from it (typify)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Medium |
| Cost | Less control over Rust types (derives, specta, docs) |
| Scalability | Same as A |
| Team familiarity | Medium |

**Pros:** truly schema-first.
**Cons:** the Rust types lose hand-tuned derives and attributes the host and
Tauri bindings need.

### Option C: Off-the-shelf client generators (Kiota, OpenRPC Generator)
| Dimension | Assessment |
|-----------|------------|
| Complexity | Low to start |
| Cost | Generated runtimes and dependencies per language |
| Scalability | Poor fit |
| Team familiarity | Medium |

**Pros:** no emitter code to maintain.
**Cons:** Kiota targets OpenAPI/HTTP and has no Rust target; OpenRPC
Generator assumes a one-way client over HTTP or WebSocket. Neither models a
plugin that both answers and sends requests on one pipe, so their runtimes
would be discarded anyway.

## Trade-off Analysis

The generated part (types) is easy and the valuable part (bidirectional
dispatch) is what generic tools get wrong, so generating types and
hand-writing a tiny runtime gives the most correct SDKs for the least code.

## Consequences

- Easier: adding a method (edit Rust, run `gen:protocol`); adding a language.
- Harder: emitters must grow if the protocol starts using `oneOf`, recursive
  types or `allOf`; swap in quicktype or a language's best generator then.
- Revisit: emitter choice when the schema subset grows.

## Action Items
1. [x] `gen-protocol` binary and staleness test.
2. [x] TypeScript and C# emitters over a shared model.
3. [x] TypeScript and .NET runtimes with sample plugins.
