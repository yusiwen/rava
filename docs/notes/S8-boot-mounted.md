# S8 — The boot milestone closes on a program that runs, not one that prints

- **Slice:** S8
- **Requirements:** R1 (the "runs" half)
- **Acceptance:** `cargo test -p rava-cli --test boot` exited 0 on 2026-11-20 (2 tests).

## What surprised me

**A self-checking program is an observable program.** The fixture cannot print, but it does not
need to: it compares `Boot.class.getClass()` with the `java.lang.Class` class literal, checks
`desiredAssertionStatus`, and falls into `athrow` on a mismatch. The test sees the process exit
code, so "the program ran" is a fact, not a claim — and a second fixture with a branch into the
middle of an instruction proves the harness fails a bad program.

**The gate was the only thing between M0 and done.** Every M0 deliverable had landed; what was
missing was a program to run, and the program that *prints* needs the `Unsafe` surface. Moving
the printing demonstration out of the gate (ADR-0014) let the milestone close on what it
actually delivers.

**M1's first job became visible.** Strings are still `HeapObject::Str` — a Rust string, not an
instance of `java.lang.String` — so `String.length()`/`charAt()` read `value`/`coder` fields the
VM does not have. That is the same shape of wall as class objects, and it is where M1 starts.

## What the specification says

JVMS §5.3 puts the class object and the class initializer in the loading/linking story, and
§2.2/§2.6 put operand-stack discipline in the interpreter; neither says a program must produce
output to count as running. R1's byte-for-byte contract is unchanged — its evidence moves to
G2.

## The bug I hit

None in this slice's code: the fixture passed on the first run, hand-computed branch offsets and
all. The finding was about what the program *cannot* do. `String.length()` fails with
`expected an object reference, found Ref(..)` because `HeapObject::Str` is not an instance of
`java.lang.String` — the same shape as the class-object wall — which is why the boot program
compares class objects by identity instead of checking a name.

## Measurements

| Fact | Value |
| :--- | ---: |
| Acceptance tests | 2 (runs, and a malformed program fails) |
| `rava run Boot --home $JAVA21_HOME --cp <dir>` | exit 0, stdout and stderr empty |
| Path to printing, measured | stops at `Unsafe.objectFieldOffset1` |

## Open threads

- **G2 (printing) is deferred to after M4**; the `Unsafe` field-offset model is its first
  dependency, and `StringCoding`/`StringUTF16` then need `Unsafe` array access.
- M1 starts with strings: the VM's `Str` representation must become (or behave like) the host's
  `java.lang.String` before `length()` or `charAt` can run.
- `System.initPhase1` is not called by the VM: a real launcher does it after bootstrapping. It
  is G2's job to install the standard streams once `PrintStream` can be constructed.
