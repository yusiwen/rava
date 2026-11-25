# M3.2 proxy fixture

Committed source and class files for the `java.lang.reflect.Proxy` /
`InvocationHandler` acceptance test (`crates/cli/tests/c2_proxy.rs`). CI has no JDK, so the
`classes/` output is committed next to the source; the test never compiles anything.

## Contents

- `src/demo/Service.java` — the proxied interface: `name()`, `add(int, int)`,
  `widen(long, int)` and the default `greet()`.
- `src/demo/ProxyProbe.java` — creates an `InvocationHandler` lambda, obtains a proxy
  through `Proxy.newProxyInstance`, and prints one fact per line.
- `classes/demo/Service.class`, `classes/demo/ProxyProbe.class` — the compiled classes.

## Rebuilding

Requires a JDK 17 or newer (the committed classes target the Java 17 class-file version):

```sh
cd crates/cli/tests/fixtures/proxy
javac --release 17 -d classes src/demo/Service.java src/demo/ProxyProbe.java
```

`javac` may print a deprecation note for `InvocationHandler.invokeDefault`; it does not
change the output.

## Expected stdout

`demo.ProxyProbe.run()` prints one fact per line:

```text
rava
5
9
hello rava
proxy!
true
true
true
true
```

The values are, in order: the handler's `name` result, the summed `add` arguments, the
summed `widen` arguments, the default `greet` result, the handler's `toString` result,
`Proxy.isProxyClass(proxy.getClass())`, the reference identity of two `getProxyClass`
calls, `proxy.getClass().getInterfaces()[0] == Service.class`, and
`Proxy.getInvocationHandler(proxy) == handler`.
