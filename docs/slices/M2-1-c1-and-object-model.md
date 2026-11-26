# M2.1–M2.12 slice detail — C1 metadata, object model and execution

Per-slice record for M2.1–M2.12. `PROJECT.md` § Slices owns the status and the one-line
description; this file owns what landed, where it stops and the evidence. The checks every
entry closed with are in the [gate baseline](README.md).

## M2.1 — C1 metadata model and normalized dump — 2026-11-24

**Acceptance.** `cargo test -p rava-classfile --test metadata_model` — exit 0 (29 new tests).

**Delivered.**
- A typed model for every C1 attribute ADR-0006 lists, with each raw payload retained.
- A normalized dump with committed golden files (`Kitchen`, `Defaults`).

**Boundary.**
- Generic signatures stay encoded strings; no `Type` graph or reflection native yet.
- The dump is a fixture, not a reflection API; `Code`-level type annotations are decoded but unexercised.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md).

## M2.2 — JDK 17 signature stubs and zero metadata diff — 2026-11-24

**Acceptance.** `cargo run -p rava-cli -- compat metadata --check` — exit 0.

**Delivered.**
- `stdlib/c1/classes/`: 1047 class files — 1011 projected from `ct.sym` release 17 and 36 header-only placeholders.
- The normalized dump differs by 0 across the 1011 referenced classes; the M1 tree is untouched.

**Boundary.**
- No `Code`, no verifier and no reflection native: the metadata shape and its diff only.
- The only normalization is the class-file version line.
- 69 platform owners sit outside the tree's closure ([`c1-metadata.md`](../explanation/c1-metadata.md) records the tolerance).

See [`c1-metadata.md`](../explanation/c1-metadata.md).

## M2.3 — `stdlib/c1/classes` as the bootstrap library and its metadata through the VM — 2026-11-24

**Acceptance.** `cargo test -p rava-vm --test c1_runtime` — exit 0 (7 tests).

**Delivered.**
- `stdlib/c1/classes` is the bootstrap source through the existing `DirectorySource` and `(loader, name)` identity.
- Metadata decodes without a `Code` attribute, so a signature-only class loads.

**Boundary.**
- Loads and reads metadata only: no initialization, execution or reflection objects.

See [`c1-metadata.md`](../explanation/c1-metadata.md).

## M2.4 — Class objects and basic `Object`/`Class` reflection — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test class_objects` — exit 0 (8 tests).

**Delivered.**
- `HeapObject::Class(ClassId)` with one cached object per identity, enumerated by `Shared::roots`.
- The native ABI carries a receiver, and the registry is consulted before `Code`, which is what makes the C1 stubs run.
- `Object.getClass` plus `Class.getName`/`getModifiers`/`isArray`/`getSuperclass`/`forName`.

**Boundary.**
- An array has no class identity yet, so `get_class` returns a typed error; `forName` is bootstrap-only and does not initialize.
- `Field`, `Method`, `Constructor`, annotations and generics remain later slices.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md).

## M2.5 — Instance allocation, constructors, fields and instance calls — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test instance_execution` — exit 0 (8 tests).

**Delivered.**
- `new`/`dup`/`invokespecial`/`getfield`/`putfield` and narrow/wide `aload`/`astore`.
- Superclass-first field layouts and allocation with default values.
- `run_instance_method` puts `this` in local 0 and leaves static calls unchanged.

**Boundary.**
- The JVM's uninitialized state is not modelled: `new`/`dup`/`invokespecial` is the supported sequence.
- No access checks, no `putstatic`, no interface or default-method dispatch.

See [`interpreter-execution.md`](../explanation/interpreter-execution.md) §3.

## M2.6 — Arrays and integer control flow — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test arrays_control` — exit 0 (12 tests).

**Delivered.**
- `newarray`/`anewarray`/`multianewarray`, `arraylength`, the primitive loads/stores and `aaload`/`aastore`.
- `goto`, the integer `if` family and `ifnull`/`ifnonnull`, resolved to instruction indices at decode time.

