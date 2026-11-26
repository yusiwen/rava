# M2.1–M2.12 slice detail — C1 metadata, object model and execution

Per-slice record for M2.1–M2.12. `PROJECT.md` § Slices owns the status and the one-line
description; this file owns what each slice delivered, where it stops and the evidence
behind it. Every entry is recorded from the slice's own acceptance run on 2026-11-24.

## M2.1 — C1 metadata model and normalized dump — 2026-11-24

**Acceptance.** `cargo test -p rava-classfile --test metadata_model` — exit 0.

**Delivered.** `rava-classfile`'s `metadata` module decodes the C1 attributes ADR-0006 lists into a typed model: access flags, binary name, superclass, interfaces, `Signature`, visible/invisible annotations with every element-value tag and repeatable containers as encoded, parameter and type annotations (all `target_info` variants and all four `type_path` kinds), nest host/members, permitted subclasses, inner and enclosing classes, `Record` components with their own annotations, `SourceFile`, `ConstantValue`, `Deprecated`, `Exceptions`, `MethodParameters`, `AnnotationDefault`; every attribute payload is still copied verbatim, so an undecoded attribute is retained; malformed payloads return `Error::BadAttribute` with a structured cause; a normalized dump with committed golden files for `Kitchen` and `Defaults`; 29 new tests, 43 `rava-classfile` tests; `cargo xtask ci` exits 0.

**Boundary.** The model decodes the C1 attributes from a parsed `ClassFile`. A generic
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

## M2.2 — JDK 17 signature stubs and zero metadata diff — 2026-11-24

**Acceptance.** `cargo run -p rava-cli -- compat metadata --check` — exit 0.

**Delivered.** `stdlib/c1/classes/` holds 1047 committed class files: 1011 projected from the host JDK 25 `lib/ct.sym` release-17 view (1,160 fields, 13,359 methods, class-file version 61.0) and 36 header-only placeholders for manifest owners Java 17 does not define, listed in `compat/surface/c1-unreferenced.txt`; the normalized M2.1 metadata dump has 0 differences across the 1011 referenced classes, the only normalization being the class-file version line (ct.sym stores 69); `stdlib/classes` is untouched and `compat check` still resolves 1047 classes and 4,851 member records with `unresolved 0`; `cargo test -p rava-cli` passes 67 tests; `cargo xtask ci` exits 0.

**Boundary.** `stdlib/c1/classes/` is a second committed tree, separate from
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
library. Issue #53 measured the tree's closure boundary rather than regenerating it:
following every superclass and interface edge leaves 69 platform owners absent — 35
distinct direct superclass names over 44 direct superclass edges (for example
`java/util/AbstractSequentialList`) and 34 distinct interface names over 70 interface
edges. Those are properties of the observed manifest tree, not a metadata diff; the
tolerance is documented in `docs/explanation/c1-metadata.md` and used by
`Class.getFields`/`Class.getMethods`.

## M2.3 — `stdlib/c1/classes` as the bootstrap library and its metadata through the VM — 2026-11-24

**Acceptance.** `cargo test -p rava-vm --test c1_runtime` — exit 0.

**Delivered.** `stdlib/c1/classes` is the bootstrap class source through the existing `DirectorySource` and `(loader, name)` identity; `Shared::with_library_dir`, `Shared::load_library_class`, `Shared::library_metadata` and `ClassEntry::metadata()` decode C1 metadata without a `Code` attribute; the test loads `java.lang.String`, `java.lang.Class`, `java.util.List`, sealed `java.lang.reflect.Executable` and annotation type `java.lang.Deprecated`, checks superclass/interfaces/signatures/field and method descriptors/annotations/permitted subclasses, a second loader's distinct identity and no `Code` on any stub method; 7 tests; `cargo test -p rava-vm` passes 42 tests; `cargo xtask ci` exits 0.

**Boundary.** `stdlib/c1/classes` is the bootstrap class source. It is read through the
existing `DirectorySource` and `(loader, name)` identity, so there is no second class
table and no second identity model. `Shared::with_library_dir` is a constructor over that
source; `Shared::load_library_class`, `Shared::library_metadata` and
`ClassEntry::metadata()` decode metadata on demand and do not cache it. Loading parses a
class file and metadata decoding reads attributes, so neither requires a `Code`
attribute: the signature-only stubs load without an execution body. This slice does not
initialize or execute a class, does not build `Class`, `Field`, `Method` or `Constructor`
objects, and adds no reflection native or Spring path. The committed tree has no record
class, so record metadata is exercised by the M2.1 fixtures rather than here.

