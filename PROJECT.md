# PROJECT — rava (a Java 17 runtime for Spring)

**Tier:** L
**Owner:** yusiwen
**License:** MIT
**Started:** 2026-11-22

> Ledger. P0 and P1 were accepted on 2026-11-23. M1 is complete; M2 is in progress
> (M2.1, M2.2, M2.3, M2.4, M2.5, M2.6, M2.7, M2.8, M2.9, M2.10, M2.11, M2.12,
> M2.13, M2.14, M2.15, M2.16, M2.17 and M2.18 done). Keep this file honest and terse.

## Phase status

| Phase | What must be true | Artifact | Approved by | Status |
| :--- | :--- | :--- | :--- | :--- |
| P0 | Requirements confirmed by the human | `docs/requirements.md` | human | done |
| P1 | Decisions recorded and accepted | `docs/adr/` | human | done |
| P2 | Work sliced into verifiable increments | `PROJECT.md` | self | done |
| P3 | Every slice implemented | code + commits | self | todo |
| P4 | Tests pass, definition of done met | test suite | self | todo |
| P5 | Deployed, smoke-tested, rollback ready | `docs/deploy.md` | human | n/a |

P5 is `n/a` because this project ships a runtime library, not a hosted service; a
release process, if wanted, becomes its own phase.

## Milestones

> Gates are defined in `docs/requirements.md` § Milestones. P0 and P1 are accepted;
> M0 and M1 are complete, and both gate reviews have passed. M2 is in progress, with
> M2.1, M2.2, M2.3, M2.4, M2.5, M2.6, M2.7, M2.8, M2.9, M2.10, M2.11, M2.12, M2.13,
> M2.14, M2.15, M2.16, M2.17 and M2.18 done.

| Milestone | Delivers | Gate | Status |
| :--- | :--- | :--- | :--- |
| M0 | Foundation: parser/model, loader/ClassSource, heap/IR, Shared/ThreadState, safepoints, verifier skeleton, native registry, MemoryRegion | Hello + small corpus; metadata dump; loader identity; no-lock invariant | done |
| M1 | Surface manifest, target lock, `link-check` | Manifest reproducible; unresolved = 0 | done |
| M2 | C1 metadata, minimal library, Spring context | Metadata diff 0; bean graph starts | in-progress |
| M3 | C2 dynamic code: Proxy, CGLIB, Objenesis, `Lookup`, `Unsafe` U0 | C2 suite exits 0 | todo |
| M4 | C3 resources/jars, `-jar`, non-web Boot | Boot non-web application starts | todo |
| M5 | C3 concurrency, JMM, references, cleaner, Tomcat | Tomcat concurrent; JCStress 0 | todo |
| M6 | C4 reference web application | HTTP suite exits 0 | todo |
| M7 | Spring test subset | Spring subset exits 0 | todo |
| M8 | Performance envelope | ADR-0010 benchmark exits 0 | todo |
| M9 | Reactive target (after M0–M8) | Reactive HTTP suite exits 0 | todo |

## Slices

| Id | Slice | Requirements | Acceptance command | Status |
| :--- | :--- | :--- | :--- | :--- |
| M0.1 | Cargo workspace, toolchain, crates and the `xtask ci` gate | R13 | `cargo xtask ci` exits 0 | done |
| M0.2 | Class-file parser with full metadata retention | R13 | `cargo test -p rava-classfile` exits 0 | done |
| M0.3 | ClassSource and `(loader, name)` class identity | R13 | `cargo test -p rava-vm --test loader_identity` exits 0 | done |
| M0.4 | Heap handles, field layout and the `Shared`/`ThreadState` seams | R13 | `cargo test -p rava-vm --test heap_handles` exits 0 | done |
| M0.5a | Decoded IR, frames and a static call | R13 | `cargo test -p rava-interp --test arithmetic` exits 0 | done |
| M0.5b | Hello through the interpreter: class library and native minimum | R13 | `cargo test -p rava-interp --test hello` exits 0 | done |
| M0.6 | Safepoint polling, GC roots and the no-lock-across-Java-call invariant | R13 | `cargo test -p rava-vm --test safepoints` exits 0 | done |
| M0.7 | Verifier dataflow skeleton and the metadata dump fixture | R13 | `cargo test -p rava-vm --test verifier_skeleton` exits 0 | done |
| M1.1 | Target lock, reference app and pinned jar set | R2 | `cargo run -p rava-cli -- compat target` exits 0 | done |
| M1.2 | Static manifest over the pinned jars | R1 | `cargo run -p rava-cli -- compat manifest --check` exits 0 | done |
| M1.3 | Stub library surface and zero-unresolved link-check | R2 | `cargo run -p rava-cli -- compat link-check` exits 0 | done |
| M1.4 | Manifest reproducibility and M1 gate review | R1, R2 | `cargo run -p rava-cli -- compat check` exits 0 | done |
| M2.1 | C1 metadata model and normalized dump | R3 | `cargo test -p rava-classfile --test metadata_model` exits 0 | done |
| M2.2 | JDK 17 signature stubs and zero metadata diff | R3 | `cargo run -p rava-cli -- compat metadata --check` exits 0 | done |
| M2.3 | `stdlib/c1/classes` as the bootstrap library and its metadata through the VM | R3 | `cargo test -p rava-vm --test c1_runtime` exits 0 | done |
| M2.4 | Class objects and basic `Object`/`Class` reflection | R3 | `cargo test -p rava-interp --test class_objects` exits 0 | done |
| M2.5 | Instance allocation, constructors, fields and instance calls | R3 | `cargo test -p rava-interp --test instance_execution` exits 0 | done |
| M2.6 | Arrays and integer control flow | R3 | `cargo test -p rava-interp --test arrays_control` exits 0 | done |
| M2.7 | Exceptions, `athrow` and exception tables | R3 | `cargo test -p rava-interp --test exceptions` exits 0 | done |
| M2.8 | Reflection member objects (`Field`/`Method`/`Constructor`) | R3 | `cargo test -p rava-interp --test reflection_members` exits 0 | done |
| M2.9 | Core `Object`/`String` behavior | R3 | `cargo test -p rava-interp --test string_object` exits 0 | done |
| M2.10 | Boxed values and reflection invocation | R3 | `cargo test -p rava-interp --test reflection_invoke` exits 0 | done |
| M2.11 | Annotation reflection and proxies | R3 | `cargo test -p rava-interp --test annotations` exits 0 | done |
| M2.12 | `StringBuilder` and core collection behavior | R3 | `cargo test -p rava-interp --test collections` exits 0 | done |
| M2.13 | `HashMap` and `String.valueOf`/value conversion | R3 | `cargo test -p rava-interp --test maps_values` exits 0 | done |
| M2.14 | Named member lookup (`getDeclaredMethod`/`getField`/`getConstructor`) | R3 | `cargo test -p rava-interp --test member_lookup` exits 0 | done |
| M2.15 | Primitive `Class` identity and primitive member types | R3 | `cargo test -p rava-interp --test primitive_classes` exits 0 | done |
| M2.16 | Array `Class` identity and array member types | R3 | `cargo test -p rava-interp --test array_classes` exits 0 | done |
| M2.17 | `ldc`/`ldc_w` class constants and class literals | R3 | `cargo test -p rava-interp --test class_literals` exits 0 | done |
| M2.18 | `invokedynamic` string concatenation | R3 | `cargo test -p rava-interp --test invokedynamic_concat` exits 0 | done |

P0 and P1 are accepted; P2 is done because the M2 increments are sliced. All M0 and
M1 slices are done, and both gate reviews passed. M2.1, M2.2, M2.3, M2.4, M2.5,
M2.6, M2.7, M2.8, M2.9, M2.10, M2.11, M2.12, M2.13, M2.14, M2.15, M2.16, M2.17 and
M2.18 are done; M2.19 is next.

## Slice log

