# M3.7 Spring AOP and configuration fixture

Committed source and class files for the C2 gate (`crates/cli/tests/c2_gate.rs`). CI has
no JDK, so the `classes/` output is committed next to the source; the test never compiles
anything and never uses a host JVM.

The fixture runs against the pinned `compat/jars` Spring modules and exercises one real
proxy path per technology in a single `run()`:

- `@Configuration(proxyBeanMethods = true)` enhancement — Spring's repackaged CGLIB
  subclasses the configuration class, and the `greeting()` bean method's call to
  `hello()` goes through the bean factory;
- bare `org.springframework.cglib.proxy.Enhancer` over a superclass;
- `ObjenesisStd.newInstance` on a class whose constructor sets `value = 42` and whose
  static initializer sets `marker = 7`;
- JDK `Proxy.newProxyInstance` with an `InvocationHandler`;
- `ProxyFactory` over a target with an `Advice` (JDK dynamic proxy);
- `ProxyFactory` with `setProxyTargetClass(true)` (Spring's `CglibAopProxy`).

No proxy class is committed: the configuration subclass, the CGLIB AOP proxy and the
`rava/proxy/$ProxyN` classes are all generated while the program runs and defined through
the VM's ordinary class-definition path, which verifies them.

## Contents

- `src/demo/C2Gate.java` — the fixture. `run()` prints one fact per line; `main` calls
  `run()` so the fixture can also be started with `rava run demo.C2Gate`.
- `classes/**` — the compiled classes (class-file version 61, Java 17), including the
  nested classes (`C2Gate$Config`, `C2Gate$Greeter`, `C2Gate$GreeterImpl`, `C2Gate$Bean`,
  `C2Gate$Constructed`) and the four anonymous classes that hold the interceptors and the
  invocation handler.

## Spring's CGLIB marker

Spring 6.2's repackaged CGLIB uses `org.springframework.cglib.core.SpringNamingPolicy` for
both the enhanced configuration class and the CGLIB AOP proxy, so their names carry
`$$SpringCGLIB$$` (for example `demo.C2Gate$Config$$SpringCGLIB$$0` and
`demo.C2Gate$GreeterImpl$$SpringCGLIB$$0`). `$$EnhancerByCGLIB$$` names only the bare
`Enhancer` result. The fixture checks the marker each path actually produces; a check for
`$$EnhancerBySpringCGLIB$$` is false even on a stock JDK with these jars.

## Rebuilding

Requires a JDK 17 or newer:

```sh
cd crates/cli/tests/fixtures/aop
javac --release 17 \
  -cp ../../../../../compat/jars/* \
  -d classes \
  src/demo/C2Gate.java
```

`javac` may print deprecation notes; they do not change the output.

## Expected stdout

`demo.C2Gate.run()`:

```text
config-enhanced:true
greeting:hello!
cglib:cglib:hello
objenesis-value:0
objenesis-marker:7
jdk-proxy:proxy:greet
jdk-proxy-class:true
aop-jdk:aop:hello
aop-jdk-class:true
aop-cglib:aop:hello
aop-cglib-class:true
```

The lines are, in order:

1. `ctx.getBean(Config.class)`'s class name contains `$$SpringCGLIB$$`;
2. the `greeting` bean is `hello!`, so the bean method call was intercepted;
3. the bare `Enhancer` subclass's intercepted `hello()`;
4. Objenesis skipped `Constructed`'s constructor (`value` stays `0`);
5. Objenesis still initialized the class (`marker` is `7`);
6. the JDK proxy's handler answered `proxy:greet`;
7. that class is a `Proxy.isProxyClass` class;
8. the `ProxyFactory` JDK proxy's advice answered `aop:hello`;
9. that class is a `Proxy.isProxyClass` class;
10. the `ProxyFactory` CGLIB proxy's advice answered `aop:hello`;
11. that class name contains Spring CGLIB's `$$SpringCGLIB$$` marker.
