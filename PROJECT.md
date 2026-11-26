# PROJECT — rava (a Java 17 runtime for Spring)

**Tier:** L
**Owner:** yusiwen · **License:** MIT · **Started:** 2026-11-22

> Ledger and index: status, the slice index, decisions and a resume card. Slice narrative
> lives in `docs/slices/`, milestone history in `docs/notes/`, mechanism in
> `docs/explanation/`. `bash scripts/doc-check.sh .` holds this file to 200 lines.

**Now:** M0–M4 done; M5 is next and not yet sliced · **Last gates:** C2 and the M4 non-web
Boot application are green (2026-11-26) · **Open decisions:** none

## Phase status

| Phase | What must be true | Artifact | Approved by | Status |
| :--- | :--- | :--- | :--- | :--- |
| P0 | Requirements confirmed by the human | `docs/requirements.md` | human | done |
| P1 | Decisions recorded and accepted | `docs/adr/` | human | done |
| P2 | Work sliced into verifiable increments | `PROJECT.md` | self | done |
| P3 | Every slice implemented | code + commits | self | todo |
| P4 | Tests pass, definition of done met | test suite | self | todo |
| P5 | Deployed, smoke-tested, rollback ready | `docs/deploy.md` | human | n/a |

P5 is `n/a` because this project ships a runtime library, not a hosted service; a release
process, if wanted, becomes its own phase.

## Milestones

> Gates are defined in `docs/requirements.md` § Milestones; `Closed` is the date the
> milestone's last slice's acceptance run passed.

| Milestone | Delivers | Gate | Status | Closed |
| :--- | :--- | :--- | :--- | :--- |
| M0 | Foundation: parser/model, loader identity, heap/IR, safepoints, verifier skeleton, native registry | Hello + corpus; metadata dump; loader identity; no-lock invariant | done | 2026-11-24 |
| M1 | Surface manifest, target lock, `link-check` | Manifest reproducible; unresolved = 0 | done | 2026-11-24 |
| M2 | C1 metadata, minimal library, Spring context | Metadata diff 0; bean graph starts | done | 2026-11-24 |
| M3 | C2 dynamic code: Proxy, CGLIB, Objenesis, `Lookup`, `Unsafe` U0 | C2 suite exits 0 | done | 2026-11-25 |
| M4 | C3 resources/jars, `-jar`, non-web Boot | Boot non-web application starts | done | 2026-11-26 |
| M5 | C3 concurrency, JMM, references, cleaner, Tomcat | Tomcat concurrent; JCStress 0 | todo | — |
| M6 | C4 reference web application | HTTP suite exits 0 | todo | — |
| M7 | Spring test subset | Spring subset exits 0 | todo | — |
| M8 | Performance envelope | ADR-0010 benchmark exits 0 | todo | — |
| M9 | Reactive target (after M0–M8) | Reactive HTTP suite exits 0 | todo | — |

## Slices

