# S28 — Reflection fields and methods, without the JDK's MethodHandle accessors

- **Slice:** S28
- **Requirements:** R4
- **Acceptance:** `cargo test -p rava-cli --test reflection_members` exited 0 on 2026-11-20 — a
  `javac`-compiled program reads/writes an instance field, a static field, invokes a method and
  counts the declared members, compared with `java` byte for byte.

## What surprised me

**The JDK builds field accessors with MethodHandles now.** `Field.getInt` on a real `Field`
reached `MethodHandleAccessorFactory.newFieldAccessor` and threw `InternalError` (JDK 21 replaced
the `UnsafeFieldAccessorImpl` factory with a MethodHandle-based one). So the accessor path is M4's
`MethodHandle` surface, not this slice's; the VM registers intrinsics for `Field.get`/`set` and
the primitive variants instead, reading and writing its own layout slots.

**Static fields needed an offset encoding.** `Unsafe.staticFieldOffset(Field)`/`staticFieldBase`
back the JDK's static accessors: the VM returns `-1 - declared-field-index` as the offset and the
class object as the base, and the Unsafe accessors recognise a negative offset on a class object
as "read/write `class.statics[...]`". That keeps the encoding local to the Unsafe model and would
also serve the JDK's own accessors if MethodHandles were available.

**`Method.invoke` must box its result.** The intrinsic runs the method through the call seam, but
a primitive return has to come back as a wrapper (the Java signature returns `Object`); the first
run failed with `expected reference, found Int(42)`.

**A registration bug hid the whole slice.** The `field_pair!` macro registered the Rust function
names (`field_get_int`) instead of the Java names (`getInt`), so no intrinsic was bound and the
JDK body ran. A probe printing `MethodInfo.native` for the three names found it in one run.

## Measurements

| Fact | Value |
| :--- | ---: |
| Reflection members test | 1 differential test, 4 output lines |
| Workspace tests | 120 |
| Intrinsics added | `Field.get`/`set`, `getInt`/`setInt`, `getLong`/`setLong`, `getBoolean`/`setBoolean`, `getFloat`/`setFloat`, `getDouble`/`setDouble`, `Method.invoke` |

## Open threads

- `Field.getByte`/`getShort`/`getChar` (and the setters) are not intrinsics yet; they fall back to
  the MethodHandle accessor and fail like `getInt` did. Mechanical to add.
- Access checks, final-field rules and `InvocationTargetException` wrapping are skipped.
- `EnclosingMethod`/`InnerClasses` attributes are still reported as absent (S27).
- Resource bundles (#45) remain the `printf` blocker.
