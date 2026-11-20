# S13 — The `Class` queries answer from the model, not from reflection

- **Slice:** S13
- **Requirements:** R4
- **Acceptance:** `cargo test -p rava-cli --test class_basics` exited 0 on 2026-11-20 (2 tests).

## What surprised me

**All four are still native in JDK 21.0.9.** `javap -p java.lang.Class` shows `isInstance`,
`isAssignableFrom`, `getSuperclass` and `getModifiers` as `public native`; there is no
`getModifiers0` to chase. That fixes the registration keys exactly, and it means the natives are
reached through the same load-time resolution S7 built — the test only has to register before
`java/lang/Class` is loaded.

**The descriptor-backed class objects are where the semantics split.** A primitive or array class
object is `ClassTarget::Type(descriptor)` with no `ClassId`, so `is_assignable` cannot answer for
it. Three answers then fall out of the representation instead of needing a special case: a `Type`
is assignable only from the identical descriptor, has no superclass, and reports 0 modifiers.
That is right for the primitive cases, and knowingly wrong for arrays — the Java contract says an
array's superclass is `Object.class`. The VM cannot do better until arrays carry their element
type, which is S9's open thread.

**The modifier mask drops a bit that is set on almost every class.** `String`'s class-file flags
are `0x0031` (`PUBLIC | FINAL | SUPER`); the reflection answer is `0x0011`, so `ACC_SUPER`
(0x0020) must not leak. The `0x7611` mask is `PUBLIC | FINAL | INTERFACE | ABSTRACT | SYNTHETIC |
ANNOTATION | ENUM`; `ACC_ABSTRACT` is 0x0400 in a class file, not 0x0100.

## What the specification says

JVMS §5.4.4 defines assignability as the class itself, a superclass, or a transitively implemented
interface — exactly [`Shared::is_assignable`] — and the `java.lang.Class` javadoc fixes the edge
cases: `isInstance(null)` is `false`, `isAssignableFrom(null)` throws `NullPointerException`, and
`getSuperclass` is `null` for `Object`, interfaces and primitives. `getModifiers` reports the
`java.lang.reflect.Modifier` bits, which is a subset of the class-file access flags; the two
vocabularies share numbers but not their full set.

`A.isAssignableFrom(B)` is the same predicate as `B instanceof A`-without-an-instance:
`is_assignable(B, A)`. For a `Type` target the descriptor is the whole identity, so only an
identical descriptor is assignable; this VM does not model array covariance or boxing yet.

## The bug I hit

**`.clone()` that cloned the wrong thing.** The helper first returned `Result<&ClassTarget>`, and
chaining `.clone()` on it is ambiguous: the blanket `impl Clone for &T` clones the reference, not
the enum, so the match bindings disagreed with the rest of the function about whether `class` was
a `ClassId` or a `&ClassId`. The compiler reported it as E0106 (missing lifetime on the helper)
and then E0614/E0308 (`ClassId` deref, expected `ClassId` found `&ClassId`). The fix is smaller
than the diagnosis: borrow the target, pattern-match by reference, and copy the `ClassId` out only
before the one mutable call (`class_object`) in `getSuperclass`.

**The receiver is `args[0]`, and two of the four take no other argument.** My first draft gave
`getSuperclass`/`getModifiers` `expect_args::<2>`, which the arity check caught at compile time.
The fixture's `max_stack` is 3: a class literal plus `new` + `dup` is the deepest sequence.

## Measurements

| Fact | Value |
| :--- | ---: |
| Acceptance tests | 2 (the queries, and the `null` argument) |
| Workspace tests | 77 (was 75 before this slice) |
| Natives registered by this slice | 4 |
| `String` class-file flags | `0x0031` → `0x0011` (17) |
| `Runnable` class-file flags | `0x0601` → `0x0601` (1537), interface bit set |
| `Object` class-file flags | `0x0021` → `0x0001`, `ACC_SUPER` dropped |
| `String.getSuperclass()` | identical to the cached `Object.class` |
| `int.class.getSuperclass()` / `getModifiers()` | `null` / `0` |

## Open threads

- **Arrays are only half-modelled.** `class_of` returns `None` for `HeapObject::Array`, so
  `Object.class.isInstance(new int[0])` is `0`; and an array class object is a `Type`, so
  `getSuperclass()` is `null` instead of `Object.class`. Both need the array's element type.
- **Primitive modifiers are not guessed.** `int.class.getModifiers()` returns `0`, where the host
  returns `PUBLIC | FINAL | ABSTRACT` (`0x0411`). The acceptance only checks loaded classes.
- **`isAssignableFrom` does not model array covariance or boxing.** `Object[].class` is a `Type`
  descriptor, so `Object[].class.isAssignableFrom(String[].class)` is `0` even though Java says
  `true`; nothing on the boot path asks yet.
- **The reflection queries are still missing**: `getDeclaredFields`/`getDeclaredMethods`,
  `getInterfaces`, `getComponentType`, `isEnum`, `getEnclosingClass` and the annotation surface.
  The four here are the ones the `Class` contract's core needs.
- `Class.isAssignableFrom` treats a `Class` instance the VM did not allocate as non-assignable
  (returns `0`) rather than an error; there is no way for Java code to create one (the constructor
  is private), so this is defensive only.