> One line per slice: id, what it delivers, the requirement it serves, the command
> whose exit code 0 proves it, and the status. Detail is in `## Slice records`.

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
| M1.2 | Static manifest over the pinned jars | R1 | `cargo run -p rava-cli -- compat manifest --check` exits 0 | done |
| M1.3 | Stub library surface and zero-unresolved link-check | R2 | `cargo run -p rava-cli -- compat link-check` exits 0 | done |
| M1.4 | Manifest reproducibility and M1 gate review | R1, R2 | `cargo run -p rava-cli -- compat check` exits 0 | done |
| M2.1 | C1 metadata model and normalized dump | R3 | `cargo test -p rava-classfile --test metadata_model` exits 0 | done |
| M2.2 | JDK 17 signature stubs and zero metadata diff | R3 | `cargo run -p rava-cli -- compat metadata --check` exits 0 | done |
| M2.3 | `stdlib/c1/classes` as the bootstrap library and its metadata through the VM | R3 | `cargo test -p rava-vm --test c1_runtime` exits 0 | done |
| M2.4 | Class objects and basic `Object`/`Class` reflection | R3 | `cargo test -p rava-interp --test class_objects` exits 0 | done |
| M2.5 | Instance allocation, constructors, fields and instance calls | R3 | `cargo test -p rava-interp --test instance_execution` exits 0 | done |
| M2.6 | Arrays and integer control flow | R3 | `cargo test -p rava-interp --test arrays_control` exits 0 | done |
| M2.7 | Exceptions, `athrow` and exception tables | R3 | `cargo test -p rava-interp --test exceptions` exits 0 | done |
| M2.8 | Reflection member objects (`Field`/`Method`/`Constructor`) | R3 | `cargo test -p rava-interp --test reflection_members` exits 0 | done |
| M2.9 | Core `Object`/`String` behavior | R3 | `cargo test -p rava-interp --test string_object` exits 0 | done |
| M2.10 | Boxed values and reflection invocation | R3 | `cargo test -p rava-interp --test reflection_invoke` exits 0 | done |
| M2.11 | Annotation reflection and proxies | R3 | `cargo test -p rava-interp --test annotations` exits 0 | done |
| M2.12 | `StringBuilder` and core collection behavior | R3 | `cargo test -p rava-interp --test collections` exits 0 | done |
| M2.13 | `HashMap` and `String.valueOf`/value conversion | R3 | `cargo test -p rava-interp --test maps_values` exits 0 | done |
| M2.14 | Named member lookup (`getDeclaredMethod`/`getField`/`getConstructor`) | R3 | `cargo test -p rava-interp --test member_lookup` exits 0 | done |
| M2.15 | Primitive `Class` identity and primitive member types | R3 | `cargo test -p rava-interp --test primitive_classes` exits 0 | done |
| M2.16 | Array `Class` identity and array member types | R3 | `cargo test -p rava-interp --test array_classes` exits 0 | done |
| M2.17 | `ldc`/`ldc_w` class constants and class literals | R3 | `cargo test -p rava-interp --test class_literals` exits 0 | done |
| M2.18 | `invokedynamic` string concatenation | R3 | `cargo test -p rava-interp --test invokedynamic_concat` exits 0 | done |
| M2.19 | Lambda metafactory and `invokeinterface` | R3 | `cargo test -p rava-interp --test lambdas` exits 0 | done |
| M2.20 | Classpath runner (`rava run`) | R3 | `cargo test -p rava-cli --test classpath_run` exits 0 | done |
| M2.21 | Static fields, casts, switches and numeric constants | R3 | `cargo test -p rava-interp --test bytecode_surface` exits 0 | done |
| M2.22 | Generic `Type` graph and generic reflection | R3 | `cargo test -p rava-interp --test generic_types` exits 0 | done |
| M2.23 | Spring `AnnotationConfigApplicationContext` bean graph | R3 | `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` exits 0 | done |
| M2.24 | `java.util.stream` pipeline for Spring's annotation metadata | R3 | `cargo test -p rava-interp --test streams` exits 0 | done |
| M2.25 | Full primitive-box family and Number accessors | R3 | `cargo test -p rava-interp --test primitive_boxes` exits 0 | done |
| M2.26 | `instanceof(null)` correctness and map constructor surface | R3 | `cargo test -p rava-interp --test spring_fixes` exits 0 | done |
| M2.27 | Descriptor-closure manifest and C1-tree re-baseline | R3 | `cargo run -p rava-cli -- compat check` exits 0; the Spring rerun reaches the next blocker | done |
| M2.28 | Method/Executable bridge, varargs and synthetic flags | R3 | `cargo test -p rava-interp --test method_flags` exits 0 | done |
| M2.29 | Minimal `java.text.MessageFormat` for Spring messages | R3 | `cargo test -p rava-interp --test message_format` exits 0 | done |
| M2.30 | `NoSuchMethodException`/`NoSuchFieldException` lookup misses | R3 | `cargo test -p rava-interp --test lookup_exceptions` exits 0 | done |
| M3.1 | Generated-class definition: verify every `Code` method before insertion; decode switches and `wide` | R4 | `cargo test -p rava-cli --test c2_generated` exits 0 | done |
| M3.2 | `java.lang.reflect.Proxy` / `InvocationHandler`, generated proxy classes and default-method dispatch | R4 | `cargo test -p rava-cli --test c2_proxy` exits 0 | done |
| M3.3 | `ClassLoader` hierarchy, `defineClass`, loader-aware `Class.forName` and `getClassLoader` | R4 | `cargo test -p rava-cli --test c2_classloader` exits 0 | done |
| M3.4 | `MethodHandles.Lookup`, `MethodType`/`MethodHandle`, `Lookup.defineClass` | R4 | `cargo test -p rava-cli --test c2_lookup` exits 0 | done |
| M3.5 | `Unsafe` U0, VM-installed static reflection, direct buffers and the U0 call log | R4, R12 | `cargo test -p rava-cli --test c2_unsafe` exits 0 | done |
| M3.6 | Unmodified Spring CGLIB enhancement and Objenesis instantiation | R4 | `RAVA_C2_ACCEPTANCE=1 cargo test -p rava-cli --test c2_cglib` exits 0 | done |
| M3.7 | Spring AOP proxies (JDK and CGLIB) and the C2 gate | R4, R12 | `RAVA_C2_ACCEPTANCE=1 cargo test -p rava-cli --test c2_gate` exits 0 | done |
| M4.1 | Classpath resources: directory/jar lookup, `ClassLoader`/`Class` resource natives, `file:`/`jar:` URL streams | R8 | `cargo test -p rava-cli --test resources` exits 0 | done |
| M4.2 | `ServiceLoader` over `META-INF/services`, lazy provider iteration and typed configuration errors | R8 | `cargo test -p rava-cli --test service_loader` exits 0 | done |
| M4.3 | Executable-jar launch: manifest parsing, `Main-Class`, `Class-Path` and `rava -jar` | R8 | `cargo test -p rava-cli --test jar_launch` exits 0 | done |
| M4.4 | Boot nested-jar layout: `BOOT-INF/classes`, `BOOT-INF/lib/*.jar`, nested `jar:` URLs and launcher classpath | R8 | `cargo test -p rava-cli --test nested_jars` exits 0 | done |
| M4.5 | Non-web Boot gate: real `SpringApplication`, `WebApplicationType.NONE` | R5, R8 | `RAVA_BOOT_ACCEPTANCE=1 cargo test -p rava-cli --test boot_nonweb` exits 0 | done |

