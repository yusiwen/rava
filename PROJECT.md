# rava — project ledger

A Java virtual machine in Rust that hosts the reference class library (ADR-0001). This file is
the authoritative state: phases, milestones, slices, acceptance commands, decisions and the
resume point. Everything else links here.

**Tier:** M

## Phase status

> Phase ids P0…P5 are fixed. Status: `todo` | `in-progress` | `done` | `blocked` | `n/a`.
> A phase marked `done` must name an artifact that exists on disk; the checker enforces that.
> `n/a` is only legal where the tier allows it.

| Phase | What must be true | Artifact | Approved by | Status |
| :--- | :--- | :--- | :--- | :--- |
| P0 | Requirements confirmed by the human | docs/requirements.md | human | done |
| P1 | Decisions recorded and accepted | docs/adr/ | human | done |
| P2 | Work sliced into verifiable increments | PROJECT.md | self | done |
| P3 | Every slice implemented | code + commits | self | in-progress |
| P4 | Tests pass, Definition of Done met | test suite | self | in-progress |
| P5 | Deployed, smoke-tested, rollback ready | docs/deploy.md | human | n/a |

P3 and P4 are `in-progress` by construction: the migration baseline is implemented and tested,
and milestones M0–M5 are planned but not built. `scripts/workflow-check.sh` runs without
`--strict` until the last milestone closes; `--strict` is the completion check.

## Milestones

> A milestone is a stage of the plan (requirements § Milestone gates), between the phases above
> and the slices below. Every row names its evidence.

| Milestone | What it delivers | Slices | Status | Record |
| :--- | :--- | :--- | :--- | :--- |
| — | **Migration baseline**: the VM core from the previous iteration, without its own class library or synthetic `Object` | S1–S3 | done | notes S1–S3 |
| M0 | Boot: read the host image, complete the instruction set the library uses, bootstrap `Object`/`String`/`Class`/`System`, first JCL natives, a program that runs with a verified result | S4–S8 | done | notes S4–S9; ADR-0014 |
| M1 | The core library: strings, arrays, exceptions, `Class` basics | S9–S16 | done | Deferred walls tracked in #21 and #28; [M1 note](docs/notes/M1-core-library.md) |
| M2 | Loaders and modules: `(loader, name)` identity, resources | — | todo | — |
| M3 | Real threads and monitors | — | todo | ADR-0010 |
| M4 | `MethodHandle`, `VarHandle`, `Unsafe` | — | todo | ADR-0012 |
| M5 | Breadth: `java.util`, `java.io`, `java.nio`; a real jar | — | todo | — |
| G2 | A program prints on the host library, compared with the reference launcher | — | deferred | requirements § Deferred goals, ADR-0014 |
| G1 | A JIT: compile one hot method, with the interpreter as the fallback and the oracle | — | deferred | requirements § Deferred goals, ADR-0013 |
| — | Deferred non-goals: JNI for third-party natives, Java SE certification | — | deferred | requirements § Non-goals |

## Definition of Done

A slice is done only when all of the following hold:

1. Its acceptance command exits 0, and the command is in the slice row.
2. A note in `docs/notes/` records what it taught — the surprises, the specification, the bug.
3. The slice row and the slice log both say `done`, with the date and the record.
4. The documents that own a changed fact are updated in the same change (requirements
   traceability, ADR, README) — not afterwards.
5. `cargo xtask ci` and `bash scripts/workflow-check.sh .` exit 0.

## Ways of working

- **The reference launcher is the oracle.** A program is accepted when `rava` prints exactly
  what the host `java` prints, byte for byte. When a test needs a golden file, a script
  generates it and the file is committed; the test itself must not need a JDK.
- **One note per slice**, in `docs/notes/`, written when the acceptance command first passes.
- **Signed commits.** `commit.gpgsign` is on; a slice lands as one commit (or one pull request).
- **Pull requests merge locally**, never through the host's rebase button:
  `git switch main && git merge --ff-only <branch> && git push`, then verify
  `git log --format='%h %G?' -1` reports `G`. A rebase merge rewrites the commit and strips its
  signature; that is not accepted here.
- **No owned class library.** `java.*` is the host's; `scripts/check-invariants.sh` fails if a
  `stdlib/` or `samples/` tree appears at the root.
- **No third-party Rust crates** without an ADR.

## Slices

> Vertical slices, thinnest first. Each row must carry an acceptance command — a slice without
> one is a wish. Status: `todo` | `in-progress` | `done` | `dropped`. The checker reads the
> columns in this exact order.

