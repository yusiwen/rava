# S12 — No throwable exists without a stack trace

- **Slice:** S12 (M1's exceptions)
- **Requirements:** R6
- **Acceptance:** `cargo test -p rava-cli --test exceptions` exited 0 on 2026-11-20 (3 tests).

## What surprised me

**The first thing an exception needs is not `athrow`.** `rava-interp` has unwound `athrow`
through exception tables since S2, but `new RuntimeException()` never reached the throw:
`javap -c java.lang.Throwable` shows the no-argument constructor ends with
`invokevirtual fillInStackTrace()`, a `synchronized` Java method that calls the private native
`fillInStackTrace(int)`. Without that native the VM could not create a single throwable, so the
first exception fixture (`MakeException`) failed inside a constructor, not at a throw. The
slice's dependency was the trace capture.

**The trace belongs to the VM, not to the host object.** The host `Throwable` has `stackTrace`
and a native `backtrace`, but filling them means constructing `StackTraceElement` objects and
an array of them — the string/array work that owns M1. `Shared::traces: HashMap<ObjRef,
Vec<TraceFrame>>` already existed for the interpreter's own throws and for VM-generated
exceptions; the native writes that map and lets the host's Java code set its own
`stackTrace = UNASSIGNED_STACK` field, which is exactly what `Throwable` expects a VM to do.

**First capture wins — and that is a deviation, measured.** `Throwable.fillInStackTrace()`
re-enters the native on every explicit call, because after the first one it resets `stackTrace`
to the non-null `UNASSIGNED_STACK` (so the guard `stackTrace != null` is true). The reference
JDK therefore re-captures: a program that calls `t.fillInStackTrace()` from a second method
gets `frames=7 first=again` from `t.getStackTrace()`. This VM keeps the first trace, so the
construction site survives a re-call; the interpreter's `athrow` path already had the same
`contains_key` rule for rethrows. That is the task's specified behaviour and it is recorded as
an open thread rather than copied from the JDK, because copying it would need the host's
`backtrace` field, which is exactly the array work this slice does not have.

**The uncaught report now carries the host's line numbers.** The CLI's
`Shared::describe_throwable` prints the frames from `Shared::traces`, so an uncaught exception
shows the real `LineNumberTable` of the host image:

    Exception in thread "main" java.lang.IllegalStateException
    	at java.lang.Throwable.fillInStackTrace(Throwable.java:820)
    	at java.lang.Throwable.<init>(Throwable.java:258)
    	at java.lang.Exception.<init>(Exception.java:55)
    	at java.lang.RuntimeException.<init>(RuntimeException.java:52)
    	at java.lang.IllegalStateException.<init>(IllegalStateException.java:43)
    	at ThrowException.main(Unknown Source:0)

The fixture has no line table, hence `Unknown Source:0`; every host frame has one. The six
frames are innermost first, which is how `capture_trace` builds them and how Java prints them.

## What the specification says

`Throwable.fillInStackTrace()`'s contract is "Fills in the execution stack trace … records
within this Throwable object information about the current state of the stack frames for the
current thread". It does not say a later call must *not* re-record, and the JDK's implementation
does re-record; the VM's choice to keep the first trace is a simplification, not a reading of
the spec.

JVMS §2.11.10 makes a `synchronized` method hold its monitor for the whole body, so constructing
any throwable now exercises `monitorenter`/`monitorexit` on the new object (JAVA 21's
`Throwable.<init>` calls the non-static synchronized `fillInStackTrace()` on `this`). JVMS
§4.7.3's exception table is the `athrow` half, already exercised by the caught fixture and by
S2's interpreter tests.

## The bug I hit

None in the native: all three fixtures passed on the first run. The real work was the one above
— discovering that the constructor, not `athrow`, is where the trace has to be taken, and
measuring the JDK's re-call behaviour instead of assuming the VM's first-capture rule was the
JDK's. The caught fixture needed the exception table's byte offsets counted by hand (`new` at 0,
`dup` at 3, `invokespecial` at 4, `athrow` at 7, handler at 8, `return` at 13), because the
builder takes offsets rather than labels.

## Measurements

| Fact | Value |
| :--- | ---: |
| Acceptance tests | 3 (make, caught, uncaught) |
| `rava run ThrowException --home $JAVA21_HOME --cp <dir>` | exit 1 |
| Frames in the uncaught report | 6 (innermost `Throwable.fillInStackTrace`) |
| Workspace tests, `cargo xtask ci` | 78 passed / 0 failed over 20 test binaries |
| Mutation: `exceptions::register` removed | 0 passed / 3 failed, each `no native implementation for java/lang/Throwable.fillInStackTrace(I)Ljava/lang/Throwable;` |
| JDK re-call oracle, `java Fill.java` | `frames=7 first=again` (the JDK re-captures) |

## Open threads

- **A later explicit `fillInStackTrace()` does not move the trace** in this VM; the JDK
  re-records it. Modelling that needs a per-call capture that distinguishes construction from
  re-fill, or the host's `backtrace` field — array work, not this slice.
- `getStackTrace()` and `printStackTrace()` still cannot run from Java: they read/fill
  `StackTraceElement[]`, which needs array and `StackTraceElement` construction. The CLI report
  uses `Shared::describe_throwable` instead, and that is what the acceptance checks.
- The host object's `stackTrace`/`backtrace` fields are never written by the VM, so Java code
  that reads `stackTrace` directly sees `UNASSIGNED_STACK`, and `getStackTrace()` would return
  an empty array rather than the captured frames.
