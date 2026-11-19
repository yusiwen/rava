# Requirements

- **Status:** living document; scope frozen by the human on 2026-11-18
- **Owner:** `PROJECT.md` owns progress; this document owns scope.

## Purpose

`rava` is a Java virtual machine written in Rust that hosts the reference class library
(ADR-0001). It is a learning artefact with a hard external contract: a program compiled by
`javac` for JDK 21 runs on `rava` and prints what the reference launcher prints.

The shape that follows from hosting is the important part: the VM must implement the **native
surface the library declares** — `java.base` alone declares 700 native methods — rather than
reimplement the library's API (ADR-0005).

## Requirements

### R1 — Execute a JDK 21 class file end to end

- **Statement:** a class file at version 65, compiled by `javac --release 21`, loads, runs
  `main`, and writes the same bytes to stdout as the reference launcher.
- **Acceptance:** `cargo test -p rava-cli` exits 0 on the boot test, which compares the
  program's stdout with the host JVM's, byte for byte.
- **Milestone:** M0.

### R2 — Parse the class-file format forward-compatibly

- **Statement:** parse every structure of JVMS chapter 4 for version 65, keep unknown
  attributes as raw bytes, and never reject a file merely because its version is newer.
- **Acceptance:** `cargo test -p rava-classfile` exits 0.
- **Milestone:** migrated (S1).

### R3 — Read classes from the host image

- **Statement:** the class path is a JDK home or an extracted module directory; the bootstrap
  loader resolves binary names to classes from it, without a committed copy of any class
  library in this repository.
- **Acceptance:** `cargo test -p rava-vm` exits 0 on the image test, which loads
  `java/lang/Object.class` from the host image.
- **Milestone:** M0.

### R4 — Bootstrap the library's core classes without a substitute

- **Statement:** `java/lang/Object`, `java/lang/String`, `java/lang/Class`, `java/lang/System`
  and `java/lang/Thread` come from the host library, not from a synthetic class invented by
  the VM.
- **Acceptance:** the boot test of R1 fails if any of those classes is loaded from a
  `synthetic` path; the VM has no `synthetic_object` fallback.
- **Milestone:** M0.

### R5 — Implement JCL natives in Rust

- **Statement:** every native method the hosted library declares and the executed program
  reaches is implemented in `rava-native`, keyed by `(owner, name, descriptor)`, and admitted
  by the slice that needs it. No JNI, no `dlopen`, no C ABI.
- **Acceptance:** `cargo test -p rava-cli` exercises the natives on the boot path;
  `rava natives` reports a registration that no class declares as dangling.
- **Milestone:** M0 onward.

### R6 — Handle exceptions

- **Statement:** `athrow`, per-method exception table lookup, unwinding, `finally` as a
  catch-all, and a stack trace captured where the frames still exist.
- **Acceptance:** `cargo test -p rava-vm -p rava-interp` exits 0 (the migrated tests).
- **Milestone:** migrated (S2).

### R7 — Monitors and object identity

- **Statement:** every object carries a real monitor; `monitorenter`/`monitorexit` and
  `ACC_SYNCHRONIZED` work on it; identity `hashCode` is stable for the object's lifetime.
- **Acceptance:** the migrated tests exit 0.
- **Milestone:** migrated (S2), reworked for real threads in M3.

### R8 — Report unsupported features by name

- **Statement:** a well-formed class file that uses something this VM does not implement
  yields a typed error naming the feature and the class — never a panic, never a misparse.
- **Acceptance:** the migrated tests plus every new slice's negative test.
- **Milestone:** migrated (S2), kept for every later feature.

### R9 — Keep the interpreter free of version logic

- **Statement:** no class-file version, dialect or capability inspection appears in the
  dispatch loop; version policy lives in the loader.
- **Acceptance:** `bash scripts/check-invariants.sh` exits 0.
- **Milestone:** migrated (S2).

### R10 — Run programs against the hosted library

- **Statement:** a program that uses the library's own classes (`String`, `ArrayList`,
  `Math`, reflection) runs and matches the reference launcher, with the library's classes
  loaded from the host image.
- **Acceptance:** a differential test per program, comparing stdout byte for byte with `java`.
- **Milestone:** M1–M5.

### R11 — Real threads

- **Statement:** each Java thread has its own stack and runs concurrently with the others;
  monitors are real locks; visibility follows the Java memory model for the primitives the
  library uses.
- **Acceptance:** a program with two threads incrementing one `synchronized` counter reaches
  the same total every run.
- **Milestone:** M3.

### R12 — The library's VM-facing primitives

- **Statement:** `MethodHandle` invocation, `VarHandle` accesses and the `Unsafe` subset the
  library itself uses are implemented with their specified memory semantics.
- **Acceptance:** the library's reflection and concurrency classes run without a fallback path.
- **Milestone:** M4.

The scope was frozen by the human on 2026-11-18; anything that would change it is recorded as a
superseding ADR, not as an open question here.

## Open questions

None.

## Non-goals

> Permanently out of scope, with what we do instead.

- **JNI for third-party native libraries.** The library's own natives are ours (R5), but
  exposing a C ABI and an object layout to arbitrary `.so` files is refused: it contradicts
  the heap and handle model, and it buys compatibility with code this project does not want to
  run. Reversing it takes a superseding ADR.
- **Shipping a class library.** `java.*` is the host's (ADR-0001). A committed copy would
  change the model and is refused by `scripts/check-invariants.sh`.
- **Java SE certification.** The success criterion is the differential tests, not a
  compatibility badge.
- **Running arbitrary frameworks as a goal.** Programs are chosen for what they exercise, not
  for their brand.

## Milestone gates

| Milestone | What it delivers | Gate |
| :--- | :--- | :--- |
| M0 | Boot: read the image, bootstrap the core classes, first JCL natives, `println` | R3, R4, R5 |
| M1 | The core library: strings, arrays, exceptions, `Class` basics | M0 |
| M2 | Class loading and modules: `(loader, name)` identity, resources | M1 |
| M3 | Real threads and monitors | M2 |
| M4 | `MethodHandle`, `VarHandle`, `Unsafe` | M3 |
| M5 | Breadth: `java.util`, `java.io`, `java.nio`; a real jar | M4 |

## Deferred goals

> Committed in direction, not yet scheduled. A deferred goal is work the project intends to do,
> with the increment that would prove it started; a non-goal is work it will not do.

### G1 — A JIT compiler

- **Goal:** compile the methods the interpreter spends its time in, and fall back to the
  interpreter everywhere else.
- **Why deferred:** the interpreter is the readable artefact and the oracle a compiler is
  checked against; compiling before the VM runs real programs would optimise code that cannot
  yet execute. It is also the largest single subsystem after the hosted library's native
  surface.
- **Priority:** after M5. The first increment is narrow: compile one hot method's body, keep
  the interpreter as the fallback, and prove the compiled path produces the same output as the
  interpreter on the same program.
- **Trigger:** a measurement showing the interpreter is the bottleneck for a program the VM
  already runs, and a target worth the complexity. The measurement is the entry condition, not
  the decision: ADR-0013 records the plan.
- **Risk if the VM never gets there:** none. M5 is a complete, useful VM without a compiler.

## Constraints

- Rust 1.100 (pinned in `rust-toolchain.toml`), edition 2024; no `unsafe` (checked).
- JDK 21 supplies the class library and the reference launcher; there is no second JDK.
- MIT licence; dependencies must remain licence-compatible.
- Signed commits; one note per completed slice; the merge procedure is in `PROJECT.md`.
- No third-party Rust crates without an ADR: the point is to read the machine, not to import it.
