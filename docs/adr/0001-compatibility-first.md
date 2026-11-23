# ADR-0001 — Compatibility-first: the success criterion is a pinned external application

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

A Rust VM can parse, link, verify and execute javac output against a class library we
own. That proves the mechanism; it does not prove that a real framework runs. A corpus
exercises the instructions its author chose, while a framework exercises the platform
contract its ecosystem depends on: annotations, generic signatures, dynamic
subclasses, reflection, concurrency and resources.

This project starts from the opposite end. The product is compatibility with a pinned
external application. The VM, the class library and the tooling exist to make that
application run, and every scope decision is justified by what that application
reaches.

## Decision

The success criterion is: **the pinned reference application passes the C0–C4
compatibility gates**, as defined by [ADR-0003](0003-compatibility-contract.md). The
corpus becomes a regression suite, not the goal.

Learning remains a constraint — mechanisms are still built to be understood, not
copied — but it is no longer the success criterion. A design that is easy to explain
and does not run the application is a failed design.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Treat framework compatibility as an optional experiment | No acceptance gate; every compatibility gap becomes a judgement call, and "it booted once" substitutes for evidence. |
| Load the real JDK `java.base` and implement only the VM | The identity split between our `Object`/`String` and the JDK's is a correctness trap, and the JDK's internals (modules, `VarHandle`, compact strings) dominate the work. ADR-0004 rejects it. |
| Implement all of Java SE 17 before running anything | Unbounded scope with no feedback loop; most of the API is never reached by the reference application. |
| Start from the full JVM specification surface, verifier included, before the first application | The specification is larger than the target; a testable application gives a better ordering than a checklist. |

## Consequences

**Positive**

- Every gap has an owner: the application's manifest entry and the failing gate.
- The project can stop at a declared surface instead of chasing the Java SE
  specification.
- Progress is measurable: unresolved symbols, metadata diffs, passing tests, startup
  time.

**Negative / accepted costs**

- The project is a platform-compatibility effort, not a weekend VM; the cost is in
  years, not weekends.
- Third-party code outside the manifest is explicitly unsupported and fails at
  defined boundaries.
- The reference application becomes a dependency of the project's definition of done;
  its version is part of the target (ADR-0002).
