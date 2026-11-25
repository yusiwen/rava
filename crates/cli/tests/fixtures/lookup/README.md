# M3.4 method-handles fixture

Committed source and class files for the `java.lang.invoke.MethodHandles.Lookup` /
`Lookup.defineClass` acceptance test (`crates/cli/tests/c2_lookup.rs`). CI has no JDK, so
the `classes/` output is committed next to the source; the test never compiles anything.

## Contents

- `src/demo/LookupProbe.java` — `public static int add(int, int)`, `public String echo()` on
  an instance, and `public static void run()`, which takes a private lookup on its own
  class, resolves both members through it and prints one fact per line.
- `classes/demo/LookupProbe.class` — the compiled class (class-file version 61, Java 17).

## Rebuilding

Requires a JDK 17 or newer (the committed class targets the Java 17 class-file version):

```sh
cd crates/cli/tests/fixtures/lookup
javac --release 17 -d classes src/demo/LookupProbe.java
```

`javac` prints a deprecation note for the `Class[]`-taking `MethodType.methodType`
overload; it does not change the output.

## Expected stdout

`demo.LookupProbe.run()` prints one fact per line:

```text
5
demo.LookupProbe
true
lookup
```

The values are, in order: `add.invokeWithArguments(2, 3)`, the lookup class's name,
`lookup.hasPrivateAccess()`, and the virtual `echo` result invoked with a new instance.

Values that are not strings are concatenated with `""` before printing, because the VM's
`PrintStream` stubs implement only the `String` overloads. That keeps each line one
deterministic fact; the repository's proxy fixture uses the same convention.

## What the fixture pins

The fixture is the Java-side shape of CGLIB's
`org.springframework.cglib.core.ReflectUtils.defineClass` bootstrap:

```java
Lookup lookup = MethodHandles.privateLookupIn(LookupProbe.class, MethodHandles.lookup());
MethodHandle add = lookup.findStatic(
        LookupProbe.class, "add",
        MethodType.methodType(int.class, int.class, int.class));
```

The acceptance test also drives the same lookup/`defineClass` path from Rust, and mirrors
CGLIB's reflective bootstrap (`Class.getMethod("privateLookupIn", ...)`,
`Lookup.class.getMethod("defineClass", byte[].class)`, both through `Method.invoke`).