## M2.4 — Class objects and basic `Object`/`Class` reflection — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test class_objects` — exit 0.

**Delivered.** `HeapObject::Class(ClassId)`, its heap allocator/accessor, `Shared::class_object` (one cached object per `(loader, name)` identity) and `Shared::get_class` for `Str`/`Instance`/`Class` (arrays return a typed error); cached class objects are enumerated by `Shared::roots`; the native ABI takes the receiver `ObjRef` (`ObjRef::NULL` for static calls), and `run_static_method`/`invokevirtual` consult the registry before `Code`, so the C1 stubs (no `Code`, no `ACC_NATIVE`) execute; `invokevirtual` resolves `HeapObject::Class` as `java/lang/Class` and `HeapObject::Str` as `java/lang/String`, walking the superclass chain; `Object.getClass`, `Class.getName/getModifiers/isInterface/isArray/getSuperclass/forName`; `ReflectProbe` fixture; 8 tests; `cargo test -p rava-interp` 16, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 175; `cargo xtask ci` exits 0.

**Boundary.** a class object is `HeapObject::Class(ClassId)` and carries no metadata of
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

## M2.5 — Instance allocation, constructors, fields and instance calls — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test instance_execution` — exit 0.

**Delivered.** `new`, `dup`, `aconst_null`, `invokespecial`, `getfield`, `putfield` and narrow/wide `aload`/`astore` decode and execute; `run_instance_method` reuses the existing `Frame`, puts `this` in local 0 and leaves static calls on the unchanged path; `Shared::field_layout` builds superclass-first instance layouts (static fields excluded) and `Shared::allocate_instance` allocates with field defaults; `new` resolves and initializes the class from the current class's loader; `getfield`/`putfield` resolve the exact owner/name/descriptor, use the receiver's runtime layout and reject a static field through an instance opcode and a descriptor mismatch; `invokespecial` resolves on the instruction's owner while `invokevirtual` dispatches from the receiver's class, and both prefer the native registry before executing `Code`; `instance::register_natives` supplies `java/lang/Object.<init>()V` as a no-op for the C1 stub that has no `Code`; `Point`/`PointProbe` fixture; 8 tests; `cargo test -p rava-interp` 24, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 183; `cargo xtask ci` exits 0.

**Boundary.** `new` allocates the instance immediately, with the layout's default field
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

## M2.6 — Arrays and integer control flow — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test arrays_control` — exit 0.

**Delivered.** `newarray` (all eight primitive `atype` values), `anewarray`, `multianewarray`, `arraylength`, the eight primitive array loads/stores, `aaload`/`aastore`, `iinc`, `goto`, `ifeq`/`ifne`/`iflt`/`ifge`/`ifgt`/`ifle`, `if_icmp{eq,ne,lt,ge,gt,le}` and `ifnull`/`ifnonnull` decode and execute; branch offsets are resolved at decode time to instruction indices, so the dispatch loop never computes a byte address and a non-boundary target is a decode error; `newarray`/`anewarray`/`multianewarray` allocate through the existing `Heap`/`ArrayKind`/`Array`, and `anewarray` resolves an object component through the current class's loader; bounds, null, negative-size and `ArrayKind`/element-type errors are typed; the `ArrayProbe` fixture (`sum` with `if_icmpge`/`goto`/`iaload`/`arraylength`, `make` with `newarray`/`iastore`, `refs` with `anewarray`/`aastore`, a two-dimensional `multianewarray`, and one method per comparison) exercises them; 12 tests; `cargo test -p rava-interp` 36, `cargo test -p rava-vm` 42, `cargo test -p rava-classfile` 50 and `cargo test -p rava-cli` 67 pass, workspace 195; `cargo xtask ci` exits 0.

**Boundary.** Arrays are the existing `Heap` `Array`/`ArrayKind` model; this slice adds
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

## M2.7 — Exceptions, `athrow` and exception tables — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test exceptions` — exit 0.

