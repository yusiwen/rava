# PROJECT — rava (a Java 17 runtime for Spring)

**Tier:** L
**Owner:** yusiwen
**License:** MIT
**Started:** 2026-11-22

> Ledger. P0 and P1 were accepted on 2026-11-23. M0, M1 and M2 are complete; M3
> (C2 dynamic code: Proxy, CGLIB, Objenesis, `Lookup`, `Unsafe` U0) is in progress.
> M3.1 defines generated classes through the verifier before insertion. M2.30
> completed the JDK's lookup-miss exceptions and the Spring rerun then reached
> `hello\n`, so M2.23 is done and the M2 gate is claimed. Keep this file honest and
> terse.

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
> M0, M1 and M2 are complete, and all three gate reviews have passed. All M2.1–M2.30
> slices are done; the Spring `AnnotationConfigApplicationContext` acceptance exits 0
> and prints `hello`.

| Milestone | Delivers | Gate | Status |
| :--- | :--- | :--- | :--- |
| M0 | Foundation: parser/model, loader/ClassSource, heap/IR, Shared/ThreadState, safepoints, verifier skeleton, native registry, MemoryRegion | Hello + small corpus; metadata dump; loader identity; no-lock invariant | done |
| M1 | Surface manifest, target lock, `link-check` | Manifest reproducible; unresolved = 0 | done |
| M2 | C1 metadata, minimal library, Spring context | Metadata diff 0; bean graph starts | done |
| M3 | C2 dynamic code: Proxy, CGLIB, Objenesis, `Lookup`, `Unsafe` U0 | C2 suite exits 0 | in-progress |
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
| M2.19 | Lambda metafactory and `invokeinterface` | R3 | `cargo test -p rava-interp --test lambdas` exits 0 | done |
| M2.20 | Classpath runner (`rava run`) | R3 | `cargo test -p rava-cli --test classpath_run` exits 0 | done |
| M2.21 | Static fields, casts, switches and numeric constants | R3 | `cargo test -p rava-interp --test bytecode_surface` exits 0 | done |
| M2.22 | Generic `Type` graph and generic reflection | R3 | `cargo test -p rava-interp --test generic_types` exits 0 | done |
| M2.23 | Spring `AnnotationConfigApplicationContext` bean graph | R3 | `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` exits 0 | done |
| M2.24 | `java.util.stream` pipeline for Spring's annotation metadata | R3 | `cargo test -p rava-interp --test streams` exits 0 | done |
| M2.25 | Full primitive-box family and Number accessors | R3 | `cargo test -p rava-interp --test primitive_boxes` exits 0 | done |
| M2.26 | `instanceof(null)` correctness and map constructor surface | R3 | `cargo test -p rava-interp --test spring_fixes` exits 0 | done |
| M2.27 | Descriptor-closure manifest and C1-tree re-baseline | R3 | `cargo run -p rava-cli -- compat check` exits 0 and `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` reaches the next blocker | done |
| M2.28 | Method/Executable bridge, varargs and synthetic flags | R3 | `cargo test -p rava-interp --test method_flags` exits 0 | done |
| M2.29 | Minimal `java.text.MessageFormat` for Spring messages | R3 | `cargo test -p rava-interp --test message_format` exits 0 | done |
| M2.30 | `NoSuchMethodException`/`NoSuchFieldException` lookup misses | R3 | `cargo test -p rava-interp --test lookup_exceptions` exits 0 | done |
| M3.1 | Generated-class definition: verify every `Code` method before insertion; decode switches and `wide` | R4 | `cargo test -p rava-cli --test c2_generated` exits 0 | done |
| M3.2 | `java.lang.reflect.Proxy` / `InvocationHandler`, generated proxy classes and default-method dispatch | R4 | `cargo test -p rava-cli --test c2_proxy` exits 0 | done |

