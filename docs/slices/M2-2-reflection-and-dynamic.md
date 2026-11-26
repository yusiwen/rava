# M2.13–M2.22 slice detail — maps, lookup, dynamic execution and generics

Per-slice record for M2.13–M2.22. `PROJECT.md` § Slices owns the status and the one-line
description; this file owns what each slice delivered, where it stops and the evidence
behind it. Every entry is recorded from the slice's own acceptance run on 2026-11-24.

## M2.13 — `HashMap` and `String.valueOf`/value conversion — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test maps_values` — exit 0.

**Delivered.** `HeapObject::Map(Vec<(Value, Value)>)` is an insertion-ordered map with a heap allocator and accessors, selected by the `java.util.HashMap` class identity in `Shared::allocate_instance`; `Shared::get_class`, `invokevirtual` dispatch and `Object.toString`'s runtime name report `java.util.HashMap`, and `Shared::roots` enumerates every non-null key and value a map holds. The natives cover the `()V`/`(I)V` constructors, `size`, `isEmpty`, `put` (returning the previous value or null), `get`, `containsKey`, `containsValue`, `remove` and `clear`; null keys and values are stored, a string key compares by content, a boxed `Integer` key by value and every other key by reference identity, and a negative capacity or a non-reference argument is a typed `VmError::Native`. `String.valueOf` covers `Object`, `int`, `long`, `boolean`, `char`, `float` and `double`: null renders `"null"`, a `String` returns itself, a boxed `Integer` its decimal value, any other object the VM's `ClassName@hex` shape that matches M2.9 `Object.toString`, `valueOf(int)` is byte-identical to `Integer.toString(int)`, and a lone surrogate `char` is `UnpairedSurrogate`; the value-conversion helpers moved from `collections` to `string`, so `StringBuilder.append(Object)` keeps the same text. The assembled `MapProbe` exercises the bytecode path, two distinct equal-content string keys and two distinct boxed `Integer` keys, roots, and typed errors; 16 tests; `cargo test -p rava-interp` 108, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 272; `cargo xtask ci` exits 0.

**Boundary.** a map is `HeapObject::Map(Vec<(Value, Value)>)`, an insertion-ordered
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

## M2.14 — Named member lookup (`getDeclaredMethod`/`getField`/`getConstructor`) — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test member_lookup` — exit 0.

**Delivered.** `Class.getDeclaredField`/`getField`, `getDeclaredMethod`/`getMethod` and `getDeclaredConstructor`/`getConstructor` are registry natives; a field matches by name and an executable by name plus the parameter descriptors built from its `Class[]` argument (reference parameter classes only), first in class-file order, and a method's return type is not part of the query; `get*` searches the class, its superclass chain and its interfaces with a visited set, while `getDeclared*` and the constructor queries stay on the receiver; a miss is null (the JDK's `NoSuchFieldException`/`NoSuchMethodException` is deferred because exception objects are not synthesized), and a primitive, array, null or non-`Class` parameter class is a typed `VmError::Native`; every hit is the M2.8 `Shared::reflect_member` object, so it is reference-identical to the array queries and stable across calls; the interpreter's `argument_count` now skips an array parameter's element descriptor, so `Class.getDeclaredMethod(String, Class[])` is callable from bytecode; the fixture has a field-hiding subclass, an interface method and three constructors; 5 tests; `cargo test -p rava-interp` 113, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 277; `cargo xtask ci` exits 0.

**Boundary.** The six named lookup natives are `Class.getDeclaredField`/`getField`,
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

## M2.15 — Primitive `Class` identity and primitive member types — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test primitive_classes` — exit 0.

**Delivered.** `HeapObject::PrimitiveClass(PrimitiveKind)` is a heap kind distinct from `HeapObject::Class(ClassId)`, with `Shared::primitive_class` as the one-object-per-kind cache, its own lock and no-lock probe entry, and `Shared::roots` leaving every cached primitive class object a root; `Class.forName` of `boolean`, `byte`, `char`, `short`, `int`, `long`, `float`, `double` and `void` returns that cached object, whose `getName` is the Java name, `getModifiers` is `0x0411` (`public abstract final`, `void` included), `isPrimitive` is true, `isArray`/`isInterface` are false and `getSuperclass`/`getComponentType` are null, while `getClass()` reports `java.lang.Class`; a primitive class object declares no members, so the declared-member array queries return empty arrays and the named lookups return the documented null miss; `Field.getType`, `Method.getReturnType`/`getParameterTypes` and `Constructor.getParameterTypes` resolve primitive descriptors to primitive class objects, and a `Class[]` of primitive class objects builds the exact one-character descriptor (`I`, `Z`, ...) a named lookup matches, while a reference class object still builds `L...;` and a `Void.TYPE` parameter builds `V`, which matches nothing; the nine wrapper `TYPE` fields are resolved virtually by `Shared::static_field_value` on the interpreter's `getstatic` fallback because the C1 stubs have no initializer `Code`, and every other wrapper static (`Integer.MAX_VALUE`) stays absent; the `Bean`/`PrimitiveProbe` fixture asserts the flags, names, member types, lookup hits and misses, a bytecode `setValue`/`getValue` round trip through `int.class`, all nine `TYPE` fields, and the documented boundaries; 6 new tests; `cargo test -p rava-interp` 119, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 283; `cargo xtask ci` exits 0.

