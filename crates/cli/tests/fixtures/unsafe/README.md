# M3.5 `Unsafe` U0 fixture

Committed source and class files for the `sun.misc.Unsafe` U0 / direct-buffer acceptance
test (`crates/cli/tests/c2_unsafe.rs`). CI has no JDK, so the `classes/` output is
committed next to the source; the test never compiles anything.

## Contents

- `src/demo/UnsafeProbe.java` — `public int value`, `public static int marker`, a static
  initializer that sets `marker = 7`, and a constructor that sets `value = 42`.
- `classes/demo/UnsafeProbe.class` — the compiled class (class-file version 61, Java 17).

The two observable facts are independent on purpose: `marker` shows whether the class was
initialized, while `value` shows whether the constructor ran.

## Rebuilding

Requires a JDK 17 or newer:

```sh
cd crates/cli/tests/fixtures/unsafe
javac --release 17 -d classes src/demo/UnsafeProbe.java
```

## Expected behavior

After the class is loaded but before initialization, `marker` is absent and
`jdk.internal.misc.Unsafe.shouldBeInitialized` reports `true` (the JDK's meaning: it still
needs initialization).

`Unsafe.allocateInstance(UnsafeProbe.class)` must:

1. initialize the class, so `UnsafeProbe.marker == 7`;
2. allocate the instance, so `value == 0`; and
3. skip the constructor, so `value` is not `42`.

A normal `new UnsafeProbe()` (allocate through the ordinary allocator, then run
`<init>()V`) still sets `value == 42`, which proves the constructor was not globally
suppressed.

`ensureClassInitialized` is idempotent: a second call leaves `marker == 7` and the class
initialized.
