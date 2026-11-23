# Requirements — rava (a Java 17 runtime for Spring)

> **Status:** approved (P0, 2026-11-23). The open decisions are recorded in
> `docs/open-questions.md`; no item there blocks M1.
> **Date:** 2026-11-22
> **Owner:** yusiwen

## Problem

A Rust VM can parse, link, verify and execute javac output against a class library we
own. That proves the mechanism, not that a real framework runs on it.

That is a different problem. The interpreter is no longer the hard part; the platform
contract is. Spring loads hundreds of classes, reflects over annotations and generic
signatures, generates subclasses at run time, and depends on collection, concurrency,
resource and class-loading behaviour that a corpus never exercises. Without a way to
measure that contract, "run Spring" is a wish, not a requirement.

## Goals

- **G1 — Run the pinned reference application unchanged.** The application's jars are
  produced by the reference JDK; rava loads and runs them without source changes.
- **G2 — Make the platform surface measurable.** The scope is a versioned inventory of
  the JDK classes and members the reference application actually reaches, not "all of
  Java SE".
- **G3 — Keep the runtime library ours.** `java.*` classes remain our class files with
  Rust implementations; the real `java.base` is never loaded at run time.
- **G4 — Verify against the reference JDK.** Behaviour and metadata are compared with
  JDK 17 on the same host, and every claim is an exit code.
- **G5 — Operate within a performance envelope.** Startup and request latency are
  measured against JDK 17 and held to a budget fixed by ADR-0010.
- **G6 — Add reactive web after the servlet target.** Spring WebFlux is a committed
  later target, not part of the first acceptance (see § Later targets).

## Target

| Item | Value |
| :--- | :--- |
| Platform | Java SE 17 API subset, class-file version 61 |
| Spring Framework | 6.2.19 |
| Spring Boot | 3.5.16 |
| Embedded server | Tomcat 10.1.55 |
| Reference JDK | 17 (compile time and differential baseline) |
| Reference application | one servlet web app, `text/plain` endpoint, no JSON, no database, no Actuator |
| Host | Linux arm64 (the measurement host) |

The initial target is Java 17 because it is the baseline of Spring Framework 6.2.19
and Spring Boot 3.5.16, and because it keeps the surface smaller than Java 21. Virtual
threads and other Java 21 APIs are out of scope until a manifest says otherwise.

The exact patch versions and the reference application's commit are part of the
target, not an implementation detail: a version bump changes the jars, the manifest
and therefore the contract. They live in one place (`compat/targets.toml`, proposed)
and a bump is a re-baseline, not a silent upgrade. The reference application's commit
is created and pinned as the first M1 task (ADR-0002).

Reactive web (Spring WebFlux) is a committed later target, not part of the first
acceptance. Its server choice and `Unsafe`/direct-memory consequences are deferred to
M9 (ADR-0002).

## Success criterion

The **first (servlet) target** is successful when all of the following pass on the
reference host:

1. `rava link-check` reports **0 unresolved** `(owner, name, descriptor)` references
   over the pinned jar set.
2. The reference application starts on rava, serves `GET /` with `200` and the
   expected `text/plain` body, and shuts down cleanly.
3. The compatibility suite for C0–C4 exits `0`.
4. The selected JCStress suite exits `0`.
5. The performance budget in § Performance passes.

Loading a jar, or starting a context once, is not success.

## Scope: the observed surface

The scope is a **surface manifest** derived from the reference run, not from the
Java SE specification:

- **Static references.** Scan every pinned jar's constant pool for references to
  platform packages (`java/`, `javax/`, `jdk/`, `sun/`), recording owner, name and
  descriptor.
- **Dynamic loading.** Run the reference application on JDK 17 with class-load logging
  (`-Xlog:class+load=info` or `-verbose:class`) and record every loaded class.
- **Reflective access.** Record the members reached through `Class`, `Field`, `Method`,
  `Constructor`, `MethodHandles`, `VarHandle` and `Unsafe`. The mechanism (agent, JFR
  or a logged VM shim) is an implementation decision; the manifest must include it.
- **Metadata.** Record the annotations, generic signatures, record components, sealed
  hierarchies and parameter names the application reads.

The union is written to a versioned file (for example
`compat/surface-<target>.txt`) with counts. That file is the scope: members in it
must exist with matching signatures and behaviour; members outside it are not
promised. Every target-version change regenerates the manifest and re-opens the gap
count.

**Closed-world caveat.** A path not exercised by the reference run can still be
reached in production. The manifest reduces the risk; the Spring test subset in R10
and the failure-attribution rule in R11 bound it. This is accepted, not hidden.

## Compatibility levels

Each level is a gate with an exit code. A later level may start before an earlier one
is complete, but the reference application is not "supported" until all five pass.