**Boundary.** a primitive class object is `HeapObject::PrimitiveClass(PrimitiveKind)`,
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

## M2.16 — Array `Class` identity and array member types — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test array_classes` — exit 0.

**Delivered.** `ClassRef` (a named class, a primitive or a nested array) and `ArrayClass` (a component) describe a class identity beyond `ClassId`; `HeapObject::ArrayClass(ArrayClass)` is a heap kind distinct from `HeapObject::Class(ClassId)` and `HeapObject::PrimitiveClass(PrimitiveKind)`, and `Array` gains the `component` its allocation named, so `newarray`/`anewarray`/`multianewarray` and `String.toCharArray` carry an exact component; `Shared::array_class` is the one-object-per-component cache, with its own lock, a no-lock probe entry and `Shared::roots` entries, and `Shared::get_class` reports an array's array class; `Class.forName` accepts `[I`, `[[I` and `[Ljava.lang.String;` (dotted or internal component names) and returns that cached object, whose `getName` is the JDK descriptor form, `getModifiers` is `0x0411` (`public abstract final`), `isArray` is true, `isPrimitive`/`isInterface` are false, `getComponentType` is the primitive, named or nested-array component class object, and `getSuperclass` is `java.lang.Object` for primitive and reference arrays; `Field.getType`, `Method.getReturnType`/`getParameterTypes` and `Constructor.getParameterTypes` resolve array descriptors through the declaring class's loader, and the named lookups build the exact array descriptor from array `Class[]` entries (`([I)[I`, `([I[Ljava/lang/String;)V`), while a mismatch is the documented null miss; an array class declares no members, so its declared-member arrays are empty and its named lookups are null misses; a malformed array descriptor (`[Q`, `[V`, `[L;`) and a missing named component are typed errors, and a `Class.forName` array resolves only through the bootstrap loader; the assembled `Bean`/`ArrayClassProbe` fixture (an `int[]`, `String[]` and `int[][]` field, `()V`/`([I)V`/`([I[Ljava/lang/String;)V` constructors and `([I)[I`/`([Ljava/lang/String;)[Ljava/lang/String;` methods) asserts names, flags, component identities, `getClass` on `new int[...]`/`new String[...]`/`new int[...][...]`, member types, named lookup hits and misses, and root/cache stability; 6 new tests; `cargo test -p rava-interp` 125, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 289; `cargo xtask ci` exits 0.

**Boundary.** a class identity beyond `ClassId` is a `ClassRef` — a named class, a
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

## M2.17 — `ldc`/`ldc_w` class constants and class literals — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test class_literals` — exit 0.

**Delivered.** `ldc`/`ldc_w` decode a `CONSTANT_Class` into `Instr::LdcClass(name)` and execute by resolving through the executing frame's loader: a named class through `load_class`, a primitive name through `Shared::primitive_class` and an array descriptor through `Shared::class_ref_for_descriptor`, so a child loader's own class keeps its identity while a library class still delegates to the bootstrap loader; the class is loaded but not initialized (JVMS §5.4.3); `ldc` of a `String` still interns, `ldc_w`'s u2 index is decoded, and `Integer`/`Float`/`Long`/`Double` and every other constant kind are typed decode errors (there is no `ldc2_w`); repeated execution pushes the same cached `ObjRef` from the class, primitive and array caches, which `Shared::roots` enumerates; `if_acmpeq`/`if_acmpne` were added so the fixture can compare two literals with `==`; the assembled `ClassLiteralProbe` covers `String.class`, `int.class`, `int[].class`, `String[].class`, `int[].class.getName()` and `int.class == int.class`, and one class constant above index 255 exercises `ldc_w`; a `ChildLiteralProbe` defined in a child `MapSource` loader proves the loader-relative resolution; a missing class (`VmError::ClassNotFound`), a malformed `[Q`/`[V`/`[L;` descriptor (`VmError::Native`) and a `Long` constant (`InterpError::Decode`) are typed errors; 5 new tests; `cargo test -p rava-interp` 130, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 294;

