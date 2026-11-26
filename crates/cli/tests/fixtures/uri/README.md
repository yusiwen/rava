# uri — raw/decoded URI components (#71)

`demo.UriProbe` is ordinary Java compiled with `javac --release 17`; `crates/cli/tests/uri.rs`
asserts the JDK 25 answers:

- `getRawPath`/`getPath`, `getRawQuery`/`getQuery`, `getRawFragment`/`getFragment` and
  `getSchemeSpecificPart`/`getRawSchemeSpecificPart` keep the raw and decoded forms apart;
- an opaque `mailto:` scheme-specific part is decoded;
- `resolve("../d")` follows RFC 3986;
- `new URI("http://exa mple.com/")` and `new URI("http://example.com/%zz")` raise a
  catchable `URISyntaxException`.

Rebuild after editing the source (needs a JDK 17 `javac`):

```sh
JAVAC=/path/to/jdk-17/bin/javac
"$JAVAC" --release 17 -d crates/cli/tests/fixtures/uri/classes \
  crates/cli/tests/fixtures/uri/src/demo/UriProbe.java
```
