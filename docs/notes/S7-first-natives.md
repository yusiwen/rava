# S7 — The natives are small; the initialisation chain is not

- **Slice:** S7
- **Requirements:** R5
- **Acceptance:** `cargo test -p rava-cli --test natives` exited 0 on 2026-11-20 (14 tests;
  the image-gated ones need the devShell's `JAVA21_HOME`).

## What surprised me

**Eleven natives, and none of them is the hard part.** `System.registerNatives` and
`setIn0`/`setOut0`/`setErr0`, `Class.registerNatives`, `FileDescriptor.initIDs`/`getHandle`/
`getAppend`, `FileOutputStream.initIDs`/`write(IZ)`/`writeBytes([BIIZ)` — each is a few lines.
What costs the time is the *initialisation chain* that calls them: `FileDescriptor(int)` caches
its handle and append flag through `getHandle`/`getAppend`, `FileDescriptor.<clinit>` builds
the three standard descriptors, and `Class.<clinit>` registers its own natives before any of
that can run.

**A native is reachable from Java today.** A fixture whose `main` reads `FileDescriptor.out`
and calls `System.setOut0` runs through the interpreter: the class initialisers fire, the
natives are dispatched, and `System.out` ends up pointing at the descriptor. That is the first
executed Java program in this repository's history, even if it prints nothing.

**The wall is the Class object model, not a native.** `FileOutputStream`'s initializer calls
`SharedSecrets.getJavaIOFileDescriptorAccess()`, which initialises
`java.lang.invoke.MethodHandles`; that class's initializer evaluates its `assert`s through
`Class.desiredAssertionStatus()`, a Java method whose first instruction reads
`Class.classLoader`. This VM represents class objects as `HeapObject::ClassRef`, which has no
fields, so the read fails with a type mismatch. Every JDK class that contains an `assert`
triggers it. Fixing that is M1/M2 work (a class object must be an instance of the host's
`java.lang.Class`), not another native.

**The second wall is the one #9 recorded.** `PrintStream`'s text path ends at
`HeapByteBuffer.<clinit>`, which calls `Unsafe.arrayBaseOffset` — an M4 native.

## What the specification says

R5: the natives are the library's declarations, and the class file is the signature. Nothing
here is JNI. Standard output and error are the VM's buffers (`Shared::out`/`err`), flushed by
the CLI at the end of the run; descriptors other than 1 and 2 are refused, because opening
files is a later slice. That is a deliberate simplification, recorded here because it means
`System.out` is byte-correct but not interleaved with the host's streams.

## The bug I hit

`FileDescriptor.<clinit>` failed first on `getHandle(I)J` — adding it exposed `getAppend(I)Z`,
which the constructor also calls. The report tool's reverse check also misfired on the S7
tests: a registration whose owner class is not on the class path was reported as *dangling*,
which made a fixture-only `rava natives` run fail. The check now distinguishes "the class says
it does not declare this" (dangling) from "the class is not here to ask" (unverified).

## Measurements

| Fact | Value |
| :--- | ---: |
| Natives registered | 11 |
| `rava natives java.lang.System` before | 679 declared, 0 registered |
| … after | 679 declared, 11 registered, 668 missing |
| Tests in `--test natives` | 14 |
| Java-level runs | `GetDescriptor` (initIDs/getHandle/getAppend), `SetOut` (setter + descriptor init) |

## Open threads

- **S8 is blocked on #9.** Reaching `println` needs `Class` objects that behave like the
  host's (assertions, `getName`, `classLoader`) *and* `Unsafe` for the charset path. The three
  options in #9 are still the decision.
- `FileOutputStream.open0` and descriptors beyond 1/2 are not implemented; `System.currentTimeMillis`
  and the rest of `System`'s natives are untouched because nothing on this path calls them.
