# ADR-0014 — `println` is the acceptance of the `Unsafe` work, not of the boot milestone

- **Status:** accepted
- **Date:** 2026-11-20
- **Supersedes:** the `println` part of M0's gate in `docs/requirements.md`
- **Superseded by:** —

## Context

M0's gate ended with a program that prints, compared byte for byte with the reference launcher.
The rest of M0 landed: the image is read (S4), the instruction set is complete (S5), the core
classes load and verify (S6), the first natives are registered (S7), and class objects are real
instances of the host's `java.lang.Class` (S9). The measured path to `System.out.println` is:

1. `System.initPhase1` reads properties through `SystemProps` (two natives), calls
   `Charset.isSupported` and constructs `BufferedInputStream`/`FileOutputStream`/`PrintStream`,
   then installs the streams through `setIn0`/`setOut0`/`setErr0` (all implemented in S7).
2. `PrintStream`'s text path is `BufferedWriter` → `StreamEncoder` → `CharsetEncoder` →
   `ByteBuffer`.
3. `FileOutputStream`'s initializer pulls in `SharedSecrets`, which initializes
   `MethodHandles`; its initializer asks `Class.desiredAssertionStatus` (fixed in S9) and then
   builds the member-name machinery.
4. That machinery stops at `jdk/internal/misc/Unsafe.objectFieldOffset1(Class, String)`. With
   class objects fixed this is the first instruction on the path the VM cannot answer: our
   objects have no byte layout, so a field offset cannot be fabricated — the caller uses it to
   read and write memory.
5. The encoding path also uses `Unsafe` array access (`StringCoding`, `StringUTF16`) and
   `ByteBuffer` uses `arrayBaseOffset`/`arrayIndexScale`.

So printing is not a boot concern with an `Unsafe` dependency; it *is* the `Unsafe` and
VarHandle surface, which the plan puts in M4 (after class loading/modules and threads).

## Decision

- **M0's gate** is the boot itself: image (R3), instruction set (R2), bootstrap (R4), first
  natives (R5), and a program that loads from the image, runs, and whose result is verified
  (R1's "runs" half). S8's acceptance is `cargo test -p rava-cli --test boot`, which runs a
  self-checking program through `rava run` and fails on a non-zero exit or stray output.
- **Byte-for-byte stdout** becomes deferred goal **G2** ("a program prints on the host
  library"), scheduled after M4, with the `System.out.println` comparison as its first
  increment. R1 keeps the contract but splits its acceptance between the two.
- The `Unsafe`/VarHandle slice that M4 starts is where `println` belongs as its acceptance test.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Keep `println` in M0's gate (option A) | Makes M0 equivalent to "after M4", so the ladder loses its boot rung; it also pulls the charset provider loading (M2 territory) ahead of the loaders it depends on. |
| Substitute a VM-owned `PrintStream` | ADR-0001: the library is the host's, not ours; a substitute would mean the acceptance no longer tests the hosted library. |
| Implement the minimal `Unsafe` now | The first native is a field offset; a minimal version needs an offset model plus array accessors, which is M4's core, and the print path also needs `Class.forName`/provider loading. Calling it "minimal" would misname the work. |
| Compare stdout without PrintStream (write to fd 1 directly) | A program cannot reach `FileOutputStream`'s natives without constructing one, and construction is exactly the blocked path. |

## Consequences

**Positive**

- M0 closes on work that is boot, and S8's test proves a program actually runs (exit code, no
  stray output) rather than only loading.
- The `println` demonstration becomes the acceptance of the slice that makes it possible, which
  is a sharper test than a milestone-wide one.
- The milestone ladder stays ordered: M1 (core library), M2 (loaders), M3 (threads), M4
  (`Unsafe`/VarHandle), and G2 rides on M4.

**Negative / accepted costs**

- Until G2, the VM cannot print; a program's observable result is its exit code and side
  effects the VM models (statics, natives), not stdout. Tests must be written that way.
- R1's byte-for-byte contract is not demonstrated for some time, so the "programs are accepted
  by comparing with the host launcher" rule has no M0 evidence yet.

**Follow-up**

- The first M4 slice should take G2's increment as its acceptance: one program that prints, its
  bytes compared with the reference launcher.
