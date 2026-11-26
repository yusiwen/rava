# boot_nonweb — the M4.5 non-web Spring Boot fixture

A real Boot application: `@SpringBootConfiguration` (a genuine Boot context) started by
`SpringApplication` with `WebApplicationType.NONE`, registering one `@Bean
ApplicationRunner` that prints exactly `boot:started`. It is compiled against the pinned
`compat/jars` set with Java 17, and the gate (`crates/cli/tests/boot_nonweb.rs`) runs it
with `rava-cli` and every pinned jar on the classpath.

Committed files:

- `src/demo/BootNonWeb.java` — the source.
- `classes/demo/BootNonWeb.class` — the compiled class, so CI needs no JDK.

Rebuild after editing the source (the host needs a JDK 17 `javac`):

```sh
JAVAC=/path/to/jdk-17/bin/javac
CP=$(ls compat/jars/*.jar | tr '\n' ':')
"$JAVAC" --release 17 -cp "$CP" -d crates/cli/tests/fixtures/boot_nonweb/classes \
  crates/cli/tests/fixtures/boot_nonweb/src/demo/BootNonWeb.java
```

## Boundary: `@SpringBootConfiguration`, not `@SpringBootApplication`

The fixture deliberately does not carry `@EnableAutoConfiguration` (so not
`@SpringBootApplication`). The M4 gate's sentence is "a non-web Boot application starts":
this fixture proves the real `SpringApplication` lifecycle, the annotation context, the
bean definition for the runner, `ApplicationRunner` invocation and the `NONE` web type.
The full auto-configuration path (Jackson, SSL, HTTP message converters, task executors
and their optional C3 dependencies) is the next target, not this gate; enabling it here
would make the gate about those auto-configurations instead of Boot startup. This is a
documented scope boundary, not a hidden failure: the fixture uses the real Boot classes
and the real startup path, with a smaller application.
