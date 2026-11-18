# ADR-0013 — A JIT is planned, after the interpreter has real programs to run

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

The interpreter is the readable artefact and the oracle: every compiled path would be checked
against it, output for output. A compiler is also the largest subsystem after the hosted
library's native surface, and it only pays when there is something to run and a measurement
that says where the time goes.

At this point the VM cannot yet print a string: M0 is unbuilt. Compiling anything before M5
would optimise code that cannot execute, against a workload that does not exist. The previous
iteration did measure the ceiling of the interpreter it carried over — a call-free loop at
about 10.8 ns per instruction against roughly 0.1 ns for the same arithmetic in Rust — but that
number describes the old VM's loop, not the hosted one, and it is not a reason to build a
compiler for programs that do not run yet.

## Decision

A JIT is a **deferred goal (G1)**, not a non-goal:

- It starts after M5, when the VM runs real programs against the hosted library.
- The first increment is deliberately narrow: compile one hot method's body, keep the
  interpreter as the fallback, and show that the compiled path produces byte-identical output
  to the interpreter on the same program.
- The entry condition is a measurement showing the interpreter is the bottleneck for a program
  the VM already runs. The measurement is the trigger, not the decision: this ADR records the
  commitment; the slice that starts it records the number.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| A non-goal (never) | The project does intend to compile, once there is code worth compiling; calling it permanent would misstate the plan. |
| Build the JIT now | Nothing runs yet; a compiler with no programs and no oracle measurement optimises guesses. |
| Ahead-of-time compilation only | It solves deployment, not the interpreter's ceiling, and it needs the same code generator plus a class-file writer. |
| Keep the previous iteration's deferral unchanged | It was a non-goal there because the project was corpus-driven; the hosted model changes what "the interpreter's ceiling" means and makes a compiler a natural later stage. |

## Consequences

**Positive**

- The interpreter stays the single oracle: every compiled path is checked against it.
- The decoder (ADR-0003) is already the right input for a code generator: an instruction form
  with resolved targets, not a byte array.
- No timeline commitment is made; the goal can wait for a real measurement.

**Negative / accepted costs**

- Performance stays at interpreter level until then, which is visible in any program the VM
  runs.
- `Shared` and the frame layout must stay simple enough to compile against later; a design that
  hides the operand stack behind indirection would make a compiler harder.

**Follow-up**

- `rava bench` and the step counters are the measuring instruments; they must keep working as
  the hosted library grows.
