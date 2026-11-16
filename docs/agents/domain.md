# Domain Docs

How engineering skills consume this repository's domain documentation.

## Before exploring, read these

- `GLOSSARY.md` at the repository root.
- ADRs in `docs/adr/` relevant to the area being explored.

If these files do not exist, proceed silently. Do not flag their absence or suggest creating them upfront. `/domain-modeling` (also reached through `/grill-with-docs` and `/improve-codebase-architecture`) creates them lazily when terms or decisions are resolved.

## File structure

This repository uses a single-context layout across its workspaces:

- `GLOSSARY.md`: shared domain vocabulary.
- `docs/adr/`: architectural decisions.

## Use the glossary's vocabulary

Use domain terms as defined in `GLOSSARY.md` in issue titles, refactor proposals, hypotheses, and test names. Avoid synonyms the glossary explicitly excludes.

If a needed concept is missing, reconsider whether it belongs to the domain or note the gap for `/domain-modeling`.

## Flag ADR conflicts

Surface contradictions with existing ADRs explicitly instead of silently overriding them. Identify the conflicting ADR and explain why it may need to be reopened.