| Level | What it means | Acceptance |
| :--- | :--- | :--- |
| **C0 — Link** | Every symbolic reference in the pinned jars resolves; classes load, link and verify | `rava link-check --cp <pinned jars>` reports 0 unresolved |
| **C1 — Metadata** | Reflection sees the same shape as JDK 17: annotations, generic signatures, records, sealed types, nestmates, parameter names | `rava compat metadata` produces a normalized diff of 0 |
| **C2 — Dynamic code** | `Proxy`, `defineClass`, CGLIB/Objenesis, `MethodHandles.Lookup`, the open `Unsafe` tier (U0), the verifier accepts generated classes | proxy/CGLIB/Objenesis tests exit 0 |
| **C3 — Runtime services** | Collections, concurrency, time, I/O, NIO, networking, charsets, regex, `java.beans`, jars and resources, `ServiceLoader` | manifest-driven API tests exit 0; Spring context starts |
| **C4 — Framework** | The reference Boot web application and the selected Spring test subset run end to end | HTTP assertions and Spring tests exit 0; JCStress exits 0 |

## Requirements

> Command names below are provisional; the ADRs own the mechanism, this table owns the
> observable property.

### R1 — Surface manifest

- **Statement:** Produce a reproducible manifest of platform classes, members,
  annotations and signatures reached by the pinned application.
- **Acceptance:** `rava compat manifest --target <target>` regenerates the committed
  manifest; a byte-for-byte comparison against the committed file exits 0; the report
  includes class and member counts.
- **Priority:** must.

### R2 — Link compatibility (C0)

- **Statement:** Every `(owner, name, descriptor)` referenced by the pinned jars resolves
  to a class or member in our library.
- **Acceptance:** `rava link-check --cp <pinned jars>` exits 0 with `unresolved = 0`.
- **Priority:** must.

### R3 — Metadata compatibility (C1)

- **Statement:** For every class in the manifest, the reflective view matches JDK 17:
  modifiers, superclass, interfaces, fields, methods, constructors, annotations,
  generic signatures, record components, sealed permits, nestmates and parameter names.
- **Acceptance:** `rava compat metadata` diff against the JDK 17 dump exits 0 after
  normalizing ordering and synthetic members.
- **Priority:** must.

### R4 — Dynamic code (C2)

- **Statement:** Run-time generated classes work: `java.lang.reflect.Proxy`,
  `ClassLoader.defineClass`, `MethodHandles.Lookup.defineClass`, CGLIB enhancement and
  Objenesis instantiation; the verifier accepts the generated bytecode.
- **Acceptance:** the C2 suite (proxy, CGLIB, Objenesis, AOP) exits 0; generated classes
  pass verification, not a bypass.
- **Priority:** must.

### R5 — Runtime services (C3)

- **Statement:** The manifest's collection, concurrency, time, I/O, NIO, networking,
  charset, regex, `java.beans`, logging, jar/resource and `ServiceLoader` members
  behave as JDK 17 does for the exercised paths.
- **Acceptance:** manifest-driven API tests exit 0; the Spring context and the
  non-web Boot application start.
- **Priority:** must.

### R6 — Concurrency and memory model

- **Statement:** Java threads are real OS threads; `synchronized`, `wait`/`notify`,
  `Thread` lifecycle, `ThreadLocal`, `java.util.concurrent` and the `volatile`/`final`
  visibility rules work under contention.
- **Acceptance:** the selected JCStress suite exits 0; Tomcat serves concurrent
  requests with no lost updates.
- **Priority:** must.

### R7 — References and cleaner

- **Statement:** Weak, soft and phantom references, `ReferenceQueue`, `Cleaner`,
  `WeakHashMap` and class unloading work; direct buffers are reclaimed.
- **Acceptance:** reference tests and a soak test exit 0; no object is retained solely
  by a cleared reference.
- **Priority:** must.

### R8 — Resources, jars and modules

