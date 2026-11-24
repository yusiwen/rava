# rava

A Java 17 runtime in Rust, built to run Spring — not just a corpus.

## Status

**Decisions accepted (2026-11-23). M0 is complete; M1.1, M1.2 and M1.3 are done
(2026-11-24):** workspace, parser, loader identity, heap/seams, the interpreter core,
Hello, safepoint/GC-root and no-lock-across-Java-call invariants, the verifier
dataflow skeleton with the metadata dump fixture, the pinned target lock with the
reference application and its 35-jar set, the static surface manifest
(`compat/surface/spring-boot-3.5.16.txt`: 980 classes, 177 fields, 3,626 methods,
1,048 interface methods), and the signature-only stub library under `stdlib/classes/`
(980 class files) with a zero-unresolved link-check. **M1.4 (manifest reproducibility
and M1 gate review) is next.**

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

The Spring, Boot and Tomcat versions are pinned in `compat/targets.toml`, together
with the reference application's source hash; `compat/jars.lock` pins the sha256 of
every jar in the flat `compat/jars/` set. The jars are fetched, not committed.
Reactive web (Spring WebFlux) is a later target, not part of the first acceptance.

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

The pinned target and jar set are checked with:

```sh
cargo run -p rava-cli -- compat target
bash compat/fetch-jars.sh --check
```

The static surface manifest is generated and checked with:

```sh
cargo run -p rava-cli -- compat manifest
cargo run -p rava-cli -- compat manifest --check
```

The signature-only stub library is generated and link-checked with:

```sh
cargo run -p rava-cli -- compat stubs
cargo run -p rava-cli -- compat link-check
```

`stdlib/classes/` is a generated deliverable, not the runtime class library; see
[`stdlib/README.md`](stdlib/README.md) for what the stubs do and do not contain.

`bash compat/fetch-jars.sh` (re)fetches the jars from Maven Central and needs
`JAVA_HOME` and Maven; `--check` verifies the existing directory without fetching.

## License

MIT.

## Review order

1. Read the requirements.
2. Read the resolved decision record.
3. Read the ADRs in order; 0001–0005 define the project, 0006–0011 the platform
   surfaces.