| Slice | Acceptance command | Exit | Date | Evidence |
| :--- | :--- | :--- | :--- | :--- |
| M0.1 | `cargo xtask ci` | 0 | 2026-11-24 | Workspace, crates and the local gate; no runtime behavior yet |
| M0.2 | `cargo test -p rava-classfile` | 0 | 2026-11-24 | Parser implemented; raw attributes retained; 13 tests pass |
| M0.3 | `cargo test -p rava-vm --test loader_identity` | 0 | 2026-11-24 | Directory and DEFLATE jar sources; parent-first delegation; 6 tests pass |
| M0.4 | `cargo test -p rava-vm --test heap_handles` | 0 | 2026-11-24 | Generation handles, field layout, arrays and safepoint handshake; 6 tests pass |
| M0.5a | `cargo test -p rava-interp --test arithmetic` | 0 | 2026-11-24 | Decoded integer IR, frame, `invokestatic`; 3 tests pass |
| M0.5b | `cargo test -p rava-interp --test hello` | 0 | 2026-11-24 | `getstatic`/`ldc`/`invokevirtual`, class initialization and native PrintStream; 2 tests pass |
| M0.6 | `cargo test -p rava-vm --test safepoints` | 0 | 2026-11-24 | Frame-root stack with RAII cleanup, publish-before-acknowledge safepoint handshake, `Shared` root enumeration, lock probe at call boundaries, `MemoryRegion` seam and hook registration points; 9 tests pass; `cargo xtask ci` exits 0 |
| M0.7 | `cargo test -p rava-vm --test verifier_skeleton` | 0 | 2026-11-24 | Stack-height and definite-local dataflow over a branch/handler worklist, typed underflow/overflow/local/truncation/unsupported errors, deterministic metadata dump for `Hello.class`; 12 tests pass; `cargo xtask ci` exits 0 |
| M1.1 | `cargo run -p rava-cli -- compat target` | 0 | 2026-11-24 | `compat/targets.toml` pins Java 17, Boot 3.5.16, Spring 6.2.19, Tomcat 10.1.55, the reference app source hash and a 35-jar set; the reference app builds at release 17; a clean refetch reproduces all 35 sha256 entries and `bash compat/fetch-jars.sh --check` passes; `cargo test -p rava-cli` passes 17 tests |
| M1.2 | `cargo run -p rava-cli -- compat manifest --check` | 0 | 2026-11-24 | `compat/surface/spring-boot-3.5.16.txt` is generated by a static scan of all 11,754 `.class` entries in the 35 pinned jars: 980 classes, 177 fields, 3,626 methods and 1,048 interface methods under `java/`, `javax/`, `jdk/` and `sun/`; the manifest is deduplicated, sorted and byte-reproducible; `cargo test -p rava-cli` passes 25 tests |
| M1.3 | `cargo run -p rava-cli -- compat link-check` | 0 | 2026-11-24 | `rava compat stubs` writes 980 signature-only class files under `stdlib/classes/` from the committed manifest (177 fields, 3,626 methods, 1,048 interface methods); regeneration is byte-identical; link-check resolves all 980 classes and 4,851 member records with `unresolved 0`; `cargo test -p rava-cli` passes 41 tests; `cargo xtask ci` exits 0 |
| M1.4 | `cargo run -p rava-cli -- compat check` | 0 | 2026-11-24 | `compat check` passes all four stages: the target lock and its 35-jar set agree; the manifest regenerates byte-identically at 980 classes, 177 fields, 3,626 methods and 1,048 interface methods; link-check resolves 980 classes and 4,851 member records with `unresolved 0`; a temporary regeneration of `stdlib/classes` is byte-identical across all 980 class files and the committed tree is not rewritten; `cargo test -p rava-cli` passes 54 tests; `cargo xtask ci` exits 0 |
| M2.1 | `cargo test -p rava-classfile --test metadata_model` | 0 | 2026-11-24 | `rava-classfile`'s `metadata` module decodes the C1 attributes ADR-0006 lists into a typed model: access flags, binary name, superclass, interfaces, `Signature`, visible/invisible annotations with every element-value tag and repeatable containers as encoded, parameter and type annotations (all `target_info` variants and all four `type_path` kinds), nest host/members, permitted subclasses, inner and enclosing classes, `Record` components with their own annotations, `SourceFile`, `ConstantValue`, `Deprecated`, `Exceptions`, `MethodParameters`, `AnnotationDefault`; every attribute payload is still copied verbatim, so an undecoded attribute is retained; malformed payloads return `Error::BadAttribute` with a structured cause; a normalized dump with committed golden files for `Kitchen` and `Defaults`; 29 new tests, 43 `rava-classfile` tests; `cargo xtask ci` exits 0 |
| M2.2 | `cargo run -p rava-cli -- compat metadata --check` | 0 | 2026-11-24 | `stdlib/c1/classes/` holds 980 committed class files: 952 projected from the host JDK 25 `lib/ct.sym` release-17 view (1,104 fields, 13,008 methods, class-file version 61.0) and 28 header-only placeholders for manifest owners Java 17 does not define, listed in `compat/surface/c1-unreferenced.txt`; the normalized M2.1 metadata dump has 0 differences across the 952 referenced classes, the only normalization being the class-file version line (ct.sym stores 69); `stdlib/classes` is untouched and `compat check` still resolves 980 classes and 4,851 member records with `unresolved 0`; `cargo test -p rava-cli` passes 67 tests; `cargo xtask ci` exits 0 |
| M2.3 | `cargo test -p rava-vm --test c1_runtime` | 0 | 2026-11-24 | `stdlib/c1/classes` is the bootstrap class source through the existing `DirectorySource` and `(loader, name)` identity; `Shared::with_library_dir`, `Shared::load_library_class`, `Shared::library_metadata` and `ClassEntry::metadata()` decode C1 metadata without a `Code` attribute; the test loads `java.lang.String`, `java.lang.Class`, `java.util.List`, sealed `java.lang.reflect.Executable` and annotation type `java.lang.Deprecated`, checks superclass/interfaces/signatures/field and method descriptors/annotations/permitted subclasses, a second loader's distinct identity and no `Code` on any stub method; 7 tests; `cargo test -p rava-vm` passes 42 tests; `cargo xtask ci` exits 0 |
| M2.4 | `cargo test -p rava-interp --test class_objects` | 0 | 2026-11-24 | `HeapObject::Class(ClassId)`, its heap allocator/accessor, `Shared::class_object` (one cached object per `(loader, name)` identity) and `Shared::get_class` for `Str`/`Instance`/`Class` (arrays return a typed error); cached class objects are enumerated by `Shared::roots`; the native ABI takes the receiver `ObjRef` (`ObjRef::NULL` for static calls), and `run_static_method`/`invokevirtual` consult the registry before `Code`, so the C1 stubs (no `Code`, no `ACC_NATIVE`) execute; `invokevirtual` resolves `HeapObject::Class` as `java/lang/Class` and `HeapObject::Str` as `java/lang/String`, walking the superclass chain; `Object.getClass`, `Class.getName/getModifiers/isInterface/isArray/getSuperclass/forName`; `ReflectProbe` fixture; 8 tests; `cargo test -p rava-interp` 16, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 175; `cargo xtask ci` exits 0 |
| M2.5 | `cargo test -p rava-interp --test instance_execution` | 0 | 2026-11-24 | `new`, `dup`, `aconst_null`, `invokespecial`, `getfield`, `putfield` and narrow/wide `aload`/`astore` decode and execute; `run_instance_method` reuses the existing `Frame`, puts `this` in local 0 and leaves static calls on the unchanged path; `Shared::field_layout` builds superclass-first instance layouts (static fields excluded) and `Shared::allocate_instance` allocates with field defaults; `new` resolves and initializes the class from the current class's loader; `getfield`/`putfield` resolve the exact owner/name/descriptor, use the receiver's runtime layout and reject a static field through an instance opcode and a descriptor mismatch; `invokespecial` resolves on the instruction's owner while `invokevirtual` dispatches from the receiver's class, and both prefer the native registry before executing `Code`; `instance::register_natives` supplies `java/lang/Object.<init>()V` as a no-op for the C1 stub that has no `Code`; `Point`/`PointProbe` fixture; 8 tests; `cargo test -p rava-interp` 24, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 183; `cargo xtask ci` exits 0 |
| M2.6 | `cargo test -p rava-interp --test arrays_control` | 0 | 2026-11-24 | `newarray` (all eight primitive `atype` values), `anewarray`, `multianewarray`, `arraylength`, the eight primitive array loads/stores, `aaload`/`aastore`, `iinc`, `goto`, `ifeq`/`ifne`/`iflt`/`ifge`/`ifgt`/`ifle`, `if_icmp{eq,ne,lt,ge,gt,le}` and `ifnull`/`ifnonnull` decode and execute; branch offsets are resolved at decode time to instruction indices, so the dispatch loop never computes a byte address and a non-boundary target is a decode error; `newarray`/`anewarray`/`multianewarray` allocate through the existing `Heap`/`ArrayKind`/`Array`, and `anewarray` resolves an object component through the current class's loader; bounds, null, negative-size and `ArrayKind`/element-type errors are typed; the `ArrayProbe` fixture (`sum` with `if_icmpge`/`goto`/`iaload`/`arraylength`, `make` with `newarray`/`iastore`, `refs` with `anewarray`/`aastore`, a two-dimensional `multianewarray`, and one method per comparison) exercises them; 12 tests; `cargo test -p rava-interp` 36, `cargo test -p rava-vm` 42, `cargo test -p rava-classfile` 50 and `cargo test -p rava-cli` 67 pass, workspace 195; `cargo xtask ci` exits 0 |
| M2.7 | `cargo test -p rava-interp --test exceptions` | 0 | 2026-11-24 | The `Code` exception table is decoded after the code array and its `start_pc`/`end_pc`/`handler_pc` become instruction indices through the existing two-pass boundary map (a non-boundary, out-of-range, empty or reversed entry and a non-`Class` `catch_type` are decode errors); `athrow` and a callee's `Thrown` result search the current frame's table in class-file order for an entry whose range contains the throwing instruction and whose catch type is assignable from the throwable's runtime class through the superclass chain, clearing the stack and jumping to the handler on a match and otherwise re-raising the same `ObjRef`; `catch_type = 0` is a catch-all; `InterpError::Thrown(ObjRef)`, `NullThrow` and `NonReferenceThrow` are new; the `Boom`/`Catcher` fixture covers a direct catch returning 5, a catch-all, a superclass catch, a wrong catch type, a throw crossing two calls into the outer frame, an uncaught throw that still carries the object, null and non-reference `athrow`, and three malformed tables; 9 tests; `cargo test -p rava-interp` 45, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 204; `cargo xtask ci` exits 0 |
| M2.8 | `cargo test -p rava-interp --test reflection_members` | 0 | 2026-11-24 | `HeapObject::Reflect(ReflectMember)` with a `MemberKind` of `Field`/`Method`/`Constructor`, the declaring `ClassId`, name and descriptor; `Shared::reflect_member` caches one object per `(class, kind, name, descriptor)` and `Shared::roots` enumerates those roots, while `Shared::get_class` and `invokevirtual` dispatch a member to `java/lang/reflect/{Field,Method,Constructor}`; `Class.getDeclaredFields/getDeclaredMethods/getDeclaredConstructors` return real heap reference arrays built from the typed C1 metadata model, and the read-only member natives cover `getName`/`getModifiers`/`getDeclaringClass`, `Field.getType`, `Method.getReturnType/getParameterTypes/getParameterCount` and `Constructor.getParameterTypes/getParameterCount/getName`; the `ReflectMembersProbe` fixture exercises them through the C1 bootstrap library; 7 tests; `cargo test -p rava-interp` 52, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 211; `cargo xtask ci` exits 0 |
| M2.9 | `cargo test -p rava-interp --test string_object` | 0 | 2026-11-24 | `HeapObject::Str` stays the only string representation; the `java/lang/String` registry covers `length`, `isEmpty`, `charAt`, `equals`, `hashCode`, `compareTo`, `concat`, both `substring` overloads, `startsWith`, `endsWith`, `indexOf`, `replace(CC)`, `toCharArray`, `toString` and `intern`, and every operation reads UTF-16 code units from the stored text through `utf16_units`/`string_from_utf16`, so a supplementary character is two units; `java/lang/Object` gains `equals` (reference identity), `hashCode` (a deterministic mix of the `ObjRef` index and generation, documented as the identity hash and cached by nothing because the handle never changes) and `toString` (`ClassName@hexHashCode` with the dotted runtime binary name); `Shared::intern_string` keeps one canonical object per text as a permanent root, and `ldc` goes through it so repeated execution of a string constant reuses one object; `StringProbe` with 50+ `ldc` and argument helpers plus 8 tests; `cargo test -p rava-interp` 60, `cargo test -p rava-vm` 45 and `cargo test -p rava-cli` 67 pass, workspace 222; `cargo xtask ci` exits 0 |
| M2.10 | `cargo test -p rava-interp --test reflection_invoke` | 0 | 2026-11-24 | `HeapObject::Boxed(BoxedPrimitive::Integer)` is a heap object, not a second `Value` variant; `Shared::get_class` and `invokevirtual` dispatch it to `java/lang/Integer`, so `getClass` reports the boxed class; `Shared::box_int`/`unbox_int` and the `boxed::{unbox_argument,box_result}` helpers convert `int` boxes and pass reference handles through; the `java/lang/Integer` registry overrides `valueOf(I)`, `intValue`, `toString()`, `toString(I)`, `equals`, `hashCode`, `compareTo(Integer)` and `parseInt(String)`; `Field.get`/`set` cover `int` and reference instance fields, reject a static field and a null or non-assignable receiver, and preserve reference identity; `Method.invoke` runs static and instance methods by the reflected name and descriptor with no overload lookup, unboxes `int` arguments, boxes an `int` result and ignores the receiver for a static method; `Constructor.newInstance` allocates the declaring class, unboxes its arguments and runs `<init>` through `run_instance_method`; a Java throwable from an invoked method crosses the native ABI as `VmError::Thrown` and becomes `InterpError::Thrown`, so it is not swallowed (`InvocationTargetException` wrapping is deferred); the `Bean`/`BoxProbe` fixture covers boxed virtual calls, `(I)V` and `()V` construction, int/reference field access, getters/setters and static `sum`, plus wrong receivers, a static field, wrong arity and type, a missing-descriptor method and the no-`getDeclaredMethod` boundary; 9 tests; `cargo test -p rava-interp` 70, `cargo test -p rava-vm` 45 and `cargo test -p rava-cli` 67 pass, workspace 232; `cargo xtask ci` exits 0 |
| M2.11 | `cargo test -p rava-interp --test annotations` | 0 | 2026-11-24 | `HeapObject::Annotation` carries the annotation type's `ClassId` and the encoded values from the M2.1 metadata model, and `HeapObject::Enum` (cached per `(class, name)` as a permanent root) represents an enum constant; `Class`, `Field`, `Method` and `Constructor` gain `getDeclaredAnnotations`, `getAnnotations` and `getAnnotation(Class)` over that metadata in class-file order, returning an empty array for no declared annotations and null for a missing single annotation; `Class.getAnnotations` merges superclass annotations whose type is itself `@Inherited`, nearest declaration winning, while `getDeclaredAnnotations` stays declared-only; an omitted element falls back to the annotation type's `AnnotationDefault`; element values decode through the annotation type's loader as interned strings, `Value` primitives, class literals (`c`), enum constants (`e`), nested annotation objects (`@`) and primitive/reference arrays (`[`); `invokevirtual` dispatches an annotation receiver to its annotation type and the annotation module answers `annotationType`, every element method and deterministic `equals`/`hashCode`/`toString`; `java/lang/annotation/Annotation.annotationType` and the three queries on `Class`/`Field`/`Method`/`Constructor` (plus `AccessibleObject.getAnnotations`, where the C1 stub declares it) are registry natives, and `java/lang/Enum.name`/`toString` report a constant's name; the committed release-17 fixture under `crates/interp/tests/fixtures/annotations/` annotates a class, field, method and constructor with string/int/boolean/long/double/class/enum/nested/array elements and defaults and an `@Inherited` superclass annotation; 10 tests; `cargo test -p rava-interp` 80, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 244; `cargo xtask ci` exits 0 |
| M2.12 | `cargo test -p rava-interp --test collections` | 0 | 2026-11-24 | `HeapObject::StringBuilder(String)` and `HeapObject::List(Vec<Value>)` are ordinary heap objects selected by their class identity in `Shared::allocate_instance`; `Shared::get_class`, `invokevirtual` dispatch and `Object.toString`'s runtime name report `java/lang/StringBuilder` and `java/util/ArrayList`, and `Shared::roots` enumerates every non-null reference held by a heap list. The builder natives cover the three constructors, the `String`/`Object`/`int`/`long`/`char`/`boolean`/`float`/`double`/`CharSequence` `append` overloads, `toString`, `length`, `charAt`, `setLength`, `reverse`, `deleteCharAt` and `insert(I,String)` over UTF-16 units (null String appends `"null"`, a split surrogate is `UnpairedSurrogate`); the list natives cover both constructors, `size`, `isEmpty`, both `add` forms, `get`, `set`, `clear`, both `remove` forms, `contains` and `indexOf` with reference-equality membership and typed bounds errors. The `CollectionsProbe` fixture exercises the bytecode path (including `getClass().getName()` on both VM classes), and the registry is called directly only for the `long`/`float`/`double` overloads the decoded instruction subset cannot load. 12 new tests; `cargo test -p rava-interp` 92, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 256; `cargo xtask ci` exits 0 |
| M2.13 | `cargo test -p rava-interp --test maps_values` | 0 | 2026-11-24 | `HeapObject::Map(Vec<(Value, Value)>)` is an insertion-ordered map with a heap allocator and accessors, selected by the `java.util.HashMap` class identity in `Shared::allocate_instance`; `Shared::get_class`, `invokevirtual` dispatch and `Object.toString`'s runtime name report `java.util.HashMap`, and `Shared::roots` enumerates every non-null key and value a map holds. The natives cover the `()V`/`(I)V` constructors, `size`, `isEmpty`, `put` (returning the previous value or null), `get`, `containsKey`, `containsValue`, `remove` and `clear`; null keys and values are stored, a string key compares by content, a boxed `Integer` key by value and every other key by reference identity, and a negative capacity or a non-reference argument is a typed `VmError::Native`. `String.valueOf` covers `Object`, `int`, `long`, `boolean`, `char`, `float` and `double`: null renders `"null"`, a `String` returns itself, a boxed `Integer` its decimal value, any other object the VM's `ClassName@hex` shape that matches M2.9 `Object.toString`, `valueOf(int)` is byte-identical to `Integer.toString(int)`, and a lone surrogate `char` is `UnpairedSurrogate`; the value-conversion helpers moved from `collections` to `string`, so `StringBuilder.append(Object)` keeps the same text. The assembled `MapProbe` exercises the bytecode path, two distinct equal-content string keys and two distinct boxed `Integer` keys, roots, and typed errors; 16 tests; `cargo test -p rava-interp` 108, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 272; `cargo xtask ci` exits 0 |
| M2.14 | `cargo test -p rava-interp --test member_lookup` | 0 | 2026-11-24 | `Class.getDeclaredField`/`getField`, `getDeclaredMethod`/`getMethod` and `getDeclaredConstructor`/`getConstructor` are registry natives; a field matches by name and an executable by name plus the parameter descriptors built from its `Class[]` argument (reference parameter classes only), first in class-file order, and a method's return type is not part of the query; `get*` searches the class, its superclass chain and its interfaces with a visited set, while `getDeclared*` and the constructor queries stay on the receiver; a miss is null (the JDK's `NoSuchFieldException`/`NoSuchMethodException` is deferred because exception objects are not synthesized), and a primitive, array, null or non-`Class` parameter class is a typed `VmError::Native`; every hit is the M2.8 `Shared::reflect_member` object, so it is reference-identical to the array queries and stable across calls; the interpreter's `argument_count` now skips an array parameter's element descriptor, so `Class.getDeclaredMethod(String, Class[])` is callable from bytecode; the fixture has a field-hiding subclass, an interface method and three constructors; 5 tests; `cargo test -p rava-interp` 113, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 277; `cargo xtask ci` exits 0 |
| M2.15 | `cargo test -p rava-interp --test primitive_classes` | 0 | 2026-11-24 | `HeapObject::PrimitiveClass(PrimitiveKind)` is a heap kind distinct from `HeapObject::Class(ClassId)`, with `Shared::primitive_class` as the one-object-per-kind cache, its own lock and no-lock probe entry, and `Shared::roots` leaving every cached primitive class object a root; `Class.forName` of `boolean`, `byte`, `char`, `short`, `int`, `long`, `float`, `double` and `void` returns that cached object, whose `getName` is the Java name, `getModifiers` is `0x0411` (`public abstract final`, `void` included), `isPrimitive` is true, `isArray`/`isInterface` are false and `getSuperclass`/`getComponentType` are null, while `getClass()` reports `java.lang.Class`; a primitive class object declares no members, so the declared-member array queries return empty arrays and the named lookups return the documented null miss; `Field.getType`, `Method.getReturnType`/`getParameterTypes` and `Constructor.getParameterTypes` resolve primitive descriptors to primitive class objects, and a `Class[]` of primitive class objects builds the exact one-character descriptor (`I`, `Z`, ...) a named lookup matches, while a reference class object still builds `L...;` and a `Void.TYPE` parameter builds `V`, which matches nothing; the nine wrapper `TYPE` fields are resolved virtually by `Shared::static_field_value` on the interpreter's `getstatic` fallback because the C1 stubs have no initializer `Code`, and every other wrapper static (`Integer.MAX_VALUE`) stays absent; the `Bean`/`PrimitiveProbe` fixture asserts the flags, names, member types, lookup hits and misses, a bytecode `setValue`/`getValue` round trip through `int.class`, all nine `TYPE` fields, and the documented boundaries; 6 new tests; `cargo test -p rava-interp` 119, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 283; `cargo xtask ci` exits 0 |
| M2.16 | `cargo test -p rava-interp --test array_classes` | 0 | 2026-11-24 | `ClassRef` (a named class, a primitive or a nested array) and `ArrayClass` (a component) describe a class identity beyond `ClassId`; `HeapObject::ArrayClass(ArrayClass)` is a heap kind distinct from `HeapObject::Class(ClassId)` and `HeapObject::PrimitiveClass(PrimitiveKind)`, and `Array` gains the `component` its allocation named, so `newarray`/`anewarray`/`multianewarray` and `String.toCharArray` carry an exact component; `Shared::array_class` is the one-object-per-component cache, with its own lock, a no-lock probe entry and `Shared::roots` entries, and `Shared::get_class` reports an array's array class; `Class.forName` accepts `[I`, `[[I` and `[Ljava.lang.String;` (dotted or internal component names) and returns that cached object, whose `getName` is the JDK descriptor form, `getModifiers` is `0x0411` (`public abstract final`), `isArray` is true, `isPrimitive`/`isInterface` are false, `getComponentType` is the primitive, named or nested-array component class object, and `getSuperclass` is `java.lang.Object` for primitive and reference arrays; `Field.getType`, `Method.getReturnType`/`getParameterTypes` and `Constructor.getParameterTypes` resolve array descriptors through the declaring class's loader, and the named lookups build the exact array descriptor from array `Class[]` entries (`([I)[I`, `([I[Ljava/lang/String;)V`), while a mismatch is the documented null miss; an array class declares no members, so its declared-member arrays are empty and its named lookups are null misses; a malformed array descriptor (`[Q`, `[V`, `[L;`) and a missing named component are typed errors, and a `Class.forName` array resolves only through the bootstrap loader; the assembled `Bean`/`ArrayClassProbe` fixture (an `int[]`, `String[]` and `int[][]` field, `()V`/`([I)V`/`([I[Ljava/lang/String;)V` constructors and `([I)[I`/`([Ljava/lang/String;)[Ljava/lang/String;` methods) asserts names, flags, component identities, `getClass` on `new int[...]`/`new String[...]`/`new int[...][...]`, member types, named lookup hits and misses, and root/cache stability; 6 new tests; `cargo test -p rava-interp` 125, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 289; `cargo xtask ci` exits 0 |
| M2.17 | `cargo test -p rava-interp --test class_literals` | 0 | 2026-11-24 | `ldc`/`ldc_w` decode a `CONSTANT_Class` into `Instr::LdcClass(name)` and execute by resolving through the executing frame's loader: a named class through `load_class`, a primitive name through `Shared::primitive_class` and an array descriptor through `Shared::class_ref_for_descriptor`, so a child loader's own class keeps its identity while a library class still delegates to the bootstrap loader; the class is loaded but not initialized (JVMS §5.4.3); `ldc` of a `String` still interns, `ldc_w`'s u2 index is decoded, and `Integer`/`Float`/`Long`/`Double` and every other constant kind are typed decode errors (there is no `ldc2_w`); repeated execution pushes the same cached `ObjRef` from the class, primitive and array caches, which `Shared::roots` enumerates; `if_acmpeq`/`if_acmpne` were added so the fixture can compare two literals with `==`; the assembled `ClassLiteralProbe` covers `String.class`, `int.class`, `int[].class`, `String[].class`, `int[].class.getName()` and `int.class == int.class`, and one class constant above index 255 exercises `ldc_w`; a `ChildLiteralProbe` defined in a child `MapSource` loader proves the loader-relative resolution; a missing class (`VmError::ClassNotFound`), a malformed `[Q`/`[V`/`[L;` descriptor (`VmError::Native`) and a `Long` constant (`InterpError::Decode`) are typed errors; 5 new tests; `cargo test -p rava-interp` 130, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 294; |
| M2.18 | `cargo test -p rava-interp --test invokedynamic_concat` | 0 | 2026-11-24 | `CONSTANT_InvokeDynamic` at opcode `0xba` decodes its u2 index, its reserved zero bytes, its call-site name and descriptor and its bootstrap method index; `ClassFile::bootstrap_method` materializes a `BootstrapMethods` entry on demand as a `REF_invokeStatic` handle plus `String`/`Class`/`int`/`long`/`float`/`double` static arguments, and an out-of-range index, a non-`MethodHandle` reference, a non-static handle or an unsupported argument is `Error::BadBootstrapMethod`; `rava-vm`'s `string_concat` module implements `StringConcatFactory.makeConcat` (every argument, no recipe) and `makeConcatWithConstants` (`\u0001` consumes the next dynamic argument, `\u0002` the next recipe constant, and both sides must be consumed exactly), converts dynamic arguments through the M2.9/M2.13 `String.valueOf` rules (String, boxed Integer, null, int/long/boolean/char/float/double, Object `ClassName@hex`) and pushes an interned String result; any other bootstrap owner or method is `VmError::UnsupportedBootstrapMethod`; the committed release-17 `demo/ConcatProbe` carries four javac-emitted `makeConcatWithConstants` call sites, one using a `\u0002` recipe constant, and assembled probes cover `makeConcat`, every supported primitive descriptor, static recipe constants, a recipe/descriptor mismatch and the malformed and out-of-range bootstrap references; 13 tests; `cargo test -p rava-interp` 143, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 307; `cargo xtask ci` exits 0; |