- **Statement:** Read jars (including Boot's nested jars), serve `getResource` and
  `getResourceAsStream`, run `ServiceLoader`, read module descriptors, and launch an
  executable jar via `java -jar` with its manifest main class.
- **Acceptance:** the reference Boot fat jar starts with `rava -jar`; resource and
  `ServiceLoader` tests exit 0.
- **Priority:** must.

### R9 — Performance envelope

- **Statement:** Startup and request latency stay within the budget fixed by ADR-0010.
- **Acceptance:** the benchmark command in ADR-0010 exits 0 and writes the numbers to
  `docs/benchmarks.md`.
- **Priority:** must (budget may be met by interpreter optimisation or by a JIT/tiered
  execution decision).

### R10 — Framework conformance

- **Statement:** The reference application and a selected subset of Spring's own tests
  pass without source changes. The subset excludes agent-based tests (Mockito inline
  mock maker) and Netty.
- **Acceptance:** the HTTP assertion suite exits 0; the selected Spring test subset
  exits 0.
- **Priority:** must.

### R11 — Evidence and CI

- **Statement:** Every claim is a command with an exit code; comparisons against JDK 17
  are normalized; a flaky failure is attributed, not retried silently.
- **Acceptance:** `cargo xtask ci` (proposed) runs the gates; the PR body and the
  closing note carry the command, exit code and counts.
- **Priority:** must.

### R12 — Boundaries

- **Statement:** The native boundary and JDK-internal surface are declared, not
  accidental: no general JNI, no arbitrary `jdk.internal.*`, and `Unsafe` is a
  capability-tier model (U0–U4) with per-family fidelity (S0–S2) whose unopened
  methods fail loudly.
- **Acceptance:** a tier/allowlist check exits 0; a runtime `Unsafe` call log contains
  only methods from open tiers; any other method fails the test.
- **Priority:** must.

### R13 — Foundation invariants

- **Statement:** The M0 foundation fixes the seams later milestones enable: class
  identity `(defining loader, binary name)`, full metadata retention, `ClassSource`
  over directories and jars, `ObjRef` handles with room for a generation, the
  `Shared`/`ThreadState` split, "no VM lock across a Java call", safepoint poll
  points, the decoded IR/frame contract, `MemoryRegion`/`DirectBuffer`, a thread-safe
  native registry, and reference/class-unloading hooks.
- **Acceptance:** `cargo xtask check-foundation` (proposed) exits 0; the M0 gate in
  § Milestones passes; later milestones do not add a second frame model, class table
  or memory abstraction.
- **Priority:** must.

## Traceability

| Requirement | Decision | Milestone |
| :--- | :--- | :--- |
| R1 | ADR-0003 | M1 |
| R2 | ADR-0003 | M1 |
| R3 | ADR-0006 | M2 |
| R4 | ADR-0005 | M3 |
| R5 | ADR-0004, ADR-0009 | M4–M5 |
| R6 | ADR-0007 | M5 |
| R7 | ADR-0008 | M5 |
| R8 | ADR-0009 | M4 |
| R9 | ADR-0010 | M8 |
| R10 | ADR-0001, ADR-0003 | M6–M7 |
| R11 | ADR-0003 | all |
| R12 | ADR-0011, ADR-0005 | all |
| R13 | ADR-0012 | M0 |

## Milestones

| Milestone | Delivers | Gate |
| :--- | :--- | :--- |
| **M0 — Foundation** | Class-file model with full metadata, `(loader, name)` identity, ClassSource (dir + jar), ObjRef/field layout, decoded IR/frame/call sites, native registry, Shared/ThreadState, safepoints, GC roots/handles, MemoryRegion, verifier skeleton | Hello + small corpus runs; metadata dump matches JDK 17; loader identity test; no-lock-across-Java-call invariant; single-thread safepoint test |
| **M1 — Measure and link** | Manifest, target lock, `link-check` over ClassSource; no execution yet | M1 exits 0: manifest reproducible, unresolved = 0 |
| **M2 — Metadata and context** | C1 metadata, minimal library, Spring `AnnotationConfigApplicationContext` with `proxyBeanMethods=false` | Metadata diff 0; a bean graph starts |
| **M3 — Dynamic code** | Proxy, CGLIB, Objenesis, `Lookup.defineClass`, `Unsafe` U0, verifier for generated classes | C2 suite exits 0 |
| **M4 — Resources and Boot** | Nested jars, fat jar, resources, `ServiceLoader`, `java -jar`; non-web Boot application (basic jar loading is M0) | Boot non-web application starts |
| **M5 — Concurrency and memory** | Enable OS threads and JMM on the M0 seams; JCStress, references, cleaner, Tomcat runtime | Tomcat serves concurrent requests; JCStress 0 |
| **M6 — Web application** | C4 reference web app, HTTP assertions | HTTP suite exits 0 |
| **M7 — Spring suite** | Selected Spring tests, failure attribution | Spring subset exits 0 |
| **M8 — Performance** | Budgeted startup and latency; JIT/tiering decision on the M0 IR/frame/safepoint contracts | ADR-0010 benchmark exits 0 |
| **M9 — Reactive target** | Spring WebFlux on the chosen server; new manifest and gates | Reactive HTTP suite exits 0 |

## Later targets

These are deferred, not part of the first acceptance. Each gets its own manifest and
gate; none may delay M0–M8. The class column says whether the target is an additive
extension, a conditional one, or an architecture fork that requires a new ADR.

| Target | Class | What | Gate | Record |
| :--- | :--- | :--- | :--- | :--- |
| Reactive web | Conditional | Spring WebFlux; server choice (Netty, Tomcat or Jetty) | WebFlux HTTP suite exits 0 | Q12 |
| JNI and native libraries | Conditional (L0–L1 additive, L2–L3 conditional) | `loadLibrary`/`RegisterNatives`, refs, arrays, direct buffers, trust model | a native-library conformance suite | Q17 |
| Java 21 | Mixed | API additions are additive; virtual threads are an architecture fork | Manifest + C0–C4 (virtual threads need a separate ADR) | Q18 |
| Library additions | Additive | Jackson/JSON, JPA/Hibernate, Netty, security starters, databases | Each added only after the reference application passes | Q12, Q14 |
| Unsafe U1–U4 | Additive | Field/array tokens, atomics and fences, raw memory, class definition | Each tier opens only on a manifest entry; U3 is S1 (fake arena) in the first target | Q8, ADR-0005 |
| Unsafe S2 | S2-raw additive; S2-objects architecture fork | S2-raw (real arena) and S2-objects (real layout) | S2-raw on a measured need; S2-objects needs a new ADR and object model | Q12, Q17, ADR-0012 |
| Other surfaces | Architecture fork | Desktop, agents/JVMTI, native-image, non-Linux hosts | each promoted by its own ADR and target | Q18 |

**Architecture forks** are allowed by the no-permanent-non-goals policy, but they are
not additive upgrades: S2-objects, Java 21 virtual threads, JVMTI/agents, native-image
and any moving/compacting GC that changes handle semantics require a new ADR and a new
project-level decision.

## Performance envelope

The protocol is fixed; the numbers are provisional until ADR-0010 records a baseline.

- Same host, same JDK 17 reference, same reference application, 5 cold starts, median
  reported; latency measured after warm-up with a fixed request mix.
- Provisional ceiling: cold start **≤ 20×** JDK 17 and p50 latency **≤ 10×** JDK 17.
- ADR-0010 may tighten these after the first measurement. Loosening requires a new
  ADR, not an edit to this document.
- No performance claim is made before the benchmark exists.

## Evidence rules

- A claim without a command and an exit code is not evidence.
- Differential tests normalize ordering, identity hashes, timings and stack-trace
  formatting; the normalization is committed with the test.
- A flaky failure carries a capture recipe and is attributed to a cause; "did not
  reproduce" is not acceptance.
- Manifest changes are reviewed like code: a new member appears with the test or
  application path that needs it.
- Numbers live in `docs/benchmarks.md`, not in ADRs or commit messages alone.

## Not in the first target

These are **ordering decisions, not permanent non-goals**. Each can be promoted later
by a new ADR that names the target application, the manifest it adds, the acceptance
gate and the boundary it crosses (ADR-0011). The first target stays finite; the future
does not get closed off.

- General JNI and loading arbitrary native libraries (Q17).
- AWT, Swing, JavaSound, JavaFX and other desktop surfaces (`java.beans` is allowed
  headless because Spring uses it).
- Arbitrary `jdk.internal.*` access; only a declared allowlist is supported.
- SecurityManager, JVMTI, Java agents, AppCDS and native-image.
- Full Java SE API coverage and bug-for-bug HotSpot fidelity.
- Windows and macOS as first-class hosts; Linux arm64 is the reference host.
- The full Spring portfolio: Jackson/JSON, JPA/Hibernate, Netty, security starters and
  databases.

Everything here and in § Later targets is deferred, not permanent.

## Risks

- **Surface explosion.** Even the observed Spring surface may be hundreds of classes
  and thousands of members. M1 produces the count; if it exceeds the agreed budget,
  scope is renegotiated before M2.
- **`Unsafe` and layout.** The audited Spring/Boot/Tomcat stack uses only
  `allocateInstance` and `invokeCleaner` (ADR-0005, U0). Libraries outside that stack
  may require `objectFieldOffset`, raw memory or offset arithmetic; those are
  tripwires until the relevant tier (U1–U4) opens. The first target is S1; S2 is a
  per-family later capability (ADR-0005), not a first-target promise.
- **`MethodHandles` and `defineClass`.** CGLIB's preferred path needs
  `privateLookupIn` + `Lookup.defineClass`; this is larger than the `Unsafe` work.
- **Verifier completeness.** Generated classes must verify; a bypass would make C2
  meaningless.
- **Concurrency and JMM.** Real threads force the `Shared` locking and GC safepoint
  work this runtime has not yet done.
- **Performance.** An interpreter-only runtime may not meet the budget; ADR-0010 must
  choose JIT/tiering from a measurement, not from hope.
- **Test bootstrap.** JUnit 5, AssertJ and Mockito themselves need C1/C2; the first
  probes must be independent of them.
- **Licensing and clean room.** API signatures may be compared with `javap`/`ct.sym`;
  implementations are written from scratch, not copied from JDK source.
- **Version drift.** Spring and Tomcat patch releases change the manifest; the target
  lock and the re-baseline are part of the contract.
