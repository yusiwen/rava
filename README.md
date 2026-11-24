# rava

A Java 17 runtime in Rust, built to run Spring — not just a corpus.

## Status

**Decisions accepted (2026-11-23). M0 and M1 are complete (2026-11-24):** workspace,
parser, loader identity, heap/seams, the interpreter core, Hello, safepoint/GC-root
and no-lock-across-Java-call invariants, the verifier dataflow skeleton with the
metadata dump fixture, the pinned target lock with the reference application and its
35-jar set, the static surface manifest (`compat/surface/spring-boot-3.5.16.txt`:
980 classes, 177 fields, 3,626 methods, 1,048 interface methods), and the
signature-only stub library under `stdlib/classes/` (980 class files) with a
zero-unresolved link-check. The manifest and stubs regenerate byte-identically, and
`rava compat check` runs the whole M1 gate. **M2.1 is done (2026-11-24):** a typed C1
metadata model in `rava-classfile` (annotations of every element-value kind, parameter
and type annotations, generic signatures, records, sealed permits, nestmates, inner and
enclosing classes, exceptions and parameter names) with every raw attribute payload
retained, a normalized text dump, committed Java 17 fixtures and golden dumps, plus
typed errors for malformed payloads. **M2.2 is done (2026-11-24):** a separate JDK 17
signature stub tree under `stdlib/c1/classes/` (980 class files, 952 projected from the
host JDK's `lib/ct.sym` release-17 signatures and 28 header-only placeholders for
owners Java 17 does not define) with a faithful class-file writer, and a normalized
metadata check whose diff is 0 across the 952 referenced classes. `stdlib/classes/` is
unchanged and the M1 gate still passes. **M2.3 is done (2026-11-24):** the C1 tree is
the bootstrap class source, read through the existing `DirectorySource` with
`(loader, name)` identity; `Shared::with_library_dir`, `Shared::load_library_class`,
`Shared::library_metadata` and `ClassEntry::metadata()` expose its metadata without a
`Code` attribute or executing a class. **M2.4 is done (2026-11-24):** class objects are
heap objects cached once per `(loader, name)` identity and enumerated as GC roots;
`Object.getClass` and `Class.getName`, `getModifiers`, `isInterface`, `isArray`,
`getSuperclass` and `forName` are native registry entries, and the registry overrides the
C1 stubs that carry neither `Code` nor `ACC_NATIVE`. **M2.5 is done (2026-11-24):**
`new`/`dup`/`invokespecial`/`getfield`/`putfield` and the reference loads/stores execute;
`run_instance_method` puts `this` in local 0 on the existing frame, the VM owns the
superclass-first instance layout and allocation, and instance calls prefer the native
registry, then `Code`, including the registered no-op `Object.<init>()V`. **M2.6 is done
(2026-11-24):** `newarray`, `anewarray`, `multianewarray`, `arraylength` and the array
load/store opcodes allocate and access the existing heap arrays; `goto`, the
`if<cond>`/`if_icmp<cond>` comparisons, `ifnull`/`ifnonnull` and `iinc` execute with
branch targets resolved to instruction indices at decode time; and bounds, null,
negative-size and element-kind failures are typed errors. **M2.7 is done
(2026-11-24):** the `Code` exception table is decoded into instruction ranges,
`athrow` throws, and a throwing instruction or a throwing callee searches the
current frame's table in class-file order for a range that contains it and a catch
type assignable through the superclass chain; a match clears the operand stack,
pushes the throwable and jumps to the handler, while an uncaught throw leaves the
frame as a typed `Thrown` carrying the same object. A catch-all entry
(`catch_type = 0`) decodes, but `finally` duplication, stack traces,
exception-object synthesis and interfaces are not implemented, and nothing checks that
the thrown object or the catch type is a `Throwable`. **M2.8 is done (2026-11-24):**
`HeapObject::Reflect` represents a field, method or constructor by its declaring class,
name and descriptor; one object per member is cached and enumerated as a GC root;
`Class.getDeclaredFields`, `getDeclaredMethods` and `getDeclaredConstructors` return real
heap reference arrays; and the read-only member natives report names, modifiers,
declaring classes, field and return types and parameter counts. Reflective invocation,
annotations and generic types remain later slices. **M2.9 is done (2026-11-24):**
`java.lang.String` keeps the existing `HeapObject::Str` and implements `length`,
`isEmpty`, `charAt`, `equals`, `hashCode`, `compareTo`, `concat`, both `substring`
overloads, `startsWith`, `endsWith`, `indexOf`, `replace`, `toCharArray`, `toString` and
`intern` over UTF-16 code units, including surrogate pairs; `java.lang.Object` gains
reference-identity `equals`, a stable identity `hashCode` and a `ClassName@hash`
`toString`; and interned strings, including `ldc` constants, are permanent roots of a
content-keyed table. A lone
surrogate, a null argument and an out-of-range index are typed errors because exception
objects are not synthesized yet. `String.valueOf`, `StringBuilder`, collections,
annotations and generics remain later slices. **M2.10 is done (2026-11-24):** a boxed
`Integer` is `HeapObject::Boxed(BoxedPrimitive::Integer)`, so `getClass` reports
`java.lang.Integer` and `Integer.valueOf`, `intValue`, `toString`, `equals`, `hashCode`,
`compareTo` and `parseInt` have real value semantics; `Field.get`/`set` read and write
`int` and reference instance fields with boxing and identity-preserving references; and
`Method.invoke`/`Constructor.newInstance` run static and instance methods and
constructors through the existing interpreter, using the reflected name and descriptor
with no overload resolution. A Java throwable from an invoked method propagates as a
`Thrown` rather than being swallowed; `InvocationTargetException` wrapping, varargs,
access checks and `setAccessible` remain later slices. **M2.11 is done (2026-11-24):** an
annotation instance is a heap object carrying its annotation type and the class file's
encoded element values; `Class`, `Field`, `Method` and `Constructor` answer
`getDeclaredAnnotations`, `getAnnotations` and `getAnnotation`, with `AnnotationDefault`
fallback, inherited `@Inherited` class annotations and null for a missing annotation;
element values decode as strings, primitives, class literals, enum constants, nested
annotations and primitive/reference arrays; and `invokevirtual` dispatches an annotation
receiver to its annotation type, where `annotationType`, the element methods and
deterministic `equals`/`hashCode`/`toString` are served. Repeatable containers
(`getAnnotationsByType`), parameter and type annotations, annotation identity caching,
the generic `Type` graph and Spring remain later slices. **M2.12 is done
(2026-11-24):** a `StringBuilder` and an `ArrayList` are dedicated heap objects
(`HeapObject::StringBuilder` and `HeapObject::List`) selected by their class identity, so
`getClass` and virtual dispatch report `java.lang.StringBuilder` and `java.util.ArrayList`,
and `Shared::roots` enumerates the references a list holds. The builder implements its
three constructors, the `String`/`Object`/`int`/`long`/`char`/`boolean`/`float`/`double`/
`CharSequence` `append` overloads, `toString`, `length`, `charAt`, `setLength`, `reverse`,
`deleteCharAt` and `insert(I,String)` over UTF-16 units; the list implements both
constructors, `size`, `isEmpty`, both `add` and `remove` forms, `get`, `set`, `clear`,
`contains` and `indexOf`, with reference-equality membership and typed bounds errors.
`append(Object)` uses the VM's `String.valueOf`-shaped conversion rather than dispatching
an overridden `toString`; `StringBuffer` and reflection changes remain later slices. **M2.13 is done (2026-11-24):** a `HashMap` is a dedicated heap object
(`HeapObject::Map`, insertion-ordered key/value pairs) selected by its class identity, so
`getClass` and virtual dispatch report `java.util.HashMap`, and `Shared::roots` enumerates
the keys and values it holds. The map implements both constructors, `size`, `isEmpty`,
`put`, `get`, `containsKey`, `containsValue`, `remove` and `clear`; `put` returns the
previous value, null keys and values are stored, a string key compares by content, a boxed
`Integer` key by value and every other key by reference identity, and a negative capacity
or a non-reference argument is a typed error. `String.valueOf` covers `Object`, `int`,
`long`, `boolean`, `char`, `float` and `double`: null renders `"null"`, a `String` returns
itself, a boxed `Integer` its decimal value, and any other object the VM's
`ClassName@hex` shape, with `valueOf(int)` identical to `Integer.toString(int)`. Iterators,
`entrySet`/`keySet`/`values`, `StringBuffer` and the remaining `HashMap` methods remain
later slices. **M2.14 is done (2026-11-24):**
`Class.getDeclaredField`/`getField`, `getDeclaredMethod`/`getMethod` and
`getDeclaredConstructor`/`getConstructor` resolve a member from the C1 metadata: a field
by name, a method or constructor by name plus the parameter descriptors built from a
`Class[]`, with inherited lookup through the superclass chain and interfaces and
declared-only constructor lookup. A hit is the cached reflected-member object, a miss is
null, and at M2.14 a primitive or array parameter class, a null `Class[]`, a null element
or a non-`Class` element was a typed error because those class objects did not exist yet;
M2.15 added the primitive class objects and M2.16 the array ones. Access-control
filtering, the generic `Type` graph and Spring remain later slices. **M2.15 is done
(2026-11-24):** a primitive class object is a
dedicated heap kind (`HeapObject::PrimitiveClass`) cached one per `PrimitiveKind` in
`Shared::primitive_class` and enumerated by `Shared::roots`, so `Class.forName("int")` and
`Integer.TYPE` return the same object. All nine primitives report the JDK name, the
`0x0411` modifiers (`public abstract final`, `void` included), `isPrimitive` true,
`isArray` false and a null superclass and component type, while `getClass()` on a
primitive class object reports `java.lang.Class`; a primitive declares no members, so the
declared-member arrays are empty and the named lookups are null misses.
`Field.getType`, `Method.getReturnType`/`getParameterTypes` and
`Constructor.getParameterTypes` return primitive class objects for primitive descriptors,
and a `Class[]` of primitive class objects builds the exact one-character descriptor for
`getDeclaredMethod`/`getMethod`/`getDeclaredConstructor`/`getConstructor`, so a reflected
method or constructor with an `int` parameter resolves and invokes. The nine wrapper
`TYPE` fields resolve virtually because the C1 stubs carry no initializer `Code`, while
every other wrapper static stays absent; the generic `Type` graph, annotation changes
and Spring remain later slices. **M2.16 is done (2026-11-24):** an array class is a
dedicated heap kind (`HeapObject::ArrayClass`) identified by its component (a named
class, a primitive or a nested array) and cached one per component in
`Shared::array_class`, with its own lock, no-lock probe entry and `Shared::roots` entries.
`Class.forName("[I")`, `"[Ljava.lang.String;"` and `"[[I"` return that cached object,
whose `getName` is the JDK descriptor form (`[I`, `[[I`, `[Ljava.lang.String;`),
`getModifiers` is `0x0411`, `isArray` is true, `isPrimitive` and `isInterface` are false,
`getComponentType` is the named, primitive or nested array component class object, and
`getSuperclass` is `java.lang.Object`. An array heap object carries the component its
allocation named, so `getClass()` on `new int[n]` and `new String[n]` reports the cached
array class. `Field.getType`, `Method.getReturnType`/`getParameterTypes` and
`Constructor.getParameterTypes` resolve array descriptors, and
`getDeclaredMethod`/`getMethod`/`getDeclaredConstructor`/`getConstructor` accept array
class objects in their `Class[]`, so an array parameter resolves by its exact descriptor;
a miss is still null. An array class declares no members, and a malformed array
descriptor or missing component is a typed error. **M2.17 is done (2026-11-24):** `ldc`
and `ldc_w` decode a `CONSTANT_Class` and push the cached class object, so
`String.class`, `int.class`, `int[].class` and `String[].class` are the same objects
`Class.forName` and the class, primitive and array caches already own. A named class
resolves through the executing class's loader (a child loader's own class keeps its
identity, while a library class still delegates to the bootstrap loader), a primitive
name through the primitive-class cache and an array descriptor through the array-class
cache, without initializing the class. `ldc` of a `String` still interns, `ldc_w`'s
two-byte index is supported, and a numeric constant or any other constant kind is a
typed error; a missing class and a malformed array descriptor are typed errors too.
`MethodType`/`MethodHandle` constants, annotation changes and Spring remain later
slices. **M2.18 is done (2026-11-24):** `invokedynamic` decodes
opcode `0xba` and resolves its `BootstrapMethods` entry into a typed `REF_invokeStatic`
handle plus `String`/`Class`/`int`/`long`/`float`/`double` static arguments, with typed
decode errors for a malformed, non-static or out-of-range bootstrap reference.
`StringConcatFactory.makeConcat` and `makeConcatWithConstants` build the result from the
call-site arguments and the recipe (`\u0001` dynamic argument, `\u0002` recipe constant),
convert every value through the M2.9/M2.13 `String.valueOf` rules, and push an interned
String; any other bootstrap is a typed error. A committed `javac --release 17` fixture
covers the compiler-emitted call sites, and assembled probes cover `makeConcat`, every
supported primitive descriptor, static recipe constants and the documented failures.
**M2.19 is done (2026-11-24):** `invokeinterface` decodes opcode `0xb9` and runs the
interface method on the receiver's runtime class or on an interface default, with the
native registry before `Code`; a wrong receiver is a typed error. `LambdaMetafactory`'s
`metafactory` and `altMetafactory` bootstraps now create a lambda heap object carrying the
functional interface, the SAM name/type, the implementation target and the captured
call-site values, and invoking the SAM through `invokeinterface` runs that target through
the existing static/instance machinery. Static, unbound, bound and constructor method
references, a capturing lambda and a no-argument void `Runnable` are covered by a committed
`javac --release 17` fixture plus assembled probes; a lambda's class identity is its
interface, `altMetafactory` rejects serializable lambdas, and argument adaptation,
`MethodHandle`/`VarHandle` execution, call-site caching and Spring remain later slices.
**M2.20 is done (2026-11-24):** `rava run <MainClass> --stdlib <dir> --cp <path-list>`
executes a class's static `main([Ljava/lang/String;)V` from a directory or jar
classpath. A `rava-vm` `ClassPathSource` searches directories and jars in order through
the existing `DirectorySource`/`JarSource`, one application loader is parented to the
bootstrap loader in the existing `(loader, name)` class table, and a single
`rava-vm::library::register_all` entry point installs the existing natives plus a
minimal `System.out`/`System.err` and `PrintStream`. The runner validates the entry
point, allocates the `String[]` with the heap's array machinery, invokes `main` through
the interpreter, flushes the captured stdout, and exits 0 on success, 1 on a runtime
error and 2 on a usage error. Spring does not start yet.
**M2.21 is done (2026-11-24):** `putstatic` resolves the owner through the executing
class's loader, stores the value in the declaring class's `statics` table keyed by
`(name, descriptor)`, and rejects an instance field, a missing field, a wrong descriptor
and a value of the wrong kind with typed errors. `checkcast` and `instanceof` resolve a
named, primitive or array class operand through the same loader, use
superclass/interface/array-covariance assignability (including the
`Object`/`Cloneable`/`Serializable` array supertypes), treat null as an instance of every
reference type and a primitive as matching no reference value, and report a failed cast
as a typed `ClassCast` error instead of synthesizing `ClassCastException`. `tableswitch`
and `lookupswitch` map their default and case targets to instruction indices at decode
time and reject non-zero padding, non-boundary targets, inverted/truncated tables and
unsorted or duplicate lookup keys. `ldc`/`ldc_w` now decode Integer and Float constants
and `ldc2_w` Long/Double, with `lcmp`/`fcmp<op>`/`dcmp<op>` and
`lreturn`/`freturn`/`dreturn` returning and comparing them; the existing one-slot local
model is unchanged. The `BytecodeProbe` fixture and its malformed-switch methods are
assembled in Rust. **M2.22 is done (2026-11-24):** the JVMS `Signature` grammar is parsed
by `rava-classfile` into a typed class/method/field model that keeps type-parameter names,
class and interface bounds, nested owners, wildcard indicators and arrays, and reports a
malformed signature as a structured error. `rava-vm` resolves that graph through the
declaring class's loader: a raw type is the existing `Class` object, while
`ParameterizedType`, `TypeVariable`, `WildcardType` and `GenericArrayType` are cached
`HeapObject::Type` objects and GC roots. `Class.getGenericSuperclass`,
`getGenericInterfaces` and `getTypeParameters`, `Field.getGenericType`,
`Method.getGenericReturnType`/`getGenericParameterTypes`/`getTypeParameters`, and
`Constructor.getGenericParameterTypes`/`getTypeParameters` expose the graph, with the
`java.lang.reflect` interface accessors and deterministic type names. A committed
`javac --release 17` fixture (`Box<T extends Number> implements Comparable<Box<T>>` with
`List<String>`, `T[]`, `? extends Number`, `? super Integer` and `Map.Entry`, a generic
method and generic constructors) drives the APIs through `invokevirtual`/`invokeinterface`.
`getGenericExceptionTypes`, record/sealed views, annotation changes and Spring remain
later slices. **M2.24 is done (2026-11-24):** `java.util.stream` is an eager pipeline over
new heap representations — `HeapObject::Stream`, `Spliterator`, `Collector`, `Optional`
and an internal `Consumer` sink — whose `Shared::roots` entries cover every captured
reference. `StreamSupport.stream` drains a VM spliterator directly and a Java spliterator
through its own `tryAdvance` with a native consumer, so Spring's
`AnnotationsSpliterator` runs unmodified. `filter`, `map`, `flatMap`, `distinct`,
`sorted` (natural and comparator), `peek`, `limit`, `skip`, `forEach`, `toArray`,
`collect`, `count`, `anyMatch`/`allMatch`/`noneMatch`, `findFirst`/`findAny`, `min`/`max`,
`reduce`, `toList` and `iterator` are implemented, with `Collectors.toList`/`toSet`/
`toCollection`/`joining`/`counting`/`toMap`, `Collector.of`, `Collection.stream`/
`parallelStream`/`spliterator`/`removeIf`, the `Spliterator`/`Spliterators` surface,
`Optional` and `Map.forEach`. Every callback runs through the interpreter and a throwable
raised by one crosses the native ABI as a thrown object. The pipeline is eager, so there
are no lazy stages, no parallel execution, no short-circuiting source and no close action.
A committed `javac --release 17` fixture (`demo.StreamProbe` plus a real Java
`WordSpliterator`) covers the operations, a capturing lambda and a caught and an uncaught
throwing callback. **M2.25 is done (2026-11-24):** all eight primitive wrappers are heap
boxes with the JDK conversion rules. `Byte`, `Short`, `Long`, `Float` and `Double` join
`Integer`/`Character`/`Boolean`, each with `valueOf`, its `Number` accessors, `parseXxx`,
both `toString` forms, `equals`, `hashCode` and `compareTo`/`compare`; `Float`/`Double`
use canonical-bit equality and the JDK total order (`NaN` equals `NaN`, `0.0` sorts above
`-0.0`). Boxed text is shared by `String.valueOf`, `Object.toString`,
`StringBuilder.append` and `Arrays.toString`, boxed keys compare per wrapper kind, and
`Collectors.counting()` now boxes a `Long`. Reflective `Field.get`/`set`,
`Method.invoke` and `Constructor.newInstance` box and unbox every primitive descriptor
with the JLS widening rule. A malformed `parseXxx` is a typed error; a hex float literal is
rejected and the case-insensitive `inf`/`nan` spellings are accepted where Java's grammar
is not, which are the documented parser gaps. A committed `BoxProbe` fixture
covers all eight kinds (`cargo test -p rava-interp --test primitive_boxes`). **M2.23 is
blocked (2026-11-24):** the committed `demo.SpringSmoke` fixture and
`crates/cli/tests/spring_context.rs` start a real Spring
`AnnotationConfigApplicationContext` and assert the `hello` bean, but the local test still
fails — now because the interpreter pushes 1 for `null instanceof LocalResourceCache`
where JVMS §6.5 requires 0, so `CachingMetadataReaderFactory.setCacheLimit(int)` takes
the wrong branch and hands a null to `checkcast`/`invokevirtual`, failing with `operand
has the wrong type`. With that corrected locally the next blocker is
`java.util.LinkedHashMap(int, float, boolean)`, whose C1 stub has neither code nor a
native. The committed M2.21 test `instanceof_handles_null_and_primitive_operands`
asserts the same wrong `instanceof` result and has to be corrected with the fix. The startup chain before that point is in place: monitors, the missing numeric
instructions, collections, iterators, logging, concurrency natives, exception
construction, reflection, the primitive-box step and the stream pipeline. The M2 gate is
not claimed.

