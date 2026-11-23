# PROJECT — rava (a Java 17 runtime for Spring)

**Tier:** L
**Owner:** yusiwen
**License:** MIT
**Started:** 2026-11-22

> Ledger. Nothing is committed and there is no runtime code yet. P0 and P1 were
> accepted on 2026-11-23; implementation starts with M1. Keep this file honest and terse.

## Phase status

| Phase | What must be true | Artifact | Approved by | Status |
| :--- | :--- | :--- | :--- | :--- |
| P0 | Requirements confirmed by the human | `docs/requirements.md` | human | done |
| P1 | Decisions recorded and accepted | `docs/adr/` | human | done |
| P2 | Work sliced into verifiable increments | `PROJECT.md` | self | todo |
| P3 | Every slice implemented | code + commits | self | todo |
| P4 | Tests pass, definition of done met | test suite | self | todo |
| P5 | Deployed, smoke-tested, rollback ready | `docs/deploy.md` | human | n/a |

P5 is `n/a` because this project ships a runtime library, not a hosted service; a
release process, if wanted, becomes its own phase.

## Milestones

> Gates are defined in `docs/requirements.md` § Milestones. P0 and P1 are accepted; the
> M0 foundation slice is next.

| Milestone | Delivers | Gate | Status |
| :--- | :--- | :--- | :--- |
| M0 | Foundation: parser/model, loader/ClassSource, heap/IR, Shared/ThreadState, safepoints, native registry, MemoryRegion | Hello + small corpus; metadata dump; loader identity; no-lock invariant | todo |
| M1 | Surface manifest, target lock, `link-check` | Manifest reproducible; unresolved = 0 | todo |
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
| — | No slice is defined yet | — | — | — |

P0 and P1 are accepted. Slices are created per milestone; the first is M0, with a
runnable acceptance command.

## Codebase

The implementation may reuse existing Rust components (class-file parser, instruction
model, interpreter) where they fit. Each imported component is re-reviewed against
this project's ADRs and covered by tests; no decision record is imported with the code.

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
3. P0 and P1 are accepted; the next work is the M0 foundation slice, with an
   acceptance command.
4. Do not write runtime code outside an M0 slice.

## Open decisions

No open decisions. [`docs/open-questions.md`](docs/open-questions.md) records all
answers; new questions get new ids and an ADR when they change a decision.