**Boundary.** `ldc` and `ldc_w` decode a `CONSTANT_Class` into `Instr::LdcClass`, and
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

## M2.18 — `invokedynamic` string concatenation — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test invokedynamic_concat` — exit 0.

**Delivered.** `CONSTANT_InvokeDynamic` at opcode `0xba` decodes its u2 index, its reserved zero bytes, its call-site name and descriptor and its bootstrap method index; `ClassFile::bootstrap_method` materializes a `BootstrapMethods` entry on demand as a `REF_invokeStatic` handle plus `String`/`Class`/`int`/`long`/`float`/`double` static arguments, and an out-of-range index, a non-`MethodHandle` reference, a non-static handle or an unsupported argument is `Error::BadBootstrapMethod`; `rava-vm`'s `string_concat` module implements `StringConcatFactory.makeConcat` (every argument, no recipe) and `makeConcatWithConstants` (`\u0001` consumes the next dynamic argument, `\u0002` the next recipe constant, and both sides must be consumed exactly), converts dynamic arguments through the M2.9/M2.13 `String.valueOf` rules (String, boxed Integer, null, int/long/boolean/char/float/double, Object `ClassName@hex`) and pushes an interned String result; any other bootstrap owner or method is `VmError::UnsupportedBootstrapMethod`; the committed release-17 `demo/ConcatProbe` carries four javac-emitted `makeConcatWithConstants` call sites, one using a `\u0002` recipe constant, and assembled probes cover `makeConcat`, every supported primitive descriptor, static recipe constants, a recipe/descriptor mismatch and the malformed and out-of-range bootstrap references; 13 tests; `cargo test -p rava-interp` 143, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 307; `cargo xtask ci` exits 0;

**Boundary.** `invokedynamic` is implemented for `StringConcatFactory` call sites
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

## M2.19 — Lambda metafactory and `invokeinterface` — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test lambdas` — exit 0.

**Delivered.** `invokeinterface` (`0xb9`) decodes its u2 interface-method reference, u1 argument count and u1 reserved zero byte (a non-zero byte is a decode error) and executes: the interface method is resolved on the receiver's runtime class first, then as a default method in the instruction's interface hierarchy, and the ordinary native-registry-before-`Code` path runs it; a class chain without the method falls back to a declaration in the interface hierarchy, an interface-assignability check rejects a wrong receiver with a typed `NotAssignable`, and a missing transitive interface in the C1 tree is skipped instead of masking the result. `HeapObject::Lambda` carries the functional-interface `ClassId`, the SAM name and `samMethodType` descriptor, the `instantiatedMethodType`, a target (owner `ClassId`, name, descriptor and static/instance/constructor kind) and the captured `Value`s; the lambda's class identity is the interface (there is no synthetic class file), `Shared::get_class`/`Object.toString` report that interface and `Shared::roots` enumerates the captured references. `LambdaMetafactory.metafactory` and `altMetafactory` parse their `MethodType`/`MethodHandle` static arguments (the class-file `BootstrapMethods` view now types both), capture the call-site arguments and create the lambda; the SAM call concatenates captured and call arguments, uses the first value as the receiver for an instance target, allocates for a constructor target, and runs through the existing `run_static_method`/`run_instance_method`; `altMetafactory` rejects `FLAG_SERIALIZABLE` and skips marker/bridge tables. The committed release-17 fixture (`demo/IntFn`, `demo/Sink`, `demo/IntFnImpl`, `demo/SinkFactory`, `demo/LambdaProbe`) covers a capturing static lambda, a no-argument void `Runnable`, static/unbound/bound/constructor method references and ordinary/String/ArrayList receivers through `invokeinterface`; assembled probes cover `altMetafactory`, a wrong receiver, a wrong SAM name, an unsupported bootstrap owner, a serializable flag and missing metafactory arguments; 13 tests; `cargo test -p rava-interp` 156, `rava-vm` 47, `rava-classfile` 50 and `rava-cli` 67 pass, workspace 320; `cargo xtask ci` exits 0;

**Boundary.** a lambda is `HeapObject::Lambda`, carrying the functional interface's
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

## M2.20 — Classpath runner (`rava run`) — 2026-11-24

