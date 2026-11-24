# PROJECT — rava (a Java 17 runtime for Spring)

**Tier:** L
**Owner:** yusiwen
**License:** MIT
**Started:** 2026-11-22

> Ledger. P0 and P1 were accepted on 2026-11-23. M0 is complete; M1 is in progress.
> Keep this file honest and terse.

## Phase status

| Phase | What must be true | Artifact | Approved by | Status |
| :--- | :--- | :--- | :--- | :--- |
| P0 | Requirements confirmed by the human | `docs/requirements.md` | human | done |
| P1 | Decisions recorded and accepted | `docs/adr/` | human | done |
| P2 | Work sliced into verifiable increments | `PROJECT.md` | self | done |
| P3 | Every slice implemented | code + commits | self | todo |
| P4 | Tests pass, definition of done met | test suite | self | todo |
| P5 | Deployed, smoke-tested, rollback ready | `docs/deploy.md` | human | n/a |

P5 is `n/a` because this project ships a runtime library, not a hosted service; a
release process, if wanted, becomes its own phase.

## Milestones

> Gates are defined in `docs/requirements.md` § Milestones. P0 and P1 are accepted;
> M0.1–M0.7 are done, and the M0 gate review is the remaining lead step.

| Milestone | Delivers | Gate | Status |
| :--- | :--- | :--- | :--- |
| M0 | Foundation: parser/model, loader/ClassSource, heap/IR, Shared/ThreadState, safepoints, verifier skeleton, native registry, MemoryRegion | Hello + small corpus; metadata dump; loader identity; no-lock invariant | in-progress |
| M1 | Surface manifest, target lock, `link-check` | Manifest reproducible; unresolved = 0 | in-progress |
| M2 | C1 metadata, minimal library, Spring context | Metadata diff 0; bean graph starts | todo |
| M3 | C2 dynamic code: Proxy, CGLIB, Objenesis, `Lookup`, `Unsafe` U0 | C2 suite exits 0 | todo |
| M4 | C3 resources/jars, `-jar`, non-web Boot | Boot non-web application starts | todo |
| M5 | C3 concurrency, JMM, references, cleaner, Tomcat | Tomcat concurrent; JCStress 0 | todo |
| M6 | C4 reference web application | HTTP suite exits 0 | todo |
| M7 | Spring test subset | Spring subset exits 0 | todo |
| M8 | Performance envelope | ADR-0010 benchmark exits 0 | todo |
| M9 | Reactive target (after M0–M8) | Reactive HTTP suite exits 0 | todo |

## Slices

| Id | Slice | Requirements | Acceptance command | Status |
| :--- | :--- | :--- | :--- | :--- |
| M0.1 | Cargo workspace, toolchain, crates and the `xtask ci` gate | R13 | `cargo xtask ci` exits 0 | done |
| M0.2 | Class-file parser with full metadata retention | R13 | `cargo test -p rava-classfile` exits 0 | done |
| M0.3 | ClassSource and `(loader, name)` class identity | R13 | `cargo test -p rava-vm --test loader_identity` exits 0 | done |
| M0.4 | Heap handles, field layout and the `Shared`/`ThreadState` seams | R13 | `cargo test -p rava-vm --test heap_handles` exits 0 | done |
| M0.5a | Decoded IR, frames and a static call | R13 | `cargo test -p rava-interp --test arithmetic` exits 0 | done |
| M0.5b | Hello through the interpreter: class library and native minimum | R13 | `cargo test -p rava-interp --test hello` exits 0 | done |
| M0.6 | Safepoint polling, GC roots and the no-lock-across-Java-call invariant | R13 | `cargo test -p rava-vm --test safepoints` exits 0 | done |
| M0.7 | Verifier dataflow skeleton and the metadata dump fixture | R13 | `cargo test -p rava-vm --test verifier_skeleton` exits 0 | done |
| M1.1 | Target lock, reference app and pinned jar set | R2 | `cargo run -p rava-cli -- compat target` exits 0 | done |
| M1.2 | Static manifest over the pinned jars | R1 | `cargo run -p rava-cli -- compat manifest --check` exits 0 | todo |
| M1.3 | Stub library surface and zero-unresolved link-check | R2 | `cargo run -p rava-cli -- compat link-check` exits 0 | todo |
| M1.4 | Manifest reproducibility and M1 gate review | R1, R2 | `cargo xtask ci` exits 0 | todo |

P0 and P1 are accepted; P2 is done because the M0 increments are sliced. All M0 slices
are done and the M0 gate review passed.

## Slice log

