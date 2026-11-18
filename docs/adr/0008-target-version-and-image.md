# ADR-0008 — Target class-file version 65 and read the host image

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

Two questions decide what the VM parses and where classes come from: which class-file versions
are in scope, and how the host library is located. A JDK ships its classes in `lib/modules`
(a jimage archive); an extracted directory with the same layout is also possible.

## Decision

- **Version 65** (JDK 21) is the target. The parser accepts newer files and preserves unknown
  attributes (R2); the loader owns any policy about what it will execute.
- **The class source is the host's**: a JDK home whose `lib/modules` is read directly, or an
  extracted image directory. No class library is committed to this repository.
- **No JDK 8.** The previous iteration pinned JDK 8 for its corpus; hosting needs the modern
  library, where `java.lang.invoke` and the module model exist.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| JDK 8 | Its library predates the module model and the invoke machinery the hosted code uses. |
| Commit an extracted image | Redistribution and a frozen copy; the host supplies it instead. |
| Read only directories, never `lib/modules` | Requires every user to extract the image first; reading the archive is the honest interface. |

## Consequences

**Positive**

- One version to implement, and one source of classes.
- The parser stays version-agnostic, so a newer file is a named failure, not a misparse.

**Negative / accepted costs**

- The VM needs a jimage reader (or an extractor step) before it can load anything.
- Moving to another JDK is a migration, not a flag.

**Follow-up**

- The image reader is the first slice of milestone M0.
