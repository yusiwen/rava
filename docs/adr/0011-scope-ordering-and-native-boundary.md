# ADR-0011 — Scope, ordering, and the native boundary

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

A compatibility project fails in two directions: it grows to "all of Java SE" and
never finishes, or it turns every current limitation into a permanent exclusion and
cuts off legitimate later targets. The first failure is a scope problem; the second is
a decision problem.

The owner's position is explicit: **do not record permanent non-goals.** An item that
is not supported now must still be considered for later support. The boundary should
say *what the first target does not include and in what order it may be added*, not
*what this project will never do*.

That does not make the first target unbounded. Ordering plus a promotion rule keeps
scope finite while leaving the future open.

## Decision

**Ordering, not prohibition.** Every exclusion below is a statement about the first
target, not a permanent refusal. Promoting an item requires a new or superseding ADR
that names the target, the manifest it brings, its acceptance gate and — where
relevant — its trust model. It does not require rewriting this record.

**Not in the first target** (ordered after the servlet target):

- General JNI, `System.loadLibrary`, and loading arbitrary native libraries.
- AWT, Swing, JavaSound, JavaFX and other desktop surfaces. `java.beans` is allowed
  headless because Spring's introspection needs it.
- Arbitrary `jdk.internal.*` access; only a declared allowlist is implemented.
- SecurityManager, JVMTI, Java agents, AppCDS and native-image.
- Full Java SE API coverage and bug-for-bug HotSpot fidelity.
- Windows and macOS as first-class hosts; Linux arm64 is the reference host.
- The full Spring portfolio: Jackson/JSON, JPA/Hibernate, Netty, security starters and
  databases.
- Reactive web (Spring WebFlux), which is a committed later target (ADR-0002).

**Promotion rule.** A deferred item becomes a target only through an ADR that names:

1. the application or library that needs it;
2. the manifest it adds;
3. the acceptance command and the gate;
4. the boundary it crosses (JNI/native, JDK-internal, platform, security).

For JNI and native libraries the ADR must also define the ABI scope, the trusted-native
model and the failure policy (a native crash is not a VM bug). For JDK-internal access
it must extend the allowlist. For a new platform it must name the host and the CI job.

**Boundaries that still hold while an item is deferred:**

- `Unsafe` follows ADR-0005's capability/fidelity tiers (U0–U4/S0–S2); only U0 (S1)
  is in the first target. Unopened tiers fail loudly.
- No raw addresses are promised while U3 is S1; S2-raw or S2-objects requires a
  superseding ADR.
- A declared `sun.misc`/`jdk.internal` allowlist is checked in CI.
- `java.vm.name` reports `rava`; Objenesis takes the `Unsafe.allocateInstance` path.
- "Native" in this project means Rust implementations in the VM's native registry, not
  JNI. JNI is a separate, later target with its own ABI.

**Reconsideration triggers.** A deferred item is promoted when a target library needs
it, when a manifest entry appears, when a platform requirement is added, or when the
owner decides the ordering should change.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Permanent non-goals | The owner's decision: it cuts off later support and turns a scheduling statement into an identity statement. |
| Implement everything now | Unbounded; the first target would never reach C4 and the manifest would lose its meaning. |
| Support JNI early | No target library or manifest entry needs it yet; it is a large ABI and trust boundary that would delay the pure-Java path. It is deferred, not refused. |
| Allow arbitrary `jdk.internal.*` | Every JDK release would reopen the runtime; the allowlist keeps the obligation finite. |

## Consequences

**Positive**

- No future capability is ruled out by a document written before the need exists.
- Scope is still bounded by the first target and by the promotion rule.
- Each deferred item has a path: an ADR, a target, a manifest, a gate.

**Negative / accepted costs**

- The project must revisit deferred items deliberately; "later" can become "never" if
  no one schedules it. The Later targets table in `docs/requirements.md` is the
  reminder.
- Some popular libraries (Netty, Hibernate, Jackson, native JDBC drivers) are
  unavailable in the first target.
- A JNI target will be expensive when it arrives; deferring it does not make it cheap.

## Acceptance

- No "permanent non-goal" language remains in the requirements or ADRs.
- The allowlist check exits 0 and fails on an undeclared `sun.*`/`jdk.internal.*`
  reference.
- A runtime `Unsafe` call log contains only methods from open tiers (ADR-0005; U0 at
  M3).
- The first target has no JNI shim or native-library loader; JNI is tracked as a later
  target (Q17).
