# M3.6 CGLIB and Objenesis fixture

Committed source and class files for the Spring CGLIB / Objenesis acceptance test
(`crates/cli/tests/c2_cglib.rs`). CI has no JDK, so the `classes/` output is committed next
to the source; the test never compiles anything and never uses a host JVM.

The fixture runs against the pinned `compat/jars/spring-core-6.2.19.jar`, which contains
Spring's repackaged `org.springframework.cglib`, `org.springframework.asm` and
`org.springframework.objenesis`. Neither library is replaced or pre-generated: the enhanced
class is produced by `Enhancer`/ASM while the program runs and defined through the VM's
ordinary `ClassLoader.defineClass` path, which verifies it.

## Contents

- `src/demo/CglibProbe.java` — `Enhancer` over `CglibProbe$Bean` with a `MethodInterceptor`
  that answers `hello` and `add`. `run()` prints one fact per line; `main` calls `run()` so
  the probe can also be started with `rava run demo.CglibProbe`.
- `src/demo/ObjenesisProbe.java` — `ObjenesisStd.newInstance(CglibProbe$Constructed.class)`.
  The class's constructor sets `value = 42` and a static initializer sets `marker = 7`, so
  `value=0` proves the constructor was skipped and `marker=7` proves the class was
  initialized. `main` calls `run()`.
- `src/demo/CglibRejectionProbe.java` — an `Enhancer` over a `final` class. CGLIB's own
  `IllegalArgumentException` must reach the program and no class may be defined.
- `classes/**` — the compiled classes (class-file version 61, Java 17), including the inner
  classes (`CglibProbe$Bean`, `CglibProbe$1`, `ObjenesisProbe$Constructed`,
  `CglibRejectionProbe$FinalBean`).

## Rebuilding

Requires a JDK 17 or newer:

```sh
cd crates/cli/tests/fixtures/cglib
javac --release 17 \
  -cp ../../../../../compat/jars/spring-core-6.2.19.jar \
  -d classes \
  src/demo/CglibProbe.java src/demo/ObjenesisProbe.java src/demo/CglibRejectionProbe.java
```

`javac` may print deprecation notes; they do not change the output.

## Expected stdout

`demo.CglibProbe.run()`:

```text
intercepted:hello
5
true
```

The lines are the interceptor's `hello` answer, the interceptor's `add(2, 3)` answer, and
`bean.getClass().getName().contains("$$EnhancerByCGLIB$$")`.

`demo.ObjenesisProbe.run()`:

```text
value=0
marker=7
```

`demo.CglibRejectionProbe.run()`:

```text
Cannot subclass final class demo.CglibRejectionProbe$FinalBean
```
