# Architecture Decision Records

Every record below was accepted on 2026-11-23. An accepted record is immutable and is
superseded, never rewritten.

| ADR | Decision | Status |
| :--- | :--- | :--- |
| [0001](0001-compatibility-first.md) | The success criterion is a pinned external application, not a corpus | accepted |
| [0002](0002-target-platform-and-versions.md) | Java 17, Spring 6.2.19, Boot 3.5.16, Tomcat 10.1.55 | accepted |
| [0003](0003-compatibility-contract.md) | The contract is an observed surface manifest plus C0–C4 gates | accepted |
| [0004](0004-runtime-class-library.md) | The runtime library is a full replacement, owned by us | accepted |
| [0005](0005-class-loading-and-dynamic-code.md) | Loaders, `defineClass`, `Proxy`, `Lookup`, and `Unsafe` capability/fidelity tiers U0–U4/S0–S2 | accepted |
| [0006](0006-reflection-and-metadata.md) | Reflection must reproduce JDK 17 metadata, not just method dispatch | accepted |
| [0007](0007-concurrency-and-memory-model.md) | Real OS threads and the Java memory model are required | accepted |
| [0008](0008-references-and-cleaner.md) | Weak/soft/phantom references, `Cleaner` and class unloading | accepted |
| [0009](0009-resources-jars-and-modules.md) | Jars, nested jars, resources, `ServiceLoader` and `-jar` | accepted |
| [0010](0010-execution-and-performance.md) | Measure first; choose interpreter/JIT against a budget | accepted |
| [0011](0011-scope-ordering-and-native-boundary.md) | Scope ordering, promotion rule and native/internal boundary | accepted |
| [0012](0012-runtime-foundation-and-upgrade-seams.md) | M0 foundation seams and target classification | accepted |

## Format

An ADR has four sections: Context, Decision, Alternatives, Consequences. Status is one
of `proposed`, `accepted`, `rejected`, `superseded`. Numbers are never reused.
