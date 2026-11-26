# M2.23–M2.30 slice detail — the Spring context and its blocker chain

Per-slice record for M2.23–M2.30. `PROJECT.md` § Slices owns the status and the one-line
description; this file owns what each slice delivered, where it stops and the evidence
behind it. Every entry is recorded from the slice's own acceptance run on 2026-11-24.

## M2.23 — Spring `AnnotationConfigApplicationContext` bean graph — 2026-11-24

**Acceptance.** `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` — exit 0.

**Delivered.** The committed `demo.SpringSmoke` fixture (`javac --release 17`, `proxyBeanMethods = false`) starts a real Spring `AnnotationConfigApplicationContext` against the pinned Spring 6.2.19 jars, reads the `String` bean named `hello` and closes the context through try-with-resources; the acceptance prints exactly `hello\n`, exits 0 and does not skip (the jars are present). The chain that this slice and M2.24–M2.30 completed runs through class loading, `java.util.logging`, annotation metadata and reflection, the primitive-box step, the eager stream pipeline, `CachingMetadataReaderFactory`, `computeIfAbsent`, message formatting, the typed lookup misses, and the shutdown path (lifecycle groups, `EventObject`, enum constants and `TimeZone`/`TimeUnit` statics).

**Boundary.** The committed `crates/cli/tests/fixtures/spring/` fixture
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

## M2.24 — `java.util.stream` pipeline for Spring's annotation metadata — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test streams` — exit 0.

