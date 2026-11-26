# History notes

History documents record what happened: the order of blockers, the fixes, the surprises
and the lessons. They are not current-state documents and not explanations of the final
mechanism. `PROJECT.md` owns the ledger; `docs/explanation/` owns how the current
mechanism works.

## Contract

A milestone history note should contain:

1. **Scope and date** — which milestone/slice cluster, and when it closed.
2. **Delivered sequence** — the slices/groups and what each added.
3. **Blocker chain** — symptoms, root causes, fixes, in the order they appeared.
4. **Lessons** — what future work should avoid or re-check.
5. **Still fragile** — known gaps that are easy to forget after the green gate.
6. **Evidence** — commands, outputs and links to the ledger/explanation documents.

One note covers a milestone (or a coherent slice cluster), not every individual slice:
the per-slice state and evidence live in `docs/slices/`, and `PROJECT.md` keeps the
status row. A later note can supersede a
lesson, but an accepted decision is still an ADR, not a note.

## Index

| Note | Scope | Date |
| :--- | :--- | :--- |
| [`M2.md`](M2.md) | M2: C1 metadata, minimal library and the first Spring context | 2026-11-24 |
| [`M3.md`](M3.md) | M3: dynamic code, CGLIB/Objenesis, Spring AOP and the C2 gate | 2026-11-25 |