**Delivered.** The `Code` exception table is decoded after the code array and its `start_pc`/`end_pc`/`handler_pc` become instruction indices through the existing two-pass boundary map (a non-boundary, out-of-range, empty or reversed entry and a non-`Class` `catch_type` are decode errors); `athrow` and a callee's `Thrown` result search the current frame's table in class-file order for an entry whose range contains the throwing instruction and whose catch type is assignable from the throwable's runtime class through the superclass chain, clearing the stack and jumping to the handler on a match and otherwise re-raising the same `ObjRef`; `catch_type = 0` is a catch-all; `InterpError::Thrown(ObjRef)`, `NullThrow` and `NonReferenceThrow` are new; the `Boom`/`Catcher` fixture covers a direct catch returning 5, a catch-all, a superclass catch, a wrong catch type, a throw crossing two calls into the outer frame, an uncaught throw that still carries the object, null and non-reference `athrow`, and three malformed tables; 9 tests; `cargo test -p rava-interp` 45, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 204; `cargo xtask ci` exits 0.

**Boundary.** The exception table is decoded from the `Code` attribute and its three
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

## M2.8 — Reflection member objects (`Field`/`Method`/`Constructor`) — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test reflection_members` — exit 0.

**Delivered.** `HeapObject::Reflect(ReflectMember)` with a `MemberKind` of `Field`/`Method`/`Constructor`, the declaring `ClassId`, name and descriptor; `Shared::reflect_member` caches one object per `(class, kind, name, descriptor)` and `Shared::roots` enumerates those roots, while `Shared::get_class` and `invokevirtual` dispatch a member to `java/lang/reflect/{Field,Method,Constructor}`; `Class.getDeclaredFields/getDeclaredMethods/getDeclaredConstructors` return real heap reference arrays built from the typed C1 metadata model, and the read-only member natives cover `getName`/`getModifiers`/`getDeclaringClass`, `Field.getType`, `Method.getReturnType/getParameterTypes/getParameterCount` and `Constructor.getParameterTypes/getParameterCount/getName`; the `ReflectMembersProbe` fixture exercises them through the C1 bootstrap library; 7 tests; `cargo test -p rava-interp` 52, `cargo test -p rava-vm` 42 and `cargo test -p rava-cli` 67 pass, workspace 211; `cargo xtask ci` exits 0.

**Boundary.** a reflected member is `HeapObject::Reflect(ReflectMember)`, carrying a
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

## M2.9 — Core `Object`/`String` behavior — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test string_object` — exit 0.

**Delivered.** `HeapObject::Str` stays the only string representation; the `java/lang/String` registry covers `length`, `isEmpty`, `charAt`, `equals`, `hashCode`, `compareTo`, `concat`, both `substring` overloads, `startsWith`, `endsWith`, `indexOf`, `replace(CC)`, `toCharArray`, `toString` and `intern`, and every operation reads UTF-16 code units from the stored text through `utf16_units`/`string_from_utf16`, so a supplementary character is two units; `java/lang/Object` gains `equals` (reference identity), `hashCode` (a deterministic mix of the `ObjRef` index and generation, documented as the identity hash and cached by nothing because the handle never changes) and `toString` (`ClassName@hexHashCode` with the dotted runtime binary name); `Shared::intern_string` keeps one canonical object per text as a permanent root, and `ldc` goes through it so repeated execution of a string constant reuses one object; `StringProbe` with 50+ `ldc` and argument helpers plus 8 tests; `cargo test -p rava-interp` 60, `cargo test -p rava-vm` 45 and `cargo test -p rava-cli` 67 pass, workspace 222; `cargo xtask ci` exits 0.

**Boundary.** `java.lang.String` is still `HeapObject::Str`, a Rust `String` holding
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

## M2.10 — Boxed values and reflection invocation — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test reflection_invoke` — exit 0.

