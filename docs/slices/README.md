# Slice records

Owning question: **what did each slice deliver, where does it stop, and what proves it?**

`PROJECT.md` § Slices owns every slice's status and its one-line description; this
directory owns the per-slice record. History notes ([`../notes/`](../notes/)) own what
happened in a milestone and what it taught; explanation documents
([`../explanation/`](../explanation/)) own how a mechanism runs today.

## Entry contract

```markdown
## <id> — <the ledger's description> — <date the acceptance ran>

**Acceptance.** <the command from PROJECT.md> — exit 0 (<n> tests).

**Delivered.**
- <what landed: the new type or module and the seam it plugs into>
- <the behaviour that became observable>

**Boundary.**
- <what this slice deliberately does not do, and which slice or ADR owns it>

See [`<mechanism>.md`](../explanation/<mechanism>.md).
```

Rules:

- An entry is at most **12 non-empty lines** and every line at most **200 characters**;
  `scripts/doc-check.sh` fails otherwise. If it does not fit, the detail belongs in the
  explanation document, and the entry links to it.
- The heading is `<id> — <the ledger's description> — <date the acceptance ran>`.
- `Acceptance` quotes the command from `PROJECT.md`, its exit code and its test count.
- `Delivered` carries 1–3 bullets: what landed and what became observable, not how it was
  tested. `Boundary` carries 1–3 bullets: what the slice deliberately does not do and what
  is left fragile. A slice with no boundary is assumed to have one nobody wrote down.
- The `See` line names the explanation document that owns the mechanism. Do not restate
  the mechanism here; a second copy of it goes stale.
- No decision lives here. A reversal becomes an ADR; a record is corrected, never quietly
  rewritten to match the code.
- `PROJECT.md` keeps the status. This file never repeats the status column, so the two
  cannot disagree.

## Gate baseline

Every entry closed with the same workspace gate, so the entries do not repeat it:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo run -p rava-cli -- compat check`, and `cargo run -p rava-cli -- compat metadata
  --check` from M2 on.
- `cargo xtask ci` from M0, which runs the three above.
- `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` in the slices that
  touched the Spring path (M2.23–M2.30, M3.7, M4).
- `RAVA_C2_ACCEPTANCE=1 cargo test -p rava-cli --test c2_gate` from M3.7 on.
- `RAVA_BOOT_ACCEPTANCE=1 cargo test -p rava-cli --test boot_nonweb` for M4.5.
- `cargo test -p rava-cli`, `cargo test -p rava-interp` and `cargo test -p rava-vm` for one
  crate's suite. Plain `cargo test` skips the jar-dependent acceptances; the `RAVA_*`
  variables turn a missing jar into a failure.

An entry's `Acceptance` line carries the command that proves that slice. The test counts of
the workspace itself are recorded in the milestone notes ([`../notes/`](../notes/)) and the
gate review sections, not per slice.

## Files

| File | Slices | Closed |
| :--- | :--- | :--- |
| [`M0.md`](M0.md) | M0.1–M0.7, plus the M0 gate review | 2026-11-24 |
| [`M1.md`](M1.md) | M1.1–M1.4, plus the M1 gate review | 2026-11-24 |
| [`M2-1-c1-and-object-model.md`](M2-1-c1-and-object-model.md) | M2.1–M2.12 | 2026-11-24 |
| [`M2-2-reflection-and-dynamic.md`](M2-2-reflection-and-dynamic.md) | M2.13–M2.22 | 2026-11-24 |
| [`M2-3-spring-context.md`](M2-3-spring-context.md) | M2.23–M2.30 | 2026-11-24 |
| [`M3.md`](M3.md) | M3.1–M3.7, plus the post-M3 issues | 2026-11-25 |
| [`M4.md`](M4.md) | M4.1–M4.5 | 2026-11-26 |

M2 is split three ways because one file would be ~122 KB. Cite a slice by id, not by
anchor: the heading text is the search key, and anchors derived from em dashes are not
stable enough to depend on.

## Adding a slice

1. Add the row to `PROJECT.md` § Slices with its acceptance command **before** writing
   code; a slice without an acceptance command is a wish.
2. On close, add the `## <id>` entry here with the acceptance run's exit code and date.
3. Touch `Resume` only if the milestone or the next slice changed.

`bash scripts/doc-check.sh .` fails when a `done` slice has no entry here, or when an
entry breaks the shape above.