**Delivered.** `rava-vm` gains `HeapObject::Stream(StreamObject)`, `Spliterator(SpliteratorObject)`, `Collector(CollectorObject)` with a `CollectorKind`, `Optional(Option<Value>)` and the internal `Sink(SinkObject)` consumer; each allocator/accessor is ordinary heap state and `Shared::roots` enumerates stream and spliterator elements, a collector's supplier/key/value/accumulator/combiner/finisher callbacks, an optional's value and a sink's destination, while `Shared::get_class`/`Object.toString` report `java.util.stream.Stream`, `java.util.Spliterator`, `java.util.stream.Collector`, `java.util.Optional` and `java.util.function.Consumer`. `rava-interp::streams` is the eager pipeline: `StreamSupport.stream` drains a VM spliterator directly and a Java spliterator through its own `tryAdvance` with a native consumer that appends to the stream, so Spring's `AnnotationsSpliterator` runs unmodified; `Stream.empty/of/ofNullable/concat`, `filter`, `map`, `flatMap`, `distinct`, `sorted()` (natural order over `Integer`/`Long`/`Float`/`Double`/`String`) and `sorted(Comparator)`, `peek`, `limit`, `skip`, `forEach`, `forEachOrdered`, `toArray` (both forms), `collect(Collector)` and `collect(Supplier,BiConsumer,BiConsumer)`, `count`, `anyMatch`/`allMatch`/`noneMatch`, `findFirst`/`findAny`, `min`/`max`, `reduce`, `toList` and `iterator`; `BaseStream.isParallel`/`sequential`/`parallel`/`unordered`/`onClose`/`close`; `Collection.stream`/`parallelStream`/`spliterator`/`removeIf`; `Spliterator.tryAdvance`/`forEachRemaining`/`trySplit`/`estimateSize`/`getExactSizeIfKnown`/`characteristics`/`hasCharacteristics`/`getComparator`; `Spliterators.emptySpliterator`/`spliterator(Object[],...)`/`spliterator(Collection,int)`/`iterator`; `Collectors.toList`/`toSet`/`toUnmodifiableList`/`toUnmodifiableSet`/`toCollection`/`joining()`/`joining(CharSequence)`/`joining(delim,prefix,suffix)`/`counting`/`toMap` and `Collector.of` in its four- and five-argument forms; `Optional.empty/of/ofNullable/get/isPresent/isEmpty/orElse/orElseGet/orElseThrow/ifPresent/map/filter/toString`; and `Map.forEach(BiConsumer)`. Every callback is invoked the way the rest of the interpreter invokes one — a `LambdaObject` through `lambda::invoke_sam`, any other object through its own SAM on the receiver's superclass chain — and a throwable raised by a callback crosses the native ABI as `VmError::Thrown` rather than being swallowed. An instance-target method reference now dispatches on the receiver's runtime class (then the target owner's interfaces), which is what Spring's `MergedAnnotation::isDirectlyPresent` predicate needs. The chain also exposed and fixed: `Map.of` up to ten pairs, the full map surface on `LinkedHashMap` and `Properties`, concrete `toArray` overrides, `Collections.newSetFromMap`/`emptyIterator`, `Enum.equals`/`hashCode`/`getDeclaringClass`, `Objects.requireNonNull`/`isNull`/`nonNull`/`equals`/`hashCode`/`toString`, `boolean`/`char` (and `byte`/`short` as `Integer`) results in reflective boxing, and superclass walks that now use `Shared::resolution_superclass` so the library's deliberate `AbstractStringBuilder`/`Dictionary` omissions do not break assignability. The committed `crates/interp/tests/fixtures/streams/` fixture (`demo.StreamProbe` plus a real Java `WordSpliterator`, `javac --release 17`) drives filter/map/collect toList and toSet, joining, sorted/limit, a capturing lambda, the VM spliterator surface, a Java spliterator, `Collector.of` with identity and real finishers, and a caught and an uncaught throwing callback; 13 tests; `cargo test -p rava-interp` 191, `rava-vm` 50 and `rava-cli` 78 pass, workspace 375; `cargo xtask ci` exits 0. The Spring rerun now passes the stream calls and stops at `java/lang/Byte.valueOf(B)Ljava/lang/Byte;`, the primitive-box expansion the startup path needs; M2.23 stays blocked.

**Boundary.** The stream pipeline is **eager**. A `Stream` holds its whole element list, and `filter`/`map`/`flatMap`/`distinct`/`sorted`/`peek`/`limit`/`skip`/`sequential`/`parallel`/`unordered` each allocate a new stream with a computed vector; there is no lazy stage graph, no short-circuiting until a terminal operation, no `Spliterator`-backed stream after construction, no parallel execution (the `boolean` of `StreamSupport.stream` is recorded and `isParallel` reports it) and no close action (`onClose` returns the stream, `close` does nothing). `sorted()` compares `Integer`/`Long`/`Float`/`Double` numerically and `String` by UTF-16 code unit; any other element or a null is a typed error rather than a synthesized `ClassCastException`/`NullPointerException`. `distinct` and `toSet` compare strings by content, boxed primitives by per-kind value equality and everything else by reference identity; `Collectors.toMap` appends without the JDK's duplicate-key `IllegalStateException`, `toList`/`toUnmodifiableList` return an ordinary modifiable heap list (there is no unmodifiable wrapper), `toUnmodifiableSet` is the ordinary linked set, and `Collectors.counting()` boxes a `Long` (M2.25 replaced the temporary `Integer` count that stood in before the primitive-box family existed). `joining` renders a non-null element through the VM's `String.valueOf` shape, so a non-`CharSequence` is accepted where the JDK would reject it. A VM `Spliterator` is a snapshot plus a cursor with `ORDERED|SIZED|SUBSIZED` (`DISTINCT` added for a set); `trySplit` returns null and `getComparator` null. A Java object implementing `Spliterator` is drained through its own `tryAdvance` and a native `Consumer` sink, so a Java default `forEachRemaining` is not executed by the VM; Spring's and the fixture's spliterators implement the abstract `tryAdvance`, which is the shape this slice supports. `Optional.empty()` allocates a fresh object rather than the JDK singleton, and a present value is a `Value`, so `Optional` identity is not the JDK's. `Collection.stream`/`spliterator` snapshot the collection, so a later mutation is not observed. `removeIf` and `Map.forEach` are the two callback-taking collection operations added here; `compute`/`merge`/`removeIf`'s concurrent variants and the primitive `IntStream`/`LongStream`/`DoubleStream` pipelines are out of scope. `java.util.stream` `gather`, `mapMulti`, `takeWhile`/`dropWhile`, `reduce` with three arguments, `Collectors.groupingBy`/`partitioningBy`/`teeing`/`summarizing*` and `Spliterator.OfInt`/`OfLong`/`OfDouble` are not implemented and fail loudly. No Spring behavior is faked: the acceptance fixture and test are unchanged, and the remaining blocker is recorded above.

## M2.25 — Full primitive-box family and Number accessors — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test primitive_boxes` — exit 0.

**Delivered.** `HeapObject::Boxed(BoxedPrimitive)` now carries all eight primitives, and `BoxedPrimitive` implements the Java conversion rules: `byteValue`/`shortValue`/`intValue` narrow through the low bits (a `float`/`double` saturates through `int`, NaN becomes 0), `longValue`/`floatValue`/`doubleValue` widen (a `long` to `float` rounds), `equals`/`hashCode` use canonical `Float`/`Double` bits, and `compareTo` is the JDK total order. `Shared` gains `boxed_value`/`boxed`/`box_long`/`box_double`, and `Heap` gains one boxed allocator per kind. The wrapper registry covers `valueOf` (primitive, `String`, `String`+radix), the six `Number` accessors, `parseXxx` (decimal and radix), both `toString` forms, `equals`, `hashCode` (instance and static), `compareTo`/`compare`, the `Float`/`Double` `isNaN`/`isInfinite`/`isFinite`/bit methods, `Long.toHexString`, the `Character` comparison surface and `Boolean.valueOf(String)`/`hashCode`/`compare`/`getBoolean`; the `Number` accessors are also registered on `java.lang.Number`, so a call that resolves to the abstract superclass still converts. A malformed or out-of-range `parseXxx` is a typed `VmError::Native`; the documented syntax gaps are a rejected hex floating literal and an accepted case-insensitive `inf`/`nan` spelling, and the float/double text keeps the M2.9 plain-decimal boundary. `String.valueOf`, `Object.toString`, `StringBuilder.append`, stream `joining` and `Arrays.toString` render every box through `BoxedPrimitive::text`; boxed equality/hashing is shared by `Objects`, maps, callbacks, arrays and streams, and `Collectors.counting()` now boxes a `Long`. Reflective adaptation covers every descriptor: `box_result` returns each primitive's own wrapper, `unbox_argument` applies the JLS §5.3 widening rule (a `Character` widens as its code unit, a wrong or wider box is typed), so `Field.get`/`set`, `Method.invoke`, `Constructor.newInstance` and `Method.getDefaultValue` work for all primitives. The assembled `BoxProbe` fixture covers valueOf, the accessors, parse, text, equals/hash/compare, field get/set, method invoke, constructor arguments and `String.valueOf` for all eight kinds; 11 tests; `cargo test -p rava-interp` 202, `rava-vm` 50, `rava-cli` 78 and `rava-classfile` 56 pass, workspace 386; `cargo xtask ci` exits 0.

**Boundary.** Every primitive wrapper is a `BoxedPrimitive` variant, so all eight
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

## M2.26 — `instanceof(null)` correctness and map constructor surface — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test spring_fixes` — exit 0.

**Delivered.** `instanceof` of a null operand now pushes 0 for a named class, an interface and an array (JVMS §6.5) instead of 1, and the committed M2.21 test `instanceof_handles_null_and_primitive_operands` was corrected to expect 0; null still passes every reference `checkcast`. `java/util/LinkedHashMap.<init>(IFZ)V` exists and its `accessOrder` flag is honoured: `get`/`getOrDefault`/`put` (existing key)/`putIfAbsent` (existing key) move the entry to the end of `MapObject::entries`, which `keySet`/`values`/`entrySet` and iteration observe; `(I)V`, `(IF)V` and `HashMap(IF)V` keep insertion order, and a negative capacity or a non-positive/NaN load factor is a typed error. The registry's map constructors were corrected from the non-existent `(ILF)V` to the JDK's `(IF)V` on every map surface. The Spring rerun then exposed and this slice fixed: a heap-lock re-entry deadlock in `computeIfAbsent` (`keys_equal` locked the heap while the caller held it), `Class.getDeclaredClasses` over the `InnerClasses` metadata (member rows only; a member outside the C1 subset is skipped), `Comparator.comparingInt` over the new `LambdaTargetKind::Native` lambda target, `Comparator.reversed`, and non-SAM default interface calls on a lambda (`invokeinterface` on a `Comparator` lambda now falls through to a concrete declaration instead of the SAM-mismatch error). The new `crates/interp/tests/spring_fixes.rs` has 9 tests; `cargo test -p rava-interp` 211, `rava-vm` 50, `rava-cli` 78 and `rava-classfile` 56 pass, workspace 395; `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` and `cargo xtask ci` exit 0. The Spring acceptance still exits 1, now at `error: runtime error: class java/lang/annotation/ElementType not found from loader 0`; the M2.23 boundary records the root cause.

**Boundary.** `instanceof` of a null reference pushes 0 for a named class, an interface
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

## M2.27 — Descriptor-closure manifest and C1-tree re-baseline — 2026-11-24

**Acceptance.** `cargo run -p rava-cli -- compat check` — exit 0.

**Delivered.** The M1.2 scanner now records every platform class named by a field or method descriptor, a class/method/field `Signature` (type-parameter bounds, type arguments, wildcard bounds, nested `Outer<T>.Inner<U>` owners and `^` throws) and an annotation's encoded type descriptors (the annotation type, an enum constant's type and a class literal), with arrays unwrapped. The re-baselined manifest has 1047 classes (67 more: 17 named only by a declared descriptor or `Signature`, 36 named only by a member-reference descriptor and 14 named only by an annotation), 177 fields, 3,626 methods and 1,048 interface methods; `compat/surface/c1-unreferenced.txt` grows from 28 to 36 owners (the JSR-305 `javax.annotation` qualifiers, `jdk.internal.vm.annotation.IntrinsicCandidate`, JCache's `Cache` and Moneta's `NumberValue`); `stdlib/classes` and `stdlib/c1/classes` each hold 1047 regenerated class files (1011 projected from ct.sym, 1,160 fields and 13,359 methods); `compat manifest --check`, `compat check` (link-check 1047 classes and 4,851 member records with `unresolved 0`; stub determinism 1047 files) and `compat metadata --check` (0 differences across 1011 classes) exit 0; `cargo test -p rava-cli` passes 85 tests and `cargo xtask ci` passes 402 workspace tests. The Spring rerun gets past `ElementType` and stops at `error: method java/lang/reflect/Method.isBridge()Z has no bytecode`; M2.23 stays blocked.

