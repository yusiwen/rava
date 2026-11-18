# ADR-0009 — Mark-sweep with a free list

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

References are indices (ADR-0004), so "freed" cannot mean "invalid": every slot a program holds
stays in range. A collector must find unreachable objects from a complete root set, and the
allocation path must be able to reuse the result.

## Decision

Stop-the-world **mark-sweep** with a free list. Roots are the threads' frames, the static fields
of loaded classes, the interned string table and the suppressed-exception map. Collection runs
on `System.gc()` and when the allocation budget is reached; freed slots are pushed onto a free
list the allocator reuses. Objects do not move.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Reference counting | Java graphs are cyclic; the collector would leak exactly the shapes programs use. |
| Copying or compacting | Possible with indices, but it rewrites every root and object for locality nothing here measures. |
| Generational | A later optimisation, once a measurement asks for it. |

## Consequences

**Positive**

- One pass, no root rewriting, and a bounded heap.
- The root set is explicit, which is also what makes the collector testable.

**Negative / accepted costs**

- Stop-the-world pauses; no compaction; a stale index is undetectable.

**Follow-up**

- Collection statistics and a budget policy are measured before changing anything.
