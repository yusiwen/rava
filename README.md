# rava

A Java virtual machine in Rust that **hosts the reference class library** instead of owning one.

The VM supplies the machine: class-file parsing, the heap, class loading and resolution, the
interpreter, monitors, the verifier — and the part that makes hosting possible, the native
methods the library declares. `java.base` alone declares 700 of them; nothing in a class file
implements them.

The library supplies `java.lang`, `java.util`, `java.io` and the rest. It is the unmodified
JDK 21 class library, compiled by `javac` at class-file version 65.

## Why this shape

A JVM is two separable things: the machine, and the class library it runs. Almost every JVM
in use pairs its own machine with the same library — the OpenJDK one — because the library is
the largest and least interesting part to write.

rava takes that split seriously:

- **The VM is ours**, and it is the thing worth reading: ~80 decoded instructions, a heap of
  indices, a mark-sweep collector, a call path with resolved-call-site caches.
- **The library is the reference one.** We do not ship `java.*`; we host it, which means
  implementing the native surface it declares (ADR-0005) rather than reimplementing its API.
- **The contract is the image**, not a hand-built stub directory: classes are read from the
  JDK's `lib/modules` (ADR-0008).

That is the difference between this project and a corpus-driven toy: a program that uses
`ArrayList`, `Math` or `System.getProperty` is limited by what the library needs from the VM,
not by which members someone remembered to stub.

## Status

The workspace is a **migration baseline**: the parser, heap, class model, resolution,
monitors and interpreter are in place and tested; the class path still reads directories. The
hosting work starts at milestone **M0 (boot)**: read the module image, bootstrap `Object` /
`String` / `Class` / `System`, implement the first JCL natives, and run
`System.out.println("hi")` on the reference library. The ledger is
[`PROJECT.md`](PROJECT.md).

## Quick start

```bash
nix develop          # or: direnv allow
cargo xtask ci       # fmt, clippy, tests, architecture invariants
bash scripts/workflow-check.sh .
```

The CLI is a thin window on the VM:

```bash
cargo run -p rava-cli -- --dump path/to/Foo.class          # the parser's view
cargo run -p rava-cli -- disasm Foo --home <jdk> --cp out  # decoded instructions
cargo run -p rava-cli -- verify Foo --home <jdk> --cp out  # checks, without running
cargo run -p rava-cli -- run Foo --home <jdk> --cp out     # execute main
```

`--home` is the JDK that supplies the class library; `--cp` is the application's class path.

## How it is organised

| Crate | Owns |
| :--- | :--- |
| `rava-classfile` | the class-file format both ways: parser, builder, assembler |
| `rava-vm` | heap, classes, loading, resolution, verifier, collector, the native registry |
| `rava-interp` | the dispatch loop; it never sees a byte offset or a class-file version |
| `rava-native` | the JCL natives: what the hosted library declares, implemented in Rust |
| `rava-cli` | `rava run` / `verify` / `bench` / `module` / `disasm` |

## What is deliberately absent

- **JNI.** The library's natives are implemented in Rust (ADR-0005). Third-party native
  libraries are out of scope, because supporting them means exposing an object layout and a C
  ABI the VM does not want.
- **A JIT.** Deferred until the interpreter is the measured bottleneck; the interpreter is
  the readable artefact.
- **A shipped class library.** `java.*` is the host's, not ours.
- **Java SE certification.** The target is a VM that runs real programs, not a
  compatibility badge.

## Where the thinking lives

| Document | Owns |
| :--- | :--- |
| [`PROJECT.md`](PROJECT.md) | the ledger: phases, milestones, slices, acceptance commands, resume |
| [`docs/README.md`](docs/README.md) | the index of every document and the rule each kind follows |
| [`AGENTS.md`](AGENTS.md) | the entry point for a fresh session or another agent |

## Licence

MIT ([ADR-0007](docs/adr/0007-license-mit.md)). The hosted class library is the user's JDK:
it is loaded at run time, never redistributed here.