M0.6 boundary: the reference and class-unloading hooks are registration points only.
M0 has no collector, so no weak/soft/phantom reference is cleared and no class is
unloaded; M5 (ADR-0008) implements them on this seam. `MemoryRegion` is a VM-owned
byte region with a fake address, not an off-heap allocator; S2-raw replaces the
backing store without changing the address/bounds API.

M0 gate review (2026-11-24): `cargo xtask ci` exits 0; Hello and the integer corpus
run through the interpreter; the committed metadata dump matches `javap`'s structural
view of `Hello.class`; loader identity has six tests; the safepoint/no-lock suite has
nine tests. Reflection-level JDK 17 metadata diff remains the M2/C1 gate.

M0.7 boundary: the verifier tracks operand-stack slots and definitely assigned local
slots, not Java types. It covers straight-line code, conditional and unconditional
branches, and exception-handler entry; `jsr`/`ret`, `wide`, `tableswitch` and
`lookupswitch` return `UnsupportedOpcode`. Type states, stack-map frames and
generated-class verification land at M3 (ADR-0012). The metadata dump is a stable text
fixture, not a reflection API.

M1.1 boundary: `compat target` validates that the target lock parses, that its
`[jars].count` agrees with `compat/jars.lock`, and that `compat/jars/` holds the
declared number of jars. It does not hash the jars (that is
`bash compat/fetch-jars.sh --check`) and does not scan the manifest; M1.2 added the
static manifest and M1.3 the link-check.

