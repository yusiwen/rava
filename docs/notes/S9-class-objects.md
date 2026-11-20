# S9 — Class objects are instances, and the wall behind the wall

- **Slice:** S9 (M1's first slice, landed early because S8 needs it)
- **Requirements:** R4
- **Acceptance:** `cargo test -p rava-cli --test class_object` exited 0 on 2026-11-20 (4 tests).

## What surprised me

**The wall was one `getfield`.** `FileOutputStream`'s initializer pulls in `SharedSecrets`,
which initializes `java.lang.invoke.MethodHandles`, whose static initializer evaluates
`MethodHandles.class.desiredAssertionStatus()`. That method is Java, and its first instruction
reads the `classLoader` field of the `Class` object. The VM stored class objects as
`HeapObject::ClassRef(ClassId)` — a tag with no fields — so the read was a type error. Every
class with an `assert` builds this trap; the io path just walked into it first.

**The fix is smaller than the diagnosis.** A class object is now a real
`Instance` of the host's `java.lang.Class` (allocated lazily, once), plus one VM-side map
`ObjRef -> ClassTarget` for the reverse direction. `class_of` already answered "what class is
this reference": now that answer is `java.lang.Class` for a class object, which is what makes
`instanceof Class`, virtual dispatch, and `getClass` work without special cases.

**OpenJ9 has the same shape with one difference.** It allocates the class object with
`J9AllocateObject(jlClass, TENURED | NON_INSTRUMENTABLE | HASHED)`, writes `classLoader`,
`protectionDomain`, `module` and a `vmRef` long holding the `J9Class*`, and stores it in
`J9Class.classObject`. The `vmRef` field exists because OpenJ9 ships its own JCL
(`jcl/src/java.base/.../Class.java`, line 191). This repository hosts the unmodified library
(ADR-0001), so the pointer lives in the VM's map instead — same semantics, no JCL patch.

## What the specification says

JVMS §5.3 makes the class object part of loading/linking, and nothing in the specification says
it is special at run time: it is an instance of `java.lang.Class`, and `assert` compiles to a
plain call on it. That is exactly the assumption the old representation violated.

## The bug I hit

Two, both mine. `getClass` on `new Object()` returns `Object.class`, not the enclosing fixture's
class — the first version of the fixture compared against the wrong object. And `initClassName`
must write the name into the object's `name` field: the host's `getName` is
`name != null ? name : initClassName()`, so without the write every call would re-enter the
native (and the test would not see the cache).

## Measurements

| Fact | Value |
| :--- | ---: |
| Acceptance tests | 4 |
| Natives test target | 22 (was 21) |
| Workspace tests | 72 |
| Class object for `String` | `Instance` of `java.lang.Class`, `classLoader` null |
| `initClassName("java.lang.String")` | cached in `Class.name` |

The probe that started this slice now runs `FileOutputStream`'s initializer through ten more
natives — CDS flags, `System.nanoTime`/`currentTimeMillis`, `Object.hashCode`, float/double bit
conversions, `Class.getPrimitiveClass`, `Reflection.getCallerClass`, `Runtime.availableProcessors`
— and stops at `jdk/internal/misc/Unsafe.registerNatives`, i.e. the M4 wall #9 predicted.

## Open threads

- **S8 is still blocked**, now on `Unsafe` alone: `MethodHandles.<clinit>` continues into the
  member-name machinery, which wants `Unsafe`. The class-object wall is gone.
- Array class objects are instances, but `class_of` still returns `None` for arrays (the heap
  array carries no element type), so `someArray.getClass()` is not modelled yet.
- `Class.isInstance`, `isAssignableFrom`, `getModifiers`, `getSuperclass` and the reflection
  natives are not implemented; nothing on the boot path calls them yet.
