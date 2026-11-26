# records — the record `ObjectMethods` fixture

`demo.RecordProbe` is ordinary Java compiled with `javac --release 17`; its static methods
return the JDK's answers for the record `equals`/`toString`/`hashCode` bootstrap
(`crates/cli/tests/records.rs`, issue #72):

- `this == o` before any component, so a NaN record equals itself;
- `Float.compare`/`Double.compare` per component: NaN equals NaN, `0.0` differs from
  `-0.0`;
- `boolean` renders `true`/`false`, `char` renders the character;
- `Float.hashCode`/`Double.hashCode` (canonical NaN bits) and the `31 * hash` fold.

Committed files:

- `src/demo/RecordProbe.java` — the source.
- `classes/demo/RecordProbe.class` — the compiled class, so CI needs no JDK.

Rebuild after editing the source (needs a JDK 17 `javac`):

```sh
JAVAC=/path/to/jdk-17/bin/javac
"$JAVAC" --release 17 -d crates/cli/tests/fixtures/records/classes \
  crates/cli/tests/fixtures/records/src/demo/RecordProbe.java
```
