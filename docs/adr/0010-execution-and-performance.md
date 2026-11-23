# ADR-0010 — Measure first; choose the execution strategy against a budget

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

Spring's startup is dominated by class loading, annotation parsing, ASM, CGLIB
generation and reflection. A bytecode interpreter may be one or two orders of
magnitude slower than the JIT on these paths; the exact factor is unknown until the
application runs. Committing to a JIT before measuring risks weeks of work on the wrong
bottleneck; committing to interpreter-only risks a runtime that technically passes C4
but is unusable.

The project needs a performance requirement that is honest about the missing baseline:
a protocol now, a budget after the first measurement.

## Decision

- **Measure before optimising.** M1 records class-load counts and a startup profile;
  M8 runs the full benchmark.
- **Protocol:** same host, same JDK 17 reference, same application, 5 cold starts with
  the median reported; latency after warm-up with a fixed request mix; all numbers go
  to `docs/benchmarks.md` with the machine named.
- **Initial budget (provisional):** cold start ≤ 20× JDK 17 and p50 request latency
  ≤ 10× JDK 17. ADR-0010 may tighten these after the first measurement; loosening
  requires a new ADR.
- **Execution strategy is a consequence of the measurement**, not a premise:
  1. interpreter with call-site caches and a fast native path for the hot library
     methods;
  2. a tiered/baseline JIT if dispatch or reflection dominates the budget;
  3. AOT specialisation only if a measurement shows it beats the alternatives.
- The JIT (if any) compiles the same decoded instruction form the interpreter executes,
  and its output is compared byte-for-byte with the interpreter on the corpus.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Interpreter only, no performance goal | The application may start in minutes; "it runs" would not be a usable result. |
| Build a JIT first | Weeks of work with no measurement identifying the bottleneck; startup may be dominated by class loading and reflection, not dispatch. |
| AOT-compile the application | Changes the "unmodified jars" property and does not help dynamically generated CGLIB classes. |
| Chase the JDK's throughput | The target is a usable framework runtime, not a benchmark winner; a relative budget is the right shape. |

## Consequences

**Positive**

- Optimisation effort follows evidence; the budget can be defended with numbers.
- The interpreter remains the reference semantics for any JIT.
- Startup and steady-state are treated separately, as they should be.

**Negative / accepted costs**

- The performance gate cannot be evaluated until M8; earlier milestones may pass
  correctness while remaining unusably slow.
- A JIT adds a second execution engine and a deoptimisation story; the budget must
  justify it.
- The reference host and JDK version become part of the measurement contract.

## Acceptance

- The benchmark command documented in this ADR exits 0 and writes startup, p50 and p99
  to `docs/benchmarks.md`.
- The result is within the budget, or a superseding ADR records why the budget changed.
- Any JIT path produces output identical to the interpreter on the corpus and the
  reference application.
