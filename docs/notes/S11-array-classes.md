# S11 — An array knows its class, and `getComponentType` is a field, not a native

- **Slice:** S11
- **Requirements:** R4 (the library's `Class` queries, not a private array model)
- **Acceptance:** `cargo test -p rava-cli --test arrays` exited 0 on 2026-11-20 (3 tests).

## What surprised me

**The array class was the last anonymous heap object.** `HeapObject::Array` carried its element
kind and its elements, and nothing else. `Shared::class_of` therefore had no answer for an array:
`getClass()` failed with `getClass on a value with no runtime class`, and `checkcast [B` went
through `load_class("[B")`, which reports `ClassNotFound: [B`. The fix is a descriptor on the
array, not a special case at every call site: `Array.class_name` holds `[I`,
`[Ljava/lang/String;` or `[[I`, and it is filled at the six places that allocate an array.

**JDK 21's `Class.getComponentType()` is Java, not native.** The task said to add it as a native;
`javap -p -c java.lang.Class` shows a method body instead:

```
0: aload_0
1: invokevirtual #46   // Method isArray:()Z
4: ifeq 12
7: aload_0
8: getfield #13        // Field componentType:Ljava/lang/Class;
11: areturn
12: aconst_null
```

So a registered native would never be reached. The model fix is to populate the final
`componentType` field when `Shared::type_class` creates an array class object. The native is
registered anyway, for the native seam and for an image that declares it native, but the field is
what the host library reads. This is the same shape as S9: the library owns the field layout, and
the VM fills the field the library actually reads.

**Arrays are not `Object[]` by being `Object`s.** The JVM subtype rules are their own: an array is
an `Object`, `Cloneable` and `Serializable`; array-to-array is covariant in a reference component
(`String[]` is an `Object[]`) and exact in a primitive one (`int[]` is not an `Object[]`); a nested
array component is a reference, so `[[I` is an `[Ljava/lang/Object;`. Implementing the recursion
was smaller than documenting an exact-match exception, so covariance is complete rather than
partial.

**The host's array modifiers are `PUBLIC | FINAL | ABSTRACT`.** Measured on the host JDK:
`int[].class.getModifiers()` returns 1041 (`0x411`), not `0` and not `PUBLIC|FINAL`. The probe
also confirmed `int[].class.getSuperclass()` is `Object.class`, which the query now reports.

## What the specification says

- JVMS §4.3.2: an array class has a descriptor of the form `[` + component descriptor; it is not
  loaded from a class file and has no `ClassId` in this VM.
- JVMS §5.3 / JLS §10.8: the direct superclass of an array type is `Object`.
- JVMS §4.10.1.2: array assignability is by component, with reference components covariant.
- JVMS §6.5 `anewarray`: the operand names the *element* type, so the array descriptor is
  `[L<name>;` for a class and `[` + descriptor for an array element.
- JVMS §6.5 `multianewarray`: the operand is the full array descriptor, and each allocated level
  is itself an array class (`[[I` -> `[I` -> the `int` array).

## The bug I hit

The first fixture run reported `SAME = 0` while the Rust assertion that both class references
were the same `ObjRef` passed. The test's `store_boolean` emitted `0xa6` for the identity branch,
but `0xa6` is `if_acmpne`; `if_acmpeq` is `0xa5`. The wrong branch inverted the answer, exactly
the family of mistake S10's hand-computed offset made. The assembler helper now records the
opcode position and patches the offset, and the opcode was corrected against the JVMS table.
The mutation run reproduced the same failure mode from the VM side: with the `get_class` array
branch removed, the test stops at `getClass on a value with no runtime class (ObjRef(2))`
(exit 101, 2 passed / 1 failed).

**A second, older bug surfaced while writing the descriptor.** `allocate_multi` took the
*innermost* primitive kind (`element_kind("[[I")` is `Int`) and used it for the last allocated
level. That is right for `new int[2][3]`, but wrong for `new int[2][]`: the outer level is an
array of `[I`, so its elements are references, not ints. The fixture now stores a `new int[3]`
into `new int[1][]` and reads its length back; with the old kind that `aastore` is a type
mismatch. Each level derives its kind from its own component descriptor now.

## Measurements

| Fact | Value |
| :--- | ---: |
| Acceptance tests (`arrays.rs`) | 3 passed / 0 failed |
| Arrays test fixture assertions | 27 static fields: 18 booleans, 4 int answers, 3 names, 2 class refs |
| Workspace tests (`cargo test --workspace`) | 85 passed / 0 failed |
| Natives test target | 23 (unchanged) |
| Arrays test target | 3 |
| `int[].class.getModifiers()` | 1041 (`0x411`) on host and VM |
| Array allocation sites given a descriptor | 3 in the interpreter (`newarray`, `anewarray`, `multianewarray`) + 3 VM-created (`String.value`, `string_array`, `empty_reference_array`) |
| `String[].class.getName()` | `[Ljava.lang.String;` |
| Mutation (`get_class` array branch removed) | exit 101, 2 passed / 1 failed |

The wall this removes, measured: `StringLatin1.hashCode` -> `ArraysSupport.<clinit>` ->
`vectorizedHashCode` -> `checkcast [B` now passes the cast. `hashCode` itself still needs the
`Unsafe` unaligned accessors; `equals`/`concat`/`substring` still need `Unsafe.getIntUnaligned`
and `System.arraycopy`.

## Open threads

- **`Unsafe` and `System.arraycopy` are still the string wall.** S11 makes `checkcast [B`
  resolvable and gives `class_of` an array path; the remaining `String` methods wait for the
  `Unsafe` work and for S14's `arraycopy`.
- **`Shared::class_of` still returns `None` for arrays.** That is deliberate: an array has no
  `ClassId`, and returning `java.lang.Class` would make `instanceof` and `isAssignableFrom`
  treat the array as a `Class` instance. Callers that need the array class ask
  `Shared::array_class` (descriptor) or `Shared::type_class` (class object). Unifying the two
  would need an array class in the class table, i.e. a bigger model change.
- **`Class.getComponentType` is a field read, not a native, on this image.** The registered
  native is defensive; if a future image declares it native it answers from the same
  `class_targets` model.
- **Array class objects are not loaded classes.** `getComponentType`, `getName`, `isInstance`,
  `isAssignableFrom`, `getSuperclass`, `getModifiers`, `isArray` and `isPrimitive` answer for
  them; reflection (`Array.newInstance`, `Class.cast`, generic signatures) does not yet.
- **Covariance is complete, not partial:** reference components recurse through the loaded
  hierarchy, primitive components require an exact descriptor. There is no boxing conversion
  (`int[]` is not `Object[]`) and no test claims one.
