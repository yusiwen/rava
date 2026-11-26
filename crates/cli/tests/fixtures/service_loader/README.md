# M4.2 `ServiceLoader` fixture

Committed source, class files, provider-configuration files and jar for the M4.2
acceptance test (`crates/cli/tests/service_loader.rs`). CI has no JDK, so every artifact
is committed and the test never compiles anything.

## Contents

- `src/demo/Greeter.java` — the service interface.
- `src/demo/HelloGreeter.java` — directory provider; its static initializer and
  constructor print `clinit:demo.HelloGreeter` / `init:demo.HelloGreeter`.
- `src/demo/WorldGreeter.java` — jar provider; the same two prints with its own name.
- `src/demo/MissingService.java` — a service with no provider configuration.
- `src/demo/InvalidService.java` and `src/demo/NotAGreeter.java` — a configuration that
  names a class which does not implement its service.
- `src/demo/ServiceLoaderProbe.java` — the `javac --release 17` probe; `run(String)`
  selects one mode so the typed-error tests can call it through `run_static_method`.
- `classes/demo/*.class` — the compiled probe and providers (class-file version 61).
- `classes/META-INF/services/demo.Greeter` — the directory half of the provider
  configuration for `demo.HelloGreeter`, with a comment, a blank line and surrounding
  spaces to exercise parsing.
- `classes/META-INF/services/demo.InvalidService` — `demo.NotAGreeter`, the invalid
  provider.
- `jar/META-INF/services/demo.Greeter` — the jar half, naming `demo.WorldGreeter`.
- `providers.jar` — a jar with only that provider-configuration file.

The acceptance test puts `classes` and `providers.jar` on the classpath in that order,
then in the reverse order, so the two provider files and the output order distinguish the
two. The provider classes themselves stay in the directory classpath.

## Rebuilding

Requires a JDK 17 or newer (the committed class targets the Java 17 class-file version):

```sh
cd crates/cli/tests/fixtures/service_loader
javac --release 17 -d classes src/demo/*.java
(cd jar && jar --create --file ../providers.jar META-INF/services/demo.Greeter)
```

## Expected behaviour

`load` reads both `META-INF/services/demo.Greeter` resources and validates both provider
classes, but constructs neither. The `order` mode prints `before-iteration`,
`after-iterator`, `hasNext=true` and `after-hasNext` before any provider output, then
`clinit:`/`init:` immediately before each `next:` line; that ordering is the laziness
proof. The `identity` mode shows that the dotted configuration name resolves to the same
`demo.HelloGreeter` class as `ClassLoader.loadClass` and `Class.forName`. Absolute host
paths never appear in the golden output.
