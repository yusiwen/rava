# Slice records

Owning question: **what did each slice deliver, where does it stop, and what proves it?**

`PROJECT.md` § Slices owns every slice's status and its one-line description; this
directory owns the per-slice record. History notes ([`../notes/`](../notes/)) own what
happened in a milestone and what it taught; explanation documents
([`../explanation/`](../explanation/)) own how a mechanism runs today.

## Entry contract

One `##` heading per slice, three labelled paragraphs in this order:

```markdown
## M4.1 — Classpath resources — 2026-11-26

**Acceptance.** `cargo test -p rava-cli --test resources` — exit 0.

**Delivered.** What landed, in the order a reader needs it: the new types, the seam they
plug into, the behaviour that became observable.

**Boundary.** What this slice deliberately does not do, and what is left fragile. Naming
the gap is the point; "not implemented" on its own is not a boundary.
```

Rules:

- The heading is `<id> — <the ledger's description> — <date the acceptance ran>`.
- `Acceptance` quotes the command from `PROJECT.md` and the exit code it returned.
- `Delivered` records what landed; `Boundary` records where it stops. A slice with no
  boundary paragraph must be assumed to have one that nobody wrote down.
- Evidence is a command and a count, never prose: a number without the command that
  produced it is not evidence.
- No decision lives here. A reversal becomes an ADR; a record is corrected, never quietly
  rewritten to match the code.
- `PROJECT.md` keeps the status. This file never repeats the status column, so the two
  cannot disagree.

## Files

| File | Slices | Closed |
| :--- | :--- | :--- |
| [`M0.md`](M0.md) | M0.1–M0.7, plus the M0 gate review | 2026-11-24 |
| [`M1.md`](M1.md) | M1.1–M1.4, plus the M1 gate review | 2026-11-24 |
| [`M2-1-c1-and-object-model.md`](M2-1-c1-and-object-model.md) | M2.1–M2.12 | 2026-11-24 |
| [`M2-2-reflection-and-dynamic.md`](M2-2-reflection-and-dynamic.md) | M2.13–M2.22 | 2026-11-24 |
| [`M2-3-spring-context.md`](M2-3-spring-context.md) | M2.23–M2.30 | 2026-11-24 |
| [`M3.md`](M3.md) | M3.1–M3.7 | 2026-11-25 |
| [`M4.md`](M4.md) | M4.1–M4.5 | 2026-11-26 |

M2 is split three ways because one file would be ~122 KB (`#64` tracks a further
editorial pass). Cite a slice by id, not by anchor: the heading text is the search key,
and anchors derived from em dashes are not stable enough to depend on.

## Adding a slice

1. Add the row to `PROJECT.md` § Slices with its acceptance command **before** writing
   code; a slice without an acceptance command is a wish.
2. On close, add the `## <id>` entry here with the acceptance run's exit code and date.
3. Touch `Resume` only if the milestone or the next slice changed.

`bash scripts/doc-check.sh .` fails when a `done` slice has no entry here.