**Boundary.** The manifest's descriptor closure is static and jar-only. It reads
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

## M2.28 — Method/Executable bridge, varargs and synthetic flags — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test method_flags` — exit 0.

**Delivered.** `rava-vm`'s reflection natives now answer `Method.isBridge()Z`, `Method`/`Constructor`/`Executable.isVarArgs()Z` and `Field`/`Method`/`Constructor`/`Executable`/`Member.isSynthetic()Z` from the parsed class-file `ACC_BRIDGE`/`ACC_VARARGS`/`ACC_SYNTHETIC` bits, never by inference; a member absent from its class file is a typed `VmError::Native` instead of a fabricated flag. `getModifiers()` keeps the JDK masks: a method's includes `bridge`, `varargs` and `synthetic` (`0x1DFF`, the JDK's `Modifier.methodModifiers()` mask), a field's includes `synthetic` and `enum`, a constructor's is the three access bits, so `Constructor.isVarArgs()` reports the `0x0080` its `getModifiers()` hides. The Spring rerun then exposed and this slice fixed `Set(Collection)` constructor deduplication on `LinkedHashSet`/`HashSet`/`TreeSet`/`CopyOnWriteArraySet` (a repeated element, `null` included, contributes once in first-occurrence order), the `(Collection)` copy constructor on `ArrayList`, `LinkedList`, `ArrayDeque` and `CopyOnWriteArrayList`, `removeAll`/`retainAll`/`containsAll` over the heap lists and sets on every declaration site a virtual call reaches (`AbstractCollection`, `AbstractSet`, the `Collection`/`List`/`Set` interfaces, the concrete collections and `ConcurrentHashMap$KeySetView`), and callback dispatch that resolves `accept`/`apply`/`compare`/`applyAsInt` up the receiver's superclass chain, which is what Spring's `AnnotationAwareOrderComparator` needs for its inherited `OrderComparator.compare`. The committed `crates/interp/tests/fixtures/method_flags/` fixture (`demo.FlagsProbe`, `javac --release 17`) pins the exact flags of a bridge method (`0x1041`), a varargs method (`0x0089`), a varargs constructor (`0x0081`), a synthetic lambda method (`0x1002`), the enum `$VALUES` field (`0x101A`) and `$values()` (`0x100A`), plus the discovery loops, the `Executable`/`Member` declarations, a missing-declaration typed error, a lookup miss that stayed null until M2.30, and the collection/callback fixes; 10 tests; `cargo test -p rava-interp` 221, `rava-vm` 50, `rava-classfile` 56 and `rava-cli` 85 pass, workspace 412; `cargo test -p rava-interp --test method_flags` exits 0. The Spring acceptance still exits 1, now at `error: method java/text/MessageFormat.<init>(Ljava/lang/String;)V has no bytecode` (Spring's `AbstractMessageSource` static initializer builds `new MessageFormat("")`); M2.23 stays blocked.

