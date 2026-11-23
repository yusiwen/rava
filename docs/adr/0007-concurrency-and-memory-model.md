# ADR-0007 — Real OS threads and the Java memory model are required

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

A servlet container is concurrent by construction: an acceptor, poller threads and a
worker pool, with `synchronized` blocks, `wait`/`notify`, `ThreadLocal` request state,
`java.util.concurrent` collections and atomics. A runtime that serializes `start()` to
run the body on the calling thread may pass a corpus, but it cannot serve two requests
at once, and it hides exactly the memory-model bugs a real application exposes.

The index heap and the single `&mut Shared` borrow were designed for a single thread.
Real concurrency is therefore a structural change: shared structures need interior
locking and the collector needs safepoints.

## Decision

- Threads are real OS threads. `Thread.start`, `join`, `interrupt`, `sleep`, daemon
  status, priorities (advisory) and uncaught-exception handlers behave as on JDK 17
  for the exercised paths.
- Monitors support `monitorenter`/`monitorexit`, `ACC_SYNCHRONIZED`, re-entrancy,
  `wait`/`notify`/`notifyAll`, and `IllegalMonitorStateException`.
- `ThreadLocal` and `InheritableThreadLocal` are implemented with the same isolation
  guarantees as the reference JDK.
- `java.util.concurrent`: the manifest's collections, executors, futures, atomics,
  locks and synchronizers, built on the VM's primitives rather than on `Unsafe`
  offsets.
- The memory model is respected: `volatile` visibility, `synchronized` visibility,
  `final` field semantics, and the happens-before edges of `Thread.start/join`,
  executor submission and concurrent collections.
- The collector stops all threads at a safepoint before marking. The rule "never hold a
  VM lock across a Java call" becomes mandatory, not vacuous.
- `Shared` is split into independently lockable parts (heap, class table, interned
  strings, native registry, output), with a read-mostly fast path where measurements
  justify it.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Keep serialized threads | Tomcat cannot serve concurrent requests; `wait`/`notify` and `ThreadLocal` semantics are untested; the application's behaviour is not the reference behaviour. |
| One global `Mutex<Shared>` per instruction (a GIL) | Interleaving exists, but nested calls re-lock, and the "no lock across a Java call" problem is unsolved. It also taxes every instruction. |
| Green threads inside the interpreter | A second scheduler and blocking semantics to get right before the JMM questions are answered; wrong default for a platform that must host native-blocking Tomcat. |
| Implement concurrency later | C3/C4 cannot pass without it; deferring moves the largest risk to the end. |

## Consequences

**Positive**

- Tomcat and Spring's executors run with their real threading model.
- The memory model is tested, not assumed.
- The collector's safepoint design is forced early, when it is still cheap to change.

**Negative / accepted costs**

- The `Shared` split and locking are invasive; every field access in the dispatch loop
  must respect the locking discipline.
- Locking the hot path costs performance; the budget in ADR-0010 must absorb it.
- JMM bugs are intermittent; JCStress and targeted tests are mandatory.

## Acceptance

- The selected JCStress suite exits 0 on the reference host.
- Two threads increment one `synchronized` counter 10^6 times with no lost updates.
- Tomcat serves concurrent requests with correct `ThreadLocal` isolation.
- `wait`/`notify` and interrupt semantics match the JDK 17 tests.
