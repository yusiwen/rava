# Spring context: from `rava run` to a bean

## Question

How does a real Spring `AnnotationConfigApplicationContext` start on rava, and what
exactly does the smoke test prove?

## Fixture

[`crates/cli/tests/fixtures/spring/src/demo/SpringSmoke.java`](../../crates/cli/tests/fixtures/spring/src/demo/SpringSmoke.java)
is compiled at release 17 and committed as a `.class` file:

```java
@Configuration(proxyBeanMethods = false)
class SpringSmoke {
    @Bean String hello() { return "hello"; }

    public static void main(String[] args) {
        try (AnnotationConfigApplicationContext ctx =
                 new AnnotationConfigApplicationContext(SpringSmoke.class)) {
            System.out.println(ctx.getBean(String.class));
        }
    }
}
```

The fixture is a real Spring class path: it is compiled against the pinned Spring
6.2.19 jars, not against a rava-specific API.

## Entry point and class path

The acceptance invokes the built CLI exactly as a user would:

```console
$ RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context
```

The test builds a class path of seven pinned jars (the Spring modules the context needs,
without the optional logging adapters) plus the fixture classes, then runs:

```console
$ target/debug/rava-cli run demo.SpringSmoke \
    --stdlib stdlib/c1/classes \
    --cp "<jar>:...:<jar>:crates/cli/tests/fixtures/spring/classes"
```

The runner is in [`crates/cli/src/run.rs`](../../crates/cli/src/run.rs). It:

1. creates a `ClassPathSource` over the ordered jar/directory entries;
2. adds one application loader under the bootstrap loader, so class identity remains
   `(defining loader, binary name)` and parent-first delegation applies;
3. registers the VM's library natives through
   [`crates/vm/src/library.rs`](../../crates/vm/src/library.rs);
4. loads `demo.SpringSmoke`, checks the static `main([Ljava/lang/String;)V`, allocates an
   empty `String[]` and invokes it through `rava-interp`.

## Runtime path

The context is not simulated. The call chain is:

```
SpringSmoke.main
  -> new AnnotationConfigApplicationContext(SpringSmoke.class)
     -> Spring's own bytecode, arrays, collections, streams and lambdas
     -> C1 stub class files for java.* (metadata only, no Code)
     -> native registry for the java.* behaviour
     -> Spring's @Bean method, invoked reflectively
  -> ctx.getBean(String.class)
     -> Spring's bean factory lookup
  -> System.out.println("hello")
  -> try-with-resources close
```

The key interface is the native override. C1 stubs have no `Code` and generally no
`ACC_NATIVE`; the interpreter consults the native registry **before** requiring
bytecode, so `java.lang.String`, `Class`, reflection members, collections, streams,
logging, etc. are implemented by VM natives while their class files still provide the
JDK 17 metadata shape. The M2.4 receiver-aware native ABI and the M2.8/M2.11/M2.22
reflection objects are what let Spring's own code call those methods. The M2.12–M2.29
library slices fill the collection, box, stream, message, lookup and shutdown paths.

## Evidence

```console
$ RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context
running 1 test
test spring_annotation_context_starts_and_exposes_the_bean ... ok

test result: ok. 1 passed; 0 failed
```

The test asserts the child process exits 0 and its stdout is exactly `hello\n`; the
context is closed by try-with-resources. On a CI checkout without the fetched
`compat/jars`, the test prints a skip message and returns; the local acceptance requires
the jars and is the one that counts. The test source is
[`crates/cli/tests/spring_context.rs`](../../crates/cli/tests/spring_context.rs).

## Boundaries

- This is a **minimal context**, not a Boot application: one `@Configuration` class with
  `proxyBeanMethods = false` and a single `@Bean String`.
- The seven-jar class path is deliberately smaller than the 35-jar target; the optional
  logging adapters are not needed for this smoke.
- It does not exercise `proxyBeanMethods=true`, CGLIB, component scanning of a large
  class path, web/Tomcat, Boot auto-configuration, or C2 dynamic code.
- It proves that a real Spring context can start and produce a bean on this runtime; it
  does not prove every Spring or java.* path beyond the exercised chain.
- The `hello` bean is `String`; the context still exercises annotation metadata,
  reflection, generics, collections, streams, boxing and lifecycle closure along the way.

## Next documents

The mechanism-level companions are `class-loading-and-libraries.md` (loader identity,
the two `stdlib` trees and native dispatch) and `interpreter-execution.md` (frames,
exceptions, `invokedynamic`, lambdas and streams). They are in the
[explanation index](README.md#planned-implementation-documents).
