# ADR-0004 — The runtime class library is a full replacement, owned by us

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

A Java runtime needs `java.lang.Object`, `java.lang.String` and the rest of the
platform classes. There are three ways to obtain them: load the JDK's, mix the JDK's
with ours, or own the whole library.

Loading the real `java.base` means implementing its internal dependencies (module
layer, `VarHandle`, compact strings, `jdk.internal.*`) and adopting a new obligation
on every JDK release. Mixing produces two `Object`/`String` identities, which must
never meet; framework code creates objects across that boundary constantly. Owning the
library keeps the runtime surface under our control and preserves the project's
identity: the JCL really is Rust.

## Decision

The runtime class library is a **full replacement**:

- `java.*` classes are our `.class` files, compiled against signatures we own, with
  behaviour implemented by Rust natives registered by `(owner, name, descriptor)`.
- The real `java.base` is never loaded at run time. The JDK is a compile-time and
  signature-reference tool only.
- `java.lang.Object` and `java.lang.String` are ours; no hybrid with the JDK's
  classes.
- The library grows to the observed surface (ADR-0003), not to the full specification.
  `java.lang.invoke` and `VarHandle` are in scope to the extent the manifest requires
  them; a blanket refusal is no longer appropriate.
- API signatures are compared with `javap`/`ct.sym`; implementations are written from
  scratch, not copied from JDK source.
- `java.beans` is allowed headless (Spring's introspection uses it); the rest of
  `java.desktop` is not.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Load the real `java.base` | Identity split, module layer, compact strings, `VarHandle`, and a lasting compatibility obligation to JDK internals. |
| Hybrid: our `java.lang`, the JDK's `java.util` | Two `Object`/`String` identities must never meet; Spring creates objects on both sides of the boundary. Correctness trap. |
| Patch the JDK's classes | Requires the JDK's internals, build system and licence; still a runtime we must maintain. |
| Provide classes only from the VM (no `.class` files) | Skips loading, linking and `<clinit>`, and leaves reflection/ASM without the metadata frameworks depend on. |

## Consequences

**Positive**

- The runtime library surface is versioned with the target and independent of JDK
  release churn.
- Every library call still traverses loading, linking, verification and native
  dispatch, which keeps the runtime honest.
- The "JCL is Rust" property survives: classes are real class files, behaviour is
  native.

**Negative / accepted costs**

- Signature fidelity is an ongoing obligation: a missing or wrong descriptor is a
  `NoSuchMethodError` at run time.
- The library is large even when bounded by the manifest; it is the dominant work,
  not the interpreter.
- Java language features that depend on newer library classes (`java.lang.invoke`,
  records helpers, `VarHandle`) must be implemented rather than inherited.

