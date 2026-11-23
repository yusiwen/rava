# ADR-0008 — References, `Cleaner` and class unloading

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

Spring caches metadata and proxies. `ConcurrentReferenceHashMap`, CGLIB caches and
Objenesis instances use weak or soft references; `java.lang.ref.Cleaner` and
`ReferenceQueue` are used by libraries that manage native or direct resources. A
mark-sweep collector that only knows strong references either keeps these caches alive
forever or cannot reclaim the objects behind them.

Tomcat and Netty also use `Cleaner`/`invokeCleaner` for direct buffers. If a direct
buffer's storage is VM-managed, its lifetime must be tied to the buffer object, not to
a native address.

## Decision

Extend the collector and the reference subsystem with:

- `WeakReference`, `SoftReference`, `PhantomReference` and `ReferenceQueue`, with the
  JDK's reachability and enqueue semantics.
- `Cleaner` and `Cleanable`, backed by phantom references and a cleaner thread.
- `WeakHashMap`, `ConcurrentReferenceHashMap`-style caches and `ThreadLocal` cleanup
  as clients of the reference subsystem.
- Class unloading when the defining loader and all classes become unreachable.
- VM-managed direct buffers: the buffer object owns the storage; `Unsafe.invokeCleaner`
  releases it or is a no-op, and the collector reclaims it if the application never
  calls the cleaner.

The collector remains mark-sweep initially; generational or concurrent collection is a
later decision driven by measurement.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Strong references only | Spring's caches leak; `Cleaner` never fires; direct buffers leak. |
| Finalization-based cleanup | Deprecated, unpredictable, and not the mechanism Spring/Tomcat use. |
| Full generational collector now | A second large subsystem before the references work; the first target needs correctness, not throughput. |
| Off-heap allocator with `Unsafe` addresses | Contradicts the index heap and ADR-0005's address-free boundary; VM-managed storage is enough for the target. |

## Consequences

**Positive**

- Framework caches behave like they do on the JDK; memory is reclaimed.
- Direct buffers have a defined owner and a defined release path.
- Class unloading prevents metaspace-like growth in long-running reload scenarios.

**Negative / accepted costs**

- Reachability is a second graph traversal with subtle ordering rules.
- Cleaner threads and reference queues add concurrency to the collector.
- Class unloading interacts with the class table and the JIT (if one exists later).

## Acceptance

- Reference tests cover weak/soft/phantom enqueueing, `ReferenceQueue` polling and
  clearing.
- A soak test shows cache entries are reclaimed and no object is retained solely by a
  cleared reference.
- `Cleaner` runs its action exactly once, and direct buffers are released.
- Class unloading is observed in a reload test without leaking the loader.