M1.2 boundary: the manifest is the static half of ADR-0003 only. It parses every
`.class` entry in the pinned jars and records `CONSTANT_Class`, `CONSTANT_Fieldref`,
`CONSTANT_Methodref` and `CONSTANT_InterfaceMethodref` entries whose owner has a
`java/`, `javax/`, `jdk/` or `sun/` prefix; array class descriptors are skipped and
non-platform owners are filtered out. Types that appear only inside a descriptor are
not recorded unless the same class also appears as a `CONSTANT_Class`. The dynamic
load, reflective access and metadata steps of ADR-0003 are later slices, so the
manifest is a lower bound on the surface, not the full contract. Multi-release
`META-INF/versions/` entries are scanned as they appear in the jar; no version
selection is applied.

M1.3 boundary: `stdlib/classes/` is a signature-only stub library, not the runtime
library. Each manifest owner gets one class file; every field, method and
interface-method record is declared directly on its owner with the exact name and
descriptor, so inherited members are declared rather than resolved through a
superclass. Every class is public and abstract, an owner named by an
`interface-method` record is written as a public interface, and non-constructor
methods are `public abstract` with no `Code` attribute. `<init>` records carry a
minimal concrete body because JVMS §4.6 forbids an abstract constructor; `<clinit>`
is a no-op. The class header is synthetic: every class extends `java/lang/Object`,
and JDK 17 access flags, real superclasses and interface lists from `ct.sym` are not
preserved, so C1 metadata is not claimed. M2.2 adds the JDK 17 metadata stubs in a
separate tree (`stdlib/c1/classes/`), and M2.3 promotes that tree to the runtime
library; this M1 tree is not rewritten. `compat link-check` resolves each record against
the declarations on its owner; it does not walk superclasses or interfaces. It parses
the committed manifest and the stub class files only; it never reads the pinned jars,
never loads or executes a class, and never generates a missing stub.

M1 gate review (2026-11-24): the target is pinned (Java 17, Boot 3.5.16, Spring
6.2.19, Tomcat 10.1.55) and the jar set is checked as a set of 35 jars. The manifest
is reproducible: a fresh scan of the pinned jars regenerates the committed file
byte-for-byte at 980 classes, 177 fields, 3,626 methods and 1,048 interface methods.
Every manifest record resolves against the stub library with `unresolved 0` across
4,851 member records, and a temporary regeneration of `stdlib/classes` matches the
committed 980 class files byte-for-byte. The stubs are signature-only: abstract
classes and interfaces without JDK 17 superclasses, real interface lists or C1
metadata. `rava compat check` runs all four stages and exits 0; `cargo xtask ci`
exits 0. M2 (C1 metadata, the minimal library and the Spring context) is next.

M2.1 boundary: the model decodes the C1 attributes from a parsed `ClassFile`. A generic
`Signature` is retained as the encoded string; the generic `Type` graph and the
reflective API that reads the model are later M2 work, so no reflection native exists
yet.
Every attribute payload is also copied verbatim onto the class, field, method and record
component, so an attribute the model does not decode is never lost; `Code` stays the
interpreter's view and is not metadata. Code-level type annotations (`localvar`, `catch`,
`offset`, `type_argument`) have decoders but no fixture exercises them, because they
live inside `Code` and reflection does not expose them. The dump is a stable text
fixture, not a reflection API: it keeps class-file order for members, interfaces and
annotations, sorts nest members, permitted subclasses and inner classes, and emits no
host path, timestamp or hash.

