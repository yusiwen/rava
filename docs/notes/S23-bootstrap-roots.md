# S23 — Two bootstrap bugs the printf probe found

- **Slice:** S23
- **Requirements:** R12
- **Acceptance:** `cargo test -p rava-cli --test boxing` exited 0 on 2026-11-20 (1 test);
  `print` and `print_differential` still pass (the bootstrap graph survives).

## What surprised me

**Autoboxing did not work.** `Integer.valueOf(42)` threw `IllegalStateException` from
`jdk.internal.misc.VM.getSavedProperty`: `VM.savedProps` was null because the bootstrap replaced
`System.initPhase1`, and the only thing that fills that field is `VM.saveProperties` — which the
bootstrap never called. S21's fixtures used `Value::Int` directly, so nothing boxed an `int`
until `printf`'s varargs did.

**A collection during the bootstrap freed `System.out`.** The first fix for the property map
tried to run `VM.saveProperties` itself; while it ran a collection, the PrintStream that
`initialize_system` had built but not yet stored was collected, its slot reused for a String, and
`System.out.printf` dispatched on a String (`method not found:
java/io/PrintStream.printf(...)`). `allocate` only sets a hint; the collection happens at the
interpreter's safepoint inside the constructors the bootstrap calls, and a Rust local is not a
root. Of course it was not: the VM keeps roots in frames, statics and caches.

## What the specification says

`VM.getSavedProperty` requires `savedProps` to be non-null; the JDK fills it from
`VM.saveProperties(Map)` during `initPhase1`. The VM's own bootstrap must keep the objects it is
assembling reachable — JVMS §2.5.3 lists the roots (frames, statics, JNI handles); a bootstrap
that builds a graph needs a root list of its own.

## The fixes

- `Shared::pinned: Vec<ObjRef>` is a GC root set (`pin`/`unpin_all`); `initialize_system` pins
  the Properties, the Charset, each stream as it is built, and releases them once they are
  reachable from `System`/`VM` statics.
- `initialize_system` sets `System.props` (now including `java.class.version` and
  `java.specification.version`) and `VM.savedProps` directly, instead of running
  `VM.saveProperties`, which also expects `Runtime.maxMemory` and parses `sun.nio.*` keys.
- `Runtime.maxMemory` answers 512 MiB (the native `VM.saveProperties` would have called).

## Measurements

| Fact | Value |
| :--- | ---: |
| Boxing test | 1 (`Integer.valueOf(42).intValue()` = 42) |
| Workspace tests | 114 |
| `System.in` probe | `System.in.read()` → NullPointerException at the call site (#36) |
| `printf` probe | now reaches `Formatter`: `expected int, found Ref(ObjRef(642))` (#37) |

## Open threads

- **`System.in` is still null** (#36): the bootstrap installs out/err only.
- **`printf`/`String.format` stop inside `Formatter`** (#37): the next I/O-breadth slice, not a
  bootstrap issue.
- `Runtime.maxMemory` is a fixed 512 MiB; `freeMemory`/`totalMemory` are not implemented.