## Slice records

- M0 — [`docs/slices/M0.md`](docs/slices/M0.md)
- M1 — [`docs/slices/M1.md`](docs/slices/M1.md)
- M2 — [M2.1–M2.12](docs/slices/M2-1-c1-and-object-model.md),
  [M2.13–M2.22](docs/slices/M2-2-reflection-and-dynamic.md),
  [M2.23–M2.30](docs/slices/M2-3-spring-context.md)
- M3 — [`docs/slices/M3.md`](docs/slices/M3.md)
- M4 — [`docs/slices/M4.md`](docs/slices/M4.md)

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

> Current state only, never history. History lives in `docs/slices/` and `docs/notes/`.
> Keep this under 20 lines and update it in place instead of appending to it.

**Now:** M0–M4 are done. M5 (concurrency, Java memory model, references, cleaner, Tomcat)
is next but is not sliced yet; nothing is in progress.

**Gates that must stay green:** `cargo xtask ci`; `RAVA_C2_ACCEPTANCE=1 cargo test -p
rava-cli --test c2_gate`; `RAVA_BOOT_ACCEPTANCE=1 cargo test -p rava-cli --test boot_nonweb`;
`cargo run -p rava-cli -- compat check`.

1. Run `bash scripts/doc-check.sh .` first; then read `## Phase status` and `## Slices`.
   The first row that is not `done` is where to continue. If the ledger and the code
   disagree, the code wins — fix the ledger.
2. Read `docs/slices/M4.md` for the shape and the boundaries the last milestone left.
3. Read the ADRs that govern the next work: 0007 (threads and the memory model) and 0008
   (references and cleaner).
4. Read `docs/explanation/` for the mechanism the next slice touches.
5. Slice before code: add the rows to `## Slices` with their acceptance commands, then
   implement, then record `Delivered`/`Boundary`/`Evidence` in the milestone's slice file.

## Open decisions

No open decisions. [`docs/open-questions.md`](docs/open-questions.md) records all
answers; new questions get new ids and an ADR when they change a decision.
