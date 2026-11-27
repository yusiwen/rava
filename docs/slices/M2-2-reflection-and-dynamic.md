# M2.13–M2.22 slice detail — maps, lookup, dynamic execution and generics

Per-slice record for M2.13–M2.22. `PROJECT.md` § Slices owns the status and the one-line
description; this file owns what landed, where it stops and the evidence. The checks every
entry closed with are in the [gate baseline](README.md).

## M2.13 — `HashMap` and `String.valueOf`/value conversion — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test maps_values` — exit 0 (16 tests).

**Delivered.**
- `HeapObject::Map` as an insertion-ordered vector of pairs, with `Shared::roots` enumerating keys and values.
- The map constructors and `size`/`isEmpty`/`put`/`get`/`containsKey`/`containsValue`/`remove`/`clear`.
- `String.valueOf` over `Object` and the primitives, sharing the M2.9 conversion rules.

**Boundary.**
- Linear scan, no hashing, tree bins or resize: capacity and load factor are hints.
- Keys compare by the VM rule (strings by content, `Integer` by value, the rest by identity).
- No `keySet`/`values`/`entrySet`, iterators or `Map`-interface dispatch yet.

See [`runtime-library.md`](../explanation/runtime-library.md) §4.

## M2.14 — Named member lookup (`getDeclaredMethod`/`getField`/`getConstructor`) — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test member_lookup` — exit 0 (5 tests).

**Delivered.**
- The six `getDeclared*`/`get*` lookups, matching a field by name and an executable by name plus parameter descriptors.
- Inherited lookup walks the superclass chain and interfaces once; a hit is the cached M2.8 object.

**Boundary.**
- A miss returns `null` at this point; M2.30 turns it into the JDK exceptions.
- No access-control filter, and primitive or array parameter classes are not modelled yet.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §3.

## M2.15 — Primitive `Class` identity and primitive member types — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test primitive_classes` — exit 0 (6 tests).

**Delivered.**
- `HeapObject::PrimitiveClass` for the nine kinds with a one-object-per-kind cache.
- Primitive member types and named-lookup descriptors; the wrapper `TYPE` fields resolved by `getstatic`.

**Boundary.**
- A primitive class declares no members; array identity is still absent.
- Annotation queries on a primitive class remain typed errors.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §2.

## M2.16 — Array `Class` identity and array member types — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test array_classes` — exit 0 (6 tests).

**Delivered.**
- `ClassRef`/`ArrayClass` and `HeapObject::ArrayClass`, cached per component and enumerated as roots.
- Array member types and lookup descriptors, and `Class.forName` of `[I`, `[[I` and `[Ljava.lang.String;`.

**Boundary.**
- Array covariance and `ArrayStoreException` stay absent; an array class declares no members.
- `Class.forName` resolves a named component through the bootstrap loader only.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §2.

## M2.17 — `ldc`/`ldc_w` class constants and class literals — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test class_literals` — exit 0 (5 tests).

**Delivered.**
- `ldc`/`ldc_w` of a `CONSTANT_Class` resolve through the executing frame's loader, so a child loader's class keeps its identity.
- Named, primitive and array literals all push the cached class object; the class loads but does not initialize.

**Boundary.**
- Only class constants: numeric constants and `ldc2_w` are typed decode errors at this point.
- `Class.toString` keeps the M2.9 `ClassName@hex` shape rather than the JDK's `class X`.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §2.

## M2.18 — `invokedynamic` string concatenation — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test invokedynamic_concat` — exit 0 (13 tests).

**Delivered.**
- Opcode `0xba` decoding with a typed on-demand `BootstrapMethods` view.
- `StringConcatFactory.makeConcat`/`makeConcatWithConstants` over the shared value conversion, with the recipe constants consumed exactly.

**Boundary.**
- `StringConcatFactory` call sites only; every result is interned, with no call-site cache.
- `LambdaMetafactory`, method handles and constant-dynamic remain later slices.

See [`interpreter-execution.md`](../explanation/interpreter-execution.md) §4.

## M2.19 — Lambda metafactory and `invokeinterface` — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test lambdas` — exit 0 (13 tests).

**Delivered.**
- `HeapObject::Lambda` with the interface identity, SAM, target and captured values; no generated class file.
- `LambdaMetafactory.metafactory`/`altMetafactory`, and `invokeinterface` resolving the runtime class first, then a default method in the interface hierarchy.

**Boundary.**
- No argument adaptation: boxing, widening, casts and generic bridges are typed errors.
- `FLAG_SERIALIZABLE` is accepted but not represented: no generated class or `writeReplace`, so serialization fails; no call-site cache, `SerializedLambda` or access checks.

See [`interpreter-execution.md`](../explanation/interpreter-execution.md) §4.

## M2.20 — Classpath runner (`rava run`) — 2026-11-24

**Acceptance.** `cargo test -p rava-cli --test classpath_run` — exit 0 (13 tests).

**Delivered.**
- `ClassPathSource` over directories and jars in order, and one application loader parented to the bootstrap loader.
- `rava-vm::library::register_all` as the single registration point, plus a minimal `PrintStream` and `System.out`/`System.err`.
- `rava run` requiring a static `main([Ljava/lang/String;)V` and allocating its `String[]`.

**Boundary.**
- No `-jar`, `verify`, wildcard classpath, manifest `Class-Path` or multi-release selection.
- `PrintStream` has three methods, and both streams write the same captured stdout.

See [`class-loading-and-libraries.md`](../explanation/class-loading-and-libraries.md).

## M2.21 — Static fields, casts, switches and numeric constants — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test bytecode_surface` — exit 0 (15 tests).

**Delivered.**
- `putstatic` keyed by `(name, descriptor)` on the declaring class, with typed static/instance and kind errors.
- `checkcast`/`instanceof` over named, interface and array types with covariance.
- `tableswitch`/`lookupswitch` decoded with alignment and key-order checks; int/float/long/double constants with the comparison and return forms that observe them.

**Boundary.**
- Assignability is a name-level walk of loaded classes, and a missing C1 interface is skipped.
- No `wide`, monitors or `final`/access checks; a `long`/`double` still uses one local slot until issue #52.

See [`interpreter-execution.md`](../explanation/interpreter-execution.md) §2.

## M2.22 — Generic `Type` graph and generic reflection — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test generic_types` — exit 0 (7 tests).

**Delivered.**
- A JVMS signature parser for class, method and field signatures, with typed errors.
- `HeapObject::Type` objects for parameterized types, type variables, wildcards and generic arrays, cached structurally.
- The generic queries on `Class`, `Field`, `Method` and `Constructor` plus the four interface accessors.

**Boundary.**
- `Signature` only: a member without one reports raw `Class` objects.
- No `getGenericExceptionTypes`, record-component generics or `AnnotatedType`.
- Type text is the VM's deterministic formatter, not the JDK's exact spelling.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md).
