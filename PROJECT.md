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
| M0 | Boot: read the host image, complete the instruction set the library uses, bootstrap `Object`/`String`/`Class`/`System`, first JCL natives, `println` | S4–S8 | todo | — |
| M1 | The core library: strings, arrays, exceptions, `Class` basics | — | todo | #1 (`ConstantValue`) |
| M2 | Loaders and modules: `(loader, name)` identity, resources | — | todo | — |
| M3 | Real threads and monitors | — | todo | ADR-0010 |
| M4 | `MethodHandle`, `VarHandle`, `Unsafe` | — | todo | ADR-0012 |
| M5 | Breadth: `java.util`, `java.io`, `java.nio`; a real jar | — | todo | — |
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
| S4 | Read the host image: enumerate `java.base`, load `java/lang/Object` from `lib/modules` or an extracted image | R3 | `cargo test -p rava-vm --test image` exits 0 | todo |
| S5 | Complete the instruction set the library uses: `dup2`, `dup_x1`, `dup_x2`, `dup2_x1`, `dup2_x2`, `swap`, and `wide`/`goto_w` if the host needs them | R2 | `cargo test -p rava-vm --test instr` exits 0 | todo |
| S6 | Bootstrap the core classes from the host; the synthetic `Object` path is already gone | R4 | `cargo test -p rava-vm --test bootstrap` exits 0 | todo |
| S7 | First JCL natives: `System.out`, `PrintStream`, `FileOutputStream`, `FileDescriptor` | R5 | `cargo test -p rava-cli --test natives` exits 0 | todo |
| S8 | M0 acceptance: a program that prints, compared with the host launcher | R1 | `cargo test -p rava-cli --test boot` exits 0 | todo |

## Slice log

> Acceptance evidence for completed slices: the command, its exit code, the date, and the
> record that holds the detail.

| Id | Command | Exit | Date | Record |
| :--- | :--- | :--- | :--- | :--- |
| S1 | `cargo test -p rava-classfile` | 0 | 2026-11-18 | [S1 note](docs/notes/S1-parser-migrated.md) |
| S2 | `cargo test -p rava-vm -p rava-interp` | 0 | 2026-11-18 | [S2 note](docs/notes/S2-vm-core-migrated.md) |
| S3 | `bash scripts/workflow-check.sh .` | 0 | 2026-11-18 | [S3 note](docs/notes/S3-hosting-plan.md) |

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
3. **Next is S4**: read the host image. The JDK's classes live in `lib/modules`; the first
   acceptance is loading `java/lang/Object.class` from it, with an extracted directory as the
   fallback. Nothing can run until the bootstrap classes load, so this is the critical path.
   **S5 exists because of a measurement**: the host library's own methods use opcodes this
   interpreter does not decode (`dup2` in `java.lang.String`, `dup_x1`/`dup_x2` in
   `ArrayList`/`HashMap`/`Thread`), and decoding happens at load, so the set must be complete
   before the bootstrap classes can even be loaded.
4. **The previous project is archived** at `/home/yusiwen/git/jvm` and is read-only history.
   The GitHub repository is deleted and recreated by the human; the local origin in
   `## Environments` is the intended remote, not a live one.
5. **A JIT is planned, not refused** (ADR-0013): it starts after M5, with one hot method and
   the interpreter as the oracle. It is not on the critical path, so nothing blocks on it.
6. Known blockers: none.
