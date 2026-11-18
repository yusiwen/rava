# ADR-0006 — A class is (defining loader, binary name)

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

The hosted library is modular and loads classes through its own loader hierarchy. Two loaders
may define the same binary name as two different classes, and the library relies on that.
Keying classes by name alone — as the previous iteration did — cannot express it.

## Decision

Class identity is the pair `(defining loader, binary name)`. The VM's class table is keyed by
that pair; the bootstrap loader reads the host image. Modules and the loader hierarchy are
introduced in milestone M2; until then there is one defining loader, but the key is already the
pair, so adding more is not a re-architecture.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Global names (previous model) | Cannot host a loader hierarchy; a second definition of the same name would collide. |
| A loader id inside the class name string | Packs two facts into one and leaks into diagnostics and caches. |

## Consequences

**Positive**

- The model the library assumes is representable from the start.
- Resolution, caches and `getClass` all have one identity to compare.

**Negative / accepted costs**

- Every class key gains a loader component, and the bootstrap path must be explicit.

**Follow-up**

- Module readability and exports (M2).
