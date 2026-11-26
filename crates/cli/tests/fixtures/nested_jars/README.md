# M4.4 Spring Boot nested-jar fixture

Committed source, class files, manifest and jars for the `rava -jar` nested-jar
acceptance test (`crates/cli/tests/nested_jars.rs`). CI has no JDK, so every artifact is
committed and the test never compiles anything.

## Contents

- `src/demo/NestedMain.java` — the manifest's `Start-Class`. It loads
  `demo.NestedHelper` from the nested jar, reads `outer.txt` from
  `BOOT-INF/classes/`, reads `inner.txt` from the nested jar, enumerates both through
  `getResources`, opens both Boot URL forms through `URL.openStream` (including a
  `new URL(...)` round trip) and runs `ServiceLoader` over `META-INF/services`.
- `src/demo/NestedHelper.java`, `src/demo/Greeter.java`, `src/demo/InnerGreeter.java`,
  `src/demo/ResourceReader.java` — the helper classes that exist only in
  `BOOT-INF/lib/demo-helper.jar`. `InnerGreeter` is the service provider.
- `classes/demo/NestedMain.class` — the compiled application class.
- `lib/classes/` — the compiled nested-jar classes, `inner.txt`, and
  `META-INF/services/demo.Greeter` (`demo.InnerGreeter`).
- `app/META-INF/MANIFEST.MF`, `app/BOOT-INF/classes/outer.txt` — the staged outer-jar
  inputs. `app/BOOT-INF/classes/demo/NestedMain.class` and
  `app/BOOT-INF/lib/demo-helper.jar` are also staged from `classes/` and `lib/classes/`
  by the rebuild command.
- `fat.jar` — the executable Boot jar. Its manifest is exactly:

  ```text
  Manifest-Version: 1.0
  Created-By: rava fixture
  Main-Class: org.springframework.boot.loader.launch.JarLauncher
  Start-Class: demo.NestedMain
  ```

  and it carries `BOOT-INF/classes/demo/NestedMain.class`,
  `BOOT-INF/classes/outer.txt` and `BOOT-INF/lib/demo-helper.jar`.

The outer `Main-Class` names Boot's launcher, but the pinned jars carry no Boot loader
classes. `rava -jar` therefore substitutes `Start-Class` over the Boot-aware
`BootJarSource`; that is a documented launcher substitution, not a run of Boot's
`JarLauncher`. A Boot layout whose `Main-Class` is an ordinary application class (not
`org.springframework.boot.loader...`) runs that class directly and needs no
`Start-Class`.

## The two URL forms

The resources are visible to Java as:

```text
jar:file:<outer-absolute-path>!/BOOT-INF/classes!/outer.txt
jar:file:<outer-absolute-path>!/BOOT-INF/lib/demo-helper.jar!/inner.txt
```

`NestedMain` never prints the absolute path. It prints `getProtocol()`, checks that
`getPath()`/`getFile()`/`toString()` contain the Boot separator, and prints only the
resource contents, so the golden output has no host path.

## Rebuilding

Requires a JDK 17 or newer (the committed classes target the Java 17 class-file
version):

```sh
cd crates/cli/tests/fixtures/nested_jars
javac --release 17 -d lib/classes \
  src/demo/Greeter.java src/demo/InnerGreeter.java \
  src/demo/NestedHelper.java src/demo/ResourceReader.java
(cd lib/classes && jar --create --file ../../app/BOOT-INF/lib/demo-helper.jar \
  demo/Greeter.class demo/InnerGreeter.class demo/NestedHelper.class \
  demo/ResourceReader.class inner.txt META-INF/services/demo.Greeter)
javac --release 17 -cp lib/classes -d classes src/demo/NestedMain.java
cp classes/demo/NestedMain.class app/BOOT-INF/classes/demo/NestedMain.class
(cd app && jar --create --file ../fat.jar --manifest META-INF/MANIFEST.MF \
  BOOT-INF/classes/outer.txt BOOT-INF/classes/demo/NestedMain.class \
  BOOT-INF/lib/demo-helper.jar)
```

`app/META-INF/MANIFEST.MF` is the input to `jar --manifest`; the tool preserves the
headers and adds the normal `META-INF/MANIFEST.MF` entry. Do not pass the manifest as a
plain input file: `jar` then replaces it with its generated manifest and the
`Start-Class` header is lost.

## Expected stdout

`rava -jar fat.jar --stdlib stdlib/c1/classes`:

```text
nested:helper
same-loader:true
outer:outer-resource
inner:inner-resource
inner-classroot:inner-resource
resources-outer:1
resources-inner:1
url-outer-protocol:jar
url-outer-path:classes
url-outer-file:classes
url-outer-string:classes
url-outer-open:outer-resource
url-copied-open:outer-resource
url-inner-path:nested
url-inner-open:inner-resource
service:inner-greeter
```

- `same-loader:true` proves the class from the nested jar shares the application loader,
  so the layout does not create a second identity domain.
- `inner-classroot` proves the classes-root lookup falls through to the nested jar, the
  way Boot's class loader searches its whole classpath.
- `resources-*:1` proves `getResources` sees every copy (one of each name here).
- `url-*-open` and `url-copied-open` prove `URL.openStream` and `new URL(externalForm)`.
- `service:inner-greeter` proves `ServiceLoader` reads the provider file from the nested
  jar.

## Limits

- No Boot loader classes and no `URLClassLoader`: a Boot-launcher `Main-Class` is replaced
  by `Start-Class`, and an ordinary `Main-Class` in a Boot layout runs directly.
- A Boot layout's manifest `Class-Path` is ignored, as Boot's launcher builds its own
  classpath from `BOOT-INF`; `loader.path`, exec and a nested jar's own `Class-Path` are
  not implemented.
- Nested jars are searched in lexical `BOOT-INF/lib/*.jar` entry order.
