# format — `String.format` conversions against the JDK (#71)

`demo.FormatProbe` is ordinary Java compiled with `javac --release 17`;
`crates/cli/tests/format.rs` asserts the JDK 25 answers:

- `%e` prints `1.234500e+03` (sign and two exponent digits), including `0.000000e+00`;
- `%x`/`%o` use the boxed width (`Byte` 2, `Short` 4, `Integer` 8, `Long` 16 digits);
- `%c` accepts a `Character`, and `%1$s` reuses an explicit argument;
- a `Locale` supplies the decimal separator (`1,50` for `Locale.GERMANY`), and the
  no-locale form uses the default locale's separator.

Rebuild after editing the source (needs a JDK 17 `javac`):

```sh
JAVAC=/path/to/jdk-17/bin/javac
"$JAVAC" --release 17 -d crates/cli/tests/fixtures/format/classes \
  crates/cli/tests/fixtures/format/src/demo/FormatProbe.java
```
