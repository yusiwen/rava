# Compatibility gates: C0–C4 and the commands behind them

## Question

How are C0–C4 measured, which artifact and command owns each level, and where does
`cargo xtask ci` fit?

## Two kinds of gate

The project has two different kinds of green:

- **Engineering gate** — `cargo xtask ci`: `cargo fmt --all --check`, clippy with
  `-D warnings`, and the workspace tests. It says the code is formatted, lint-clean and
  unit/integration-tested. It does not measure compatibility.
- **Compatibility gates** — C0–C4 from
  [ADR-0003](../adr/0003-compatibility-contract.md): progressively stronger statements
  about the pinned external target. They use `rava compat …` commands and, for C3/C4,
  the Spring/reference-application suites.

Do not substitute one for the other. A green `cargo xtask ci` does not mean C0 is
complete, and a successful Spring smoke does not replace the C0/C1 checks.

## The levels

| Level | Statement | Evidence artifact | Current command | Status |
| :--- | :--- | :--- | :--- | :--- |
| **C0 — Link** | Every symbolic reference in the observed surface resolves in our library. | `compat/surface/spring-boot-3.5.16.txt` + `stdlib/classes/` | `rava compat check` | Pass for the observed manifest: 1,047 classes, 4,851 member records, `unresolved 0`. |
| **C1 — Metadata** | The manifest classes' metadata has the JDK 17 shape. | `stdlib/c1/classes/` + `compat metadata --check` | `rava compat metadata --check` | Qualified: 1,011 classes with a `ct.sym` reference, `differences 0`; 36 placeholders excluded. |
| **C2 — Dynamic** | Proxy, `defineClass`, CGLIB, Objenesis, `Lookup`, `Unsafe` U0 and generated-bytecode verification work. | C2 suite (M3) + generated classes | `RAVA_C2_ACCEPTANCE=1 cargo test -p rava-cli --test c2_gate` | Pass for the gate: real Spring AOP, CGLIB, Objenesis and Proxy; U0-only log. |
| **C3 — Runtime** | The exercised collections, concurrency, I/O, NIO, networking, charsets, regex, `java.beans`, logging, jars/resources and `ServiceLoader` behave; the Spring context starts. | API tests and `spring_context` | Narrow Spring smoke: `RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context` | Partial: one real Spring context with one `@Bean`. |
| **C4 — Framework** | The reference Boot web application and selected Spring tests run unmodified. | HTTP suite / Spring subset (M6–M7) | Not implemented | Future (M6–M7). |

The definitions and promotion rule are owned by
[ADR-0003](../adr/0003-compatibility-contract.md) and
[ADR-0011](../adr/0011-scope-ordering-and-native-boundary.md). This document owns how
the current gates are wired, not the definitions themselves.

## C0: `compat check`

`rava compat check` is the local C0 gate. It runs four stages in order:

1. **Target lock + jars** — parse `compat/targets.toml`, verify the pinned versions,
   the jar directory and the jar count against `compat/jars.lock`. Per-jar hashes are
   checked separately by `bash compat/fetch-jars.sh --check`.
2. **Manifest --check** — regenerate the surface manifest from the 35 pinned jars and
   compare it byte-for-byte with the committed
   `compat/surface/spring-boot-3.5.16.txt`.
3. **Link-check** — resolve every manifest class/field/method/interface-method record
   against the declarations in the M1 synthetic stub tree `stdlib/classes/`. A miss or
   an ambiguous member is unresolved; the command exits 1.
4. **Stub determinism** — regenerate the synthetic stubs into a temporary directory and
   compare all class files with the committed tree.

Current output:

```console
$ cargo run -p rava-cli -- compat check
  target lock + jars  ok (java 17, 35 jars)
  manifest --check    ok (1047 classes, 177 fields, 3626 methods, 1048 interface-methods)
  link-check          ok (1047 classes, 4851 member records, unresolved 0)
  stub determinism    ok (1047 class files byte-identical)
compat check: ok
```

The important boundary: this is **symbol resolution against the observed manifest**, not
a full load/link/verify pass over all 1,047 classes. `link-check` resolves each record
against declarations on its owner; it does not walk superclasses/interfaces and it does
not execute or verify the stubs. The Spring smoke loads and runs a subset through the
real runtime, but it is not a C0 proof for the whole surface.

The M1 source of truth is the manifest and the synthetic tree; [`stdlib/README.md`](../../stdlib/README.md)
owns their exact construction rules.

## C1: `compat metadata --check`