| Id | Slice | Requirements | Acceptance command | Status |
| :--- | :--- | :--- | :--- | :--- |
| S1 | Workspace, crate boundaries and the class-file parser, migrated from the previous iteration | R2 | `cargo test -p rava-classfile` exits 0 | done |
| S2 | Heap, class model, resolution, monitors and the interpreter, migrated | R6–R9 | `cargo test -p rava-vm -p rava-interp` exits 0 | done |
| S3 | The plan documents and the ledger for the hosting model | process | `bash scripts/workflow-check.sh .` exits 0 | done |
| S4 | Read the host image: enumerate `java.base`, load `java/lang/Object` from `lib/modules` or an extracted image | R3 | `cargo test -p rava-vm --test image` exits 0 | done |
| S5 | Complete the instruction set the library uses: `dup2`, `dup_x1`, `dup_x2`, `dup2_x1`, `dup2_x2`, `swap`, and `wide`/`goto_w` if the host needs them | R2 | `cargo test -p rava-vm --test instr` exits 0 | done |
| S6 | Bootstrap the core classes from the host; the synthetic `Object` path is already gone | R4 | `cargo test -p rava-vm --test bootstrap` exits 0 | done |
| S7 | First JCL natives: `System.out`, `PrintStream`, `FileOutputStream`, `FileDescriptor` | R5 | `cargo test -p rava-cli --test natives` exits 0 | done |
| S8 | M0 acceptance: a program loads from the image, runs, and its result is verified | R1 | `cargo test -p rava-cli --test boot` exits 0 | done |
| S9 | Class objects are instances of the host's `java.lang.Class`, with a VM-side reverse map | R4 | `cargo test -p rava-cli --test class_object` exits 0 | done |
| S10 | Strings are instances of the host's `java.lang.String`; the field-only methods run | R4 | `cargo test -p rava-cli --test strings` exits 0 | done |
| S11 | Array class objects: an array knows its class descriptor, so `getClass`, `instanceof`/`checkcast`, `getComponentType` and the `Class` queries work for arrays | R4 | `cargo test -p rava-cli --test arrays` exits 0 | done |
| S12 | Exception basics: `Throwable.fillInStackTrace` captures a trace once per throwable, so constructing one works and an uncaught one is reported | R6 | `cargo test -p rava-cli --test exceptions` exits 0 | done |
| S13 | The `Class` query natives: `isInstance`, `isAssignableFrom`, `getSuperclass`, `getModifiers` | R4 | `cargo test -p rava-cli --test class_basics` exits 0 | done |
| S16 | `StringBuilder` runs through the host classes; the VM-side `Builder` shortcut is removed | R4 | `cargo test -p rava-cli --test stringbuilder` exits 0 | done |
| S15 | The rest of `String` runs (`hashCode`, `equals`, `compareTo`, `concat`, `substring`), on top of a local-variable slot fix | R4 | `cargo test -p rava-cli --test strings --test locals` exits 0 | done |
| S14 | `System.arraycopy`: primitives and reference arrays, bounds and kind checks, overlap-safe | R4 | `cargo test -p rava-cli --test arraycopy` exits 0 | done |

## Slice log

> Acceptance evidence for completed slices: the command, its exit code, the date, and the
> record that holds the detail.

| Id | Command | Exit | Date | Record |
| :--- | :--- | :--- | :--- | :--- |
| S1 | `cargo test -p rava-classfile` | 0 | 2026-11-18 | [S1 note](docs/notes/S1-parser-migrated.md) |
| S2 | `cargo test -p rava-vm -p rava-interp` | 0 | 2026-11-18 | [S2 note](docs/notes/S2-vm-core-migrated.md) |
| S3 | `bash scripts/workflow-check.sh .` | 0 | 2026-11-18 | [S3 note](docs/notes/S3-hosting-plan.md) |
| S4 | `cargo test -p rava-vm --test image` | 0 | 2026-11-19 | [S4 note](docs/notes/S4-image-reader.md) |
| S5 | `cargo test -p rava-vm --test instr` | 0 | 2026-11-20 | [S5 note](docs/notes/S5-instr-complete.md) |
| S6 | `cargo test -p rava-vm --test bootstrap` | 0 | 2026-11-20 | [S6 note](docs/notes/S6-bootstrap.md) |
| S7 | `cargo test -p rava-cli --test natives` | 0 | 2026-11-20 | [S7 note](docs/notes/S7-first-natives.md) |
| S9 | `cargo test -p rava-cli --test class_object` | 0 | 2026-11-20 | [S9 note](docs/notes/S9-class-objects.md) |
| S10 | `cargo test -p rava-cli --test strings` | 0 | 2026-11-20 | [S10 note](docs/notes/S10-string-instances.md) |
| S11 | `cargo test -p rava-cli --test arrays` | 0 | 2026-11-20 | [S11 note](docs/notes/S11-array-classes.md) |
| S8 | `cargo test -p rava-cli --test boot` | 0 | 2026-11-20 | [S8 note](docs/notes/S8-boot-mounted.md) |
| S12 | `cargo test -p rava-cli --test exceptions` | 0 | 2026-11-20 | [S12 note](docs/notes/S12-exceptions.md) |
| S13 | `cargo test -p rava-cli --test class_basics` | 0 | 2026-11-20 | [S13 note](docs/notes/S13-class-basics.md) |
| S16 | `cargo test -p rava-cli --test stringbuilder` | 0 | 2026-11-20 | [S16 note](docs/notes/S16-stringbuilder.md) |
| S15 | `cargo test -p rava-cli --test strings --test locals` | 0 | 2026-11-20 | [S15 note](docs/notes/S15-string-methods.md) |
| S14 | `cargo test -p rava-cli --test arraycopy` | 0 | 2026-11-20 | [S14 note](docs/notes/S14-arraycopy.md) |

