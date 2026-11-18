# ADR-0010 — Real threads from the start of the concurrency work

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

The hosted library starts threads of its own (the common pool, reference handling) and programs
use `Thread`, executors and `wait`/`notify`. A serialized `Thread.start` — run the body to
completion before returning — cannot express that: a thread waiting on a queue that the main
thread fills would deadlock, and a server would never accept a connection.

## Decision

Implement **real OS threads**: each Java thread has its own stack and runs concurrently.
`Shared` is the global state and is accessed under a lock; per-object monitors are real and
blocking. The dispatch loop is per-thread. Concurrency lands at milestone M3; before that the
VM is single-threaded and programs that start a thread are out of scope.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Keep the serialized model | Deadlocks and wrong behaviour on any real concurrent program. |
| Green threads / continuations | Complexity without a benefit at this scale; the OS scheduler is sufficient. |
| A single global lock around everything | Simple, but it makes every monitor the same lock and destroys independent progress. |

## Consequences

**Positive**

- The library's own concurrency works, which is a prerequisite for hosting it.

**Negative / accepted costs**

- Every access to shared VM state must be reasoned about; the interpreter's mutation points
  become the design's most delicate part.

**Follow-up**

- The memory model's visibility rules for the primitives the library uses.
