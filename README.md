# rava

A Java 17 runtime in Rust, built to run Spring — not just a corpus.

## Status

**Decisions accepted (2026-11-23). M0 is complete:** workspace, parser, loader
identity, heap/seams, the interpreter core, Hello, safepoint/GC-root and
no-lock-across-Java-call invariants, and the verifier dataflow skeleton with the
metadata dump fixture. **M1 is next.**

## What this project is

rava is a compatibility-first Java runtime. Its success criterion is a pinned Spring
Boot application running unmodified on the reference JDK's jars: linked, reflected
over, dynamically subclassed, served over HTTP, and measured against a performance
budget.

The runtime class library is ours: `java.*` classes are real class files with Rust
implementations, and the real `java.base` is never loaded at run time. "The JCL is
Rust" is a property of the design, not a slogan.

The scope is not "all of Java SE". It is an observed surface manifest: the classes,
members, annotations and signatures the target actually reaches, versioned with the
target and enforced by C0–C4 gates.

## Target

| Item | Value |
| :--- | :--- |
| Platform | Java SE 17 API subset, class-file version 61 |
| Spring Framework | 6.2.19 |
| Spring Boot | 3.5.16 |
| Embedded server | Tomcat 10.1.55 |
| Reference application | servlet web app, `text/plain` endpoint, no JSON, no database |
| Reference JDK | 17 |

The Spring, Boot and Tomcat versions are pinned in `compat/targets.toml`; the reference
application's commit is created and pinned as the first M1 task. Reactive web
(Spring WebFlux) is a later target, not part of the first acceptance.

## Documents

- [Requirements](docs/requirements.md) — R1–R13, C0–C4, milestones, performance,
  scope ordering.
- [Open questions](docs/open-questions.md) — the resolved decision record.
- [ADR index](docs/adr/README.md) — the accepted decision stack.
- [Project ledger](PROJECT.md) — phase status, milestones, decision log.

Start with the requirements and the ADRs; the open questions are all resolved and
remain as a decision record.

## What this project is not

- Not a full Java SE implementation in the first target.
- JNI and native libraries are not in the first target; they are a later target (Q17).
- Not a reactive runtime yet; WebFlux is a committed later target.

## Development

The Rust toolchain is pinned by `rust-toolchain.toml`; the local gate is:

```sh
cargo xtask ci
```

## License

MIT.

## Review order

1. Read the requirements.
2. Read the resolved decision record.
3. Read the ADRs in order; 0001–0005 define the project, 0006–0011 the platform
   surfaces.