## What this project is

rava is a compatibility-first Java runtime. Its success criterion is a pinned Spring
Boot application running unmodified on the reference JDK's jars: linked, reflected
over, dynamically subclassed, served over HTTP, and measured against a performance
budget.

The runtime class library is ours: `java.*` classes are real class files with Rust
implementations, and the real `java.base` is never loaded at run time. "The JCL is
Rust" is a property of the design, not a slogan.

The scope is not "all of Java SE". It is an observed surface manifest: the classes,
members, annotations and signatures the target actually reaches, versioned with the
target and enforced by C0–C4 gates.

## Target

| Item | Value |
| :--- | :--- |
| Platform | Java SE 17 API subset, class-file version 61 |
| Spring Framework | 6.2.19 |
| Spring Boot | 3.5.16 |
| Embedded server | Tomcat 10.1.55 |
| Reference application | servlet web app, `text/plain` endpoint, no JSON, no database |
| Reference JDK | 17 |

The Spring, Boot and Tomcat versions are pinned in `compat/targets.toml`, together
with the reference application's source hash; `compat/jars.lock` pins the sha256 of
every jar in the flat `compat/jars/` set. The jars are fetched, not committed.
Reactive web (Spring WebFlux) is a later target, not part of the first acceptance.

## Documents

- [Requirements](docs/requirements.md) — R1–R13, C0–C4, milestones, performance,
  scope ordering.
