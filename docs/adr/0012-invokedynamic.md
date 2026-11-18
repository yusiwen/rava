# ADR-0012 — Bootstraps by name first, real method handles later

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

The hosted library compiles string concatenation, lambdas, records and pattern switches to
`invokedynamic`. Their bootstrap methods are Java code in `java.lang.invoke`; running them
faithfully requires method handles, and that machinery needs VM intrinsics
(`MethodHandle.invokeExact`) that do not exist yet.

## Decision

Recognise the four bootstrap families the library emits — `StringConcatFactory`,
`LambdaMetafactory`, `ObjectMethods`, `SwitchBootstraps` — by name, and build the call site
directly. Anything else is a named error. Real `MethodHandle`/`VarHandle` support is milestone
M4; until then the by-name path is a declared debt, not a design.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Implement `java.lang.invoke` first | It is the largest subsystem in the VM and would delay the boot milestone by months. |
| Reject `invokedynamic` | The hosted library's `String` and lambdas need it; nothing would run. |

## Consequences

**Positive**

- The library links and runs early, and the missing subsystem is visible in one place.

**Negative / accepted costs**

- A user-defined bootstrap method fails until M4, and the by-name table must track the
  library's compilers.

**Follow-up**

- M4 replaces the table with real method handles; the debt is recorded in `PROJECT.md`.
