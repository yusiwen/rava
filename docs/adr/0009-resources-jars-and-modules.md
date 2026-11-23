# ADR-0009 — Jars, resources, `ServiceLoader` and `-jar`

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

A directory-only classpath is enough for a corpus but not for Spring Boot. Boot
packages an executable jar whose classes live in nested jars, loads
`META-INF/spring.factories`/`AutoConfiguration.imports` as classpath resources, and
relies on `ServiceLoader` for pluggable components. Tomcat reads
`META-INF/services`, TLDs and web resources. A runtime without jar reading, resource
enumeration and `java -jar` cannot start the reference application.

The class-file version is not the hard part; the resource and loader protocol is.

## Decision

Implement:

- `java.util.zip`/`java.util.jar` reading, including central-directory parsing and
  DEFLATE (a reviewed, permissively licensed dependency or an in-tree implementation;
  the dependency choice is recorded during M4).
- Nested-jar URLs (`jar:file:...!/BOOT-INF/lib/...!/`) and a `URLStreamHandler` /
  `URLClassLoader` path sufficient for Boot's launcher.
- `ClassLoader.getResource`, `getResources`, `getResourceAsStream` and the
  `Class.getResource` variants, with the same delegation order as the reference JDK.
- `ServiceLoader` over `META-INF/services`, including lazy iteration and error
  reporting.
- Module descriptors: parse and expose `Module` metadata; classpath-only execution for
  the first target. A full module layer is a later target, not a first-target promise.
- `rava -jar <file>` reads `META-INF/MANIFEST.MF`, honors `Main-Class` and
  `Class-Path`, and starts the application.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Directory classpath only | Cannot run a Boot fat jar; misses resources and `ServiceLoader`, which frameworks use constantly. |
| Support only flat jars | Boot's nested layout is the target's layout; a flat-only reader fails at the first starter. |
| Implement the full JPMS module layer | Larger than the target needs; Boot runs on the classpath by default. |
| Shell out to `unzip` and treat the result as directories | Breaks resource URLs, nested jars and the application's class-loader identity; not a runtime feature. |

## Consequences

**Positive**

- The reference Boot application runs in its shipped form, with no repackaging.
- Resources and `ServiceLoader` become first-class, testable surfaces.
- `-jar` gives a realistic launch path that mirrors production.

**Negative / accepted costs**

- DEFLATE is a non-trivial dependency or implementation; the choice needs a licence and
  supply-chain review.
- URL handlers and nested-jar paths are fiddly and security-sensitive.
- Module descriptors may be parsed long before a module layer exists; the API must say
  what is not supported.

## Acceptance

- `rava -jar reference-app.jar` starts the application and serves the endpoint.
- A jar with resources and a `META-INF/services` entry loads both correctly.
- Nested-jar class loading and resource lookup work with Boot's launcher.
- Invalid or corrupt jars produce a typed error, not a panic.
