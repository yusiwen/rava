# M4.1 classpath-resource fixture

Committed source, class file, resources and jar for the resource acceptance test
(`crates/cli/tests/resources.rs`). CI has no JDK, so every artifact is committed and the
test never compiles anything.

## Contents

- `src/demo/ResourceProbe.java` — uses `ClassLoader.getResource(s)`,
  `getResourceAsStream`, `Class.getResource` (relative and absolute), `URL.getProtocol`,
  `URL.getPath`, `URL.openStream`, `URLConnection.getInputStream` and the stream EOF
  contract.
- `classes/demo/ResourceProbe.class` — the compiled probe (class-file version 61,
  Java 17).
- `classes/demo/data.txt` — the directory resource, payload `directory-payload`.
- `jar/demo/data.txt` — the jar resource source, payload `jar-payload`.
- `resources.jar` — a jar with only `demo/data.txt`.

The acceptance test puts `classes` and `resources.jar` on the classpath in that order,
then in the reverse order, so the two payloads distinguish directory-first from jar and
prove the enumeration follows classpath order.

## Rebuilding

Requires a JDK 17 or newer (the committed class targets the Java 17 class-file version):

```sh
cd crates/cli/tests/fixtures/resources
javac --release 17 -d classes src/demo/ResourceProbe.java
(cd jar && jar --create --file ../resources.jar demo/data.txt)
```

## Expected behaviour

With `classes` first, `getResource` reports `file`, `getResources` returns the directory
URL then the jar URL, and each payload matches its source. With `resources.jar` first,
the same two URLs come back in the opposite order. Absolute host paths never appear in the
golden output: the probe prints protocol, booleans, counts and payloads.