**Acceptance.** `cargo test -p rava-cli --test classpath_run` — exit 0.

**Delivered.** `ClassPathSource` searches directories and jars in classpath order through the existing `ClassSource`/`DirectorySource`/`JarSource` and reports a missing class as `ClassNotFound`; the CLI adds one application loader parented to the bootstrap loader (no second class table, `(loader, name)` identity) and `rava run` loads `MainClass`, requires a static `main([Ljava/lang/String;)V`, allocates a `String[]` through the heap's `ClassRef` machinery and invokes it; `rava-vm::library::register_all` installs the existing Object/String/builder/list/map/Integer/annotation/reflection/instance natives plus a minimal `PrintStream` and initializes `System.out`/`System.err`, and the runner also registers the interpreter's reflective-invocation natives; a committed `javac --release 17` `demo/HelloRun` class and its stored jar, 10 CLI tests (directory, jar, classpath order through a colon list, repeated `--cp`, missing class, missing main, non-static main, usage exit codes, the default stdlib and program arguments) plus 3 `ClassPathSource` tests; `cargo test -p rava-cli` passes 77, `rava-vm` 50, `rava-interp` 156, `rava-classfile` 50, workspace 333; `cargo xtask ci` exits 0.

**Boundary.** `rava run <MainClass> --stdlib <dir> --cp <path-list>` is the only
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

## M2.21 — Static fields, casts, switches and numeric constants — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test bytecode_surface` — exit 0.

**Delivered.** `putstatic` writes `ClassEntry.statics` keyed by `(name, descriptor)` after resolving the declaring class through the executing loader, with typed static/instance, missing-field, descriptor and value-kind errors; `checkcast`/`instanceof` resolve named/primitive/array operands and cover superclass, interface, array-covariance and `Object`/`Cloneable`/`Serializable` assignability, with a typed `ClassCast` on failure; `tableswitch`/`lookupswitch` map default and case targets through the two-pass boundary map and reject non-zero padding, non-boundary targets, inverted/truncated tables and unsorted/duplicate lookup keys; `ldc`/`ldc_w` decode Integer/Float and `ldc2_w` Long/Double, with `lcmp`/`fcmp<op>`/`dcmp<op>` and `lreturn`/`freturn`/`dreturn`; assembler helpers cover each new form; 15 tests; `cargo test -p rava-interp` 171, `rava-vm` 50, `rava-classfile` 50 and `rava-cli` 77 pass, workspace 348; `cargo xtask ci` exits 0.

**Boundary.** `putstatic` resolves the owner through the executing class's loader and the field on that class or a superclass; a field declared in a superclass is stored on the *declaring* class's statics table. It requires `ACC_STATIC`, so an instance field through a static opcode is `InterpError::InstanceFieldAccess`, while a static field through `getfield`/`putfield` stays `StaticFieldAccess`; a missing field, a name under another descriptor and a value whose kind does not match the descriptor (`FieldValueMismatch`) are typed errors. A stored value is the ordinary `ClassEntry.statics` map keyed by `(name, descriptor)`. `getstatic` now uses the same resolution walk, so a superclass static field reads back and the M2.15 `TYPE`/`System.out` paths are unchanged. No `final`-field or access-control check is added, and a static field inherited through an interface is still outside the superclass-only resolution walk.
`checkcast`/`instanceof` decode a class operand and resolve it through the executing loader as a named class, a primitive (Java name or one-character descriptor) or an array descriptor. `instanceof` pushes 1/0. Null is an instance of every reference type and of no primitive, so a null operand short-circuits without resolving a named target; a primitive operand never matches a reference value. `checkcast` leaves null and assignable references unchanged and reports a mismatch as `InterpError::ClassCast { expected, found }`; there is no synthesized `ClassCastException`. Assignability covers the superclass chain, the interface lists on it, array covariance with primitive-component equality, and the array supertypes `Object`/`Cloneable`/`Serializable`. It is still a name-level walk of loaded classes: an interface the C1 tree omits is skipped, and a class object's runtime type is `java.lang.Class`, just as `Shared::get_class` reports.
`tableswitch`/`lookupswitch` decode their JVMS §6.5 alignment padding; a non-zero padding byte is malformed, not ignored. The default and every case target become instruction indices through the existing two-pass boundary map, and a non-boundary target, a negative pair count, a `tableswitch` whose high is below its low, a truncated table, and a `lookupswitch` whose keys are unsorted or duplicated are decode errors. `ldc`/`ldc_w` decode `CONSTANT_Integer` and `CONSTANT_Float` in addition to the M2.17 string and class forms; `ldc2_w` decodes `CONSTANT_Long`/`CONSTANT_Double`; a `long`/`double` through `ldc`/`ldc_w`, and `ldc2_w` of any other kind, are typed decode errors. `lcmp`, `fcmpl`/`fcmpg`, `dcmpl`/`dcmpg` and `lreturn`/`freturn`/`dreturn` were added so a fixture can return and compare those constants. The interpreter keeps one local slot per value, so a `long`/`double` argument is read from one local and a fixture may declare the JVM's two-slot width without using the second slot; this is the existing M2.18 deviation, not two-slot widening. There is no `wide` instruction, monitor, `ldc` of `MethodType`/`MethodHandle`/`Dynamic`, or Spring in this slice. The `BytecodeProbe` fixture is assembled in Rust, and its malformed switch methods are decoded when called, so those paths are typed errors rather than hand-built `DecodedMethod` values.

