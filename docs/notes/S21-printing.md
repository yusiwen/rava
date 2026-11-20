# S21 — G2: printing through the host library

- **Slice:** S21
- **Requirements:** G2 (the deferred printing goal), R12
- **Acceptance:** `cargo test -p rava-cli --test print` exited 0 on 2026-11-20 (2 tests:
  `println(String)`/`println(int)` and `printStackTrace`).

## What surprised me

**`System.<clinit>` does almost nothing in JDK 21.** It calls `registerNatives` and nulls
`in`/`out`/`err`; the streams are installed by `System.initPhase1`, a *Java* method the VM must
call at startup. Its only native dependency is `SystemProps.initProperties`, which returns the
platform property map — and that is the one piece this VM has no source for. Instead of faking a
`Map`, the bootstrap runs the host's own constructors (`FileOutputStream(FileDescriptor.out)` →
`BufferedOutputStream` → `PrintStream(out, true, UTF-8)`), installs a minimal `Properties` with
the keys the streams and charset read, and calls `System.setJavaLangAccess()`.

**Six walls between the Unsafe model and a printed line**, each a missing native or a VM bug:

1. `[B.clone` in the charset path — S19's array `clone` (already merged).
2. `Buffer.UNSAFE` — an *inherited* static field; the VM only looked at the referencing class.
   Fixed by `Shared::static_field_owner` (JVMS §5.4.3.2).
3. `ScopedMemoryAccess.registerNatives`, `VM.initialize`, `Thread.registerNatives`,
   `NullPointerException.getExtendedNPEMessage` — no-ops or `null`.
4. `System.getProperty` NPE — the private `props` field was null; the bootstrap now installs it
   *before* the streams (the first ordering had it after, and `BufferedOutputStream`'s
   `InternalLock` read a property during construction).
5. `Thread.currentThread` — a static native (no receiver) returning a lazily created, cached
   `Thread` object; the constructor is not run (it reaches `ThreadGroup`), the fields identity
   and naming need are set directly.
6. `sun.nio.cs.UTF_8.JLA` null, which surfaced as a `CoderMalfunctionError` wrapping an NPE deep
   in `encodeArrayLoop`. `System.setJavaLangAccess()` installs it.

**Superclass initialization order was wrong.** `HeapByteBuffer.<clinit>` reads
`Buffer.UNSAFE`, which `Buffer.<clinit>` sets; `ensure_initialized` never initialized the
superclass first, so the read saw null. JVMS §5.5 step 7 makes this a bug, not a shortcut; it is
fixed in `ensure_initialized`.

## What the specification says

`System.out` is a `PrintStream` over `FileOutputStream(FileDescriptor.out)`; a line goes through
the charset encoder to bytes and the native write. The VM's job is to install the streams and the
write primitive, not to print itself — which is why the sink is `FileOutputStream.writeBytes`.

## The bug I hit

`printStackTrace()` writes to `System.err`, not `System.out`; the first assertion read the wrong
sink and saw an empty string. The VM writes fd 1 to `Shared::out` and fd 2 to `Shared::err`.

## Measurements

| Fact | Value |
| :--- | ---: |
| Print test target | 2 tests (`hi\n42\n`; a full stack trace on stderr) |
| Workspace tests | 111 |
| Bootstrap | `FileOutputStream` + `BufferedOutputStream` + `PrintStream` + a 7-key `Properties` |
| Natives added | `FileDescriptor.initIDs`, `VM.initialize`, `ScopedMemoryAccess.registerNatives`, `Thread.registerNatives`/`currentThread`/`currentCarrierThread`, `NullPointerException.getExtendedNPEMessage` |

## Open threads

- **G2's byte-for-byte comparison against the reference launcher is the next increment**: this
  repository has no `samples/` corpus, so a printing sample and its golden output are still to
  be added; the VM-side bytes are pinned, the differential is not.
- `System.in` is not installed (the streams bootstrap skips it); `System.getenv` and
  `System.getProperties()` enumeration return the minimal set only.
- `printStackTrace(PrintStream)` and `System.err` formatting work; `StackWalker` and
  `Thread.getStackTrace` are not exercised.
