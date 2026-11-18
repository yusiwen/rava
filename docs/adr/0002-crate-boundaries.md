# ADR-0002 — Five crates, one-way dependencies

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

The parser, the runtime state, the dispatch loop, the native implementations and the CLI change
for different reasons. A cycle between them would let parser types leak into the dispatch loop,
and the interpreter would start to know about class-file details it must not know (ADR-0003,
ADR-0009).

## Decision

Five crates with a compiler-enforced dependency direction:

```
rava-classfile  <-  rava-vm  <-  rava-interp
                            <-  rava-native
rava-cli depends on all of them.
```

`rava-interp` never depends on `rava-classfile`; it sees decoded instructions and the VM's
types. `rava-native` implements behaviour, never parsing. The direction is checked by
`scripts/check-invariants.sh`, so the picture in this ADR is executable.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| One crate | Nothing stops the cycle; the interpreter would eventually read bytes. |
| Two crates (parser, everything else) | `interp` and `native` share a crate, so the callback direction cannot be enforced. |
| Traits everywhere to invert dependencies | Indirection without a second implementer; the crate graph already says what is allowed. |

## Consequences

**Positive**

- The dependency graph is the documentation, and a violation fails the build check.
- `rava-classfile` is reusable as a parser without dragging the VM in.

**Negative / accepted costs**

- Some types are duplicated across the boundary (`ClassId` lives in `vm`), and the CLI is the
  only place allowed to see everything.

**Follow-up**

- None planned.
