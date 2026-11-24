# Stream fixture (M2.24)

`demo.StreamProbe` is the acceptance fixture for slice M2.24: a Java 17 class whose
static methods drive the eager `java.util.stream` pipeline through real `javac`
bytecode. It returns heap values (`List`, `Set`, `String`, `long`, `boolean`) that
`crates/interp/tests/streams.rs` inspects, so the assertions never depend on a Rust
helper the fixture could fake.

`StreamProbe$WordSpliterator` is a real Java `Spliterator`: `StreamSupport.stream` must
drain it through its own `tryAdvance` and a VM-native consumer, which is the shape
Spring's `MergedAnnotationsCollection$AnnotationsSpliterator` has. The fixture also
covers a captured lambda argument, a caught and an uncaught throwing callback,
`Collectors.toList`/`toSet`/`joining`/`toCollection`, `Collector.of` with an identity and
a real finisher, `sorted`/`limit`, and the VM `Spliterator` surface.

The source and the compiled classes are committed so the test needs no JDK.

Rebuild with:

```sh
javac --release 17 \
  -d crates/interp/tests/fixtures/streams/classes \
  crates/interp/tests/fixtures/streams/src/demo/StreamProbe.java
```