- [Open questions](docs/open-questions.md) — the resolved decision record.
- [ADR index](docs/adr/README.md) — the accepted decision stack.
- [Project ledger](PROJECT.md) — phase status, milestones, decision log.

Start with the requirements and the ADRs; the open questions are all resolved and
remain as a decision record.

## What this project is not

- Not a full Java SE implementation in the first target.
- JNI and native libraries are not in the first target; they are a later target (Q17).
- Not a reactive runtime yet; WebFlux is a committed later target.

## Development

The Rust toolchain is pinned by `rust-toolchain.toml`; the local gate is:

```sh
cargo xtask ci
```

A class runs from a directory or jar classpath with:

```sh
cargo run -p rava-cli -- run demo.HelloRun --stdlib stdlib/c1/classes --cp crates/cli/tests/fixtures/run/classes
```

The same class is committed as a stored jar, so
`--cp crates/cli/tests/fixtures/run/HelloRun.jar` is the jar form; `--cp` is repeatable
and colon-separated. The runner's tests need no JDK:

```sh
cargo test -p rava-cli --test classpath_run
```

The pinned target and jar set are checked with:

```sh
cargo run -p rava-cli -- compat target
bash compat/fetch-jars.sh --check
```

The static surface manifest is generated and checked with:

```sh
cargo run -p rava-cli -- compat manifest
cargo run -p rava-cli -- compat manifest --check
```

