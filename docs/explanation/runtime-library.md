# Runtime library: values, text and collections

## Question

How does the runtime library represent a string, a boxed primitive, a list, a map, an
annotation, a stream and a message pattern — and where does each representation stop?

## Entry points

- Value shapes: [`crates/vm/src/heap.rs`](../../crates/vm/src/heap.rs) and the allocator and
  caches in [`crates/vm/src/shared.rs`](../../crates/vm/src/shared.rs).
- Native bodies: [`string.rs`](../../crates/vm/src/string.rs),
  [`boxed.rs`](../../crates/vm/src/boxed.rs),
  [`collections.rs`](../../crates/vm/src/collections.rs),
  [`format.rs`](../../crates/vm/src/format.rs) and
  [`crates/interp/src/streams.rs`](../../crates/interp/src/streams.rs).
- The registry is consulted before `Code`, which is why these natives run against C1 stubs
  that have no body ([`class-loading-and-libraries.md`](class-loading-and-libraries.md) §6).

## 1. A library type is a heap shape, not an `Instance`

A type whose Java fields the runtime cannot use gets a dedicated `HeapObject` variant —
`Str`, `Boxed`, `StringBuilder`, `List`, `Map`, `Annotation`, `Enum`, `Lambda`, `Stream` —
and `Shared::allocate_instance` selects it from the class identity, so `new ArrayList<>()`
and a reflective allocation agree. Such a shape is not an `Instance`: instance-field opcodes
and a reflected `Field.get`/`set` do not apply to it. `Shared::roots` enumerates every
reference a shape holds, so the retention is visible; without a collector an element of an
otherwise unreachable collection is over-retained rather than traced.

## 2. Strings

`HeapObject::Str` holds valid UTF-8 text, and the library operations count UTF-16 units:
`length`, `charAt`, `substring`, `indexOf`, `compareTo`, `hashCode` and `toCharArray`
index code units, so a supplementary character is two units. A lone surrogate cannot be
stored in a Rust `String`, so an operation that would leave one (`substring` splitting a
pair, a `replace` rewriting half of it) raises `VmError::UnpairedSurrogate` where the JDK
returns a string holding the lone unit.

The C1 tree does not ship `java.lang.AbstractStringBuilder`, so
`Shared::resolution_superclass` ends that one chain at `Object` instead of failing the
superclass walk, for `StringBuilder` and `StringBuffer` alike.

`Shared::intern_string` is a permanent content-keyed table, and `ldc` goes through it, so
two executions of one constant see the same object. `Object.equals` is reference identity,
`Object.hashCode` is a deterministic mix of the `ObjRef` index and generation, and
`Object.toString` prints `ClassName@hex`.

## 3. Boxed primitives

All eight primitives are `HeapObject::Boxed(BoxedPrimitive)`, i.e. heap objects that can
live in an `Object` slot. Every `valueOf` allocates a fresh object: there is no small-value
cache, so `==` on two equal boxes is false where the JDK caches small values.

Conversion follows the JDK: `byteValue`/`shortValue`/`intValue` narrow through the low bits
(a `float`/`double` saturates through `int`, NaN becomes 0), the wide accessors widen with
the JLS §5.3 rule, `Float`/`Double` equality uses canonical bits and `compareTo` is the
total order. One text formatter (`BoxedPrimitive::text`) is shared by `String.valueOf`,
`Object.toString`, `StringBuilder.append`, stream `joining` and `Arrays.toString`; float
and double text is the shortest round-trip spelling, printed in plain decimal.

## 4. Collections

`ArrayList` is `HeapObject::List` and `HashMap` is `HeapObject::Map`, an insertion-ordered
vector of pairs: lookup is a linear scan, and there is no hashing, tree bin, resize or load
factor, so a capacity or load factor is a validated hint. `LinkedHashMap(int,float,boolean)`
records access order, so a successful `get` and an update of an existing key move the entry
to the end, which iteration then observes.

