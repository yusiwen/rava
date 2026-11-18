# ADR-0001 — Host the reference class library

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

The previous iteration owned its class library: hand-written `java.*` classes compiled by
`javac`, with behaviour in Rust. That taught the pipeline — parse, load, link, `<clinit>`,
native dispatch — but it had a hard ceiling: a program could only use the members someone had
stubbed. A program that called `ArrayList`, `Math` or `Objects` failed with `class not found`,
and closing that gap meant reimplementing the JDK one class at a time.

A JVM is separable into the machine and the library it runs. Production VMs generally supply
their own machine and run the reference library, because the library is the largest and least
interesting part to write. Hosting replaces the unbounded "reimplement the API" problem with
a bounded one: implement the **native methods the library declares**, which is a finite,
enumerable surface.

## Decision

Host the unmodified JDK 21 class library and implement its native surface in Rust.

- No `java.*` class is committed to this repository; the library is supplied at run time by
  the user's JDK (or by the devShell's JDK 21).
- `rava-native` implements every native method the executed program reaches, keyed by the
  exact `(owner, name, descriptor)` triple the class file stores (ADR-0005).
- The class-file target is version 65 (ADR-0008).
- The VM adopts the library's representations where the library assumes them — notably
  `String` as `byte[] value + byte coder` — because library code manipulates those fields
  directly.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Own the class library (the previous model) | Unbounded: the surface is whatever a program needs, and every gap is a class to reimplement. The corpus can only ever cover what was stubbed. |
| Own `java.lang`, host `java.util` | The two representations disagree: the library's `java.util` is compiled against the JDK's `String` and reaches into `jdk.internal.*`. A mixed tree cannot be made consistent. |
| Commit an extracted copy of the library and run it from the repository | Redistribution of GPLv2+CE code inside an MIT repository is a licensing change, and it freezes the library into the tree. The host supplies it instead. |
| Build a certified Java SE implementation | The contract would be a compatibility suite, not a VM worth reading; that is a different project. |

## Consequences

**Positive**

- Programs that use the real library can run; the limit becomes what the VM implements, not
  what somebody remembered to stub.
- The work is enumerable: `java.base` declares 700 native methods in 110 classes, and each
  slice adds exactly the ones it needs.
- The VM stays the readable artefact, because the library is neither written nor maintained
  here.

**Negative / accepted costs**

- The VM must adopt the JDK's object and class layouts, its module model and its loaders.
- The native surface is large: 700 in `java.base` alone, plus hundreds more in the modules a
  real program pulls in.
- The library is versioned externally: moving to a newer JDK is a deliberate migration, not a
  free upgrade.

**Follow-up**

- ADR-0005 (how natives are implemented), ADR-0006 (class identity), ADR-0008 (which classes
  and where they are read from).