**Boundary.** The reflected-member predicate flags read the parsed class file's
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

## M2.29 — Minimal `java.text.MessageFormat` for Spring messages — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test message_format` — exit 0.

**Delivered.** `rava-vm::format` adds `HeapObject::MessageFormat` (the pattern text in a dedicated heap shape, since the C1 stub declares no pattern field), `HeapObject::StringBuffer` and `Shared::allocate_instance` selection for both; the natives are `<init>(String)`, `<init>(String, Locale)` (locale accepted and ignored), `applyPattern`, `toPattern`, the inherited `Format.format(Object)` Spring calls, both `format(..., StringBuffer, FieldPosition)` forms and the static `format(String, Object...)`. The parser supports literal text, `{index}`, repeated indices, the JDK's single-quote rules (`''`, quote toggling) and the JDK's literal `{index}` for an absent argument; a null argument renders `"null"`, and `number`/`date`/`time`/`choice` subformats are typed errors once a non-null argument reaches them. A null pattern and a malformed pattern are typed errors (no exception synthesis), and an argument renders through the VM's `String.valueOf` shape rather than a virtual Java `toString`. `StringBuffer` gets both constructors, `append(String)`, `toString` and `length`. The Spring rerun then exposed and this slice fixed four more concrete gaps: the `AccessibleObject` annotation queries (`getAnnotation`/`isAnnotationPresent`/`getDeclaredAnnotations`) that virtual resolution finds on that owner, `AccessibleObject.isAccessible`/`setAccessible` with a real per-member flag, `List.addAll(int, Collection)` on its four declaration sites, and the C1 tree's omitted transitive interfaces (`java.lang.constant.Constable` on `String`) in `Class.getInterfaces` and the inherited-lookup walk, skipped with the same rule M2.19 already used. The new `crates/interp/tests/message_format.rs` has 11 tests through the interpreter (`new`/`invokespecial`/`invokevirtual`/`invokestatic`), including the simple, repeated, missing, quoted, null-argument, null-pattern, static-format and `StringBuffer` forms and a typed subformat error; `cargo test -p rava-interp` passes, `cargo test -p rava-vm` and `cargo test -p rava-cli` pass, `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` exit 0. The Spring acceptance now stops at `Class.getMethod`'s null miss: `Class.getMethod("close", new Class[0])` returns `null` instead of throwing `NoSuchMethodException`, so `DisposableBeanAdapter.inferDestroyMethodsIfNecessary` calls `Method.getName` on null and the CLI reports `error: operand has the wrong type`; M2.23 stays blocked.

