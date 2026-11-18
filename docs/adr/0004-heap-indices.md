# ADR-0004 — References are heap indices, never pointers

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

Java object graphs are cyclic by construction. Smart pointers model ownership the language does
not have; raw pointers would force `unsafe` into every field access and forbid relocating
objects; and a collector needs to find every reference without the program's help.

## Decision

A reference is `Value::Ref(ObjRef)` where `ObjRef(u32)` indexes `Shared::heap`. `ObjRef(0)` is
`null`. No `Rc`, `Arc` or raw pointer appears inside a `Value`; identity `hashCode` is derived
from the index and documented as not matching the JDK's (nothing observable depends on the
value).

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| `Rc<RefCell<_>>` | Cycles leak; a native that re-enters Java while a borrow is live panics; no collection. |
| Raw pointers or `&mut` into an arena | `unsafe` at every access, and objects could never move. |
| Generational handles (index + generation) | The right shape for use-after-free detection, but the heap does not move or reuse yet; the `u32` can grow into it without changing `Value`. |

## Consequences

**Positive**

- Cycles are ordinary; there is no borrow negotiation at call boundaries.
- Relocation stays possible: a later collector can move objects without rewriting references.

**Negative / accepted costs**

- One indirection per field access, and the heap is a single owned structure (a contention
  point once threads are real — ADR-0010).
- A stale index is undetectable until generation counters exist.

**Follow-up**

- Generation counters, if a use-after-free is ever observed.
