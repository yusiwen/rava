# Documentation

The index for `docs/`. Every document here has exactly one owner, and the owner is the only
place that fact lives — a second copy is a defect, not redundancy. `scripts/workflow-check.sh`
fails when a document is missing from this table.

## Six kinds

| Kind | Where | Answers | Rule |
| :--- | :--- | :--- | :--- |
| State | [`../PROJECT.md`](../PROJECT.md), [`requirements.md`](requirements.md) | what is true now | the ledger wins every disagreement |
| Decisions | [`adr/`](adr/) | why this and not the alternative | `NNNN-slug.md`, four sections, one status |
| Terminology | `concepts/` | what a word means here | one page per term; no commands, no output |
| Explanation | `explanation/` | how a mechanism runs, end to end | must quote real output |
| Measurement | `benchmarks.md` | how fast, and how it was measured | the only home of a performance number |
| History | [`notes/`](notes/) | what each slice taught | one note per slice |

`concepts/`, `explanation/` and `benchmarks.md` do not exist yet: they appear when the first
term, the first end-to-end mechanism and the first measurement do. Each directory, when it
appears, carries a `README.md` with its own admission rule.

## Reading order

1. [`../README.md`](../README.md) — what the project is, and what it deliberately is not.
2. [`requirements.md`](requirements.md) — R1–R12, the non-goals, and the milestone gates.
3. [`adr/0001-host-the-reference-class-library.md`](adr/0001-host-the-reference-class-library.md)
   — the decision the whole project rests on.
4. [`../PROJECT.md`](../PROJECT.md) — what landed, what is next.

## Index

| Path | Kind | Owns |
| :--- | :--- | :--- |
| [`requirements.md`](requirements.md) | state | R1–R12, non-goals, milestone gates, constraints |
| [`adr/`](adr/) | decisions | `0001`–`0014`, one decision per record |
| [`notes/`](notes/) | history | one note per completed slice |