P0 and P1 are accepted; P2 is done because the M2 and M3.1 increments are sliced. All
M0, M1 and M2 slices are done, and all three gate reviews passed. M2.30 made the six
named `Class` lookups raise the JDK's `NoSuchFieldException`/`NoSuchMethodException`, and
the Spring acceptance then printed `hello` and exited 0, so M2.23 is done and the M2 gate
is claimed (see the M2.23 and M2.30 boundaries). M3 is in progress: M3.1 closed the
definition/verifier seam, and M3.2 adds a real generated proxy class over the existing
interpreter and reflection paths.

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
| M1.2 | `cargo run -p rava-cli -- compat manifest --check` | 0 | 2026-11-24 | `compat/surface/spring-boot-3.5.16.txt` is generated by a static scan of all 11,754 `.class` entries in the 35 pinned jars: 1047 classes, 177 fields, 3,626 methods and 1,048 interface methods under `java/`, `javax/`, `jdk/` and `sun/`, including the descriptor (declared and referenced), `Signature` and annotation-type closure around the constant-pool owners; the manifest is deduplicated, sorted and byte-reproducible; `cargo test -p rava-cli` passes 25 tests |
| M1.3 | `cargo run -p rava-cli -- compat link-check` | 0 | 2026-11-24 | `rava compat stubs` writes 1047 signature-only class files under `stdlib/classes/` from the committed manifest (177 fields, 3,626 methods, 1,048 interface methods); regeneration is byte-identical; link-check resolves all 1047 classes and 4,851 member records with `unresolved 0`; `cargo test -p rava-cli` passes 41 tests; `cargo xtask ci` exits 0 |
| M1.4 | `cargo run -p rava-cli -- compat check` | 0 | 2026-11-24 | `compat check` passes all four stages: the target lock and its 35-jar set agree; the manifest regenerates byte-identically at 1047 classes, 177 fields, 3,626 methods and 1,048 interface methods; link-check resolves 1047 classes and 4,851 member records with `unresolved 0`; a temporary regeneration of `stdlib/classes` is byte-identical across all 1047 class files and the committed tree is not rewritten; `cargo test -p rava-cli` passes 54 tests; `cargo xtask ci` exits 0 |
| M2.1 | `cargo test -p rava-classfile --test metadata_model` | 0 | 2026-11-24 | `rava-classfile`'s `metadata` module decodes the C1 attributes ADR-0006 lists into a typed model: access flags, binary name, superclass, interfaces, `Signature`, visible/invisible annotations with every element-value tag and repeatable containers as encoded, parameter and type annotations (all `target_info` variants and all four `type_path` kinds), nest host/members, permitted subclasses, inner and enclosing classes, `Record` components with their own annotations, `SourceFile`, `ConstantValue`, `Deprecated`, `Exceptions`, `MethodParameters`, `AnnotationDefault`; every attribute payload is still copied verbatim, so an undecoded attribute is retained; malformed payloads return `Error::BadAttribute` with a structured cause; a normalized dump with committed golden files for `Kitchen` and `Defaults`; 29 new tests, 43 `rava-classfile` tests; `cargo xtask ci` exits 0 |
| M2.2 | `cargo run -p rava-cli -- compat metadata --check` | 0 | 2026-11-24 | `stdlib/c1/classes/` holds 1047 committed class files: 1011 projected from the host JDK 25 `lib/ct.sym` release-17 view (1,160 fields, 13,359 methods, class-file version 61.0) and 36 header-only placeholders for manifest owners Java 17 does not define, listed in `compat/surface/c1-unreferenced.txt`; the normalized M2.1 metadata dump has 0 differences across the 1011 referenced classes, the only normalization being the class-file version line (ct.sym stores 69); `stdlib/classes` is untouched and `compat check` still resolves 1047 classes and 4,851 member records with `unresolved 0`; `cargo test -p rava-cli` passes 67 tests; `cargo xtask ci` exits 0 |
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
| M2.19 | `cargo test -p rava-interp --test lambdas` | 0 | 2026-11-24 | `invokeinterface` (`0xb9`) decodes its u2 interface-method reference, u1 argument count and u1 reserved zero byte (a non-zero byte is a decode error) and executes: the interface method is resolved on the receiver's runtime class first, then as a default method in the instruction's interface hierarchy, and the ordinary native-registry-before-`Code` path runs it; a class chain without the method falls back to a declaration in the interface hierarchy, an interface-assignability check rejects a wrong receiver with a typed `NotAssignable`, and a missing transitive interface in the C1 tree is skipped instead of masking the result. `HeapObject::Lambda` carries the functional-interface `ClassId`, the SAM name and `samMethodType` descriptor, the `instantiatedMethodType`, a target (owner `ClassId`, name, descriptor and static/instance/constructor kind) and the captured `Value`s; the lambda's class identity is the interface (there is no synthetic class file), `Shared::get_class`/`Object.toString` report that interface and `Shared::roots` enumerates the captured references. `LambdaMetafactory.metafactory` and `altMetafactory` parse their `MethodType`/`MethodHandle` static arguments (the class-file `BootstrapMethods` view now types both), capture the call-site arguments and create the lambda; the SAM call concatenates captured and call arguments, uses the first value as the receiver for an instance target, allocates for a constructor target, and runs through the existing `run_static_method`/`run_instance_method`; `altMetafactory` rejects `FLAG_SERIALIZABLE` and skips marker/bridge tables. The committed release-17 fixture (`demo/IntFn`, `demo/Sink`, `demo/IntFnImpl`, `demo/SinkFactory`, `demo/LambdaProbe`) covers a capturing static lambda, a no-argument void `Runnable`, static/unbound/bound/constructor method references and ordinary/String/ArrayList receivers through `invokeinterface`; assembled probes cover `altMetafactory`, a wrong receiver, a wrong SAM name, an unsupported bootstrap owner, a serializable flag and missing metafactory arguments; 13 tests; `cargo test -p rava-interp` 156, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 320; `cargo xtask ci` exits 0; |
| M2.20 | `cargo test -p rava-cli --test classpath_run` | 0 | 2026-11-24 | `ClassPathSource` searches directories and jars in classpath order through the existing `ClassSource`/`DirectorySource`/`JarSource` and reports a missing class as `ClassNotFound`; the CLI adds one application loader parented to the bootstrap loader (no second class table, `(loader, name)` identity) and `rava run` loads `MainClass`, requires a static `main([Ljava/lang/String;)V`, allocates a `String[]` through the heap's `ClassRef` machinery and invokes it; `rava-vm::library::register_all` installs the existing Object/String/builder/list/map/Integer/annotation/reflection/instance natives plus a minimal `PrintStream` and initializes `System.out`/`System.err`, and the runner also registers the interpreter's reflective-invocation natives; a committed `javac --release 17` `demo/HelloRun` class and its stored jar, 10 CLI tests (directory, jar, classpath order through a colon list, repeated `--cp`, missing class, missing main, non-static main, usage exit codes, the default stdlib and program arguments) plus 3 `ClassPathSource` tests; `cargo test -p rava-cli` passes 77, `rava-vm` 50, `rava-interp` 156, `rava-classfile` 50, workspace 333; `cargo xtask ci` exits 0 |
| M2.21 | `cargo test -p rava-interp --test bytecode_surface` | 0 | 2026-11-24 | `putstatic` writes `ClassEntry.statics` keyed by `(name, descriptor)` after resolving the declaring class through the executing loader, with typed static/instance, missing-field, descriptor and value-kind errors; `checkcast`/`instanceof` resolve named/primitive/array operands and cover superclass, interface, array-covariance and `Object`/`Cloneable`/`Serializable` assignability, with a typed `ClassCast` on failure; `tableswitch`/`lookupswitch` map default and case targets through the two-pass boundary map and reject non-zero padding, non-boundary targets, inverted/truncated tables and unsorted/duplicate lookup keys; `ldc`/`ldc_w` decode Integer/Float and `ldc2_w` Long/Double, with `lcmp`/`fcmp<op>`/`dcmp<op>` and `lreturn`/`freturn`/`dreturn`; assembler helpers cover each new form; 15 tests; `cargo test -p rava-interp` 171, `rava-vm` 50, `rava-classfile` 50 and `rava-cli` 77 pass, workspace 348; `cargo xtask ci` exits 0 |
| M2.22 | `cargo test -p rava-interp --test generic_types` | 0 | 2026-11-24 | `rava-classfile`'s `metadata::signature` module parses class, method and field JVMS signatures into `ClassSignature`/`MethodSignature`/`TypeSignature`, preserving class and member type-parameter names with their class and interface bounds, nested `Outer<T>.Inner<U>` owners, `+`/`-`/`*` type arguments, arrays and `^` throws; malformed input, `V` outside a method result, trailing bytes and nesting past 256 are typed `SignatureError`s. `rava-vm`'s `generic` module resolves those names through the declaring class's loader: a raw type is the existing `Class`/primitive/array class object, while `ParameterizedType`, `TypeVariable`, `WildcardType` and `GenericArrayType` are `HeapObject::Type` objects cached structurally by `Shared::type_object` and enumerated as roots; `getClass` reports the public `java.lang.reflect` interface, and `Object.toString`/`Type.getTypeName` render a deterministic type name. The natives cover `Class.getGenericSuperclass`/`getGenericInterfaces`/`getTypeParameters`, `Field.getGenericType`, `Method.getGenericReturnType`/`getGenericParameterTypes`/`getTypeParameters`, `Constructor.getGenericParameterTypes`/`getTypeParameters`, `ParameterizedType.getActualTypeArguments`/`getRawType`/`getOwnerType`, `TypeVariable.getName`/`getBounds`/`getGenericDeclaration`, `WildcardType.getUpperBounds`/`getLowerBounds` and `GenericArrayType.getGenericComponentType`; the committed release-17 fixture (`demo.Box`, `StringBox`, `Plain`, `Nest`, `GenericProbe`) covers the parameterized superinterface, `List<String>`, `T[]`, `? extends Number`, `? super Integer`, a `Map$Entry` owner, a `Nest<String>.Inner<Integer>` owner, the generic method and both generic constructors; 7 tests; `cargo test -p rava-interp` 178, `rava-vm` 50, `rava-classfile` 56 and `rava-cli` 77 pass, workspace 361; `cargo xtask ci` exits 0 |
| M2.24 | `cargo test -p rava-interp --test streams` | 0 | 2026-11-24 | `rava-vm` gains `HeapObject::Stream(StreamObject)`, `Spliterator(SpliteratorObject)`, `Collector(CollectorObject)` with a `CollectorKind`, `Optional(Option<Value>)` and the internal `Sink(SinkObject)` consumer; each allocator/accessor is ordinary heap state and `Shared::roots` enumerates stream and spliterator elements, a collector's supplier/key/value/accumulator/combiner/finisher callbacks, an optional's value and a sink's destination, while `Shared::get_class`/`Object.toString` report `java.util.stream.Stream`, `java.util.Spliterator`, `java.util.stream.Collector`, `java.util.Optional` and `java.util.function.Consumer`. `rava-interp::streams` is the eager pipeline: `StreamSupport.stream` drains a VM spliterator directly and a Java spliterator through its own `tryAdvance` with a native consumer that appends to the stream, so Spring's `AnnotationsSpliterator` runs unmodified; `Stream.empty/of/ofNullable/concat`, `filter`, `map`, `flatMap`, `distinct`, `sorted()` (natural order over `Integer`/`Long`/`Float`/`Double`/`String`) and `sorted(Comparator)`, `peek`, `limit`, `skip`, `forEach`, `forEachOrdered`, `toArray` (both forms), `collect(Collector)` and `collect(Supplier,BiConsumer,BiConsumer)`, `count`, `anyMatch`/`allMatch`/`noneMatch`, `findFirst`/`findAny`, `min`/`max`, `reduce`, `toList` and `iterator`; `BaseStream.isParallel`/`sequential`/`parallel`/`unordered`/`onClose`/`close`; `Collection.stream`/`parallelStream`/`spliterator`/`removeIf`; `Spliterator.tryAdvance`/`forEachRemaining`/`trySplit`/`estimateSize`/`getExactSizeIfKnown`/`characteristics`/`hasCharacteristics`/`getComparator`; `Spliterators.emptySpliterator`/`spliterator(Object[],...)`/`spliterator(Collection,int)`/`iterator`; `Collectors.toList`/`toSet`/`toUnmodifiableList`/`toUnmodifiableSet`/`toCollection`/`joining()`/`joining(CharSequence)`/`joining(delim,prefix,suffix)`/`counting`/`toMap` and `Collector.of` in its four- and five-argument forms; `Optional.empty/of/ofNullable/get/isPresent/isEmpty/orElse/orElseGet/orElseThrow/ifPresent/map/filter/toString`; and `Map.forEach(BiConsumer)`. Every callback is invoked the way the rest of the interpreter invokes one — a `LambdaObject` through `lambda::invoke_sam`, any other object through its own SAM on the receiver's superclass chain — and a throwable raised by a callback crosses the native ABI as `VmError::Thrown` rather than being swallowed. An instance-target method reference now dispatches on the receiver's runtime class (then the target owner's interfaces), which is what Spring's `MergedAnnotation::isDirectlyPresent` predicate needs. The chain also exposed and fixed: `Map.of` up to ten pairs, the full map surface on `LinkedHashMap` and `Properties`, concrete `toArray` overrides, `Collections.newSetFromMap`/`emptyIterator`, `Enum.equals`/`hashCode`/`getDeclaringClass`, `Objects.requireNonNull`/`isNull`/`nonNull`/`equals`/`hashCode`/`toString`, `boolean`/`char` (and `byte`/`short` as `Integer`) results in reflective boxing, and superclass walks that now use `Shared::resolution_superclass` so the library's deliberate `AbstractStringBuilder`/`Dictionary` omissions do not break assignability. The committed `crates/interp/tests/fixtures/streams/` fixture (`demo.StreamProbe` plus a real Java `WordSpliterator`, `javac --release 17`) drives filter/map/collect toList and toSet, joining, sorted/limit, a capturing lambda, the VM spliterator surface, a Java spliterator, `Collector.of` with identity and real finishers, and a caught and an uncaught throwing callback; 13 tests; `cargo test -p rava-interp` 191, `rava-vm` 50 and `rava-cli` 78 pass, workspace 375; `cargo xtask ci` exits 0. The Spring rerun now passes the stream calls and stops at `java/lang/Byte.valueOf(B)Ljava/lang/Byte;`, the primitive-box expansion the startup path needs; M2.23 stays blocked. |
| M2.25 | `cargo test -p rava-interp --test primitive_boxes` | 0 | 2026-11-24 | `HeapObject::Boxed(BoxedPrimitive)` now carries all eight primitives, and `BoxedPrimitive` implements the Java conversion rules: `byteValue`/`shortValue`/`intValue` narrow through the low bits (a `float`/`double` saturates through `int`, NaN becomes 0), `longValue`/`floatValue`/`doubleValue` widen (a `long` to `float` rounds), `equals`/`hashCode` use canonical `Float`/`Double` bits, and `compareTo` is the JDK total order. `Shared` gains `boxed_value`/`boxed`/`box_long`/`box_double`, and `Heap` gains one boxed allocator per kind. The wrapper registry covers `valueOf` (primitive, `String`, `String`+radix), the six `Number` accessors, `parseXxx` (decimal and radix), both `toString` forms, `equals`, `hashCode` (instance and static), `compareTo`/`compare`, the `Float`/`Double` `isNaN`/`isInfinite`/`isFinite`/bit methods, `Long.toHexString`, the `Character` comparison surface and `Boolean.valueOf(String)`/`hashCode`/`compare`/`getBoolean`; the `Number` accessors are also registered on `java.lang.Number`, so a call that resolves to the abstract superclass still converts. A malformed or out-of-range `parseXxx` is a typed `VmError::Native`; the documented syntax gaps are a rejected hex floating literal and an accepted case-insensitive `inf`/`nan` spelling, and the float/double text keeps the M2.9 plain-decimal boundary. `String.valueOf`, `Object.toString`, `StringBuilder.append`, stream `joining` and `Arrays.toString` render every box through `BoxedPrimitive::text`; boxed equality/hashing is shared by `Objects`, maps, callbacks, arrays and streams, and `Collectors.counting()` now boxes a `Long`. Reflective adaptation covers every descriptor: `box_result` returns each primitive's own wrapper, `unbox_argument` applies the JLS §5.3 widening rule (a `Character` widens as its code unit, a wrong or wider box is typed), so `Field.get`/`set`, `Method.invoke`, `Constructor.newInstance` and `Method.getDefaultValue` work for all primitives. The assembled `BoxProbe` fixture covers valueOf, the accessors, parse, text, equals/hash/compare, field get/set, method invoke, constructor arguments and `String.valueOf` for all eight kinds; 11 tests; `cargo test -p rava-interp` 202, `rava-vm` 50, `rava-cli` 78 and `rava-classfile` 56 pass, workspace 386; `cargo xtask ci` exits 0 |
| M2.26 | `cargo test -p rava-interp --test spring_fixes` | 0 | 2026-11-24 | `instanceof` of a null operand now pushes 0 for a named class, an interface and an array (JVMS §6.5) instead of 1, and the committed M2.21 test `instanceof_handles_null_and_primitive_operands` was corrected to expect 0; null still passes every reference `checkcast`. `java/util/LinkedHashMap.<init>(IFZ)V` exists and its `accessOrder` flag is honoured: `get`/`getOrDefault`/`put` (existing key)/`putIfAbsent` (existing key) move the entry to the end of `MapObject::entries`, which `keySet`/`values`/`entrySet` and iteration observe; `(I)V`, `(IF)V` and `HashMap(IF)V` keep insertion order, and a negative capacity or a non-positive/NaN load factor is a typed error. The registry's map constructors were corrected from the non-existent `(ILF)V` to the JDK's `(IF)V` on every map surface. The Spring rerun then exposed and this slice fixed: a heap-lock re-entry deadlock in `computeIfAbsent` (`keys_equal` locked the heap while the caller held it), `Class.getDeclaredClasses` over the `InnerClasses` metadata (member rows only; a member outside the C1 subset is skipped), `Comparator.comparingInt` over the new `LambdaTargetKind::Native` lambda target, `Comparator.reversed`, and non-SAM default interface calls on a lambda (`invokeinterface` on a `Comparator` lambda now falls through to a concrete declaration instead of the SAM-mismatch error). The new `crates/interp/tests/spring_fixes.rs` has 9 tests; `cargo test -p rava-interp` 211, `rava-vm` 50, `rava-cli` 78 and `rava-classfile` 56 pass, workspace 395; `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` and `cargo xtask ci` exit 0. The Spring acceptance still exits 1, now at `error: runtime error: class java/lang/annotation/ElementType not found from loader 0`; the M2.23 boundary records the root cause. |
| M2.27 | `cargo run -p rava-cli -- compat check` | 0 | 2026-11-24 | The M1.2 scanner now records every platform class named by a field or method descriptor, a class/method/field `Signature` (type-parameter bounds, type arguments, wildcard bounds, nested `Outer<T>.Inner<U>` owners and `^` throws) and an annotation's encoded type descriptors (the annotation type, an enum constant's type and a class literal), with arrays unwrapped. The re-baselined manifest has 1047 classes (67 more: 17 named only by a declared descriptor or `Signature`, 36 named only by a member-reference descriptor and 14 named only by an annotation), 177 fields, 3,626 methods and 1,048 interface methods; `compat/surface/c1-unreferenced.txt` grows from 28 to 36 owners (the JSR-305 `javax.annotation` qualifiers, `jdk.internal.vm.annotation.IntrinsicCandidate`, JCache's `Cache` and Moneta's `NumberValue`); `stdlib/classes` and `stdlib/c1/classes` each hold 1047 regenerated class files (1011 projected from ct.sym, 1,160 fields and 13,359 methods); `compat manifest --check`, `compat check` (link-check 1047 classes and 4,851 member records with `unresolved 0`; stub determinism 1047 files) and `compat metadata --check` (0 differences across 1011 classes) exit 0; `cargo test -p rava-cli` passes 85 tests and `cargo xtask ci` passes 402 workspace tests. The Spring rerun gets past `ElementType` and stops at `error: method java/lang/reflect/Method.isBridge()Z has no bytecode`; M2.23 stays blocked. |
| M2.28 | `cargo test -p rava-interp --test method_flags` | 0 | 2026-11-24 | `rava-vm`'s reflection natives now answer `Method.isBridge()Z`, `Method`/`Constructor`/`Executable.isVarArgs()Z` and `Field`/`Method`/`Constructor`/`Executable`/`Member.isSynthetic()Z` from the parsed class-file `ACC_BRIDGE`/`ACC_VARARGS`/`ACC_SYNTHETIC` bits, never by inference; a member absent from its class file is a typed `VmError::Native` instead of a fabricated flag. `getModifiers()` keeps the JDK masks: a method's includes `bridge`, `varargs` and `synthetic` (`0x1DFF`, the JDK's `Modifier.methodModifiers()` mask), a field's includes `synthetic` and `enum`, a constructor's is the three access bits, so `Constructor.isVarArgs()` reports the `0x0080` its `getModifiers()` hides. The Spring rerun then exposed and this slice fixed `Set(Collection)` constructor deduplication on `LinkedHashSet`/`HashSet`/`TreeSet`/`CopyOnWriteArraySet` (a repeated element, `null` included, contributes once in first-occurrence order), the `(Collection)` copy constructor on `ArrayList`, `LinkedList`, `ArrayDeque` and `CopyOnWriteArrayList`, `removeAll`/`retainAll`/`containsAll` over the heap lists and sets on every declaration site a virtual call reaches (`AbstractCollection`, `AbstractSet`, the `Collection`/`List`/`Set` interfaces, the concrete collections and `ConcurrentHashMap$KeySetView`), and callback dispatch that resolves `accept`/`apply`/`compare`/`applyAsInt` up the receiver's superclass chain, which is what Spring's `AnnotationAwareOrderComparator` needs for its inherited `OrderComparator.compare`. The committed `crates/interp/tests/fixtures/method_flags/` fixture (`demo.FlagsProbe`, `javac --release 17`) pins the exact flags of a bridge method (`0x1041`), a varargs method (`0x0089`), a varargs constructor (`0x0081`), a synthetic lambda method (`0x1002`), the enum `$VALUES` field (`0x101A`) and `$values()` (`0x100A`), plus the discovery loops, the `Executable`/`Member` declarations, a missing-declaration typed error, a lookup miss that stayed null until M2.30, and the collection/callback fixes; 10 tests; `cargo test -p rava-interp` 221, `rava-vm` 50, `rava-classfile` 56 and `rava-cli` 85 pass, workspace 412; `cargo test -p rava-interp --test method_flags` exits 0. The Spring acceptance still exits 1, now at `error: method java/text/MessageFormat.<init>(Ljava/lang/String;)V has no bytecode` (Spring's `AbstractMessageSource` static initializer builds `new MessageFormat("")`); M2.23 stays blocked. |
| M2.29 | `cargo test -p rava-interp --test message_format` | 0 | 2026-11-24 | `rava-vm::format` adds `HeapObject::MessageFormat` (the pattern text in a dedicated heap shape, since the C1 stub declares no pattern field), `HeapObject::StringBuffer` and `Shared::allocate_instance` selection for both; the natives are `<init>(String)`, `<init>(String, Locale)` (locale accepted and ignored), `applyPattern`, `toPattern`, the inherited `Format.format(Object)` Spring calls, both `format(..., StringBuffer, FieldPosition)` forms and the static `format(String, Object...)`. The parser supports literal text, `{index}`, repeated indices, the JDK's single-quote rules (`''`, quote toggling) and the JDK's literal `{index}` for an absent argument; a null argument renders `"null"`, and `number`/`date`/`time`/`choice` subformats are typed errors once a non-null argument reaches them. A null pattern and a malformed pattern are typed errors (no exception synthesis), and an argument renders through the VM's `String.valueOf` shape rather than a virtual Java `toString`. `StringBuffer` gets both constructors, `append(String)`, `toString` and `length`. The Spring rerun then exposed and this slice fixed four more concrete gaps: the `AccessibleObject` annotation queries (`getAnnotation`/`isAnnotationPresent`/`getDeclaredAnnotations`) that virtual resolution finds on that owner, `AccessibleObject.isAccessible`/`setAccessible` with a real per-member flag, `List.addAll(int, Collection)` on its four declaration sites, and the C1 tree's omitted transitive interfaces (`java.lang.constant.Constable` on `String`) in `Class.getInterfaces` and the inherited-lookup walk, skipped with the same rule M2.19 already used. The new `crates/interp/tests/message_format.rs` has 11 tests through the interpreter (`new`/`invokespecial`/`invokevirtual`/`invokestatic`), including the simple, repeated, missing, quoted, null-argument, null-pattern, static-format and `StringBuffer` forms and a typed subformat error; `cargo test -p rava-interp` passes, `cargo test -p rava-vm` and `cargo test -p rava-cli` pass, `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` exit 0. The Spring acceptance now stops at `Class.getMethod`'s null miss: `Class.getMethod("close", new Class[0])` returns `null` instead of throwing `NoSuchMethodException`, so `DisposableBeanAdapter.inferDestroyMethodsIfNecessary` calls `Method.getName` on null and the CLI reports `error: operand has the wrong type`; M2.23 stays blocked. |
| M2.23 | `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` | 0 | 2026-11-24 | The committed `demo.SpringSmoke` fixture (`javac --release 17`, `proxyBeanMethods = false`) starts a real Spring `AnnotationConfigApplicationContext` against the pinned Spring 6.2.19 jars, reads the `String` bean named `hello` and closes the context through try-with-resources; the acceptance prints exactly `hello\n`, exits 0 and does not skip (the jars are present). The chain that this slice and M2.24–M2.30 completed runs through class loading, `java.util.logging`, annotation metadata and reflection, the primitive-box step, the eager stream pipeline, `CachingMetadataReaderFactory`, `computeIfAbsent`, message formatting, the typed lookup misses, and the shutdown path (lifecycle groups, `EventObject`, enum constants and `TimeZone`/`TimeUnit` statics). |
| M2.30 | `cargo test -p rava-interp --test lookup_exceptions` | 0 | 2026-11-24 | The six named lookups now throw the JDK's exceptions instead of returning null: `getDeclaredField`/`getField` raise `java.lang.NoSuchFieldException`, `getDeclaredMethod`/`getMethod`/`getDeclaredConstructor`/`getConstructor` raise `java.lang.NoSuchMethodException`. `rava_vm::exceptions::new_throwable` allocates the object through the library class table, records the detail message in the same side table the Throwable constructors use, and returns `VmError::Thrown`, so the interpreter's handler search catches it exactly like a compiled throwable. Messages name the receiver class, member and parameter descriptors (`demo.LookupMissProbe.missing(I, Ljava/lang/String;)`, constructors as `<init>`); a primitive or array receiver throws too, and `getDeclaredClasses`/`getClasses` stay empty arrays (M2.30 also implements the JDK `getClasses` superclass/public view). The committed `demo.LookupMissProbe` fixture covers all six lookups through compiled `catch` blocks plus uncaught misses at the native boundary, with 6 tests; the M2.14/M2.15/M2.16/M2.28 tests that pinned the null miss now assert the typed exception and its message. The Spring rerun then exposed and fixed the shutdown-path gaps: `Map.replaceAll`, `Map.forEach`, `AbstractMap.isEmpty`, `Collection.removeIf` over a Java view's own iterator (Spring's `ConcurrentReferenceHashMap$KeySet`), `TreeMap(Comparator)`, `Comparator.naturalOrder`/`reverseOrder`, `Record.<init>`, `EventObject.<init>(Object)`/`getSource`, and enum constant static fields. `cargo test -p rava-interp --test lookup_exceptions` exits 0 with 6 tests; the workspace passes 429 tests; `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` exit 0; the Spring acceptance exits 0 with `hello\n`, so M2.23 and the M2 gate are done. |
| M3.1 | `cargo test -p rava-cli --test c2_generated` | 0 | 2026-11-25 | `ClassTable::define_class` now verifies every method that carries a `Code` attribute after parsing and duplicate checking but before inserting the class; a verifier failure is `VmError::Verify { class, method, detail }` and leaves the class table unchanged, while a missing `Code` (native/abstract/signature-only stub) is skipped. The verifier decodes `tableswitch`/`lookupswitch` (four-byte alignment from the code start, zero padding, default/low/high or npairs, strictly ascending lookup keys, every case target validated) and `wide` local load/store and `iinc`; `ret` and other modified opcodes stay unsupported. The interpreter decodes the same `wide` forms and carries `u16` local indices and an `i32` increment. The new `c2_generated` suite (5 tests) defines and runs a generated `demo/Generated` with a constructor, field and getter, dispatches dense and sparse switches, stores/loads local slot 256, rejects an under-sized `max_stack` without changing `class_count()`, and still rejects a duplicate `(loader, name)`. The definition-time check exposed fixtures that had relied on invalid bytecode; their slot counts and local initialization are corrected without weakening any verifier rejection. `cargo test --workspace` passes 439 tests; `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` exit 0; `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` still exits 0 with `hello\n`. |
| M3.2 | `cargo test -p rava-cli --test c2_proxy` | 0 | 2026-11-25 | A proxy is a real generated class, not a special heap object: `crates/vm/src/proxy.rs` writes `rava/proxy/$ProxyN` with `ClassBuilder` + `Assembler`, extends `java/lang/reflect/Proxy`, lists the requested interfaces, and defines it through `Shared::define_class`, so the M3.1 verifier checks every generated method. The generated methods box each parameter into an `Object[]`, call `InvocationHandler.invoke` with a `Method` resolved by the runtime-private `rava/proxy/ProxySupport` helper, then unbox the result; `widen(JI)J` covers a category-2 parameter and `greet()` covers `InvocationHandler.invokeDefault`, which lives in `rava-interp::proxy` because it re-enters `run_instance_method`. `Proxy` natives cover the constructor, `newProxyInstance`, `getProxyClass` (cached by `(loader, interfaces)`), `isProxyClass` and `getInvocationHandler`; the null `Class[]` error is typed and defines no class. The committed `crates/cli/tests/fixtures/proxy/` fixture (`javac --release 17`) prints nine facts; `c2_proxy` asserts the exact stdout, verifies the generated class in the table, the cache identity on repeated and duplicate interface lists, and a distinct class for a different interface list. `cargo test --workspace` passes 445 tests; `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` exit 0; `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` still exits 0 with `hello\n`. Gaps: no access checks, no exception wrapping, the `ClassLoader` argument is ignored, and generated methods use compact argument locals over a JVM-shaped `max_locals` until the interpreter slot-layout fix. |
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
`java/`, `javax/`, `jdk/` or `sun/` prefix. M2.27 widened the scan to the descriptor
closure around those records: every platform class named by a declared field or method
descriptor, by a field/method/interface-method reference's descriptor, by a class,
field or method `Signature` (type-parameter bounds, type arguments, wildcard bounds,
nested `Outer<T>.Inner<U>` owners and `^` throws) or by an annotation's encoded type
descriptors (the annotation type, an enum constant's type and a class literal) becomes
a `class` record. Array descriptors are unwrapped to their element type and
non-platform owners are filtered out. `LocalVariableTypeTable` signatures inside
`Code`, `MethodParameters` and record-component types are not separate closure inputs
(a record component's field descriptor already covers it). The dynamic load,
reflective access and metadata steps of ADR-0003 are later slices, so the manifest is
a lower bound on the surface, not the full contract. Multi-release
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
byte-for-byte at 1047 classes, 177 fields, 3,626 methods and 1,048 interface methods.
Every manifest record resolves against the stub library with `unresolved 0` across
4,851 member records, and a temporary regeneration of `stdlib/classes` matches the
committed 1047 class files byte-for-byte. The stubs are signature-only: abstract
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
`stdlib/classes/`; the M1 tree is unchanged and its gate still passes. 1011 classes are
projected from the host JDK's `lib/ct.sym` release-17 entries: the signature is parsed
with `rava-classfile`, the class-file version is set to the pinned 61.0, and the class is
re-emitted with the constant pool at its original indices and every attribute payload
verbatim. The 36 manifest owners Java 17 does not define (the `Thread$Builder` classes,
JCache, Moneta, WSDL, the Java EE `javax.annotation.Resource`, the JSR-305
`javax.annotation` qualifiers and the JDK-internal
`jdk.internal.vm.annotation.IntrinsicCandidate`) are listed in
`compat/surface/c1-unreferenced.txt` and get header-only placeholders; the generator
fails, naming the owners, when the archive and that list disagree. `rava compat metadata
--check` re-reads the release-17 signature and compares the M2.1 normalized dump with
the committed stub: 1011 classes compared, 0 differences. The only normalization is the
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


M2.19 boundary: a lambda is `HeapObject::Lambda`, carrying the functional interface's
`ClassId`, the SAM name and `samMethodType` descriptor, the `instantiatedMethodType`, the
implementation target (owner `ClassId`, name, descriptor and a static/instance/constructor
kind) and the captured `Value`s. Its class identity is the interface, not a generated
class: the VM writes no class file for a lambda, so `getClass()` reports the interface's
class object (where the JDK reports a hidden synthetic class), `Object.toString` keeps the
M2.9 `InterfaceName@hex` shape, and `equals`/`hashCode` stay reference identity. The
captured values are ordinary heap references and `Shared::roots` enumerates them; a
lambda is not a permanent root. The target kinds are `REF_invokeStatic`,
`REF_invokeVirtual`/`REF_invokeSpecial`/`REF_invokeInterface` and
`REF_newInvokeSpecial`; the call passes captured values followed by the SAM arguments, an
instance target takes the first value as its receiver, and a constructor target allocates
the owner and runs `<init>`. There is no argument adaptation: boxing, unboxing, widening,
a cast to the instantiated type, generic bridge dispatch and captured primitive edge cases
(other than storing the primitive `Value` unchanged and requiring the target parameter's
exact representation) are typed `InterpError::Lambda` errors. A method handle that
names a field is a class-file error, because only method handles are typed.

`altMetafactory` parses its flag word: `FLAG_SERIALIZABLE` is a typed error, while
`FLAG_MARKERS` and `FLAG_BRIDGES` are consumed and ignored because this representation has
no marker-interface or bridge table. There is no `SerializedLambda`, `$deserializeLambda$`,
`MethodHandle`/`VarHandle` execution, call-site caching, access check or exception-object
synthesis for a malformed lambda.

`invokeinterface` resolves the instruction's interface through the executing frame's
loader, then selects the method on the receiver's runtime class first and on a default
method in the interface hierarchy second; the native registry wins over `Code` at each
step. A lambda receiver must match the instruction's interface and its recorded SAM
name/descriptor. Any other receiver must be interface-assignable to the instruction's
interface, which is checked by walking superclasses and interface lists; a wrong receiver is
a typed `NotAssignable`. The C1 class tree omits a few transitive interfaces (for example
`java.lang.constant.Constable` on `String`), so a missing interface during that walk is
skipped while a missing class elsewhere is still a loader error. Arrays keep M2.16's
`java/lang/Object` dispatch; there is no interface `default`-method selection beyond the
first declaration in the interface walk. `pop` (`0x57`) was added only so the fixture can
discard `ArrayList.add`'s boolean result; `pop2` and the rest of the opcode space remain
absent. Serialization, method handles and Spring stay out of this boundary.

M2.20 boundary: `rava run <MainClass> --stdlib <dir> --cp <path-list>` is the only
execution entry point this slice adds; there is no `verify`, `disasm`, `bench` or `-jar`
yet. A classpath is a `ClassPathSource`, an ordered list of directories and jars opened
through the existing `DirectorySource`/`JarSource`: the first entry that supplies a class
wins, an entry that does not exist fails while opening, and a class no entry supplies is
`VmError::ClassNotFound`. The application loader is one `add_loader` child of the
bootstrap loader in the existing class table, so identity is still `(loader, name)` and
parent-first delegation applies: a class the bootstrap library also defines resolves to
the bootstrap identity and an application copy cannot shadow it. `--cp` is repeatable and
colon-separated; `--stdlib` defaults to `stdlib/c1/classes`, the classpath defaults to
empty, and `--` ends option parsing. `rava-vm::library::register_all` is the single
registration entry point for the existing natives (Object, String, StringBuilder,
ArrayList, HashMap, Integer, annotation, Class/reflection and Object.<init>); it also
installs a minimal `java.io.PrintStream` (`print(String)`, `println(String)`, `println()`)
and writes one `PrintStream` into each of `System.out` and `System.err` because the C1
stubs have no initializer `Code`. Both streams write the VM's captured stdout, so there
is no separate error stream. `Method.invoke`/`Constructor.newInstance` stay in
`rava-interp` and the runner registers that module separately; string concatenation is
not a registry native because the interpreter dispatches it directly. The runner resolves
the main class through the application loader, requires a static
`main([Ljava/lang/String;)V`, allocates a `String[]` with the ordinary heap array and
component class reference, and invokes it through `run_static_method`; program arguments
after the main class are interned into that array. The VM's captured stdout is flushed
before an error is reported, so output printed before a failure is not lost. Exit codes
are 0 for normal return, 1 for a runtime error and 2 for a usage error. Limits: no module
path, wildcard classpath, manifest `Class-Path`, nested jar, multi-release selection or
remote classpath entry, and no verification pass before execution; `PrintStream` has only
the three methods above, so `println(int)`, `printf`, `format` and `System.exit` fail
loudly as missing natives; a `System.err` line is indistinguishable from stdout; there is
no classpath deduplication or canonicalisation, so the same directory listed twice is
searched twice. This slice does not start Spring and adds no Spring or module behavior.

M2.21 boundary: `putstatic` resolves the owner through the executing class's loader and the field on that class or a superclass; a field declared in a superclass is stored on the *declaring* class's statics table. It requires `ACC_STATIC`, so an instance field through a static opcode is `InterpError::InstanceFieldAccess`, while a static field through `getfield`/`putfield` stays `StaticFieldAccess`; a missing field, a name under another descriptor and a value whose kind does not match the descriptor (`FieldValueMismatch`) are typed errors. A stored value is the ordinary `ClassEntry.statics` map keyed by `(name, descriptor)`. `getstatic` now uses the same resolution walk, so a superclass static field reads back and the M2.15 `TYPE`/`System.out` paths are unchanged. No `final`-field or access-control check is added, and a static field inherited through an interface is still outside the superclass-only resolution walk.

`checkcast`/`instanceof` decode a class operand and resolve it through the executing loader as a named class, a primitive (Java name or one-character descriptor) or an array descriptor. `instanceof` pushes 1/0. Null is an instance of every reference type and of no primitive, so a null operand short-circuits without resolving a named target; a primitive operand never matches a reference value. `checkcast` leaves null and assignable references unchanged and reports a mismatch as `InterpError::ClassCast { expected, found }`; there is no synthesized `ClassCastException`. Assignability covers the superclass chain, the interface lists on it, array covariance with primitive-component equality, and the array supertypes `Object`/`Cloneable`/`Serializable`. It is still a name-level walk of loaded classes: an interface the C1 tree omits is skipped, and a class object's runtime type is `java.lang.Class`, just as `Shared::get_class` reports.

`tableswitch`/`lookupswitch` decode their JVMS §6.5 alignment padding; a non-zero padding byte is malformed, not ignored. The default and every case target become instruction indices through the existing two-pass boundary map, and a non-boundary target, a negative pair count, a `tableswitch` whose high is below its low, a truncated table, and a `lookupswitch` whose keys are unsorted or duplicated are decode errors. `ldc`/`ldc_w` decode `CONSTANT_Integer` and `CONSTANT_Float` in addition to the M2.17 string and class forms; `ldc2_w` decodes `CONSTANT_Long`/`CONSTANT_Double`; a `long`/`double` through `ldc`/`ldc_w`, and `ldc2_w` of any other kind, are typed decode errors. `lcmp`, `fcmpl`/`fcmpg`, `dcmpl`/`dcmpg` and `lreturn`/`freturn`/`dreturn` were added so a fixture can return and compare those constants. The interpreter keeps one local slot per value, so a `long`/`double` argument is read from one local and a fixture may declare the JVM's two-slot width without using the second slot; this is the existing M2.18 deviation, not two-slot widening. There is no `wide` instruction, monitor, `ldc` of `MethodType`/`MethodHandle`/`Dynamic`, or Spring in this slice. The `BytecodeProbe` fixture is assembled in Rust, and its malformed switch methods are decoded when called, so those paths are typed errors rather than hand-built `DecodedMethod` values.

M2.22 boundary: generic reflection reads the `Signature` attribute only. A class or member without one reports raw `Class` objects (a primitive, an array class or a named class), exactly as the JDK does; a malformed signature is `VmError::Signature`, never a silent fallback to the raw type. The exposed surface is `Class.getGenericSuperclass`/`getGenericInterfaces`/`getTypeParameters`, `Field.getGenericType`, `Method.getGenericReturnType`/`getGenericParameterTypes`/`getTypeParameters`, `Constructor.getGenericParameterTypes`/`getTypeParameters` and the four `java.lang.reflect` interface accessors; `getGenericExceptionTypes`, record-component generic types and `AnnotatedType` are not exposed. A Type object's class identity is the public interface it implements (`ParameterizedType`, `TypeVariable`, `WildcardType`, `GenericArrayType`) rather than the JDK's private `sun.reflect.generics.reflectiveObjects` class, and `getTypeName`/`toString` use the VM's deterministic formatter (`raw<arg, arg>`, `? extends X`, `? super X`, `X[]`), not the JDK's exact text. A type-variable occurrence is resolved against the executable's own parameters and then its declaring class's; an occurrence inherited from an enclosing class is kept by name with `Object` as its only bound and a null `getGenericDeclaration`, and an owner is derived from `InnerClasses` only when the outer class is loadable. `ParameterizedType.getOwnerType` reports a raw declaring class for a `$`-encoded member type and the explicit owner type when the signature spells `.Outer<...>`. No annotations changes, record or sealed views, or Spring behavior are added.

M2.23 boundary: the committed `crates/cli/tests/fixtures/spring/` fixture
(`demo.SpringSmoke`, `javac --release 17`, `proxyBeanMethods = false`) and
`crates/cli/tests/spring_context.rs` run a real Spring
`AnnotationConfigApplicationContext` through `rava run` and assert exactly `hello\n`; the
test skips with a clear message only when `compat/jars` is absent, so a jar-less CI checkout
stays green. The acceptance failed at each step of this chain: the first failure was
Spring's `AnnotatedElementUtils.getMergedAnnotationAttributes` reaching
`java/util/stream/StreamSupport.stream(Ljava/util/Spliterator;Z)Ljava/util/stream/Stream;`
when the runtime library had no `java.util.stream` pipeline (`Stream`/`Spliterator`/
`Collector`), so the context never reached `refresh()`. The first blocker fixed in this
slice was `java/util/LinkedHashSet.<init>(I)V`; the chain then exposed, and this slice
fixes, `monitorenter`/`monitorexit`, `invokedynamic`'s interface-method references,
float/long/double constants, arithmetic, conversions and the `dup`/`pop` family, and the
missing library surface Spring's startup path touches: `LinkedHashSet`/`HashSet`/`EnumSet`/
`CopyOnWriteArraySet`, `LinkedHashMap`/`Hashtable`/`Properties`/`ConcurrentHashMap`/
`IdentityHashMap`, iterators, `Collections`, `Arrays`, `ArrayDeque`, `Enumeration`,
`Optional`-free factories, `String` search/case/trim/split/join, `Character`/`Boolean`
boxing and classification, `Integer` radix strings, `Math`, `java.util.regex` over the Rust
`regex` crate, `java.util.concurrent.atomic`, `ReentrantLock`/`Condition`, `ThreadLocal`,
`Thread`/`ClassLoader`, `java.lang.ref`, `Throwable` construction, `System` properties and
`arraycopy`, `java.util.logging` (Spring's JCL falls back to it because the acceptance
classpath omits the optional SLF4J/Log4j/Logback adapters), reflective `Array`, member
`hashCode`/`equals`, `Method.getDefaultValue`, `Class.getInterfaces`/`isInstance`/
`isAnnotation`, `Object.clone`, and a `ClassNotFoundException` from
`Class.forName`. Container subclasses keep their own fields and class identity, and a
missing C1 annotation type (`java.lang.annotation.Documented`) is skipped rather than
failing annotation reflection. M2.24 landed the eager `java.util.stream` pipeline, so the
acceptance now runs through `StreamSupport.stream`, `filter`, `map`, `sorted`, `collect` and
the `Collector`/`Spliterator` callbacks. M2.25 landed the primitive-box family, so
`BeanUtils`'s static initializer now boxes every primitive type and the context gets past
that step. M2.26 fixed the `instanceof`-of-null semantics the chain exposed (the
interpreter pushed 1 where JVMS §6.5 requires 0, so
`CachingMetadataReaderFactory.setCacheLimit` took the wrong branch) and the map
constructor surface behind it, and the boundary was still **blocked** at that point. M2.27 expanded the
pinned manifest and both committed stub trees, so `ElementType` and `RetentionPolicy`
resolve and the startup proceeds further (see the M2.27 boundary). M2.28 implemented the
reflected-member predicate flags Spring's bridge-method check calls, and this slice's
rerun exposed and fixed the collection-constructor, bulk-collection and inherited
callback dispatch gaps behind it (see the M2.28 boundary). M2.29 then implemented the
`java.text.MessageFormat` class Spring's `MessageSourceSupport` static initializer
constructs and the message-formatting path calls, and fixed the gaps the next rerun
exposed (see the M2.29 boundary). M2.30 then made the six named lookups throw the JDK's
`NoSuchFieldException`/`NoSuchMethodException` (see the M2.30 boundary), so Spring's
`DisposableBeanAdapter.inferDestroyMethodsIfNecessary` takes its exception path and the
shutdown path runs through the M2.30 rerun fixes. `RAVA_SPRING_ACCEPTANCE=1 cargo test -p
rava-cli --test spring_context` now prints exactly `hello\n`, exits 0 and does not skip,
so M2.23 is **done** and the M2 gate is claimed. The test and fixture are unchanged: the
slice's own compile-time constraint, `proxyBeanMethods = false`, still keeps CGLIB out of
M2, and `java.lang.Record`/`EventObject` support was added for the record and event
objects the startup and shutdown paths construct, not to bypass Spring.

M2.27 boundary: the manifest's descriptor closure is static and jar-only. It reads
every `.class` entry in the pinned jars and adds a `class` record for each platform
class named by (a) a declared field or method descriptor, (b) a field, method or
interface-method reference's descriptor, (c) a class, field or method `Signature` and
(d) an annotation's encoded type descriptors (the annotation type, an enum constant's
type and a class literal). Arrays are unwrapped, the `java/`/`javax/`/`jdk/`/`sun/`
filter still applies, and the record sets stay sorted and deduplicated, so the output
is byte-reproducible. Annotations are read through the M2.1 metadata model; a malformed
attribute or descriptor aborts the scan with a typed error instead of being skipped.
The closure does not scan the JDK stubs or the committed trees, so a type named only
inside a JDK class that no jar class references stays out, and `LocalVariableTypeTable`
signatures inside `Code` are not an input. The re-baselined manifest has 1047 classes,
177 fields, 3,626 methods and 1,048 interface methods; both committed stub trees have
1047 class files, of which 1011 are projected from ct.sym and 36 are placeholders for
owners Java 17 does not define (the 28 from M2.2 plus the JSR-305 `javax.annotation`
qualifiers, `jdk.internal.vm.annotation.IntrinsicCandidate`, JCache's `Cache` and
Moneta's `NumberValue`). The target versions in `compat/targets.toml` are unchanged.
`cargo run -p rava-cli -- compat check` passes its four stages (link-check resolves
1047 classes and 4,851 member records with `unresolved 0`), `compat metadata --check`
reports 0 differences across 1011 classes, `cargo test -p rava-cli` passes 85 tests and
`cargo xtask ci` passes 402 workspace tests. The re-baseline cleared the M2.23
`ElementType` blocker and exposed `Method.isBridge()Z`; no runtime behavior is added by
this slice.

M2.28 boundary: the reflected-member predicate flags read the parsed class file's
`ACC_BRIDGE` (`0x0040`), `ACC_VARARGS` (`0x0080`) and `ACC_SYNTHETIC` (`0x1000`) bits,
never the descriptor or the name. `Method.isBridge()Z`, `Method`/`Constructor`/
`Executable.isVarArgs()Z` and `Field`/`Method`/`Constructor`/`Executable`/`Member.isSynthetic()Z`
are registered at every declaration site a virtual or interface call can resolve to;
the concrete `Method`, `Constructor` and `Field` classes redeclare the predicates, so
those entries are the reachable ones and the `Executable`/`Member` entries keep the
answer for a receiver type that inherits the declaration. A member object whose
declaration is absent from its class's metadata has no flag to report, so that lookup is
a typed `VmError::Native`, not `false`; a named `getDeclaredMethod` miss still returns
`null` (the M2.14 shape). `getModifiers()` keeps the JDK's `Modifier` masking and is not
widened by this slice: a method's `0x1DFF` includes `bridge`, `varargs` and `synthetic`, a
field's `0x50DF` includes `synthetic` and `enum`, and a constructor's `0x0007` is the
three access bits only, so `Constructor.isVarArgs()`/`isSynthetic()` report bits its
`getModifiers()` hides, exactly as on the reference JDK. No access-control check,
`setAccessible` behavior or annotation view is added by these flags.

The Spring rerun behind this slice exposed four library gaps, fixed here with the
production paths rather than fixtures. A `Set(Collection)` constructor now copies the
source as a set: a repeated element, `null` included, contributes once in
first-occurrence order (`LinkedHashSet`/`HashSet`/`TreeSet`/`CopyOnWriteArraySet` share
the constructor). The JDK's `TreeSet(Collection)` also sorts; this VM keeps insertion
order for it, the existing set boundary. `ArrayList`, `LinkedList`, `ArrayDeque` and
`CopyOnWriteArrayList` gain their `(Collection)` copy constructor, which keeps order and
duplicates; a null source is a typed native error (the JDK throws
`NullPointerException`). `removeAll`/`retainAll`/`containsAll` are implemented over the
heap list and set shapes on `AbstractCollection`, `AbstractSet`, the
`Collection`/`List`/`Set` interfaces, the concrete collections and
`ConcurrentHashMap$KeySetView`; they compare with the collection equality rule
(strings by content, boxed primitives by value, everything else by identity), a receiver
that is its own `removeAll` argument loses every element, and a non-collection argument
is a typed error. A callback passed to `Iterable.forEach`, `Arrays.sort`,
`List.sort`, `Map.computeIfAbsent` or `Comparator.comparingInt` is now resolved up the
receiver's superclass chain before it runs, which is what Spring's
`AnnotationAwareOrderComparator` needs for the `compare` it inherits from
`OrderComparator`; a receiver with no such method is still a typed native error. The
committed `crates/interp/tests/fixtures/method_flags/` fixture pins the exact flags and
these fixes through Java 17 bytecode. The local Spring acceptance exits 1 at
`java/text/MessageFormat.<init>(Ljava/lang/String;)V`; M2.23 was still blocked then.

M2.29 boundary: a `java.text.MessageFormat` is `HeapObject::MessageFormat`, a dedicated heap
shape holding the pattern text; the C1 stub declares no instance field for it, so the object
is not an `Instance` and an instance-field opcode does not apply to it. The pattern is stored
verbatim and parsed on each format call, so `toPattern` returns the caller's text and
`applyPattern` replaces it; the `(String, Locale)` constructor accepts and ignores the locale
(the subset is locale-independent), and `getLocale`/`setLocale` have no native.
`Shared::allocate_instance` selects the shape for `new` and reflection, `Shared::get_class`
and `Object.toString` report `java.text.MessageFormat`, and the shape holds no references of
its own.

The parser supports literal text, `{index}`, repeated indices, an absent argument (the JDK's
literal `{index}`), a null argument (`"null"`) and the JDK's single-quote rules (`''` is one
quote; a lone quote toggles a quoted section in which `{`/`}` are literal, and an unterminated
quote simply ends with the pattern). A `number`, `date`, `time`, `choice` or other subformat
is a typed `VmError::Native` once a non-null argument reaches it; a null argument with a
subformat still renders `"null"`. An unmatched `{` or a non-numeric argument index is a typed
error raised by the constructor and `applyPattern`, not a synthesized
`IllegalArgumentException`. An argument renders through the VM's `String.valueOf` conversion
(String, boxed primitives, the `ClassName@hex` default), not a virtual Java `toString`;
`parse`, `setFormat(s)`, `getFormats`, `formatToCharacterIterator` and
`equals`/`hashCode`/`clone` have no native and fail loudly.

`Format.format(Object)` — the JDK's `final` convenience method, which is the declaration a
virtual call on a `MessageFormat` resolves to — is a native on `java/text/Format`. It requires
the argument to be an `Object[]` (or null, the JDK's empty argument list); a non-array is a
typed error where the JDK throws `ClassCastException`. `MessageFormat.format(Object,
StringBuffer, FieldPosition)` and its `Object[]` overload append to a real
`HeapObject::StringBuffer` and return the same buffer; the `FieldPosition` is ignored. The
static `MessageFormat.format(String, Object...)` is registered too. `StringBuffer` gains both
constructors, `append(String)`, `toString` and `length`; it is a second heap shape (the JDK's
`AbstractStringBuilder` superclass is absent from the C1 tree, so
`Shared::resolution_superclass` continues that one chain at `Object`, as it already did for
`StringBuilder`). Other `StringBuffer` methods have no native.

The Spring rerun behind this slice exposed and fixed four further concrete gaps.
`AccessibleObject` is where virtual resolution finds `isAnnotationPresent` and `getAnnotation`,
so those two and `getDeclaredAnnotations` are now registered on that owner as well as on the
concrete member classes. `AccessibleObject.isAccessible` and `setAccessible` are implemented
with a real per-member flag in `Shared` (there is still no access-control check, so the flag
only answers the query). `List.addAll(int, Collection)` is implemented on `ArrayList`,
`LinkedList`, `AbstractList` and `List`, inserting in order and returning whether the source
was non-empty. `Class.getInterfaces` and the inherited-lookup walk now skip an interface the
C1 tree does not ship (for example `java.lang.constant.Constable` on `String`), the rule
M2.19 already applied to assignability and interface-method search, instead of turning the
walk into a loader error.

The Spring acceptance gets past `MessageFormat` and stops at reflective-lookup miss semantics:
`Class.getMethod("close", new Class[0])` returns `null` (the M2.14 shape), where the JDK throws
`NoSuchMethodException`. Spring's `DisposableBeanAdapter` relies on that exception
(`inferDestroyMethodsIfNecessary` catches it and tries `"shutdown"`), so it calls
`Method.getName` on the null result and the CLI reports `error: operand has the wrong type`.
`RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` exits 1; M2.23 stays
blocked and the M2 gate was not claimed then. M2.30 (the next slice) synthesizes
`NoSuchMethodException`/`NoSuchFieldException` for the six named lookups and updates the
M2.14/M2.15/M2.16 tests that pinned the null miss (see the M2.30 boundary).

M2.30 boundary: a named `Class` lookup that finds no member throws a real Java
exception instead of returning `null`. `getDeclaredField`/`getField` raise
`java.lang.NoSuchFieldException`; `getDeclaredMethod`/`getMethod` and
`getDeclaredConstructor`/`getConstructor` raise `java.lang.NoSuchMethodException`. The
object is built by `rava_vm::exceptions::new_throwable`: the class is loaded from the
library class table (so it is the same `ClassId` a compiled `catch` resolves to), an
ordinary instance is allocated, and the detail message goes into the same side table the
`Throwable` constructors write. The C1 stub's constructor has no `Code`, so no Java
constructor runs; `getMessage`, `toString` and handler matching still behave like a
constructed throwable. The native returns `VmError::Thrown`, which the interpreter turns
into `InterpError::Thrown` and routes through the caller's exception table. The message
names the receiver class, the member name and the parameter descriptors:
`demo.LookupMissProbe.missing(I, Ljava/lang/String;)` for a method,
`demo.LookupMissProbe.<init>(Ljava/lang/String;, I)` for a constructor, and
`demo.LookupMissProbe.missing` for a field. The text is deterministic and precise, not
byte-identical to the JDK's own `getMethodMessage` spelling. A primitive or array class
receiver now throws the same way (`int.setValue(I)`, `[I.echo([I)`) because it declares no
members, which is the JDK's behavior. `getDeclaredClasses`/`getClasses` are not lookups
and stay empty arrays; M2.30 also implements `Class.getClasses()` over the `InnerClasses`
rows, filtering to public member classes and walking superclasses (an interface does not
follow its class file's `Object` superclass).

The Spring rerun behind this slice exposed and fixed the remaining shutdown-path gaps. The
context's try-with-resources `close()` runs after the bean is printed, so `hello\n` alone
was not enough for the acceptance. `Map.replaceAll(BiFunction)` and `Map.forEach(BiConsumer)`
are callback natives that snapshot the entries and call the lambda or the object's own
`apply`/`accept`; `AbstractMap.isEmpty` asks the receiver's own `size()`, so a custom Java
map that inherits the JDK body works; `Collection.removeIf(Predicate)` keeps its in-place
behavior for VM lists/sets and, for any other Java `Collection` (Spring's
`ConcurrentReferenceHashMap$KeySet` is a view over its map), follows the JDK default
method's semantics through the receiver's own `iterator()`/`Iterator.remove()` instead of
snapshotting the view into a detached set. `TreeMap(Comparator)` accepts and validates the
comparator but does not retain it: the VM's maps are insertion-ordered (M2.13), so a sorted
`TreeMap` view is not modelled. `Comparator.naturalOrder()`/`reverseOrder()` are native
lambda targets over the same natural-order rule `Stream.sorted()` uses. `java.lang.Record.<init>()V`
is a no-op (a record's components live in its own layout) and `java.util.EventObject.<init>(Object)`
stores the source in the C1 stub's `source` field, with `getSource()` reading it back and a
null source raising a synthesized `IllegalArgumentException`. A static field of an enum's
own type now resolves to the canonical enum object even though the VM never runs the enum's
`<clinit>`; two reads are reference-identical, which is what compiled `==` comparisons
expect. These rerun fixes are covered by the Spring acceptance plus existing suites, not by
new dedicated fixtures; the lookup exceptions themselves have the committed
`crates/interp/tests/fixtures/lookup_exceptions/` fixture (`demo.LookupMissProbe`,
`javac --release 17`) and 6 tests in `crates/interp/tests/lookup_exceptions.rs`, and the
M2.14/M2.15/M2.16/M2.28 tests that pinned the null miss now assert the typed exception and
its message. `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` exits 0
and prints exactly `hello\n`; M2.23 and the M2 gate are done.

M2.24 boundary: the stream pipeline is **eager**. A `Stream` holds its whole element list, and `filter`/`map`/`flatMap`/`distinct`/`sorted`/`peek`/`limit`/`skip`/`sequential`/`parallel`/`unordered` each allocate a new stream with a computed vector; there is no lazy stage graph, no short-circuiting until a terminal operation, no `Spliterator`-backed stream after construction, no parallel execution (the `boolean` of `StreamSupport.stream` is recorded and `isParallel` reports it) and no close action (`onClose` returns the stream, `close` does nothing). `sorted()` compares `Integer`/`Long`/`Float`/`Double` numerically and `String` by UTF-16 code unit; any other element or a null is a typed error rather than a synthesized `ClassCastException`/`NullPointerException`. `distinct` and `toSet` compare strings by content, boxed primitives by per-kind value equality and everything else by reference identity; `Collectors.toMap` appends without the JDK's duplicate-key `IllegalStateException`, `toList`/`toUnmodifiableList` return an ordinary modifiable heap list (there is no unmodifiable wrapper), `toUnmodifiableSet` is the ordinary linked set, and `Collectors.counting()` boxes a `Long` (M2.25 replaced the temporary `Integer` count that stood in before the primitive-box family existed). `joining` renders a non-null element through the VM's `String.valueOf` shape, so a non-`CharSequence` is accepted where the JDK would reject it. A VM `Spliterator` is a snapshot plus a cursor with `ORDERED|SIZED|SUBSIZED` (`DISTINCT` added for a set); `trySplit` returns null and `getComparator` null. A Java object implementing `Spliterator` is drained through its own `tryAdvance` and a native `Consumer` sink, so a Java default `forEachRemaining` is not executed by the VM; Spring's and the fixture's spliterators implement the abstract `tryAdvance`, which is the shape this slice supports. `Optional.empty()` allocates a fresh object rather than the JDK singleton, and a present value is a `Value`, so `Optional` identity is not the JDK's. `Collection.stream`/`spliterator` snapshot the collection, so a later mutation is not observed. `removeIf` and `Map.forEach` are the two callback-taking collection operations added here; `compute`/`merge`/`removeIf`'s concurrent variants and the primitive `IntStream`/`LongStream`/`DoubleStream` pipelines are out of scope. `java.util.stream` `gather`, `mapMulti`, `takeWhile`/`dropWhile`, `reduce` with three arguments, `Collectors.groupingBy`/`partitioningBy`/`teeing`/`summarizing*` and `Spliterator.OfInt`/`OfLong`/`OfDouble` are not implemented and fail loudly. No Spring behavior is faked: the acceptance fixture and test are unchanged, and the remaining blocker is recorded above.

M2.25 boundary: every primitive wrapper is a `BoxedPrimitive` variant, so all eight
primitives can live in an `Object` slot and `getClass`/`Object.toString` report the boxed
class. Each `valueOf` allocates a fresh object: there is still no `Byte`/`Short`/`Integer`/
`Long` small-value cache, so `==` on two equal boxes is false where the JDK caches small
values. The wrapper registry covers `valueOf` (primitive, `String` and `String`+radix), the
six `Number` accessors, `parseXxx` (decimal and radix), both `toString` forms, `equals`,
`hashCode` (instance and static), `compareTo`/`compare`, the `Float`/`Double`
classification and bit methods, `Long.toHexString`, and the `Character` comparison
surface; the `Number` accessors are also registered on `java.lang.Number` for a call that
resolves to the abstract superclass. The conversion rules are the JDK's: `byteValue`/
`shortValue`/`intValue` narrow through the low bits (a `float`/`double` saturates through
`int`, NaN becomes 0), `longValue`/`floatValue`/`doubleValue` widen (a `long` to `float`
rounds), `Float`/`Double` `equals`/`hashCode` use canonical `floatToIntBits`/
`doubleToLongBits` (NaN equals NaN, `0.0` does not equal `-0.0`) and `compareTo` is the
JDK's total order (NaN above every value, `0.0` above `-0.0`, signed-bit tie-break).
Boxed text is `BoxedPrimitive::text`, shared by `String.valueOf`, `Object.toString`,
`StringBuilder.append`, stream `joining` and `Arrays.toString`; boxed equality and hashing
are shared by `Objects`, map keys, collection membership, callbacks, arrays and streams,
and a `Byte`/`Short`/`Long`/`Float`/`Double` key is equal only to the same wrapper kind
(where M2.13 accepted an `Integer` key only). `parseXxx` failures are the typed
`VmError::Native`, never a synthesized `NumberFormatException`; the integer parsers accept
ASCII digits only, the float parsers reject a hex literal but also accept the
case-insensitive `inf`/`infinity`/`nan` spellings that Java's grammar does not, and
`float_text`/`double_text` keep the M2.9 plain-decimal rendering where the JDK would
print an exponent form. The reflective
adaptation covers every primitive descriptor: `box_result` returns each primitive's own
wrapper, `unbox_argument` accepts any wrapper that widens to the target (JLS §5.3, so a
`Character` widens as its code unit), and a box of the wrong or wider kind is a typed
error. `Field.get`/`set`, `Method.invoke`, `Constructor.newInstance` and
`Method.getDefaultValue` therefore work for all primitives. Boundaries not in this slice:
`decode`, the unsigned/radix `toString` families beyond `Integer`'s hex/binary/octal and
`Long.toHexString`, `Character.toString(int)`, the small-value caches, and the
`instanceof`-of-null bug M2.26 fixed.

M2.26 boundary: `instanceof` of a null reference pushes 0 for a named class, an interface
and an array, and a null still passes every reference `checkcast`; a primitive operand
matches no value. The map representation gains one `access_order` flag on
`HeapObject::Map`. `LinkedHashMap(int,float,boolean)` sets it and `HashMap(int,float)`,
`LinkedHashMap(int,float)` and `LinkedHashMap(int)` leave it false; with the flag true a
successful `get`/`getOrDefault` and an update of an existing key through `put`/
`putIfAbsent` move that entry to the end, which is the order `keySet`/`values`/`entrySet`
snapshot and therefore the order an iteration observes. There is no hashing, tree bin or
resize, so capacity and load factor are still hints; they are validated the way the JDK
validates them (a negative capacity and a non-positive or NaN load factor are typed
errors). The map constructors use the JDK's `(IF)V` descriptor, not the `(ILF)V` the
registry carried before, and `LinkedHashMap`'s `(Map)`/`(I)`/`()` forms and `(IFZ)V` are
all registered. This slice also closed what the Spring rerun reached: `computeIfAbsent`
scans the entries under one heap guard (`keys_equal` takes `&Heap` instead of locking
again), `Class.getDeclaredClasses` returns the `InnerClasses` member rows of the class
in class-file order and skips a member whose class is outside the C1 subset, the lambda
representation gains `LambdaTargetKind::Native` for a VM factory body, and
`Comparator.comparingInt`/`reversed` build their comparators over it. `invokeinterface`
on a lambda now treats a call that is not the SAM as an ordinary interface call when the
interface declares a concrete method, so `Comparator.reversed` works on a `Comparator`
lambda while an abstract non-SAM method still raises the typed SAM error. Not in this
slice: the manifest's descriptor-only types (`ElementType`, `RetentionPolicy`), which
M2.27 re-baselined; `Comparator.comparing`, `comparingLong`, `comparingDouble`,
`thenComparing*` and `naturalOrder`; and any change to the committed Spring fixture or
acceptance.

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
3. M0, M1 and M2 are complete; M3 is next. M2.1 landed the typed C1 metadata model and its normalized
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
    (`cargo test -p rava-interp --test invokedynamic_concat`). M2.19 landed simple lambdas
    and `invokeinterface`: the `HeapObject::Lambda` representation, `LambdaMetafactory`'s
    `metafactory`/`altMetafactory` bootstraps, SAM dispatch through the existing
    static/instance machinery, and the committed release-17 lambda fixture
    (`cargo test -p rava-interp --test lambdas`). M2.20 landed the classpath runner:
    `rava-vm`'s ordered `ClassPathSource` over directories and jars, one application
    loader parented to the bootstrap loader, the single `library::register_all` entry
    point, a minimal `System.out`/`System.err` and `PrintStream`, and `rava run`, which
    takes `<MainClass> --stdlib <dir> --cp <path-list>`, requires a static
    `main([Ljava/lang/String;)V`, allocates its `String[]` and invokes it with exit codes
    0/1/2 (`cargo test -p rava-cli --test classpath_run`). M2.21 landed static fields,
    casts, switches and numeric constants: `putstatic` on the declaring class's statics
    table with typed static/instance and kind checks, `checkcast`/`instanceof` over named,
    interface and array types with array covariance, `tableswitch`/`lookupswitch` targets
    resolved to instruction indices, and `ldc`/`ldc2_w` of int/float/long/double with the
    returns and comparisons that observe them
    (`cargo test -p rava-interp --test bytecode_surface`). M2.22 landed the generic
    `Type` graph and generic reflection: the JVMS signature parser with typed errors, the
    `HeapObject::Type` representations and their structural cache and roots, the generic
    queries on `Class`, `Field`, `Method` and `Constructor`, the accessors on
    `ParameterizedType`, `TypeVariable`, `WildcardType` and `GenericArrayType`, and the
    committed release-17 `Box`/`StringBox`/`Nest`/`GenericProbe` fixture
    (`cargo test -p rava-interp --test generic_types`). M2.24 landed the eager
    `java.util.stream` pipeline: the `Stream`/`Spliterator`/`Collector`/`Optional`/sink
    heap representations and roots, the `rava-interp::streams` module with
    `StreamSupport.stream`, the intermediate and terminal operations, `Collectors`,
    `Spliterator`/`Spliterators`, `Optional` and `Map.forEach`, the Java-spliterator drain
    through its own `tryAdvance`, the receiver-dispatched instance method reference, and
    the committed `demo.StreamProbe` fixture
    (`cargo test -p rava-interp --test streams`). M2.25 landed the full primitive-box
    family and the `Number` accessors (`cargo test -p rava-interp --test
    primitive_boxes`). M2.26 landed the `instanceof`-of-null correction (with the M2.21
    test fixed) and the `LinkedHashMap(IFZ)V` constructor with honest access order
    (`cargo test -p rava-interp --test spring_fixes`), plus the lock, reflection and
    `Comparator` fixes the rerun exposed. M2.27 re-baselined the surface: the M1.2
    scanner now closes over declared and referenced descriptors, `Signature` types and
    annotation-encoded type descriptors, so the manifest has 1047 classes and both
    committed stub trees were regenerated (1011 projected from ct.sym plus 36
    placeholders; `compat/surface/c1-unreferenced.txt`). `cargo run -p rava-cli --
    compat check` and `compat metadata --check` exit 0. M2.28 landed the reflected-member
    predicate flags (`Method.isBridge`, the `isVarArgs`/`isSynthetic` declarations on
    `Method`, `Constructor`, `Executable`, `Field` and `Member`) with the JDK's
    `getModifiers()` masking, plus the `Set`/list `(Collection)` constructors,
    `removeAll`/`retainAll`/`containsAll` and the superclass-chain callback resolution
    the rerun exposed (`cargo test -p rava-interp --test method_flags`). M2.29 landed the
    minimal `java.text.MessageFormat` (`HeapObject::MessageFormat`, the pattern parser and
    the `Format.format(Object)`/`StringBuffer`/static forms), the `StringBuffer` shape and
    the M2.23 rerun fixes: the `AccessibleObject` annotation queries and accessibility
    flags, `List.addAll(int, Collection)` and the C1 tree's omitted transitive interfaces
    in `Class.getInterfaces` and the inherited-lookup walk
    (`cargo test -p rava-interp --test message_format`). M2.30 landed the JDK-shaped
    lookup misses: the six named lookups throw `NoSuchFieldException`/
    `NoSuchMethodException` with a class/name/descriptor message, through
    `rava_vm::exceptions::new_throwable` and the interpreter's `Thrown` path, so a compiled
    `catch` observes them (`cargo test -p rava-interp --test lookup_exceptions`). The
    Spring rerun then exposed and fixed the shutdown-path gaps behind the acceptance:
    `Map.replaceAll`, `Map.forEach`, `AbstractMap.isEmpty`, `Collection.removeIf` over a
    Java view's own iterator, `TreeMap(Comparator)`, `Comparator.naturalOrder`/
    `reverseOrder`, `Record.<init>`, `EventObject` source access and enum constant
    statics. `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` now
    prints exactly `hello\n` and exits 0, so M2.23 and the M2 gate are **done**. M3.1 then
    made class definition verify every method before insertion and extended the verifier to
    switches and `wide`, with a generated-class suite. M3.2 then added a real generated
    `java.lang.reflect.Proxy` class, handler dispatch, boxing/unboxing and
    `InvocationHandler.invokeDefault`; M3 is still in progress and the next slice is the
    `ClassLoader` hierarchy and `defineClass`.
4. Do not write runtime code outside a defined slice.

## Open decisions

No open decisions. [`docs/open-questions.md`](docs/open-questions.md) records all
answers; new questions get new ids and an ADR when they change a decision.
