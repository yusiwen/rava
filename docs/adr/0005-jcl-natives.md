# ADR-0005 — Implement the library's natives in Rust, keyed by the triple

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

Hosting the reference library (ADR-0001) means its `native` methods have declarations and no
bodies. Something must supply them. The options are to link the JDK's C++ implementations
through JNI, or to implement them here.

## Decision

Implement the native methods in Rust, in `rava-native`, keyed by
`(owner, name, descriptor)` — the exact triple the class file stores at the call site. The
registry lives in `rava-vm`; each native is admitted by the slice that needs it and tested by
that slice's acceptance command. There is no JNI, no `dlopen`, and no object layout visible
outside the VM.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Link the JDK's native libraries (JNI) | Forces a C ABI, `JNIEnv`, local/global references and an externally visible layout (JNI is a non-goal, requirements § Non-goals). |
| Generate natives from JNI headers | The headers assume the same C ABI; nothing is gained. |
| Match on method names inside the interpreter | Skips resolution and loading, and breaks the moment a program declares its own `println`. |

## Consequences

**Positive**

- The implementation is safe Rust, testable without a C toolchain, and lives in one place.
- The key is the descriptor triple, so overloaded methods are unambiguous.

**Negative / accepted costs**

- The surface is large (700 native methods in `java.base`) and every one must match the
  library's specification exactly, including error behaviour.
- Per-slice admission means the surface grows only as fast as programs demand.

**Follow-up**

- ADR-0011 (verifier) and ADR-0012 (invokedynamic) set the scope of what natives must support.
