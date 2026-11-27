# diagnostics — stream routing, streaming output and throwable traces

`demo.OutErr`, `demo.Hang`, `demo.Trace`, `demo.Suppressed`, `demo.PrintAll` and
`demo.PrintObject` are ordinary Java compiled with `javac --release 17`.
`crates/cli/tests/diagnostics.rs` runs them through the built CLI and asserts (#79, #90):

- `System.err` reaches the process's stderr and `System.out` its stdout;
- output written before a hang survives the kill (the runner streams instead of buffering);
- `printStackTrace()` prints the `Caused by:` chain and `Suppressed:` entries, with no
  frames (the VM records no Java frames);
- `addSuppressed`/`getSuppressed` round-trip;
- every `PrintStream` `print`/`println` overload (primitives, `char[]`, an Object with a
  `toString` override, and the identity fallback) prints the JDK's text on both streams;
- `PrintStream.printf`/`format` (including the Locale decimal separator) and
  `append`/`append(CharSequence,int,int)`/`append(char)` match the JDK on both streams.

Committed files:

- `src/demo/*.java` — the sources.
- `classes/demo/*.class` — the compiled classes, so CI needs no JDK.

Rebuild after editing a source (needs a JDK 17 `javac`):

```sh
JAVAC=/path/to/jdk-17/bin/javac
"$JAVAC" --release 17 -d crates/cli/tests/fixtures/diagnostics/classes \
  crates/cli/tests/fixtures/diagnostics/src/demo/*.java
```