**Boundary.**
- No array class identity; `aastore` checks no `ArrayStoreException`; byte/char/short stores keep the 32-bit value.
- Switches, `wide`, `checkcast`/`instanceof`, exceptions and interfaces remain later slices.

See [`interpreter-execution.md`](../explanation/interpreter-execution.md) §2.

## M2.7 — Exceptions, `athrow` and exception tables — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test exceptions` — exit 0 (9 tests).

**Delivered.**
- The `Code` exception table decoded to instruction ranges, with typed errors for malformed entries.
- `athrow` and a callee's `Thrown` search the frame table and jump to a handler; the object is never copied.

**Boundary.**
- No `finally` control flow, stack traces or exception-object synthesis; only `Thrown` crosses a call.
- Assignability walks superclasses only, and `athrow` of null is a typed error, not an NPE.

See [`interpreter-execution.md`](../explanation/interpreter-execution.md) §5.

## M2.8 — Reflection member objects (`Field`/`Method`/`Constructor`) — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test reflection_members` — exit 0 (7 tests).

**Delivered.**
- `HeapObject::Reflect(ReflectMember)` cached per `(class, kind, name, descriptor)` and enumerated as roots.
- `getDeclaredFields`/`Methods`/`Constructors` return real arrays; the read-only natives answer names, modifiers and types.

**Boundary.**
- No inherited lookup, `Field.get`/`set`, `Method.invoke` or `setAccessible`.
- Primitive and array descriptors are typed errors because their class identity does not exist yet.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §3.

## M2.9 — Core `Object`/`String` behavior — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test string_object` — exit 0 (8 tests).

**Delivered.**
- The `String` natives over UTF-16 units, plus `Object.equals`/`hashCode`/`toString`.
- Interning shared by `intern` and `ldc`, so a constant is one object.

**Boundary.**
- A lone surrogate is `UnpairedSurrogate` where the JDK returns a string holding it.
- Null and index failures are typed `VmError`s, not synthesized exceptions.
- No constructors, `String.valueOf`, `format`, case/trim/split or `getBytes` families yet.

See [`runtime-library.md`](../explanation/runtime-library.md) §2.

## M2.10 — Boxed values and reflection invocation — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test reflection_invoke` — exit 0 (9 tests).

**Delivered.**
- `HeapObject::Boxed(BoxedPrimitive::Integer)` as a heap object, dispatched as `java/lang/Integer`.
- `Field.get`/`set`, `Method.invoke` and `Constructor.newInstance` with `int` boxing and unboxing.
- A Java throwable crosses the native ABI as `Thrown` instead of being swallowed.

**Boundary.**
- Only `Integer` is boxed, and every `valueOf` allocates, so there is no small-value cache.
- No overload resolution, no `InvocationTargetException` wrapper and no access checks.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md).

## M2.11 — Annotation reflection and proxies — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test annotations` — exit 0 (10 tests).

**Delivered.**
- `HeapObject::Annotation` carrying the encoded values, and `HeapObject::Enum` cached per constant.
- Queries on `Class` and every member type, `@Inherited` merging, `AnnotationDefault` fallback and element dispatch.
- Deterministic `equals`/`hashCode`/`toString` on an annotation.

**Boundary.**
- Only runtime-visible annotations; annotation objects are ordinary allocations.
- No repeatable containers, `getAnnotationsByType`, parameter or type annotations.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §4.

## M2.12 — `StringBuilder` and core collection behavior — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test collections` — exit 0 (12 tests).

**Delivered.**
- `HeapObject::StringBuilder` and `HeapObject::List`, selected by class identity, with roots enumeration.
- The builder's `append` overloads and UTF-16-unit edits; the list's `add`/`get`/`set`/`remove`/`contains`/`indexOf`.

**Boundary.**
- Membership is reference equality; a Java `equals` override is not consulted.
- No iterators, streams, `HashMap`, `StringBuffer` or list `toString` in this slice.

See [`runtime-library.md`](../explanation/runtime-library.md) §4.