**Delivered.** `HeapObject::Boxed(BoxedPrimitive::Integer)` is a heap object, not a second `Value` variant; `Shared::get_class` and `invokevirtual` dispatch it to `java/lang/Integer`, so `getClass` reports the boxed class; `Shared::box_int`/`unbox_int` and the `boxed::{unbox_argument,box_result}` helpers convert `int` boxes and pass reference handles through; the `java/lang/Integer` registry overrides `valueOf(I)`, `intValue`, `toString()`, `toString(I)`, `equals`, `hashCode`, `compareTo(Integer)` and `parseInt(String)`; `Field.get`/`set` cover `int` and reference instance fields, reject a static field and a null or non-assignable receiver, and preserve reference identity; `Method.invoke` runs static and instance methods by the reflected name and descriptor with no overload lookup, unboxes `int` arguments, boxes an `int` result and ignores the receiver for a static method; `Constructor.newInstance` allocates the declaring class, unboxes its arguments and runs `<init>` through `run_instance_method`; a Java throwable from an invoked method crosses the native ABI as `VmError::Thrown` and becomes `InterpError::Thrown`, so it is not swallowed (`InvocationTargetException` wrapping is deferred); the `Bean`/`BoxProbe` fixture covers boxed virtual calls, `(I)V` and `()V` construction, int/reference field access, getters/setters and static `sum`, plus wrong receivers, a static field, wrong arity and type, a missing-descriptor method and the no-`getDeclaredMethod` boundary; 9 tests; `cargo test -p rava-interp` 70, `cargo test -p rava-vm` 45 and `cargo test -p rava-cli` 67 pass, workspace 232; `cargo xtask ci` exits 0.

**Boundary.** a boxed primitive is `HeapObject::Boxed(BoxedPrimitive::Integer)`, a
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

## M2.11 — Annotation reflection and proxies — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test annotations` — exit 0.

**Delivered.** `HeapObject::Annotation` carries the annotation type's `ClassId` and the encoded values from the M2.1 metadata model, and `HeapObject::Enum` (cached per `(class, name)` as a permanent root) represents an enum constant; `Class`, `Field`, `Method` and `Constructor` gain `getDeclaredAnnotations`, `getAnnotations` and `getAnnotation(Class)` over that metadata in class-file order, returning an empty array for no declared annotations and null for a missing single annotation; `Class.getAnnotations` merges superclass annotations whose type is itself `@Inherited`, nearest declaration winning, while `getDeclaredAnnotations` stays declared-only; an omitted element falls back to the annotation type's `AnnotationDefault`; element values decode through the annotation type's loader as interned strings, `Value` primitives, class literals (`c`), enum constants (`e`), nested annotation objects (`@`) and primitive/reference arrays (`[`); `invokevirtual` dispatches an annotation receiver to its annotation type and the annotation module answers `annotationType`, every element method and deterministic `equals`/`hashCode`/`toString`; `java/lang/annotation/Annotation.annotationType` and the three queries on `Class`/`Field`/`Method`/`Constructor` (plus `AccessibleObject.getAnnotations`, where the C1 stub declares it) are registry natives, and `java/lang/Enum.name`/`toString` report a constant's name; the committed release-17 fixture under `crates/interp/tests/fixtures/annotations/` annotates a class, field, method and constructor with string/int/boolean/long/double/class/enum/nested/array elements and defaults and an `@Inherited` superclass annotation; 10 tests; `cargo test -p rava-interp` 80, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 244; `cargo xtask ci` exits 0.

**Boundary.** An annotation instance is `HeapObject::Annotation`, carrying the annotation
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

## M2.12 — `StringBuilder` and core collection behavior — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test collections` — exit 0.

**Delivered.** `HeapObject::StringBuilder(String)` and `HeapObject::List(Vec<Value>)` are ordinary heap objects selected by their class identity in `Shared::allocate_instance`; `Shared::get_class`, `invokevirtual` dispatch and `Object.toString`'s runtime name report `java/lang/StringBuilder` and `java/util/ArrayList`, and `Shared::roots` enumerates every non-null reference held by a heap list. The builder natives cover the three constructors, the `String`/`Object`/`int`/`long`/`char`/`boolean`/`float`/`double`/`CharSequence` `append` overloads, `toString`, `length`, `charAt`, `setLength`, `reverse`, `deleteCharAt` and `insert(I,String)` over UTF-16 units (null String appends `"null"`, a split surrogate is `UnpairedSurrogate`); the list natives cover both constructors, `size`, `isEmpty`, both `add` forms, `get`, `set`, `clear`, both `remove` forms, `contains` and `indexOf` with reference-equality membership and typed bounds errors. The `CollectionsProbe` fixture exercises the bytecode path (including `getClass().getName()` on both VM classes), and the registry is called directly only for the `long`/`float`/`double` overloads the decoded instruction subset cannot load. 12 new tests; `cargo test -p rava-interp` 92, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 256; `cargo xtask ci` exits 0.

**Boundary.** a `StringBuilder` is `HeapObject::StringBuilder(String)` and an
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