**Boundary.** a `java.text.MessageFormat` is `HeapObject::MessageFormat`, a dedicated heap
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

## M2.30 — `NoSuchMethodException`/`NoSuchFieldException` lookup misses — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test lookup_exceptions` — exit 0.

**Delivered.** The six named lookups now throw the JDK's exceptions instead of returning null: `getDeclaredField`/`getField` raise `java.lang.NoSuchFieldException`, `getDeclaredMethod`/`getMethod`/`getDeclaredConstructor`/`getConstructor` raise `java.lang.NoSuchMethodException`. `rava_vm::exceptions::new_throwable` allocates the object through the library class table, records the detail message in the same side table the Throwable constructors use, and returns `VmError::Thrown`, so the interpreter's handler search catches it exactly like a compiled throwable. Messages name the receiver class, member and parameter descriptors (`demo.LookupMissProbe.missing(I, Ljava/lang/String;)`, constructors as `<init>`); a primitive or array receiver throws too, and `getDeclaredClasses`/`getClasses` stay empty arrays (M2.30 also implements the JDK `getClasses` superclass/public view). The committed `demo.LookupMissProbe` fixture covers all six lookups through compiled `catch` blocks plus uncaught misses at the native boundary, with 6 tests; the M2.14/M2.15/M2.16/M2.28 tests that pinned the null miss now assert the typed exception and its message. The Spring rerun then exposed and fixed the shutdown-path gaps: `Map.replaceAll`, `Map.forEach`, `AbstractMap.isEmpty`, `Collection.removeIf` over a Java view's own iterator (Spring's `ConcurrentReferenceHashMap$KeySet`), `TreeMap(Comparator)`, `Comparator.naturalOrder`/`reverseOrder`, `Record.<init>`, `EventObject.<init>(Object)`/`getSource`, and enum constant static fields. `cargo test -p rava-interp --test lookup_exceptions` exits 0 with 6 tests; the workspace passes 429 tests; `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` exit 0; the Spring acceptance exits 0 with `hello\n`, so M2.23 and the M2 gate are done.

**Boundary.** a named `Class` lookup that finds no member throws a real Java
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
