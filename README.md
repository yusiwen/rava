# rava

A Java 17 runtime in Rust, built to run Spring — not just a corpus.

## Status

**M0, M1, M2 and M3 are complete (2026-11-25); M4 is next.** A pinned Spring
`AnnotationConfigApplicationContext` starts on rava, returns its `hello` bean and exits
0; the C2 gate additionally runs Proxy, CGLIB, Objenesis, Spring AOP and an
`Unsafe` U0-only call log. The [project ledger](PROJECT.md) owns the per-slice record,
commands and evidence.

| Milestone | Scope | Status |
| :--- | :--- | :--- |
| M0 | Foundation: parser, loader identity, heap, interpreter, verifier skeleton | done |
| M1 | Target lock, surface manifest, stub library and link-check | done |
| M2 | C1 metadata, minimal class library, first Spring context | done |
| M3 | C2 dynamic code: `Proxy`, CGLIB, Objenesis, `Lookup`, `Unsafe` U0 | done |
| M4 | C3 resources and jars, `-jar`, non-web Boot | planned |
| M5 | C3 concurrency and the Java memory model, references, cleaner, Tomcat | planned |
| M6 | C4 reference web application | planned |
| M7 | Spring test subset | planned |
| M8 | Performance envelope | planned |
| M9 | Reactive target | planned |

Proven today: `compat check` resolves 1,047 classes and 4,851 member records with 0
unresolved; the C1 metadata diff is 0 across 1,011 referenced classes; the workspace
passes 487 tests plus `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets -- -D warnings`; and
`RAVA_C2_ACCEPTANCE=1 cargo test -p rava-cli --test c2_gate` exits 0 with real Proxy,
CGLIB, Objenesis, Spring AOP and an `Unsafe` U0-only log. Known gaps are listed in the
[M3 history note](docs/notes/M3.md); the local long/double slot bug (#52) is fixed, and
`Class.getFields` (#53) is implemented against the observed C1 tree, whose 69/35/44
closure boundary is documented in the
[C1 explanation](docs/explanation/c1-metadata.md#c1-closure).

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
- [Explanation index](docs/explanation/README.md) — how the mechanisms run, starting with the C1 metadata diff and the Spring context.
- [History notes](docs/notes/README.md) — what happened, including the M2 Spring blocker chain and the M3 CGLIB/AOP lessons.
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

A class runs from a directory or jar classpath with:

```sh
cargo run -p rava-cli -- run demo.HelloRun --stdlib stdlib/c1/classes --cp crates/cli/tests/fixtures/run/classes
```

The same class is committed as a stored jar, so
`--cp crates/cli/tests/fixtures/run/HelloRun.jar` is the jar form; `--cp` is repeatable
and colon-separated. The runner's tests need no JDK:

```sh
cargo test -p rava-cli --test classpath_run
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

The local M1 compatibility gate runs all four stages and prints `compat check: ok`
only when they pass:

```sh
cargo run -p rava-cli -- compat check
```

It reads the already-fetched `compat/jars/` (the manifest stage scans the pinned
class files) but needs no network. It does not re-hash the jars; that stays with
`bash compat/fetch-jars.sh --check`.

The C1 metadata model and its normalized dump are tested against committed Java 17
fixtures (no JDK needed at test time):

```sh
cargo test -p rava-classfile --test metadata_model
```

The JDK 17 C1 signature stubs are generated from the host `ct.sym` and checked against
it. The reference is found through `--ct-sym <path>`, `RAVA_CT_SYM`, or `JAVA_HOME`;
the local acceptance uses the JDK 25 archive at
`$JAVA_HOME/lib/ct.sym` (for example
`/home/yusiwen/.local/share/mise/installs/java/zulu-25/lib/ct.sym`). The check needs no
jars and no execution, and the test suite needs no JDK:

```sh
cargo run -p rava-cli -- compat c1-stubs
cargo run -p rava-cli -- compat metadata --check
```

The bootstrap loader reads the committed C1 tree through a `DirectorySource`; the
runtime loading test needs no JDK:

```sh
cargo test -p rava-vm --test c1_runtime
```

`stdlib/c1/classes/` is the committed C1 class library the bootstrap loader reads;
`stdlib/classes/` is the generated M1 surface-stub deliverable, not the runtime library.
See [`stdlib/README.md`](stdlib/README.md) and
[`stdlib/c1/README.md`](stdlib/c1/README.md) for what each tree does and does not
contain.

`bash compat/fetch-jars.sh` (re)fetches the jars from Maven Central and needs
`JAVA_HOME` and Maven; `--check` verifies the existing directory without fetching.

## License

MIT.

## Review order

1. Read the requirements.
2. Read the resolved decision record.
3. Read the ADRs in order; 0001–0005 define the project, 0006–0011 the platform
   surfaces.