`rava compat metadata --check` compares the normalized metadata dump of
`stdlib/c1/classes/` with the release-17 view of the host JDK's `lib/ct.sym`:

```console
$ cargo run -p rava-cli -- compat metadata --check
  classes           1047
  from ct.sym       1011
  unreferenced      36
  fields            1160
  methods           13359
  differences       0
metadata: ok
```

The C1 evidence is deliberately qualified:

- 1,011 manifest owners have a release-17 `ct.sym` signature and are compared;
- 36 owners have no Java 17 signature and are committed placeholders, listed in
  `compat/surface/c1-unreferenced.txt`, excluded from the diff;
- the comparison is a **normalized class-file metadata diff**, not a live JDK 17
  reflection differential.

The full interpretation, including what “differences 0” proves and does not prove, is
in [`c1-metadata.md`](c1-metadata.md). The tree's own state is owned by
[`stdlib/c1/README.md`](../../stdlib/c1/README.md).

## C2–C4: status and wiring

**C2** is shipped as the M3 gate:

```console
$ RAVA_C2_ACCEPTANCE=1 cargo test -p rava-cli --test c2_gate
running 2 tests
test spring_aop_proxies_run_through_the_verifier ... ok
test the_gate_touches_only_the_unsafe_u0_surface ... ok
```

It runs a real Spring context with `proxyBeanMethods = true`, a bare CGLIB
`Enhancer`, Objenesis, a JDK `Proxy`, and Spring AOP `ProxyFactory` in JDK and CGLIB
modes. It asserts the generated classes are in the application loader, every method
with `Code` verifies, and the `Unsafe` log contains only U0. The run needs the fetched
`compat/jars/`; the command sets `RAVA_C2_ACCEPTANCE=1` so a missing jar fails rather
than silently skipping. The mechanism is explained in
[`dynamic-code.md`](dynamic-code.md); the blocker chain is in
[`docs/notes/M3.md`](../notes/M3.md). This does not make every C2 surface complete:
Access checks remain open, and the verifier is not a full type verifier. The mixed
long/double local layout (#52) and `Class.getFields` (#53) are fixed; #53 also documents
the C1 closure tolerance rather than claiming a closed tree.

**C3** is partially exercised by the Spring smoke:

```console
$ RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context
test spring_annotation_context_starts_and_exposes_the_bean ... ok
```

The test is opt-in because it needs the fetched `compat/jars/`. On a CI checkout without
the jars it prints a skip and returns; `cargo xtask ci` therefore stays green without
claiming C3. The run proves one real `AnnotationConfigApplicationContext` starts, one
`@Bean String` is created, and the context closes. It does not prove the broader C3
surface or any Boot/web path. The runtime path is explained in
[`spring-context.md`](spring-context.md).

**C4** has no implementation yet; the reference application and HTTP/Spring suites are
M6–M7. The 35-jar target and the `reference-app` source are pinned now so the later
gates have stable inputs; pinning is not passing.

## How a gate is promoted

A deferred capability becomes a target only through an ADR that names:

1. the application or library that needs it;
2. the manifest it adds;
3. the acceptance command and gate;
4. the boundary it crosses (native/JNI, JDK-internal, platform, security).

This is the ADR-0011 promotion rule. For this project it means a new `compat` command or
test suite is not enough on its own: the target lock, manifest, acceptance command and
`PROJECT.md` slice must change together.

## Evidence rules

- A gate claim names the command, the exit code and the artifact/count it produced.
- A skip is not a pass. The Spring test's skip is explicitly not C3 evidence.
- A manifest/stub change is a re-baseline: regenerate both trees, update the counts in
  `PROJECT.md`/README, and re-run `compat check` and `compat metadata --check`.
- `cargo xtask ci` remains required even when a compatibility command passes; it owns
  formatting, lints and the unit/integration suite.

The machine-readable state, slice log and dates live in
[`PROJECT.md`](../../PROJECT.md); this document does not duplicate them.

## Owners

- [ADR-0003](../adr/0003-compatibility-contract.md) — C0–C4 definitions and manifest contract.
- [ADR-0011](../adr/0011-scope-ordering-and-native-boundary.md) — promotion/deferral rule.
- [`PROJECT.md`](../../PROJECT.md) — current phase/slice/gate state.
- [`c1-metadata.md`](c1-metadata.md) — the C1 diff's exact interpretation.
- [`spring-context.md`](spring-context.md) — the C3 Spring smoke path.
- [`stdlib/README.md`](../../stdlib/README.md), [`stdlib/c1/README.md`](../../stdlib/c1/README.md) — the two generated trees.
