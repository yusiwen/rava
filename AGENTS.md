# AGENTS.md — working rules for rava

Project-specific rules for an agent working in this repository. The global agent rules
still apply; where a repository rule conflicts with one of them, this file wins.

## Docs: one fact, one owner

| Fact | Owner |
| :--- | :--- |
| current state, next step | `PROJECT.md` |
| what a slice delivered, where it stops, its evidence | `docs/slices/` |
| what happened in a milestone, what it taught | `docs/notes/` |
| how a mechanism runs today | `docs/explanation/` |
| why this and not the alternative | `docs/adr/` |
| what is required | `docs/requirements.md` |

The format contracts live with their owner: `docs/slices/README.md`,
`docs/notes/README.md`, `docs/explanation/README.md`, `docs/adr/README.md`. The taxonomy
rule is in `docs/README.md`. This file only says where to write and what breaks the gate.

## Where to write

- A slice closes → three writes: its row in `PROJECT.md` § Slices; a `## <id>` entry in
  the milestone record under `docs/slices/`; `Resume` only if the milestone or the next
  slice changed.
- A milestone closes → one history note in `docs/notes/`.
- An accepted ADR is immutable; change your mind by superseding it with a new one.
- A fact already written elsewhere → link it, do not copy it. Two copies of a status
  disagree within a week (they did: four status lines contradicted each other in
  `PROJECT.md` before #63).

## Budgets

`PROJECT.md` ≤ 200 lines; no line over 200 characters; `## Resume` ≤ 25 lines; every
`done` slice has a `## <id>` entry under `docs/slices/`. Evidence belongs in the slice
record, never in a table cell. `scripts/doc-check.sh` is the source of truth for these
numbers; when it and this section disagree, fix this section.

## Gate

`bash scripts/doc-check.sh .` must exit 0 before a commit that touches documentation;
`cargo xtask ci` runs it first, so CI enforces it. `scripts/workflow-check.sh` is not
adopted yet — its pre-existing drift is tracked by #65.
