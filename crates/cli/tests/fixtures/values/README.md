# values — runtime values against the JDK (#71)

`demo.ValueProbe` is ordinary Java compiled with `javac --release 17`; `crates/cli/tests/values.rs`
asserts the JDK 25 answers:

- `RuntimeMXBean.getUptime()`/`getStartTime()` are positive (`0` was the bug);
- `Instant.toString` omits a zero fraction and keeps `.123`;
- `Duration.toString` prints `PT25H1M1S` and `PT1.5S`;
- `new Random(0)` follows the Java LCG (`-1155484576`, `-723955400`, `29`);
- `BigInteger.ZERO`/`ONE`/`TEN`, `hashCode` (`31` for `2^32`) and `add` (`5`).

Rebuild after editing the source (needs a JDK 17 `javac`):

```sh
JAVAC=/path/to/jdk-17/bin/javac
"$JAVAC" --release 17 -d crates/cli/tests/fixtures/values/classes \
  crates/cli/tests/fixtures/values/src/demo/ValueProbe.java
```
