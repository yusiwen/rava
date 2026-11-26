# M2.23–M2.30 slice detail — the Spring context and its blocker chain

Per-slice record for M2.23–M2.30. `PROJECT.md` § Slices owns the status and the one-line
description; this file owns what landed, where it stops and the evidence. The checks every
entry closed with are in the [gate baseline](README.md); the blocker sequence itself is in
[`../notes/M2.md`](../notes/M2.md).

## M2.23 — Spring `AnnotationConfigApplicationContext` bean graph — 2026-11-24

**Acceptance.** `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` — exit 0.

**Delivered.**
- The committed `demo.SpringSmoke` fixture (`javac --release 17`, `proxyBeanMethods = false`) starts a real context on the pinned Spring 6.2.19 jars, reads the `hello` bean and closes.
- The acceptance prints exactly `hello\n` and does not skip when the jars are present.

**Boundary.**
- `proxyBeanMethods = false` keeps CGLIB out of M2; that is the fixture's own constraint.
- The full auto-configuration graph stays a later C3 target.

See [`spring-context.md`](../explanation/spring-context.md).

## M2.24 — `java.util.stream` pipeline for Spring's annotation metadata — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test streams` — exit 0 (13 tests).

**Delivered.**
- `HeapObject::Stream`/`Spliterator`/`Collector`/`Optional`/`Sink` with roots enumeration.
- `rava-interp::streams`: the eager `StreamSupport`/`filter`/`map`/`sorted`/`collect` surface, collectors, optional and the Java-spliterator drain through `tryAdvance`.

**Boundary.**
- The pipeline is eager: no lazy stage graph, no short-circuit before a terminal operation and no parallel execution.
- `sorted()` orders numbers and strings only; `toList` returns a modifiable heap list.

See [`runtime-library.md`](../explanation/runtime-library.md) §6.

## M2.25 — Full primitive-box family and Number accessors — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test primitive_boxes` — exit 0 (11 tests).

**Delivered.**
- All eight primitives as `BoxedPrimitive`, with the JDK conversion rules, canonical float/double equality and the total order.
- The wrapper registry (`valueOf`, `parseXxx`, `toString`, `compare*`, the bit methods) and reflective adaptation for every descriptor.

**Boundary.**
- No small-value cache, so `==` on two equal boxes is false where the JDK caches.
- Integer parsers accept ASCII only, the float parsers accept `inf`/`nan` spellings Java rejects, and float text stays plain decimal.

See [`runtime-library.md`](../explanation/runtime-library.md) §3.

## M2.26 — `instanceof(null)` correctness and map constructor surface — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test spring_fixes` — exit 0 (9 tests).

**Delivered.**
- `instanceof` of a null reference pushes 0 for a class, an interface and an array (JVMS §6.5).
- `LinkedHashMap(IFZ)V` with the access-order flag, and the JDK `(IF)V` map constructor descriptor.
- `computeIfAbsent` re-entrancy, `getDeclaredClasses`, `Comparator.comparingInt`/`reversed` and non-SAM default calls on a lambda.

**Boundary.**
- No hashing, tree bins or resize; the access-order flag only reorders the entry vector.
- Not in this slice: the manifest's descriptor-only types (M2.27) and the wider `Comparator` family.

See [`runtime-library.md`](../explanation/runtime-library.md) §4.

## M2.27 — Descriptor-closure manifest and C1-tree re-baseline — 2026-11-24

**Acceptance.** `cargo run -p rava-cli -- compat check` — exit 0.

**Delivered.**
- The manifest scanner now records classes named by declared and referenced descriptors, `Signature` bounds/arguments and annotation type descriptors, arrays unwrapped.
- The re-baselined manifest (1047 classes) and both committed stub trees were regenerated: 1011 projected from `ct.sym` and 36 placeholders.

**Boundary.**
- Static and jar-only: the JDK stubs and the committed trees are not scan inputs.
- `LocalVariableTypeTable` signatures inside `Code` are not closure inputs, and the pinned target versions are unchanged.

See [`c1-metadata.md`](../explanation/c1-metadata.md).

## M2.28 — Method/Executable bridge, varargs and synthetic flags — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test method_flags` — exit 0 (10 tests).

**Delivered.**
- `isBridge`/`isVarArgs`/`isSynthetic` read from the parsed `ACC_*` bits, never inferred, with the JDK `getModifiers()` masks.
- The collection `(Collection)` constructors, set deduplication, `removeAll`/`retainAll`/`containsAll` and callback dispatch up the superclass chain.

**Boundary.**
- A member absent from its class file is a typed error rather than a fabricated flag.
- No access-control check or `setAccessible` behaviour is added by these flags.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §3.

## M2.29 — Minimal `java.text.MessageFormat` for Spring messages — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test message_format` — exit 0 (11 tests).

**Delivered.**
- `HeapObject::MessageFormat` and `HeapObject::StringBuffer` with the `format`/`applyPattern`/ `toPattern` natives and the static form.
- `AccessibleObject` annotation queries and a real `setAccessible` flag, `List.addAll(int,Collection)`, and the C1 tree's omitted transitive interfaces skipped in `getInterfaces` and inherited lookup.

**Boundary.**
- The pattern is stored verbatim and parsed per call; the `(String, Locale)` constructor ignores the locale.
- `number`/`date`/`time`/`choice` subformats are typed errors; `parse` and `setFormat` have no native.

See [`runtime-library.md`](../explanation/runtime-library.md) §5.

## M2.30 — `NoSuchMethodException`/`NoSuchFieldException` lookup misses — 2026-11-24

**Acceptance.** `cargo test -p rava-interp --test lookup_exceptions` — exit 0 (6 tests).

**Delivered.**
- The six named lookups throw `NoSuchFieldException`/`NoSuchMethodException` built by `rava_vm::exceptions::new_throwable`, so a compiled `catch` sees them.
- Messages name the receiver, member and parameter descriptors; `Class.getClasses` implements the public superclass walk.
- The shutdown-path gaps (`Map.replaceAll`/`forEach`, `Collection.removeIf`, `Record.<init>`, `EventObject`, enum statics) closed with it, so the Spring acceptance prints `hello\n`.

**Boundary.**
- The exception class comes from the library class table but no Java constructor runs; the message is deterministic, not byte-identical to the JDK's.
- A primitive or array receiver throws too, because it declares no members.

See [`reflection-and-metadata.md`](../explanation/reflection-and-metadata.md) §3.