Member search is per container, not one VM-wide rule. `List`/`Set` queries (`contains`,
`indexOf`, `lastIndexOf`, `remove(Object)`, `Set.equals`), the set copy constructors and
the `addAll`/`containsAll`/`removeAll`/`retainAll` bulk operations compare through the
probe's Java `equals`, via the interpreter-side overrides in
`crates/interp/src/collection_equality.rs`; the `Map` key/value operations (`get`,
`containsKey`, `containsValue`, `put`, `putIfAbsent`, `computeIfAbsent`, `getOrDefault`,
`remove`) follow the same rule, and a `put` with an equal key keeps the stored key and
replaces the value. `IdentityHashMap` keeps reference identity, including a `Map`-typed
call: the override checks the receiver's runtime class. `TreeSet`/`TreeMap` stay
insertion-ordered and compare with `equals`, not a comparator.

The `rava-vm` natives underneath still use the older rule — strings by content, boxed
primitives by per-kind value, everything else by identity — and the runner installs the
overrides after them, so a VM-only test sees the VM layer. Map copy constructors still
copy with the VM rule; the set copy constructors and the bulk operations are covered by
the overrides above.

## 5. Text formatting

`StringBuilder` and `StringBuffer` are builders over the same UTF-8 storage with UTF-16
unit semantics; a `setLength`/`deleteCharAt`/`append(char)` that would leave a lone
surrogate is a typed error. `String.valueOf`, `StringBuilder`/`StringBuffer.append(Object)`,
the `%s` conversion of `String.format`/`Formatter`/`MessageFormat`, concatenation,
`Objects.toString` and `Arrays.toString` render an object through its Java `toString`,
using the resolver in `crates/interp/src/object_text.rs` that the runner installs after the
VM library. A class that does not override `toString` resolves to `Object.toString` and
keeps the `ClassName@hex` shape; arrays, class objects, lambdas, lists/maps and the
wrapper/builder shapes keep the VM's structural text. A VM-only test sees the structural
text because the resolver lives in the interpreter.

`java.text.MessageFormat` stores the pattern verbatim and parses it on each call: literal
text, `{index}`, repeated indices, the JDK's single-quote rules, an absent argument's
literal `{index}` and a null argument rendered `"null"`. A `number`/`date`/`time`/`choice`
subformat is a typed error once a non-null argument reaches it.

## 6. Streams

The pipeline is **eager**: a `Stream` holds its element list and each stage allocates a new
stream, so there is no lazy stage graph, no short-circuit before a terminal operation, no
parallel execution and no close action. A VM spliterator is a snapshot plus a cursor; a Java
object implementing `Spliterator` is drained through its own `tryAdvance`, so a Java default
`forEachRemaining` does not run. `sorted()` orders numbers numerically and strings by code
unit; `distinct` and `toSet` use the collection equality rule; collectors append without the
JDK's duplicate-key failure.

## 7. Errors, not synthesized exceptions

A missing member, a bad argument, an unsupported subformat or a malformed pattern is a typed
`VmError` at the native boundary, which the interpreter reports as `InterpError`; the VM
does not synthesize a Java exception object for it. The named-lookup misses are the
deliberate exception: `getDeclaredField`/`getField` raise `NoSuchFieldException` and the
method/constructor lookups raise `NoSuchMethodException`, built by
`rava_vm::exceptions::new_throwable` so a compiled `catch` sees them.

## 8. Boundaries

No hashing or tree bins; no Java `equals` dispatch in collection membership; no small-value
cache; no unmodifiable collection wrappers; no lazy or parallel streams; no `MessageFormat`
subformats, `parse` or `formatToCharacterIterator`; no `CharSequence` general dispatch. Each
of those is a slice or a later milestone, not an accident, and the per-slice records in
[`../slices/`](../slices/) name the slice that owns it.

## Evidence

- `cargo test -p rava-interp --test string_object`
- `cargo test -p rava-interp --test collections`
- `cargo test -p rava-interp --test maps_values`
- `cargo test -p rava-interp --test primitive_boxes`
- `cargo test -p rava-interp --test streams`
- `cargo test -p rava-interp --test message_format`

## Owners

- Requirements: R3 (metadata and the minimal library) in
  [`../requirements.md`](../requirements.md).
- Decision: ADR-0004, a replacement runtime library owned by the project.
- Records: [`../slices/M2-1-c1-and-object-model.md`](../slices/M2-1-c1-and-object-model.md),
  [`../slices/M2-2-reflection-and-dynamic.md`](../slices/M2-2-reflection-and-dynamic.md),
  [`../slices/M2-3-spring-context.md`](../slices/M2-3-spring-context.md).
