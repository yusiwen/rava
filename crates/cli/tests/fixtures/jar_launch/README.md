# M4.3 executable-jar fixture

Committed source, class files, manifest and jars for the `rava -jar` acceptance test
(`crates/cli/tests/jar_launch.rs`). CI has no JDK, so every artifact is committed and
the test never compiles anything.

## Contents

- `src/demo/JarMain.java` — the executable jar's `Main-Class`. It prints `jar:hello`,
  the `Helper` answer, the contents of `data.txt`, then each program argument.
- `src/demo/Helper.java` — the helper class that exists only in `lib/helper.jar`, so a
  successful run proves the manifest `Class-Path` was honoured.
- `classes/demo/JarMain.class` — the compiled main class (class-file version 61).
- `lib/classes/demo/Helper.class` — the compiled helper.
- `app/META-INF/MANIFEST.MF`, `app/data.txt` — the inputs staged for `app.jar`;
  `demo/JarMain.class` is added from `classes/` by the rebuild command.
- `app.jar` — the executable jar. Its manifest is exactly:

  ```text
  Manifest-Version: 1.0
  Created-By: rava fixture
  Main-Class: demo.JarMain
  Class-Path: lib/helper.jar
  ```

  and it also carries `data.txt` and `demo/JarMain.class`.
- `lib/helper.jar` — a jar with only `demo/Helper.class`; it is never on the command
  line, only in the executable jar's `Class-Path`.

## Rebuilding

Requires a JDK 17 or newer (the committed classes target the Java 17 class-file
version):

```sh
cd crates/cli/tests/fixtures/jar_launch
javac --release 17 -d lib/classes src/demo/Helper.java
javac --release 17 -cp lib/classes -d classes src/demo/JarMain.java
(cd lib/classes && jar --create --file ../helper.jar demo/Helper.class)
(cd app && jar --create --file ../app.jar --manifest META-INF/MANIFEST.MF data.txt -C ../classes demo/JarMain.class)
```

`app/META-INF/MANIFEST.MF` is the input to `jar --manifest`; the tool preserves the
headers and adds the normal `META-INF/MANIFEST.MF` entry. Do not pass the manifest as a
plain input file: `jar` then replaces it with its generated manifest and the
`Main-Class`/`Class-Path` headers are lost.

## Expected stdout

`rava -jar app.jar --stdlib stdlib/c1/classes`:

```text
jar:hello
helper:helper
resource:jar-resource
```

Each additional program argument adds one `arg:<value>` line in the same order. Absolute
host paths never appear in the golden output: the resource payload is fixed, and the test
reports only class-visible facts.
