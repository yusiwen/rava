# Lookup-miss fixture (M2.30)

`demo.LookupMissProbe` is the acceptance fixture for slice M2.30: the six named `Class`
lookups (`getDeclaredField`/`getField`, `getDeclaredMethod`/`getMethod`,
`getDeclaredConstructor`/`getConstructor`) throw the JDK's `NoSuchFieldException` or
`NoSuchMethodException` on a miss. The fixture catches them in compiled Java, reports the
exception class and message, and exercises the success paths unchanged.

The source and the compiled class are committed so `cargo test -p rava-interp --test
lookup_exceptions` needs no JDK.

Rebuild the class with a release-17 compiler:

```sh
javac --release 17 \
  -d crates/interp/tests/fixtures/lookup_exceptions/classes \
  crates/interp/tests/fixtures/lookup_exceptions/src/demo/LookupMissProbe.java
```