M2.2 boundary: `stdlib/c1/classes/` is a second committed tree, separate from
`stdlib/classes/`; the M1 tree is unchanged and its gate still passes. 952 classes are
projected from the host JDK's `lib/ct.sym` release-17 entries: the signature is parsed
with `rava-classfile`, the class-file version is set to the pinned 61.0, and the class is
re-emitted with the constant pool at its original indices and every attribute payload
verbatim. The 28 manifest owners Java 17 does not define (the `Thread$Builder` classes,
JCache, Moneta, WSDL and the Java EE `javax.annotation.Resource`) are listed in
`compat/surface/c1-unreferenced.txt` and get header-only placeholders; the generator
fails, naming the owners, when the archive and that list disagree. `rava compat metadata
--check` re-reads the release-17 signature and compares the M2.1 normalized dump with
the committed stub: 952 classes compared, 0 differences. The only normalization is the
class-file version line, because ct.sym stores the host JDK's version (69) while the tree
is pinned to 61.0; ordering, annotations, signatures and raw attributes are not
normalized. There is no `Code`, no verifier work and no reflection native: this slice
establishes the metadata shape and the diff, not behaviour. Generic signatures are
retained as strings, not turned into a `Type` graph. The writer round-trips every
committed M2.1 fixture byte-for-byte; a lone surrogate in a string constant would not
round-trip, because the model decodes it lossily. M2.3 promoted this tree to the runtime
library.

M2.3 boundary: `stdlib/c1/classes` is the bootstrap class source. It is read through the
existing `DirectorySource` and `(loader, name)` identity, so there is no second class
table and no second identity model. `Shared::with_library_dir` is a constructor over that
source; `Shared::load_library_class`, `Shared::library_metadata` and
`ClassEntry::metadata()` decode metadata on demand and do not cache it. Loading parses a
class file and metadata decoding reads attributes, so neither requires a `Code`
attribute: the signature-only stubs load without an execution body. This slice does not
initialize or execute a class, does not build `Class`, `Field`, `Method` or `Constructor`
objects, and adds no reflection native or Spring path. The committed tree has no record
class, so record metadata is exercised by the M2.1 fixtures rather than here.

M2.4 boundary: a class object is `HeapObject::Class(ClassId)` and carries no metadata of
its own; `Shared::class_object` caches exactly one object per `(loader, name)` identity.
Those objects are permanent: they are never freed while `Shared` lives, and
`Shared::roots` lists them, so a future collector sees the retention instead of inferring
it. `Shared::get_class` maps an instance to its defining class, a string to the bootstrap
`java.lang.String` class object and a class object to `java.lang.Class`; an array has no
class identity in this slice, so it returns a typed `VmError::Native` rather than a wrong
object, and `Class.isArray` is therefore always false. The native ABI now carries the
receiver as an `ObjRef` (`ObjRef::NULL` for static calls), and the registry is consulted
before `Code`, which is what makes the C1 stubs run even though they are not
`ACC_NATIVE`. `invokevirtual` walks the superclass chain from the receiver's class
(`java/lang/Class` and `java/lang/String` for class objects and strings) and supports
only native targets; a non-native instance method still fails loudly. `Class.getName`
returns the binary name with `.`, `getModifiers` reports `Modifier.classModifiers()` with
`ACC_SUPER` masked out, `getSuperclass` is null for `Object` and for interfaces, and
`forName` is bootstrap-only, does not initialize the class and rejects array class names.
`Field`, `Method`, `Constructor`, annotation proxies, generic types, records and Spring
remain later slices.

M2.5 boundary: `new` allocates the instance immediately, with the layout's default field
values; the JVM's "uninitialized" state between `new` and `<init>` is not modeled, so an
instruction between them would observe a usable object, and `new`/`dup`/`invokespecial` is
the supported construction sequence. `new` does not reject an abstract class or an
interface, and it performs no access-control check on the class or its constructor. The
instance layout holds non-static fields only, superclass slots first and then the class's
own fields, so a field resolved in a superclass keeps its index in every subclass; it is rebuilt per access and not cached. `getfield` and
`putfield` resolve name and descriptor on the instruction's owner (walking its superclass
chain), then read the slot from the receiver's runtime layout; a static field through an
instance opcode is an error, a name that exists under another descriptor is an error, and
the receiver must be an instance assignable to the owner. Static access keeps its existing
`getstatic` path; `putstatic`, interface field inheritance and `invokeinterface` are still
absent. `invokespecial` resolves on the instruction's owner, which is what keeps `<init>` and
superclass calls non-virtual; `invokevirtual` still resolves from the receiver's runtime
class and walks superclasses only, so interface dispatch and default methods are absent.
`run_instance_method` initializes the class on entry as a convenience for direct callers;
JVMS only requires initialization at `new`, and the interpreter's `new` does it there.
Only the instructions in this slice are decoded by the interpreter: `if*`/`goto` remain
verifier-only, and `tableswitch`/`lookupswitch`, `wide`, `invokeinterface`, `checkcast`,
`instanceof` and exceptions are still unsupported. Arrays exist in the heap but this slice
adds no array instruction, array class identity or array reflection. `Object.<init>()V` is
the only new native; `Field`, `Method` and `Constructor` objects, annotations, generics,
records and Spring are later slices.