| Slice | Acceptance command | Exit | Date | Evidence |
| :--- | :--- | :--- | :--- | :--- |
| M0.1 | `cargo xtask ci` | 0 | 2026-11-24 | Workspace, crates and the local gate; no runtime behavior yet |
| M0.2 | `cargo test -p rava-classfile` | 0 | 2026-11-24 | Parser implemented; raw attributes retained; 13 tests pass |
| M0.3 | `cargo test -p rava-vm --test loader_identity` | 0 | 2026-11-24 | Directory and DEFLATE jar sources; parent-first delegation; 6 tests pass |
| M0.4 | `cargo test -p rava-vm --test heap_handles` | 0 | 2026-11-24 | Generation handles, field layout, arrays and safepoint handshake; 6 tests pass |
| M0.5a | `cargo test -p rava-interp --test arithmetic` | 0 | 2026-11-24 | Decoded integer IR, frame, `invokestatic`; 3 tests pass |
| M0.5b | `cargo test -p rava-interp --test hello` | 0 | 2026-11-24 | `getstatic`/`ldc`/`invokevirtual`, class initialization and native PrintStream; 2 tests pass |
| M0.6 | `cargo test -p rava-vm --test safepoints` | 0 | 2026-11-24 | Frame-root stack with RAII cleanup, publish-before-acknowledge safepoint handshake, `Shared` root enumeration, lock probe at call boundaries, `MemoryRegion` seam and hook registration points; 9 tests pass; `cargo xtask ci` exits 0 |
| M0.7 | `cargo test -p rava-vm --test verifier_skeleton` | 0 | 2026-11-24 | Stack-height and definite-local dataflow over a branch/handler worklist, typed underflow/overflow/local/truncation/unsupported errors, deterministic metadata dump for `Hello.class`; 12 tests pass; `cargo xtask ci` exits 0 |
| M1.1 | `cargo run -p rava-cli -- compat target` | 0 | 2026-11-24 | `compat/targets.toml` pins Java 17, Boot 3.5.16, Spring 6.2.19, Tomcat 10.1.55, the reference app source hash and a 35-jar set; the reference app builds at release 17; a clean refetch reproduces all 35 sha256 entries and `bash compat/fetch-jars.sh --check` passes; `cargo test -p rava-cli` passes 17 tests |

M0.6 boundary: the reference and class-unloading hooks are registration points only.
M0 has no collector, so no weak/soft/phantom reference is cleared and no class is
unloaded; M5 (ADR-0008) implements them on this seam. `MemoryRegion` is a VM-owned
byte region with a fake address, not an off-heap allocator; S2-raw replaces the
backing store without changing the address/bounds API.

M0 gate review (2026-11-24): `cargo xtask ci` exits 0; Hello and the integer corpus
run through the interpreter; the committed metadata dump matches `javap`'s structural
view of `Hello.class`; loader identity has six tests; the safepoint/no-lock suite has
nine tests. Reflection-level JDK 17 metadata diff remains the M2/C1 gate.

M0.7 boundary: the verifier tracks operand-stack slots and definitely assigned local
slots, not Java types. It covers straight-line code, conditional and unconditional
branches, and exception-handler entry; `jsr`/`ret`, `wide`, `tableswitch` and
`lookupswitch` return `UnsupportedOpcode`. Type states, stack-map frames and
generated-class verification land at M3 (ADR-0012). The metadata dump is a stable text
fixture, not a reflection API.

M1.1 boundary: `compat target` validates that the target lock parses, that its
`[jars].count` agrees with `compat/jars.lock`, and that `compat/jars/` holds the
declared number of jars. It does not hash the jars (that is
`bash compat/fetch-jars.sh --check`) and does not scan the manifest; M1.2 adds the
static manifest and M1.3 the link-check.

## Decision log

| ADR | Decision | Status |
| :--- | :--- | :--- |
| [0001](docs/adr/0001-compatibility-first.md) | Compatibility-first; success is a pinned external application | accepted |
| [0002](docs/adr/0002-target-platform-and-versions.md) | Java 17, Spring 6.2.19, Boot 3.5.16, Tomcat 10.1.55 | accepted |
| [0003](docs/adr/0003-compatibility-contract.md) | Observed surface manifest plus C0–C4 gates | accepted |
| [0004](docs/adr/0004-runtime-class-library.md) | Full replacement runtime library, owned by us | accepted |
| [0005](docs/adr/0005-class-loading-and-dynamic-code.md) | Loaders, `defineClass`, `Proxy`, `Lookup`, `Unsafe` capability/fidelity tiers U0–U4/S0–S2 | accepted |
| [0006](docs/adr/0006-reflection-and-metadata.md) | Reflection reproduces JDK 17 metadata | accepted |
| [0007](docs/adr/0007-concurrency-and-memory-model.md) | Real OS threads and the Java memory model | accepted |
| [0008](docs/adr/0008-references-and-cleaner.md) | References, `Cleaner`, class unloading | accepted |
| [0009](docs/adr/0009-resources-jars-and-modules.md) | Jars, nested jars, resources, `ServiceLoader`, `-jar` | accepted |
| [0010](docs/adr/0010-execution-and-performance.md) | Measure first; choose interpreter/JIT against a budget | accepted |
| [0011](docs/adr/0011-scope-ordering-and-native-boundary.md) | Scope ordering, promotion rule and native/internal boundary | accepted |
| [0012](docs/adr/0012-runtime-foundation-and-upgrade-seams.md) | M0 foundation seams and additive/conditional/architecture-fork classification | accepted |

## Resume

A fresh session should:

1. Read `README.md`, `docs/requirements.md` and `docs/open-questions.md`.
2. Read the ADRs in order; 0001–0005 are the project-defining decisions.
3. M0 is complete; M1 is in progress. The next slice is M1.2 (static manifest over
   the pinned jars).
4. Do not write runtime code outside a defined slice.

## Open decisions

No open decisions. [`docs/open-questions.md`](docs/open-questions.md) records all
answers; new questions get new ids and an ADR when they change a decision.
