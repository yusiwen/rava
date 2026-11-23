# ADR-0012 — Runtime foundation and upgrade seams

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-23
- **Amends:** ADR-0004 (handle seam), ADR-0005 (direct-memory seam), ADR-0007 (timing), ADR-0009 (jar timing), ADR-0010 (JIT seams)
- **Supersedes:** —
- **Superseded by:** —

## Context

The accepted plan put several structural concerns late: concurrency at M5, jars at M4,
the JIT decision at M8. A review of the milestones found that, without foundation
seams, those stages would not be enablement — they would require rewriting the
interpreter, class loader, heap and execution model. That is the failure this ADR
prevents.

The object-model boundary is sharper: S2-objects (a real byte-addressed layout) is not
an incremental upgrade of ADR-0004; it is a different object model, collector and JIT
interface. It must be classified as an architecture fork, not as a later tier.

This ADR does not overturn the decisions of ADR-0004/0005/0007/0009/0010. It moves the
seams those decisions need into **M0**, so the later milestones enable them instead of
retrofitting them.

## Decision

### M0 — Foundation (before M1)

Insert an M0 milestone before the existing M1. M0 builds the runtime skeleton and its
invariants:

1. **Class-file model.** A version-agnostic parser that preserves unknown attributes as
   raw bytes and retains annotations, generic signatures, records, sealed permits,
   nestmates, parameter names and debug info. The interpreter must not be the reason a
   later reflection stage has to re-parse.
2. **Class identity and loading.** Class identity is `(defining loader, binary name)`;
   the class table is keyed by the loader. The bootstrap/app loader hierarchy,
   `defineClass` entry point and resolution path exist even if initially only one
   loader is used.
3. **Class sources.** A `ClassSource` abstraction over directories and jars from the
   start. Nested jars/Boot fat jars are added at M4; the loader does not assume a
   directory tree.
4. **Object and field model.** `ObjRef` is a stable handle with room for a generation;
   field layout is explicit; no raw pointer appears in `Value`. GC roots, handles and
   class unloading have a place to attach.
5. **Threading and memory model.** `Shared`/`ThreadState` are split; shared parts are
   independently lockable; the interpreter has safepoint poll points; the invariant
   "no VM lock held across a Java call" is checked from day one. M5 enables OS threads;
   it does not introduce the split.
6. **Execution form.** The decoded instruction form, frame layout, call-site objects,
   safepoint map and deoptimisation metadata placeholders are fixed in M0. A JIT at M8
   consumes them; it does not invent a second frame model.
7. **Memory regions.** A `MemoryRegion`/`DirectBuffer` abstraction exists with the S1
   fake arena from ADR-0005. S2-raw later replaces the backing implementation; M9/Netty
   does not rewrite NIO/buffer plumbing.
8. **Native boundary.** The native registry is keyed by `(owner, name, descriptor)` and
   is thread-safe; loader and thread-attach entry points exist for a later JNI L0.
   Array/string storage has a defined copy/pin contract so JNI L2 does not force a
   representation rewrite.
9. **Verifier.** The dataflow framework (stack depth, locals, handler entry) exists;
   generated-class verification lands at M3 on top of it.
10. **References and cleaner.** The heap/GC exposes reference-queue and class-unloading
    hooks; M5 implements weak/soft/phantom and `Cleaner`, it does not change the object
    header to make room.

### Enablement, not rework

| Milestone | What it enables on the M0 seams |
| :--- | :--- |
| M1 | Manifest and `link-check` over `ClassSource` |
| M2 | Reflection metadata and a Spring context without re-parsing |
| M3 | `defineClass`/Proxy/CGLIB on the loader and verifier seams; Unsafe U0 |
| M4 | Nested jars, fat jar, `ServiceLoader`, `-jar` |
| M5 | Real OS threads, JMM tests, references/cleaner, Tomcat |
| M6–M7 | Web application and Spring tests |
| M8 | JIT/tiering decision on the fixed IR/frame/safepoint contracts |
| M9 | Reactive on the chosen server; S2-raw if Netty |

### Later targets: additive, conditional, or architecture fork

| Class | Targets | Rule |
| :--- | :--- | :--- |
| **Additive** | Java 17 API/library additions, module descriptor parsing, JNI L0–L1, S2-raw, additional servlet-compatible servers | a new manifest/gate; no change to the M0 contracts |
| **Conditional** | JNI L2–L3, reactive Netty, a JIT | requires the named seam and a measured need; new ADR if the seam changes |
| **Architecture fork** | S2-objects (real layout/B), Java 21 virtual threads, JVMTI/agents, native-image, moving/compacting GC if it changes handle semantics | a new ADR and a new project-level decision; **not** an incremental tier |

The architecture-fork items are still allowed by ADR-0011's no-permanent-non-goals
policy, but they are not promised as additive upgrades. Discovering one is needed is a
scope decision, not a bug fix.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Keep M5/M4/M8 as the first implementation points | Each would rewrite the interpreter/loader/heap; the old learning-first path demonstrated exactly that cost. |
| Implement concurrency, jars and JIT now | Unbounded; M0 fixes the seams without implementing the later features. |
| Design S2-objects now | It would discard the index heap and the collector before a target needs it; M0 keeps S2-raw additive and marks S2-objects as a fork. |
| Treat virtual threads/JVMTI as ordinary later targets | They change scheduling/instrumentation and the execution model; classifying them as additive would repeat the refactor risk. |

## Consequences

**Positive**

- M5/M8/M9 become enablement of existing seams rather than rewrites.
- The loader, metadata, heap, IR and memory abstractions are fixed before code depends
  on them.
- Later targets have an explicit class, so "consider it later" does not become "silently
  promise an additive upgrade".

**Negative / accepted costs**

- M0 is larger than a tooling-only first milestone; it carries parser/model/heap/IR/
  threading seams before the first Spring context.
- Some M0 work is not exercised until M5/M8/M9; it needs invariant tests to avoid dead
  design.
- The architecture-fork list will disappoint anyone expecting S2-objects or virtual
  threads to be drop-in upgrades; that is the honest boundary.

## Acceptance

- M0 exits 0 with: a small program run against our library; a metadata dump matching
  JDK 17 for fixtures; a loader-identity test; the no-lock-across-Java-call invariant;
  a single-thread safepoint test; and the IR/frame contract documented.
- `ClassSource` loads both a directory and a plain jar; the loader does not special-case
  directories.
- The native registry and class table are thread-safe in tests.
- No accepted ADR is contradicted; this ADR only moves seams earlier and classifies
  later targets.