M2.6 boundary: arrays are the existing `Heap` `Array`/`ArrayKind` model; this slice adds
no second representation and no array class identity. `newarray` maps all eight primitive
`atype` values, `anewarray` allocates a `reference` array and resolves an object component
through the current class's loader, and `multianewarray` allocates one `Array` per
allocated dimension (a partial allocation such as `new int[2][]` leaves the inner
references null). An array still has no `ClassId`, so `Shared::get_class` keeps returning
the typed M2.4 error and `Class.isArray` stays false for an array; `dispatch_class` still
sends an array's virtual calls to `java/lang/Object`. `aaload`/`aastore` do not check
`ArrayStoreException`: the array stores no component type, so storing a reference of an
unrelated class is accepted as long as the value is a reference. `baload`/`bastore` accept
both `boolean[]` and `byte[]`; every other primitive array opcode requires the matching
`ArrayKind`. A `byte`/`char`/`short` store keeps the 32-bit `Value` unchanged instead of
narrowing it. Every array instruction checks null (through the heap's `NullReference`) and
bounds (`ArrayIndexOutOfBounds`, negative indexes included), and a negative length is
`NegativeArraySize`. `iinc` is decoded as well, so a compiled loop counter works.
Branches are resolved once, at decode time, to instruction indices; decode rejects a
target that is not an instruction boundary but does not reject unreachable code.
`tableswitch`/`lookupswitch`, `wide`, `jsr`/`ret`, `checkcast`/`instanceof`,
`athrow`/exception handlers, `invokeinterface`, interfaces, reflection members and Spring
remain later slices.

M2.7 boundary: the exception table is decoded from the `Code` attribute and its three
byte addresses become instruction indices at decode time, so handler search compares
instruction positions, never byte addresses. `athrow` and a `Thrown` result from a
callee search the current frame's table in class-file order for an entry whose range
contains the throwing instruction and whose catch type is assignable from the
throwable's runtime class through the superclass chain; a match clears the operand
stack, pushes the throwable and jumps to the handler, and no match lets the same
`ObjRef` leave the frame as `InterpError::Thrown`. The throwable is never copied or
rewrapped. `catch_type = 0` is decoded as a catch-all, which is the table shape
`finally` compiles to, but `finally` duplication and its rethrow control flow are not
implemented, and `jsr`/`ret` remain absent. There are no stack traces (`LineNumberTable`
is not consulted), no exception-object synthesis, and no try-with-resources. `athrow`
of null is `InterpError::NullThrow` rather than a synthesized `NullPointerException`,
and a non-reference operand is `NonReferenceThrow`; neither searches the table. Only
`Thrown` is caught at `invokestatic`, `invokevirtual` and `invokespecial`: an error
raised by a native, by class initialization, or by another interpreter check is not
converted into a Java throwable. No check requires the thrown object or the catch type
to be a `Throwable`; the class library's `Throwable` hierarchy is not consulted, so any
reference can be thrown and any class name can be a catch type. Assignability walks
superclasses only, so an interface catch type, `invokeinterface` dispatch and interface
identity remain later slices.

M2.8 boundary: a reflected member is `HeapObject::Reflect(ReflectMember)`, carrying a
`MemberKind` (`Field`, `Method`, `Constructor`), the declaring `ClassId`, the member name
and its descriptor; it stores no modifiers or type objects. `Shared::reflect_member`
caches exactly one object per `(class, kind, name, descriptor)` and `Shared::roots`
enumerates those objects, so they are permanent roots rather than inferred retention.
`Shared::get_class` reports `java/lang/reflect/Field`, `Method` or `Constructor` for a
member, and `invokevirtual` dispatches its methods to that class. `Class` gains
`getDeclaredFields`, `getDeclaredMethods` and `getDeclaredConstructors`, each returning a
real heap reference array in class-file order from the typed C1 metadata; fields are all
declared fields, methods exclude `<init>`/`<clinit>`, and constructors are the `<init>`
methods. The read-only member methods are `getName`, `getModifiers`
(`Modifier.fieldModifiers`/`methodModifiers`/`constructorModifiers` masks),
`getDeclaringClass`, `Field.getType`, `Method.getReturnType`,
`Method`/`Constructor.getParameterTypes` and `getParameterCount`, and
`Constructor.getName` (the binary class name); `getClass` on a member reports the
`java.lang.reflect` class. A type descriptor is resolved only when it names an object
class: primitive, void and array descriptors return a typed `VmError::Native` because
this slice has no primitive or array class identity. Inherited lookup (`getFields`,
`getMethods`, `getConstructors`, `getField`, `getMethod`, `getDeclaredField`,
`getDeclaredMethod`, `getConstructor`, `getDeclaredConstructor`) is not implemented and
fails as the typed `NativeOrAbstract` error; `getDeclared*` is the whole query surface.
There is no `Field.get/set`, `Method.invoke`, `Constructor.newInstance`, annotation
proxy, generic `Type` graph, access check, `setAccessible`, or
`toString`/`equals`/`hashCode`; annotations and `Signature` remain metadata-only.
`java.lang.reflect.Member` and `Executable` are an interface and an abstract stub, so
they get no natives of their own: the concrete `Field`, `Method` and `Constructor`
declarations carry the registry entries.

M2.9 boundary: `java.lang.String` is still `HeapObject::Str`, a Rust `String` holding
valid UTF-8; this slice adds no second representation and no reflection-visible `value`
field. The natives derive UTF-16 code units with `String::encode_utf16`, so `length`,
`charAt`, `substring`, `indexOf`, `compareTo`, `hashCode` and `toCharArray` count and
index units, and a supplementary character is a surrogate pair. A lone surrogate cannot
be stored in a Rust `String`, so an operation that would leave one — splitting a pair in
`substring`, or a `replace` that rewrites half a pair — returns the typed
`VmError::UnpairedSurrogate` instead of a wrong value, where the JDK would return a
string holding the lone unit. A null argument and an out-of-range index are the typed
`VmError::NullStringArgument` and `VmError::StringIndexOutOfBounds`, not synthesized
`NullPointerException`/`StringIndexOutOfBoundsException`, because this slice has no
exception-object synthesis; the interpreter reports them wrapped as `InterpError::Vm`.
`substring` over the whole string, `toString`, and a `replace` that changes nothing
return the receiver, as on the JDK. `intern` is backed by `Shared::intern_string`, a
permanent content-keyed table that `Shared::roots` enumerates; there is no collector, so
interned strings are never freed. `ldc` also goes through that table, because JVMS §5.1
interns a string constant: two executions of one constant see the same object instead of
a fresh allocation. `Object.equals` is reference identity, `Object.hashCode`
is a deterministic mix of the `ObjRef` index and generation (stable without a side table,
not randomized, not a content hash) and `Object.toString` prints
`java.lang.Object@1b6d3586`-style text from the runtime class's dotted binary name. A
class object still reaches `Object.toString`, so it prints `java.lang.Class@hash` rather
than the JDK `Class.toString`'s `class X`; an array has no class identity, so it returns
a typed error rather than a wrong name. `String.valueOf`, the `String` constructors,
`getChars`, the byte/charset/code-point/locale/case/trim/split/format/repeat families,
`CharSequence`, `StringBuilder`, `Integer`, collections, reflection invocation,
annotations, generics and Spring remain later slices. The fixture needs a
supplementary-character constant, so `ClassBuilder::utf8` now writes modified UTF-8
(CESU-8 for supplementary characters) instead of standard UTF-8; a four-byte UTF-8
sequence was never a valid class-file constant.

M2.10 boundary: a boxed primitive is `HeapObject::Boxed(BoxedPrimitive::Integer)`, a
heap object rather than a second `Value` variant, so it can live in an `Object` slot.
Only `java.lang.Integer` exists; `Boolean` and `Long` boxes, the small-value cache and
the `String`-taking `valueOf`/`parseInt` overloads are later slices, and every `valueOf`
allocates a new object, so `==` on two equal boxes is false where the JDK caches small
values. `Shared::get_class`, `invokevirtual` dispatch and `Object.toString`'s runtime
name treat a box as its boxed class; the box is an ordinary allocation, not a permanent
root. The `Integer` natives are `valueOf(I)`, `intValue`, `toString()`, `toString(I)`,
`equals`, `hashCode`, `compareTo(Integer)` and `parseInt(String)`; a malformed
`parseInt` is a typed `VmError::Native`, not a synthesized `NumberFormatException`.
`Field.get`/`set` cover instance `int` and reference fields only: a static field through
either method is rejected, the receiver must be a non-null instance assignable to the
declaring class, `int` is boxed or unboxed and a reference passes its handle through
unchanged (null included). There are no `getInt`/`setInt`, volatile or final semantics,
access checks or `setAccessible`. `Method.invoke` uses the reflected name and descriptor
exactly, so there is no overload resolution; it runs a static method with the receiver
ignored or an instance method on its declaring class (not a virtual override), unboxes
`int` arguments, boxes an `int` return, passes references through and returns null for
`void`. `Constructor.newInstance` allocates the declaring class, unboxes its arguments
and runs `<init>` through `run_instance_method`; it performs no `InstantiationException`
or access check and does not support varargs.
`java.lang.reflect.InvocationTargetException` is deferred: a Java throwable raised by
the invoked method crosses the native ABI as `VmError::Thrown` and re-enters the
interpreter as `InterpError::Thrown`, so a caller's exception table can catch the
original object instead of the throwable being swallowed. `Class.getDeclaredMethod` and
`getMethod` and inherited-member lookup remain absent, and annotations, generics,
collections and Spring stay later slices.

M2.11 boundary: an annotation instance is `HeapObject::Annotation`, carrying the annotation
type's `ClassId` and the element values the class file encodes, in class-file order. Only
runtime-visible annotations are exposed; a `RuntimeInvisibleAnnotations` entry has no
annotation object, as on the JDK. Annotation objects are ordinary allocations, not cached
roots, so two queries return two objects; `Annotation.equals` compares the effective element
values (declared or default) and is consistent with `Annotation.hashCode`, which is a
deterministic FNV hash of that same encoding rather than the JDK's
`(127 * name.hashCode()) ^ valueHashCode` formula, and `toString` prints `@dotted.Type` with
only the explicitly encoded elements. Element values cover string, `boolean`/`byte`/`char`/
`short`/`int` (as `Value::Int`), `long`/`float`/`double` (as their `Value` variants), object
class literals, enum constants, nested annotations and arrays of those; nested arrays are
reference arrays because `ArrayKind` has no array-of-array kind, and an array or primitive
class literal (`[I`, `int.class`) is a typed error because arrays and primitives have no
class identity. Enum constants are cached objects that name the enum class and the constant;
the VM does not run the enum's `<clinit>`, so the object is not the enum's static field value,
and `ordinal`/`valueOf` are absent while `Enum.name`/`toString` report the name. An annotation
receiver dispatches to its annotation type, and `annotationType`, the element methods,
`equals`, `hashCode` and `toString` are served before ordinary method resolution, so a C1
stub's missing `Code` never reaches the interpreter; `Object.getClass` on an annotation
reports `java.lang.annotation.Annotation`. `Class.getAnnotations` inherits a superclass
annotation only when the annotation type carries `@Inherited`, and the nearest declaration of
a type wins; `getAnnotation` follows that same view, while member queries see only the
member's own annotations. Repeatable containers and `getAnnotationsByType`, parameter and
type annotations, `isAnnotationPresent`, `Class.isAnnotation`, `setAccessible` and access
checks, annotation identity caching, the generic `Type` graph, record/sealed reflective views
and Spring stay out of this slice. The fixture is compiled at release 17 and committed under
`crates/interp/tests/fixtures/annotations/` (sources in `src/`, classes in `classes/`) and is
loaded through a `DirectorySource` child loader, so the test needs no JDK.

M2.12 boundary: a `StringBuilder` is `HeapObject::StringBuilder(String)` and an
`ArrayList` is `HeapObject::List(Vec<Value>)`, both ordinary heap objects selected by
their class identity in `Shared::allocate_instance`; they are not
`HeapObject::Instance`s, so an instance field opcode and a reflected `Field.get`/`set` or
`Method.invoke` receiver do not apply to them. `Shared::get_class`, `invokevirtual`
dispatch and `Object.toString`'s runtime name report `java.lang.StringBuilder` and
`java.util.ArrayList`. `Shared::roots` additionally enumerates every non-null reference
held by a heap list, so a list element is visible to a future collector; there is still
no collector, so an element of an otherwise unreachable list is over-retained rather than
traced. The builder stores valid UTF-8 text and implements `length`, `charAt`,
`setLength`, `deleteCharAt`, `insert`, `reverse` and `append(String)` in UTF-16 units;
`setLength` can grow with NUL units and `reverse` keeps a surrogate pair intact the way
the JDK does, while `setLength`, `deleteCharAt` or `append(char)` that would leave a lone
surrogate are typed `UnpairedSurrogate` errors because the stored text cannot represent
one. A null String, Object or CharSequence append produces `"null"`. `append(Object)`
uses a `String.valueOf`-shaped conversion (String and StringBuilder content, a boxed
`Integer`'s decimal value, `"null"` for null, otherwise the VM's `ClassName@hex`
Object.toString shape) because a VM native cannot re-enter the interpreter to dispatch an
overridden `toString`; `append(CharSequence)` accepts String and StringBuilder only. The
float and double text uses Rust's shortest round-trip formatting with the mandatory `.0`
and prints an exponential value in plain decimal, not the JDK's exact
`Float.toString`/`Double.toString` spelling for every value. `ArrayList` implements
`size`, `isEmpty`, `add`, `add(int,E)`, `get`, `set`, `clear`, `remove(int)`,
`remove(Object)`, `contains` and `indexOf`; `contains`, `indexOf` and `remove(Object)`
compare references, not `Object.equals`, so a distinct but equal element is not found,
and a null argument matches a null element. A negative capacity is a typed
`VmError::Native`; a negative index and an index past the end are
`StringIndexOutOfBounds` (builder) or `CollectionIndexOutOfBounds` (list), not
synthesized exceptions. There are no iterators,
streams, `HashMap`, `StringBuffer`, `CharSequence.length/charAt` delegation, list
`toString`, `removeAll`/`retainAll`/`toArray`, reflection changes or Spring in this
slice. `java.lang.StringBuilder`'s package-private superclass
`java.lang.AbstractStringBuilder` is absent from the ct.sym-derived C1 tree, so
`Shared::resolution_superclass` continues that one chain at `java.lang.Object`; every
other class resolves the superclass its class file names. The `CollectionsProbe` fixture
is assembled in `crates/interp/tests/collections.rs` rather than committed, and the
`long`/`float`/`double` append overloads are called through the registry because the
decoded instruction subset has no `lload`/`fload`/`dload` or `ldc2_w`.

M2.13 boundary: a map is `HeapObject::Map(Vec<(Value, Value)>)`, an insertion-ordered
vector of pairs rather than a hash table: lookup is a linear scan, there is no bucketing,
tree bin, resizing or load factor, and `HashMap(int)` treats its capacity as a hint. Keys
and values are `Value`s, so a null key and a null value are stored exactly as the JDK
stores them; a null value is an entry, not an absent key, and `get` returns null for both.
Membership compares a string key by content, a boxed `Integer` key by value and every
other reference by identity; `containsValue` uses the same rule, which matches the JDK for
the string and `Integer` values this slice models but differs from a Java `equals`
override on any other class, because a VM native cannot re-enter the interpreter. The map
natives are the two constructors and `size`, `isEmpty`, `put`, `get`, `containsKey`,
`containsValue`, `remove` and `clear`; `put` and `remove` return the previous or removed
value and `put` of an equal key replaces the value in place. A negative capacity, a null
receiver and a non-reference key or value are typed `VmError::Native`s, not synthesized
`IllegalArgumentException`/`NullPointerException`s. `HashMap`'s `(IF)V` and `(Map)V`
constructors, `getOrDefault`, `putIfAbsent`, `putAll`, `keySet`, `values`, `entrySet`,
`forEach`, `replace`/`merge`/`compute`, `clone`, `equals`, `hashCode` and `toString` have no
registry entry and reach the C1 stub's missing `Code` as the typed
`InterpError::NativeOrAbstract`, so they fail loudly instead of guessing. There is no
`Map`-interface dispatch: a map receiver resolves to `java/util/HashMap` and there are no
iterators, sets, entry objects, streams, `Objects.equals`/`Objects.hash`, concurrency or
Spring in this slice. A map is an ordinary heap object, not a permanent root; `Shared::roots`
enumerates its non-null keys and values, and because M0 has no collector an entry of an
otherwise unreachable map is over-retained rather than traced. `String.valueOf(Object)`
cannot dispatch an overridden Java `toString` either, so a `String` returns itself, a
`StringBuilder` contributes its content, a boxed `Integer` its decimal value, null the
interned `"null"`, and every other object the M2.9 `Object.toString` shape
(`ClassName@hex`); an array has no class identity, so it is a typed error. `String.valueOf`
covers only the `Object`, `int`, `long`, `boolean`, `char`, `float` and `double` overloads;
the `char[]` forms remain absent, `valueOf(int)` uses the identical decimal text as
`Integer.toString(int)`, and the float/double text keeps the M2.12 shortest-round-trip
limitation (an exponential value is printed in plain decimal). A lone surrogate `char` is
a typed `UnpairedSurrogate`.

M2.14 boundary: the six named lookup natives are `Class.getDeclaredField`/`getField`,
`getDeclaredMethod`/`getMethod` and `getDeclaredConstructor`/`getConstructor`. A field
query is a name only, which is the JDK signature: the first declared field with that
name wins in class-file order (Java allows one field per name per class; a hidden
superclass field is reached by the ordered search, not by a duplicate within one class).
A method or constructor query matches the name (constructors are `<init>`, and
`<clinit>` is never a method) plus the parameter descriptors built from the `Class[]`
argument; a method's return type is not part of the query, as on the JDK, so two
same-name, same-parameter declarations in one class would resolve to the first in
class-file order (Java source cannot express that; a bridge method can). Inherited
lookup walks the receiver, its superclass chain, then the interfaces of each class on
that chain, visiting each `ClassId` at most once; an interface's class file names
`Object` as its superclass, but the JDK reports no superclass for an interface, so that
edge is not followed and only the interface's own `interfaces` list continues the walk.
`getDeclared*` stays on the receiver, and constructors are not inherited on the JDK
either, so `getConstructor` is the declared view too. A lookup miss is `null`: the JDK
throws `NoSuchFieldException`/`NoSuchMethodException`, but this slice has no exception
object synthesis, so `null` is the documented result instead of a synthesized throwable.
A parameter class must be a reference class object, so its binary name becomes
`Linternal/Name;`; primitive and array class objects do not exist in this slice (there is
no `int.class` or array identity), and a parameter class whose name is a primitive
keyword or an array descriptor is a typed `VmError::Native`, as is a null `Class[]`, a
null element and a non-`Class` element. There is no access-control filter yet: the JDK's
`getField`/`getMethod`/`getConstructor` are public-only and the `getDeclared*` forms
report private members, but this slice returns either without checking modifiers. Every
hit is the M2.8 `Shared::reflect_member` object for its `(class, kind, name, descriptor)`
identity, so it is reference-identical to the `getDeclared*` array queries and stable
across calls; no second cache exists. The interpreter's descriptor parser skips an array
parameter's element descriptor (`[Ljava/lang/Class;` is one parameter, not two), which is
what lets `getDeclaredMethod`/`getMethod` be called from bytecode. Array and primitive
`Class` identity, the generic `Type` graph, annotation changes and Spring remain later
slices.

M2.15 boundary: a primitive class object is `HeapObject::PrimitiveClass(PrimitiveKind)`,
a distinct heap kind from `HeapObject::Class(ClassId)`; a loaded class keeps its class id
and nothing about its identity changes. There is exactly one cached object per kind in
`Shared::primitive_class`, so `Class.forName("int")` and `Integer.TYPE` are
reference-identical; the cache is enumerated by `Shared::roots`, has its own lock and is
part of the no-lock probe. The nine kinds are `boolean`, `byte`, `char`, `short`, `int`,
`long`, `float`, `double` and `void`; `getName` is the Java name, `getModifiers` is
`0x0411` (`public abstract final`, `void` included), `isPrimitive` is true, `isArray` and
`isInterface` are false, and `getSuperclass`/`getComponentType` are null. `getClass()` on a
primitive class object reports `java.lang.Class`, as it does for any class object, and
`Shared::get_class` agrees. A primitive class object has no class file, so the
`getDeclaredFields`/`getDeclaredMethods`/`getDeclaredConstructors` queries return an empty
array and the named lookups (`getDeclaredField`, `getField`, `getDeclaredMethod`,
`getMethod`, `getDeclaredConstructor`, `getConstructor`) return the documented null miss
rather than a second lookup path. `Field.getType`, `Method.getReturnType`,
`Method.getParameterTypes` and `Constructor.getParameterTypes` resolve a primitive
descriptor to the primitive class object; an array descriptor still returns a typed error
because array class identity is not modeled. A named lookup accepts primitive class
objects in its `Class[]` and builds the one-character descriptor (`I`, `Z`, ...), while a
reference class object still builds `L...;`; `Void.TYPE` builds `V`, which matches no
method or constructor, as on the JDK, and a loaded class whose binary name is a primitive
keyword is still a typed error. The nine wrapper `TYPE` fields are resolved virtually by
`Shared::static_field_value` because the C1 stubs carry no initializer `Code`: the
interpreter's `getstatic` reads the statics table first and falls back to the primitive
class object, so `Integer.TYPE` read from bytecode is the same object `Class.forName("int")`
returns. Nothing is written into the statics table, and every other static field of a
wrapper (`Integer.MAX_VALUE`, for example) is still absent and fails exactly as before.
Generic `Type`, the generic member-type queries, array class identity, annotation changes
and Spring stay out of this slice; an annotation query on a primitive class object is
still a typed error, and `Class.toString` is not overridden, so `Object.toString` on a
primitive class object keeps the M2.9 `java.lang.Class@hash` shape instead of the JDK's
`class int`. The `crates/interp/tests/primitive_classes.rs` fixture is assembled in Rust
because it needs no JDK.

M2.16 boundary: a class identity beyond `ClassId` is a `ClassRef` — a named class, a
primitive or an array class — and an array class is an `ArrayClass` identified by its
component alone. `HeapObject::ArrayClass(ArrayClass)` is a distinct heap kind, so
`HeapObject::Class(ClassId)` and `HeapObject::PrimitiveClass(PrimitiveKind)` keep their
identity unchanged. There is exactly one array class object per component in
`Shared::array_classes`, enumerated by `Shared::roots`, with its own lock and no-lock
probe entry; a nested array's component is itself an array class ref, so the chain is
stable without a second object. An array heap object now carries the component its
allocation named, so `getClass()` on `new int[n]`, `new String[n]` and
`new int[r][c]` reports the exact cached array class. `Class.getName` for an array is the
JDK descriptor form with dots (`[I`, `[[I`, `[Ljava.lang.String;`), `getModifiers` is
`0x0411` (`public abstract final`), `isArray` is true, `isPrimitive` and `isInterface` are
false, `getComponentType` returns the primitive, named or nested-array component class
object, and `getSuperclass` is `java.lang.Object` for primitive and reference arrays.
`Class.forName` accepts an array name and resolves a named component through the
bootstrap loader only, so an array whose component is not in the bootstrap class library
is a loader error even when a child loader defines it; it does not initialize the
component or the array class. An array class declares no fields, methods or constructors,
so the declared-member queries return empty arrays and the named lookups return the
documented null miss. `Field.getType`, `Method.getReturnType`/`getParameterTypes` and
`Constructor.getParameterTypes` resolve an array descriptor through the declaring
class's loader, and a named lookup accepts array class objects in its `Class[]` and
builds the exact array descriptor. A malformed descriptor (`[Q`, `[V`, `[L;`) and a
missing named component are typed errors. This slice does not decode a class constant
with `ldc`, so Java's `int[].class` spelling is exercised through `Class.forName("[I")`
instead; array covariance and `ArrayStoreException` are still absent (`aastore` keeps
M2.6's no-check behavior), `Class.toString` keeps the M2.9 `java.lang.Class@hash` shape
rather than the JDK's `class [I`, and generic `Type`, annotation changes and Spring stay
out of this slice. The `crates/interp/tests/array_classes.rs` fixture is assembled in
Rust because it needs no JDK.

M2.17 boundary: `ldc` and `ldc_w` decode a `CONSTANT_Class` into `Instr::LdcClass`, and
the instruction stores the constant's name rather than a class id: JVMS §5.4.3 resolves
it through the defining loader of the class that executes the instruction, so decode
cannot commit to one loader. At execution a named class goes through
`Shared::load_class` on the current class's loader (a child loader's own class therefore
keeps its identity, while a library class still delegates to the bootstrap loader), a
Java primitive name (`int`, `void`, ...) goes through the M2.15
`Shared::primitive_class` cache, and an array descriptor (`[I`, `[[I`,
`[Ljava/lang/String;`) goes through `Shared::class_ref_for_descriptor` with that loader.
The class is loaded but not initialized, as the JVMS requires. The pushed value is
always the cached object - the class, primitive or array class cache - so repeated
execution of one constant and `Class.forName` of the same name are reference-identical,
and those objects stay `Shared::roots` entries. `ldc` of a `String` keeps the M2.9
interned behavior. `ldc_w`'s index is the full u2; the assembler's `ldc` emits it when an
index exceeds 255. Numeric constants (`Integer`, `Float`, `Long`, `Double`) and every
other constant kind are typed decode errors, because this slice has no `ldc2_w` and no
`MethodType`/`MethodHandle`/invokedynamic support; there is no lambda path. A primitive
class constant is spelled with the Java name here, so a one-character primitive
descriptor is not a second spelling. A missing class surfaces as
`VmError::ClassNotFound`, a malformed array descriptor as `VmError::Native`, and an
unsupported constant kind as `InterpError::Decode`. `if_acmpeq`/`if_acmpne` were added
only so the fixture can observe `int.class == int.class` as reference identity; they
compare `ObjRef`s (null included) and do not introduce reference type checking or
`checkcast`/`instanceof`. JDK-compiled `int.class` still emits
`getstatic Integer.TYPE`, which is the M2.15 path, so the class-literal instruction
itself is exercised by an assembled probe; `Class.toString` keeps the M2.9 shape,
generics, annotation changes and Spring remain later slices.

M2.18 boundary: `invokedynamic` is implemented for `StringConcatFactory` call sites
only. Opcode `0xba` decodes its u2 constant-pool index and rejects non-zero reserved
bytes; the call-site name/descriptor and the `BootstrapMethods` index come from the
`CONSTANT_InvokeDynamic`, and the entry is materialized on demand rather than at class
parse, so an unsupported bootstrap elsewhere in a class does not stop the class from
loading. The typed entry accepts one `REF_invokeStatic` handle to a method and
`String`/`Class`/`int`/`long`/`float`/`double` static arguments; a `MethodHandle`,
`MethodType`, `Dynamic` or other argument, a non-static handle or a handle that does not
name a method is a typed `Error::BadBootstrapMethod`. `makeConcat` has no recipe and
concatenates every argument; `makeConcatWithConstants` reads the first static argument
as its recipe, expands `\u0001` over the dynamic arguments and `\u0002` over the
remaining static arguments, and requires both sequences to be consumed exactly. Dynamic
arguments convert by their parameter descriptor with the M2.9/M2.13 `String.valueOf`
rules, including null, boxed `Integer`, `String`, objects in the `ClassName@hex` shape
and every primitive; a `char` that is not a scalar is a typed error. A recipe `Class`
constant contributes its dotted binary name, because this VM has no JDK
`Class.toString`. Every result is interned, so repeated calls are text-equal and
reference-identical, but there is no call-site caching: the conversion runs again each
time. `LambdaMetafactory`, method handles, `MethodType` and `Dynamic` constants,
arbitrary bootstrap methods, `invokeinterface`, constant-dynamic (`condy`) and Spring
remain later slices. The `lload`/`fload`/`dload` forms were added only so the assembled
probe can hand a long/float/double argument to a call site; the interpreter still keeps
one local per argument rather than widening long/double to two slots.

## Decision log

| ADR | Decision | Status |
| :--- | :--- | :--- |
| [0001](docs/adr/0001-compatibility-first.md) | Compatibility-first; success is a pinned external application | accepted |
| [0002](docs/adr/0002-target-platform-and-versions.md) | Java 17, Spring 6.2.19, Boot 3.5.16, Tomcat 10.1.55 | accepted |
| [0003](docs/adr/0003-compatibility-contract.md) | Observed surface manifest plus C0–C4 gates | accepted |
| [0004](docs/adr/0004-runtime-class-library.md) | Full replacement runtime library, owned by us | accepted |
| [0005](docs/adr/0005-class-loading-and-dynamic-code.md) | Loaders, `defineClass`, `Proxy`, `Lookup`, `Unsafe` capability/fidelity tiers U0–U4/S0–S2 | accepted |
| [0006](docs/adr/0006-reflection-and-metadata.md) | Reflection reproduces JDK 17 metadata | accepted |
| [0007](docs/adr/0007-concurrency-and-memory-model.md) | Real OS threads and the Java memory model | accepted |
| [0008](docs/adr/0008-references-and-cleaner.md) | References, `Cleaner`, class unloading | accepted |
| [0009](docs/adr/0009-resources-jars-and-modules.md) | Jars, nested jars, resources, `ServiceLoader`, `-jar` | accepted |
| [0010](docs/adr/0010-execution-and-performance.md) | Measure first; choose interpreter/JIT against a budget | accepted |
| [0011](docs/adr/0011-scope-ordering-and-native-boundary.md) | Scope ordering, promotion rule and native/internal boundary | accepted |
| [0012](docs/adr/0012-runtime-foundation-and-upgrade-seams.md) | M0 foundation seams and additive/conditional/architecture-fork classification | accepted |

## Resume

A fresh session should:

1. Read `README.md`, `docs/requirements.md` and `docs/open-questions.md`.
2. Read the ADRs in order; 0001–0005 are the project-defining decisions.
3. M0 and M1 are complete. M2.1 landed the typed C1 metadata model and its normalized
   dump (`cargo test -p rava-classfile --test metadata_model`). M2.2 landed the JDK 17
   signature stubs in `stdlib/c1/classes/` and the zero-diff `rava compat metadata
   --check` gate. M2.3 landed the runtime loading path: that tree is the bootstrap class
   source and its metadata is readable without `Code` (`cargo test -p rava-vm --test
   c1_runtime`). M2.4 landed class objects and the basic `Object`/`Class` natives
   (`cargo test -p rava-interp --test class_objects`). M2.5 landed instance allocation,
   constructors, instance fields and instance method calls, including the
   `Object.<init>()V` no-op (`cargo test -p rava-interp --test instance_execution`).
   M2.6 landed arrays and integer control flow: primitive and reference array creation,
   the array load/store forms and the integer branches resolved to instruction indices
   (`cargo test -p rava-interp --test arrays_control`). M2.7 landed exceptions: the
   `Code` exception table decoded to instruction ranges, `athrow`, and handler search
   across `invokestatic`/`invokevirtual`/`invokespecial` (`cargo test -p rava-interp
   --test exceptions`). M2.8 landed reflected member objects: the `HeapObject::Reflect`
   representation, the declared-member queries and the read-only
   `Field`/`Method`/`Constructor` natives (`cargo test -p rava-interp --test
   reflection_members`). M2.9 landed core `Object`/`String` behavior: reference-identity
   `Object.equals`/`hashCode`/`toString`, the UTF-16 `String` methods and `intern` on the
   existing `HeapObject::Str` (`cargo test -p rava-interp --test string_object`). M2.10
   landed boxed `Integer` values and reflective invocation: `Field.get`/`set` for `int`
   and reference fields, `Method.invoke` for static and instance methods, and
   `Constructor.newInstance`, all over the existing instance/static machinery
   (`cargo test -p rava-interp --test reflection_invoke`). M2.11 landed annotation
   reflection: the `HeapObject::Annotation` and `HeapObject::Enum` representations, the
   `getDeclaredAnnotations`/`getAnnotations`/`getAnnotation` queries on `Class`, `Field`,
   `Method` and `Constructor`, `AnnotationDefault` fallback, inherited class annotations,
   annotation-member dispatch through `invokevirtual`, and the committed release-17
   fixture loaded from a child `DirectorySource` (`cargo test -p rava-interp --test
   annotations`). M2.12 landed `StringBuilder` and core collection behavior: the
   `HeapObject::StringBuilder` and `HeapObject::List` representations, their
   constructors, append overloads and list operations as native registry entries, the
   builder's UTF-16 semantics and the list's reference-equality membership, with
   `Shared::roots` enumerating list-held references (`cargo test -p rava-interp --test
   collections`). M2.13 landed `HashMap` and value conversion: the
   `HeapObject::Map` representation, the map constructors and `size`/`isEmpty`/`put`/
   `get`/`containsKey`/`containsValue`/`remove`/`clear` natives with content-equal string
   keys and value-equal `Integer` keys, `Shared::roots` enumerating map keys and values,
   and the `String.valueOf` overloads over the shared `ClassName@hex`/decimal text
   (`cargo test -p rava-interp --test maps_values`). M2.14 landed named member lookup:
    `Class.getDeclaredField`/`getField`, `getDeclaredMethod`/`getMethod` and
    `getDeclaredConstructor`/`getConstructor` match a field by name and an executable by
    name plus parameter descriptors, search the superclass chain and interfaces for the
    inherited forms, return null for a miss, and return the M2.8 cached member object
    (`cargo test -p rava-interp --test member_lookup`). M2.15 landed primitive class
    identity and primitive member types: the `HeapObject::PrimitiveClass(PrimitiveKind)`
    heap kind and its `Shared::primitive_class` cache, the primitive `Class` queries and
    `Class.forName` names, primitive member types from `Field`/`Method`/`Constructor`,
    primitive parameter descriptors in named lookup, the virtual wrapper `TYPE` fields,
    and the `Bean`/`PrimitiveProbe` fixture (`cargo test -p rava-interp --test
    primitive_classes`). M2.16 landed array class identity and array member types: the
    `ClassRef`/`ArrayClass` representation and the `HeapObject::ArrayClass` heap kind,
    the `Shared::array_class` per-component cache and its roots, the component now
    carried by an array heap object, the array `Class` queries and `Class.forName` names,
    array descriptors in member types and in named lookup parameters, and the
    `Bean`/`ArrayClassProbe` fixture (`cargo test -p rava-interp --test array_classes`).
    M2.17 landed class literals: `ldc`/`ldc_w` of a `CONSTANT_Class` push the cached class
    object — a named class resolved through the executing class's loader, a primitive name
    through the primitive-class cache or an array descriptor through the array-class cache
    — with the string constant unchanged, typed errors for a missing class, a malformed
    descriptor and an unsupported constant kind, and the assembled `ClassLiteralProbe`
    plus a child-loader probe (`cargo test -p rava-interp --test class_literals`). M2.18
    landed `invokedynamic` string concatenation: the typed `BootstrapMethods` view,
    opcode `0xba` decoding, and `StringConcatFactory.makeConcat`/
    `makeConcatWithConstants` over the M2.9/M2.13 value conversion, with a committed
    release-17 fixture and assembled call sites
    (`cargo test -p rava-interp --test invokedynamic_concat`). M2.19 is next.
4. Do not write runtime code outside a defined slice.

## Open decisions

No open decisions. [`docs/open-questions.md`](docs/open-questions.md) records all
answers; new questions get new ids and an ADR when they change a decision.
