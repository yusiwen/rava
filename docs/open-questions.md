# Open questions

> **Status:** all questions resolved as of 2026-11-23. This file is now a decision
> record; a new decision gets a new id and, if it changes an accepted ADR, a
> superseding ADR.
>
> **Standing policy:** no permanent non-goals. Every exclusion is an ordering decision
> and can be promoted by a new ADR (ADR-0011).

## Resolved

| Id | Question | Answer | Record |
| :--- | :--- | :--- | :--- |
| **Q1** | Which Spring/Boot line? | Spring Framework **6.2.19**, Spring Boot **3.5.16**, Tomcat **10.1.55** — the official Boot 3.5.16 BOM. | ADR-0002 |
| **Q2** | Servlet or reactive first? | Servlet + `text/plain` first; reactive (Spring WebFlux) is a later target (M9). | ADR-0002, requirements § Later targets |
| **Q3** | Reuse existing code, or start clean? | Reuse existing Rust components where they fit; each imported component is re-reviewed against this project's ADRs, and no decision record is imported with the code. | `PROJECT.md` § Codebase |
| **Q4** | What does `java.vm.name` report? | `rava`; Objenesis therefore takes the `Unsafe.allocateInstance` path, which is in U0. | ADR-0011, ADR-0005 |
| **Q5** | Direct buffers: VM-managed storage or a real off-heap allocator? | VM-managed buffer with `isDirect()` support; `allocateDirect(0)` exists for Tomcat's class initialization; `invokeCleaner` is a no-op/GC hook, never an address. | ADR-0005 |
| **Q6** | Interpreter-only, tiered interpreter, or JIT? What budget? | Measure first at M1 and M8; provisional ceiling 20× cold start and 10× p50; ADR-0010 chooses the execution strategy from the measurement. | ADR-0010 |
| **Q7** | Which Spring tests form the C4 subset, and are agents allowed? | Start with `spring-core`/`spring-context` tests that need no agents; exclude Mockito inline mock maker and Netty. | requirements R10, ADR-0003 |
| **Q8** | U0 allowlist and U1–U4 triggers? | U0 is fixed (`theUnsafe`, `allocateInstance`, `invokeCleaner`, `ensureClassInitialized`, `shouldBeInitialized`); U1 opens only on a manifest entry; U3 is S1 in the first target; `objectFieldOffset` is not implemented until U1 is opened. | ADR-0005 |
| **Q9** | Module system in scope? | Classpath-only first target; parse module descriptors but do not promise a module layer. | ADR-0009 |
| **Q10** | License, name and repository layout? | MIT, name `rava`; Rust workspace layout is decided after P1. | `PROJECT.md`, `README.md` |
| **Q11** | Exact patch versions and reference-application commit? | Spring Framework **6.2.19**, Spring Boot **3.5.16**, Tomcat **10.1.55**; the reference application's repository and commit are created and pinned as the first M1 task. | ADR-0002 |
| **Q12** | Reactive target: Netty, Tomcat or Jetty? | Deferred to M9. Choose from a measured manifest; prefer a lower-risk server first. Reactor Netty requires the S2-raw/direct-memory work and is not a first-target commitment. | ADR-0002, ADR-0005 |
| **Q13** | System-property contract? | Java 17 profile in ADR-0002: `java.version=17.0.12`, `java.class.version=61.0`, `java.vm.name=rava`; Boot's `JavaVersion` is API-probed, not parsed from `java.version`, so the JCL must not expose Java 18+ methods. | ADR-0002 |
| **Q14** | Third-party crate policy? | Prefer reviewed, permissively licensed crates; the DEFLATE/zip choice is recorded in ADR-0009 at M4. | ADR-0009 |
| **Q15** | Which existing Rust components are imported, and what review? | Parser and class-file model first; each import is re-reviewed against ADR-0004/0005 and covered by tests. | `PROJECT.md` § Codebase |
| **Q16** | What does "complete Unsafe" mean, and which address model? | S0 API shape + S1 semantics for opened families; S2 is per-family later. U3 is S1 (fake arena); S2-raw/S2-objects are separate upgrades; `jdk.internal.misc.Unsafe`/`VarHandle` are separate. | ADR-0005 |
| **Q17** | JNI support level and native trust model? | A later target, not the first. Start at L0 (`System.loadLibrary` + `JNI_OnLoad` + `RegisterNatives` + one native method); the ABI, refs, exceptions, arrays, direct buffers and crash policy are defined before L1+. | ADR-0011 |
| **Q18** | In what order are deferred targets promoted after C4? | Finish C4 first; then choose one. Reactive (M9) if web is the priority, JNI if the native ecosystem is; the rest follow with their own ADRs. | ADR-0011, requirements § Later targets |

## New questions

A new question gets the next id and an entry here. If it changes an accepted ADR, write
a superseding ADR rather than editing the accepted record.