## Decision log

> One line per accepted ADR, oldest first. The ADR file holds the detail; this is the index.

| ADR | Decision | Status |
| :--- | :--- | :--- |
| [0001](docs/adr/0001-host-the-reference-class-library.md) | Host the reference class library; implement its native surface in Rust | accepted |
| [0002](docs/adr/0002-crate-boundaries.md) | Five crates with one-way, compiler-enforced dependencies | accepted |
| [0003](docs/adr/0003-decode-once.md) | Bytecode decoded into an internal instruction form at load | accepted |
| [0004](docs/adr/0004-heap-indices.md) | References are indices into a VM-owned heap, never pointers | accepted |
| [0005](docs/adr/0005-jcl-natives.md) | JCL natives in Rust, keyed by `(owner, name, descriptor)`; no JNI | accepted |
| [0006](docs/adr/0006-class-identity.md) | Class identity is `(defining loader, binary name)` | accepted |
| [0007](docs/adr/0007-license-mit.md) | MIT licence | accepted |
| [0008](docs/adr/0008-target-version-and-image.md) | Target class-file version 65; classes come from the host image | accepted |
| [0009](docs/adr/0009-collector.md) | Mark-sweep with a free list, triggered explicitly or by budget | accepted |
| [0010](docs/adr/0010-real-threads.md) | Real OS threads and monitors, not serialized `Thread.start` | accepted |
| [0011](docs/adr/0011-verifier-scope.md) | Structural verification first; type checking only when needed | accepted |
| [0012](docs/adr/0012-invokedynamic.md) | Bootstraps by name first; real method handles at M4 | accepted |
| [0013](docs/adr/0013-jit-after-interpreter.md) | A JIT is a deferred goal after M5, with the interpreter as the oracle | accepted |
| [0014](docs/adr/0014-println-after-unsafe.md) | `println` is G2, after M4, not part of M0's gate | accepted |

## Environments

| Environment | URL / target | Deploy command | Notes |
| :--- | :--- | :--- | :--- |
| local | — | `cargo test` | no deploy target; P5 is `n/a` |
| origin | `git@github.com:yusiwen/rava.git` | `git push origin main` | branch `main`; commits signed; the remote is recreated by the human after the reset |

## Resume

> For the next session, human or agent. A few lines, kept current.

1. `bash scripts/workflow-check.sh .` — read the failures first.
2. **The migration baseline is in place** (S1–S3): the parser, heap, class model, resolution,
   monitors and interpreter were carried over from the previous iteration; its class library,
   corpus and synthetic `Object` were not. `cargo test` passes on the migrated unit tests, and
   `cargo xtask ci` is green.
3. **M0 and M1 are complete (S4–S16)**. Strings, arrays, exceptions and the `Class` queries run
   against the host library; `System.arraycopy` and `StringBuilder` work. The remaining walls are
   M4-shaped and tracked: `Class.getInterfaces` and the reflection data need `Unsafe`'s
   object-field model (#28); Java-side throwable stack traces need a decision on the JDK's
   `backtrace` format (#21); printing is G2 (ADR-0014), after `Unsafe`. **Next is M2** (loaders
   and modules) unless the human reorders it before `Unsafe`.
4. **The previous project is archived** at `/home/yusiwen/git/jvm` and is read-only history.
   The GitHub repository is deleted and recreated by the human; the local origin in
   `## Environments` is the intended remote, not a live one.
5. **A JIT is planned, not refused** (ADR-0013): it starts after M5, with one hot method and
   the interpreter as the oracle. It is not on the critical path, so nothing blocks on it.
6. **The native report landed (issue #2)** as `rava natives`: it walks a program's constant-pool
   references and reconciles every declared native against the registry. From `java.lang.System`
   it reports 679 natives over 5,426 classes in about half a second. It came before S7 because
   that list is what S7 needs, not after.
7. Known blockers: **S7 is blocked on a plan decision (#9)** — `println` reaches
   `HeapByteBuffer.<clinit>` and therefore `Unsafe`, which the milestone order puts after M0.
8. **#1 landed early (M0, not M1)**: the loader applies `ConstantValue` at preparation, so
   `String.UTF16` reads 1 and `Integer.MAX_VALUE` is no longer 0.