## M2.22 — Generic `Type` graph and generic reflection — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test generic_types` — exit 0.

**Delivered.** `rava-classfile`'s `metadata::signature` module parses class, method and field JVMS signatures into `ClassSignature`/`MethodSignature`/`TypeSignature`, preserving class and member type-parameter names with their class and interface bounds, nested `Outer<T>.Inner<U>` owners, `+`/`-`/`*` type arguments, arrays and `^` throws; malformed input, `V` outside a method result, trailing bytes and nesting past 256 are typed `SignatureError`s. `rava-vm`'s `generic` module resolves those names through the declaring class's loader: a raw type is the existing `Class`/primitive/array class object, while `ParameterizedType`, `TypeVariable`, `WildcardType` and `GenericArrayType` are `HeapObject::Type` objects cached structurally by `Shared::type_object` and enumerated as roots; `getClass` reports the public `java.lang.reflect` interface, and `Object.toString`/`Type.getTypeName` render a deterministic type name. The natives cover `Class.getGenericSuperclass`/`getGenericInterfaces`/`getTypeParameters`, `Field.getGenericType`, `Method.getGenericReturnType`/`getGenericParameterTypes`/`getTypeParameters`, `Constructor.getGenericParameterTypes`/`getTypeParameters`, `ParameterizedType.getActualTypeArguments`/`getRawType`/`getOwnerType`, `TypeVariable.getName`/`getBounds`/`getGenericDeclaration`, `WildcardType.getUpperBounds`/`getLowerBounds` and `GenericArrayType.getGenericComponentType`; the committed release-17 fixture (`demo.Box`, `StringBox`, `Plain`, `Nest`, `GenericProbe`) covers the parameterized superinterface, `List<String>`, `T[]`, `? extends Number`, `? super Integer`, a `Map$Entry` owner, a `Nest<String>.Inner<Integer>` owner, the generic method and both generic constructors; 7 tests; `cargo test -p rava-interp` 178, `rava-vm` 50, `rava-classfile` 56 and `rava-cli` 77 pass, workspace 361; `cargo xtask ci` exits 0.

**Boundary.** Generic reflection reads the `Signature` attribute only. A class or member without one reports raw `Class` objects (a primitive, an array class or a named class), exactly as the JDK does; a malformed signature is `VmError::Signature`, never a silent fallback to the raw type. The exposed surface is `Class.getGenericSuperclass`/`getGenericInterfaces`/`getTypeParameters`, `Field.getGenericType`, `Method.getGenericReturnType`/`getGenericParameterTypes`/`getTypeParameters`, `Constructor.getGenericParameterTypes`/`getTypeParameters` and the four `java.lang.reflect` interface accessors; `getGenericExceptionTypes`, record-component generic types and `AnnotatedType` are not exposed. A Type object's class identity is the public interface it implements (`ParameterizedType`, `TypeVariable`, `WildcardType`, `GenericArrayType`) rather than the JDK's private `sun.reflect.generics.reflectiveObjects` class, and `getTypeName`/`toString` use the VM's deterministic formatter (`raw<arg, arg>`, `? extends X`, `? super X`, `X[]`), not the JDK's exact text. A type-variable occurrence is resolved against the executable's own parameters and then its declaring class's; an occurrence inherited from an enclosing class is kept by name with `Object` as its only bound and a null `getGenericDeclaration`, and an owner is derived from `InnerClasses` only when the outer class is loadable. `ParameterizedType.getOwnerType` reports a raw declaring class for a `$`-encoded member type and the explicit owner type when the signature spells `.Outer<...>`. No annotations changes, record or sealed views, or Spring behavior are added.
