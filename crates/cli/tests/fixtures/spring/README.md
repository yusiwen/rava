# Spring smoke fixture (M2.23)

`demo.SpringSmoke` is the acceptance fixture for slice M2.23: it starts a real Spring
`AnnotationConfigApplicationContext` with `proxyBeanMethods = false` and prints the
`String` bean named `hello`, which must be exactly `hello`.

The source and the compiled class are committed so `cargo test -p rava-cli --test
spring_context` needs no JDK when `compat/jars` is present.

Rebuild the class with the pinned jars:

```sh
javac --release 17 \
  -cp "compat/jars/*" \
  -d crates/cli/tests/fixtures/spring/classes \
  crates/cli/tests/fixtures/spring/src/demo/SpringSmoke.java
```

The classpath for the acceptance is the Spring modules the context needs
(`spring-context`, `spring-beans`, `spring-core`, `spring-expression`, `spring-aop`,
`spring-jcl` and `jakarta.annotation-api`). The optional SLF4J/Log4j/Logback adapters are
excluded on purpose so `spring-jcl` uses the runtime's `java.util.logging` natives; Boot's
logging stack is not part of this Spring-context smoke test.