The signature-only stub library is generated and link-checked with:

```sh
cargo run -p rava-cli -- compat stubs
cargo run -p rava-cli -- compat link-check
```

The local M1 compatibility gate runs all four stages and prints `compat check: ok`
only when they pass:

```sh
cargo run -p rava-cli -- compat check
```

It reads the already-fetched `compat/jars/` (the manifest stage scans the pinned
class files) but needs no network. It does not re-hash the jars; that stays with
`bash compat/fetch-jars.sh --check`.

The C1 metadata model and its normalized dump are tested against committed Java 17
fixtures (no JDK needed at test time):

```sh
cargo test -p rava-classfile --test metadata_model
```

The JDK 17 C1 signature stubs are generated from the host `ct.sym` and checked against
it. The reference is found through `--ct-sym <path>`, `RAVA_CT_SYM`, or `JAVA_HOME`;
the local acceptance uses the JDK 25 archive at
`$JAVA_HOME/lib/ct.sym` (for example
`/home/yusiwen/.local/share/mise/installs/java/zulu-25/lib/ct.sym`). The check needs no
jars and no execution, and the test suite needs no JDK:

```sh
cargo run -p rava-cli -- compat c1-stubs
cargo run -p rava-cli -- compat metadata --check
```

The bootstrap loader reads the committed C1 tree through a `DirectorySource`; the
runtime loading test needs no JDK:

```sh
cargo test -p rava-vm --test c1_runtime
```

`stdlib/c1/classes/` is the committed C1 class library the bootstrap loader reads;
`stdlib/classes/` is the generated M1 surface-stub deliverable, not the runtime library.
See [`stdlib/README.md`](stdlib/README.md) and
[`stdlib/c1/README.md`](stdlib/c1/README.md) for what each tree does and does not
contain.

`bash compat/fetch-jars.sh` (re)fetches the jars from Maven Central and needs
`JAVA_HOME` and Maven; `--check` verifies the existing directory without fetching.

## License

MIT.

## Review order

1. Read the requirements.
2. Read the resolved decision record.
3. Read the ADRs in order; 0001–0005 define the project, 0006–0011 the platform
   surfaces.
