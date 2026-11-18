# S2 — The VM core, and the one real coupling

- **Slice:** S2
- **Requirements:** R6–R9
- **Acceptance:** `cargo test -p rava-vm -p rava-interp` exited 0 on 2026-11-18 (3 unit tests
  migrated; the interpreter has no unit tests of its own yet).

## What surprised me

**The interpreter did not know about the class library either.** It matches on decoded
instructions and calls `Shared`, `ThreadState`, the exception helpers and the native registry.
The 71 natives of the previous stub library were the only library-specific code in the runtime,
and they lived in one crate.

**The single real coupling was `java.lang.Object`.** The loader synthesised it, because a
hand-built stub library had no `Object` to load. Hosting the reference library makes that
wrong: the library's own `Object` must be loaded from the image (ADR-0001, ADR-0006). Removing
the synthesis was the one semantic change of the migration, and it is what turns "the VM runs
our library" into "the VM runs the host's".

**`rava-native` survived as an empty crate on purpose.** The registry (the keyed table, the
arity check) lives in `rava-vm`; the implementations are the part that gets rewritten for the
hosted library (ADR-0005). An empty crate with a documented entry point is cheaper to keep than
to recreate later.

## What the specification says

JVMS §2 defines the structure the interpreter implements — frames, operand stacks, the per-class
runtime constant pool, monitors — and §6 the instruction set. None of it names a class library;
that separation is what made the migration possible.

## The bug I hit

The CLI carried `--stdlib`, defaulting to `stdlib/classes`, in four subcommands. With the
library gone the flag was meaningless, so it became `--home`, the JDK that supplies both the
classes and the reference launcher. The default changed from a repository path to `.`, which is
a placeholder until the image reader of S4 gives it a real meaning.

## Open threads

- The verifier still counts values, not JVM slots (ADR-0011); that is unchanged and deliberate.
- The interpreter has no unit tests of its own: the previous ones were corpus-driven. The boot
  milestone brings the first ones back, against the hosted library.
