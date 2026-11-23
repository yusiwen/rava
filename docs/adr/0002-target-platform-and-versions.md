# ADR-0002 — Target: Java 17, Spring Framework 6.2.19, Spring Boot 3.5.16

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

"Run Spring" has no testable meaning until the platform, the framework versions and
the application are pinned. Different Spring lines change the class files, the
reflective surface and the required JDK APIs; different JDK versions change the
library surface more than the interpreter.

The target is Spring Boot 3.5.16 and its official dependency management: Spring
Framework 6.2.19 and Tomcat 10.1.55, on Java 17. The initial source audit (ADR-0005)
used 6.2.0 / 3.4.0 / 10.1.34; it is a starting point, not the pin, and is repeated
against these exact versions at M1.

The first application is servlet-based and deliberately small. Reactive web
(Spring WebFlux) is a later target, not part of the first acceptance.

## Decision

The target is:

| Item | Pin |
| :--- | :--- |
| Java SE API | 17 (class-file version 61) |
| Spring Framework | 6.2.19 |
| Spring Boot | 3.5.16 |
| Embedded Tomcat | 10.1.55 |
| Reference JDK | 17 |
| Reference host | Linux arm64 |

The versions live in `compat/targets.toml` (proposed) and are printed by
`rava compat target`. A bump is a **re-baseline**: regenerate the manifest, reopen the
gap count and rerun the gates. It is never a silent dependency update.

The first reference application is a minimal servlet web application: one `text/plain`
endpoint, embedded Tomcat, no Jackson, no database, no Actuator, no security starter.
Its repository and commit are created and pinned as the first task of M1; no external
application is assumed.

### Java 17 identity and system properties

The VM reports a **Java 17 compatibility profile**. The fixed properties are part of
the contract; host-derived properties come from the reference host.

| Property | Value |
| :--- | :--- |
| `java.version` | `17.0.12` |
| `java.specification.version` | `17` |
| `java.vm.specification.version` | `17` |
| `java.class.version` | `61.0` |
| `java.vm.name` | `rava` |
| `java.vm.vendor` | `rava` |
| `java.vendor` | `rava` |
| `java.runtime.name` | `rava Runtime Environment` |
| `java.runtime.version` | `17.0.12+rava` |
| `java.vm.version` | `17.0.12+rava` |
| `java.vm.info` | `interpreter` |
| `java.specification.vendor` | `Oracle Corporation` |
| `java.vm.specification.vendor` | `Oracle Corporation` |
| `os.name` / `os.arch` / `os.version` | `Linux` / `aarch64` / host kernel |
| `file.separator` / `path.separator` / `line.separator` | `/` / `:` / `\n` |
| `file.encoding` / `sun.jnu.encoding` | `UTF-8` / `UTF-8` |
| `user.dir` / `user.home` / `user.name` / `java.io.tmpdir` | host values |
| `user.language` / `user.country` / `user.timezone` | host values |
| `java.home` | the rava installation directory |
| `java.class.path` | the launcher's class path |
| `java.library.path` | empty until a JNI target exists |
| `java.awt.headless` | `true` |
| `sun.java.command` | main class and arguments |

Rules:

- `java.version`, `java.class.version`, the specification versions and
  `java.vm.name` are **fixed**; they cannot be overridden by `-D`.
- Host-derived and user properties follow the Java 17 `-D` precedence; the VM supplies
  their defaults.
- `java.class.version` is `61.0` because this is a Java 17 profile, even though the
  loader may accept newer class files internally.
- Spring Boot 3.5.16's `JavaVersion` does **not** parse `java.version`; it probes for
  APIs such as `Console.isTerminal` and `Reader.readAllLines`. The Java 17 profile must
  therefore not expose Java 18+ methods, and the acceptance test is
  `JavaVersion.getJavaVersion() == SEVENTEEN`.

**Reactive is deferred, not refused.** Spring WebFlux is a later target with its own
manifest and its own server decision. If that server is Reactor Netty, it reaches
`Unsafe`, direct memory and `VarHandle` in ways the servlet target does not; the
`Unsafe` tiers in ADR-0005 and the S2-raw path are the relevant boundary. The server
choice is deferred to M9.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Freeze 6.2.0 / 3.4.0 | The audit used those versions, but the target is the Boot 3.5.16 BOM: Spring 6.2.19 and Tomcat 10.1.55. |
| Java 21 | Larger API surface (virtual threads, newer `java.lang` changes) before the first application runs; a second target after C4. |
| Spring Framework 7 / Boot 4 | Newer baseline and ecosystem; more unknowns for a first target. |
| Reactive first (WebFlux + Netty) | Much larger C3 surface and an `Unsafe`/direct-memory dependency before the servlet path works. Servlet first, reactive second. |
| Full Java SE 17 | Unbounded; most of the API is never reached (ADR-0003). |

## Consequences

**Positive**

- The first target is small enough to finish and precise enough to test.
- Every gate and every manifest entry is tied to a known jar set.
- The Java 17 property profile is explicit and testable.

**Negative / accepted costs**

- The 6.2.19 / 3.5.16 / 10.1.55 pins move; a patch bump invalidates measured counts and
  must be scheduled as a re-baseline.
- Reactive support is not delivered by the first target.
- The reference application is a project artifact that must be built and pinned with
  the same care as the runtime; its commit is an M1 deliverable, not an existing pin.
