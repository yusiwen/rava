# S19 — Java-side stack traces, without the JDK's private `backtrace`

- **Slice:** S19
- **Requirements:** R6
- **Acceptance:** `cargo test -p rava-cli --test stack_traces` exited 0 on 2026-11-20 (2 tests).

## What surprised me

**The JDK throws the native's work away.** `javap -c java.lang.Throwable` shows the public
`synchronized fillInStackTrace()` ending in

    14: aload_0
    15: iconst_0
    16: invokevirtual #202  // fillInStackTrace:(I)Ljava/lang/Throwable;
    19: pop
    20: aload_0
    21: getstatic     #13   // UNASSIGNED_STACK:[Ljava/lang/StackTraceElement;
    24: putfield      #17   // stackTrace

The private native is called, and then the field is overwritten with `UNASSIGNED_STACK`
unconditionally. A native that fills `stackTrace` therefore leaves an empty array behind: the
first version of this slice did exactly that, and the fixture failed with `array index 0 out of
bounds for length 0` at `[0]`. HotSpot gets away with it because its native fills the private
`backtrace` object instead, and `getOurStackTrace()` rebuilds `stackTrace` from it. This task's
constraint is to leave `backtrace` null, so the host's Java body cannot run.

**The fix is a VM intrinsic, not a larger native.** The VM now has
`Shared::intrinsics: HashMap<String, NativeFn>`: a Java method the VM implements itself, bound by
the loader in place of the method's class-file body. `Throwable.fillInStackTrace()` is the first
one. The private `fillInStackTrace(int)` stays registered and still fills the fields for a caller
that reaches it, but the host's constructor now runs the intrinsic, which never touches
`UNASSIGNED_STACK`.

**A synthesised frame keeps the trace identical.** With the Java body replaced, the frame for
`Throwable.fillInStackTrace` is no longer on `thread.frames` when the native captures. Recording
it was not optional: S12's test asserts the innermost frame is `java/lang/Throwable
.fillInStackTrace`, and the uncaught report names its line. The intrinsic synthesises the frame
and reads its line from the loaded class — the line of the `fillInStackTrace(I)` call — so the
report is byte-identical to S12's measurement, down to `Throwable.java:820`.

**An array method call resolves in `Object`, and this VM did not know that.** `getStackTrace()`
is `getOurStackTrace().clone()`, and its constant-pool method reference names
`[Ljava/lang/StackTraceElement;`, not `java.lang.Object`. JVMS §5.4.3.3 says a method reference
whose class is an array class resolves in `java.lang.Object`; this VM went straight to
`load_class("[Ljava/lang/StackTraceElement;")`, which asked the class path for
`[Ljava/lang/StackTraceElement;.class` and failed. The interpreter now rewrites an array owner to
`java/lang/Object` before loading it, and `Object.clone` has its array implementation.

## What the specification says

`Throwable.getOurStackTrace()` returns `stackTrace` when it is neither `null` nor
`UNASSIGNED_STACK`; only otherwise does it call `StackTraceElement.of(backtrace, depth)`. So a
filled `stackTrace` makes the private `Backtrace` object unnecessary — the JDK's format stays
out of this VM entirely.

`StackTraceElement` is a plain object: `declaringClass` and `methodName` are non-null strings,
`fileName` may be null, and `lineNumber == -2` is the conventional "unknown source/native
method" value (`isNativeMethod` compares against it). The fields this slice leaves null —
`classLoaderName`, `moduleName`, `moduleVersion` — are the module/loader metadata a class loader
would supply.

JVMS §5.4.3.3: "If C is an array class … the method is resolved in `java.lang.Object`." §6.5
`invokevirtual` repeats the rule for `clone`. `Object.clone`'s contract for an array is a new
array of the same type and length with the elements copied; a primitive copies its value and a
reference copies the handle.

## The bug I hit

The fixture's first run failed before reaching any of this: the exception handler stored the
caught throwable with `astore_1`, but the handler offset in the exception table was taken
*after* that instruction instead of before it, so execution resumed at `aload_1` with an
uninitialised local. The uncaught report read
`Exception in thread "main" java.lang.NullPointerException / at StackTrace.main(...)` — a
fixture bug that looked exactly like a VM bug. The bytecode builder takes raw offsets, so the
handler start has to be recorded before the handler's first instruction is emitted.

After that fix the real wall appeared: the array was length 0 (the `UNASSIGNED_STACK` reset
above), which is the measurement that forced the intrinsic.

## Measurements

| Fact | Value |
| :--- | ---: |
| Acceptance tests | 2 (one throw, two `getStackTrace()` calls) |
| Frames captured for `new IllegalStateException("boom")` | 6 |
| Innermost frame | `java.lang.Throwable.fillInStackTrace` |
| Outermost frame | `StackTrace.main` (the throwing method) |
| `getStackTrace()` twice | same length, same class, same method |
| `getMessage()` / `toString()` | `boom` / `java.lang.IllegalStateException: boom` |
| Uncaught report with the intrinsic | `at java.lang.Throwable.fillInStackTrace(Throwable.java:820)` (identical to S12) |
| Mutation: `stackTrace` field not set | 0 passed / 2 failed, `array index 0 out of bounds for length 0` |
| Workspace, `cargo test --workspace` | 101 passed / 0 failed / 0 ignored (27 test binaries, 18 non-empty) |

The `backtrace` field is never written; `Shared::traces` still holds the VM-side trace, and the
two now agree because both come from the same capture.

## Correction (S26)

The acceptance above says the array's first element is `fillInStackTrace`, which was the VM's
first implementation, not the host's behaviour. HotSpot hides the leading `fillInStackTrace` and
throwable-constructor frames; S26 measured it against `java` and changed the filter, so the trace
now starts at the throwing method (see the S26 note).

## Open threads

- **`printStackTrace()` still cannot run.** It reaches `System.out`/`PrintStream`, which is G2
  (ADR-0014) and waits for the `Unsafe`/printing slice. The Java view it would print is now in
  place: `getOurStackTrace()` returns the captured array.
- **The JDK's `backtrace` object is bypassed on purpose.** If a later slice wants the JDK's
  `ExtendedStackFrameInfo` format, it can fill `backtrace` and let `StackTraceElement.of` run;
  this slice proves that is not required for `getStackTrace()`.
- **An explicit `fillInStackTrace()` still keeps the first trace.** The JDK re-records; the S12
  deviation stands, now with the Java view behind it.
- **The intrinsic skips the method's `synchronized` monitor.** The native dispatch runs before
  the frame is pushed, so no monitor is taken for `fillInStackTrace()`. Single-threaded this is
  unobservable; the monitors slice (M3) should decide whether an intrinsic keeps the flag.
- **`Object.clone` is array-only.** An instance clone returns `Unsupported`; no slice has needed
  one yet.
- **The intrinsic mechanism is described in rustdoc only.** ADR-0005 covers natives keyed by
  `(owner, name, descriptor)`; it does not mention a VM-replaced Java body.
