# M3.3 class-loader fixture

Committed source and class files for the `java.lang.ClassLoader` acceptance test
(`crates/cli/tests/c2_classloader.rs`). CI has no JDK, so the `classes/` output is
committed next to the source; the test never compiles anything.

## Contents

- `src/demo/CustomLoader.java` — a user loader extending `java.lang.ClassLoader` with a
  public `define(String, byte[])` wrapper around the protected
  `defineClass(String, byte[], int, int)`.
- `src/demo/Existing.java` — `public static int answer()` returning 42, the class
  `loadClass` and `Class.forName` resolve through the application loader.
- `classes/demo/CustomLoader.class`, `classes/demo/Existing.class` — the compiled classes
  (class-file version 61, Java 17).

## Rebuilding

Requires a JDK 17 or newer (the committed classes target the Java 17 class-file version):

```sh
cd crates/cli/tests/fixtures/loader
javac --release 17 -d classes src/demo/CustomLoader.java src/demo/Existing.java
```

## Expected behaviour

The acceptance test adds this directory as the application loader's source. It builds a
`demo/Generated` class in process, instantiates `demo.CustomLoader` with `new`, and calls
its `define` method. The VM creates a child loader for the instance with an empty class
source, so:

- the generated class is defined in the child loader, and
  `generatedClass.getClassLoader()` is the `CustomLoader` instance;
- `loadClass("demo.Existing")` and `Class.forName("demo.Existing", false, customLoader)`
  both delegate parent-first and resolve to the *application* loader's class, so its
  `getClassLoader()` is the system loader object;
- `ClassLoader.getSystemClassLoader()` is non-null, the custom loader's `getParent()` is
  the application loader object, and the application loader's `getParent()` is null.
