# PROJECT — rava (a Java 17 runtime for Spring)

**Tier:** L
**Owner:** yusiwen
**License:** MIT
**Started:** 2026-11-22

> Ledger. P0 and P1 were accepted on 2026-11-23. M1 is complete; M2 is in progress
> (M2.1, M2.2 and M2.3 done). Keep this file honest and terse.

## Phase status

| Phase | What must be true | Artifact | Approved by | Status |
| :--- | :--- | :--- | :--- | :--- |
| P0 | Requirements confirmed by the human | `docs/requirements.md` | human | done |
| P1 | Decisions recorded and accepted | `docs/adr/` | human | done |
| P2 | Work sliced into verifiable increments | `PROJECT.md` | self | done |
| P3 | Every slice implemented | code + commits | self | todo |
| P4 | Tests pass, definition of done met | test suite | self | todo |
| P5 | Deployed, smoke-tested, rollback ready | `docs/deploy.md` | human | n/a |

P5 is `n/a` because this project ships a runtime library, not a hosted service; a
release process, if wanted, becomes its own phase.

## Milestones

> Gates are defined in `docs/requirements.md` § Milestones. P0 and P1 are accepted;
> M0 and M1 are complete, and both gate reviews have passed. M2 is in progress, with
> M2.1, M2.2 and M2.3 done.

| Milestone | Delivers | Gate | Status |
| :--- | :--- | :--- | :--- |
| M0 | Foundation: parser/model, loader/ClassSource, heap/IR, Shared/ThreadState, safepoints, verifier skeleton, native registry, MemoryRegion | Hello + small corpus; metadata dump; loader identity; no-lock invariant | done |
| M1 | Surface manifest, target lock, `link-check` | Manifest reproducible; unresolved = 0 | done |
| M2 | C1 metadata, minimal library, Spring context | Metadata diff 0; bean graph starts | in-progress |
| M3 | C2 dynamic code: Proxy, CGLIB, Objenesis, `Lookup`, `Unsafe` U0 | C2 suite exits 0 | todo |
| M4 | C3 resources/jars, `-jar`, non-web Boot | Boot non-web application starts | todo |
| M5 | C3 concurrency, JMM, references, cleaner, Tomcat | Tomcat concurrent; JCStress 0 | todo |
| M6 | C4 reference web application | HTTP suite exits 0 | todo |
| M7 | Spring test subset | Spring subset exits 0 | todo |
| M8 | Performance envelope | ADR-0010 benchmark exits 0 | todo |
| M9 | Reactive target (after M0–M8) | Reactive HTTP suite exits 0 | todo |

## Slices

| Id | Slice | Requirements | Acceptance command | Status |
| :--- | :--- | :--- | :--- | :--- |
| M0.1 | Cargo workspace, toolchain, crates and the `xtask ci` gate | R13 | `cargo xtask ci` exits 0 | done |
| M0.2 | Class-file parser with full metadata retention | R13 | `cargo test -p rava-classfile` exits 0 | done |
| M0.3 | ClassSource and `(loader, name)` class identity | R13 | `cargo test -p rava-vm --test loader_identity` exits 0 | done |
| M0.4 | Heap handles, field layout and the `Shared`/`ThreadState` seams | R13 | `cargo test -p rava-vm --test heap_handles` exits 0 | done |
| M0.5a | Decoded IR, frames and a static call | R13 | `cargo test -p rava-interp --test arithmetic` exits 0 | done |
| M0.5b | Hello through the interpreter: class library and native minimum | R13 | `cargo test -p rava-interp --test hello` exits 0 | done |
| M0.6 | Safepoint polling, GC roots and the no-lock-across-Java-call invariant | R13 | `cargo test -p rava-vm --test safepoints` exits 0 | done |
| M0.7 | Verifier dataflow skeleton and the metadata dump fixture | R13 | `cargo test -p rava-vm --test verifier_skeleton` exits 0 | done |
| M1.1 | Target lock, reference app and pinned jar set | R2 | `cargo run -p rava-cli -- compat target` exits 0 | done |
| M1.2 | Static manifest over the pinned jars | R1 | `cargo run -p rava-cli -- compat manifest --check` exits 0 | done |
| M1.3 | Stub library surface and zero-unresolved link-check | R2 | `cargo run -p rava-cli -- compat link-check` exits 0 | done |
| M1.4 | Manifest reproducibility and M1 gate review | R1, R2 | `cargo run -p rava-cli -- compat check` exits 0 | done |
| M2.1 | C1 metadata model and normalized dump | R3 | `cargo test -p rava-classfile --test metadata_model` exits 0 | done |
| M2.2 | JDK 17 signature stubs and zero metadata diff | R3 | `cargo run -p rava-cli -- compat metadata --check` exits 0 | done |
| M2.3 | `stdlib/c1/classes` as the bootstrap library and its metadata through the VM | R3 | `cargo test -p rava-vm --test c1_runtime` exits 0 | done |
| M2.4 | Class objects and basic `Object`/`Class` reflection | R3 | `cargo test -p rava-interp --test class_objects` exits 0 | todo |

P0 and P1 are accepted; P2 is done because the M2 increments are sliced. All M0 and
M1 slices are done, and both gate reviews passed. M2.1, M2.2 and M2.3 are done; M2.4 is
next.

## Slice log

| Slice | Acceptance command | Exit | Date | Evidence |
| :--- | :--- | :--- | :--- | :--- |
| M0.1 | `cargo xtask ci` | 0 | 2026-11-24 | Workspace, crates and the local gate; no runtime behavior yet |
| M0.2 | `cargo test -p rava-classfile` | 0 | 2026-11-24 | Parser implemented; raw attributes retained; 13 tests pass |
| M0.3 | `cargo test -p rava-vm --test loader_identity` | 0 | 2026-11-24 | Directory and DEFLATE jar sources; parent-first delegation; 6 tests pass |
| M0.4 | `cargo test -p rava-vm --test heap_handles` | 0 | 2026-11-24 | Generation handles, field layout, arrays and safepoint handshake; 6 tests pass |
| M0.5a | `cargo test -p rava-interp --test arithmetic` | 0 | 2026-11-24 | Decoded integer IR, frame, `invokestatic`; 3 tests pass |
| M0.5b | `cargo test -p rava-interp --test hello` | 0 | 2026-11-24 | `getstatic`/`ldc`/`invokevirtual`, class initialization and native PrintStream; 2 tests pass |
| M0.6 | `cargo test -p rava-vm --test safepoints` | 0 | 2026-11-24 | Frame-root stack with RAII cleanup, publish-before-acknowledge safepoint handshake, `Shared` root enumeration, lock probe at call boundaries, `MemoryRegion` seam and hook registration points; 9 tests pass; `cargo xtask ci` exits 0 |
| M0.7 | `cargo test -p rava-vm --test verifier_skeleton` | 0 | 2026-11-24 | Stack-height and definite-local dataflow over a branch/handler worklist, typed underflow/overflow/local/truncation/unsupported errors, deterministic metadata dump for `Hello.class`; 12 tests pass; `cargo xtask ci` exits 0 |
| M1.1 | `cargo run -p rava-cli -- compat target` | 0 | 2026-11-24 | `compat/targets.toml` pins Java 17, Boot 3.5.16, Spring 6.2.19, Tomcat 10.1.55, the reference app source hash and a 35-jar set; the reference app builds at release 17; a clean refetch reproduces all 35 sha256 entries and `bash compat/fetch-jars.sh --check` passes; `cargo test -p rava-cli` passes 17 tests |
| M1.2 | `cargo run -p rava-cli -- compat manifest --check` | 0 | 2026-11-24 | `compat/surface/spring-boot-3.5.16.txt` is generated by a static scan of all 11,754 `.class` entries in the 35 pinned jars: 980 classes, 177 fields, 3,626 methods and 1,048 interface methods under `java/`, `javax/`, `jdk/` and `sun/`; the manifest is deduplicated, sorted and byte-reproducible; `cargo test -p rava-cli` passes 25 tests |
| M1.3 | `cargo run -p rava-cli -- compat link-check` | 0 | 2026-11-24 | `rava compat stubs` writes 980 signature-only class files under `stdlib/classes/` from the committed manifest (177 fields, 3,626 methods, 1,048 interface methods); regeneration is byte-identical; link-check resolves all 980 classes and 4,851 member records with `unresolved 0`; `cargo test -p rava-cli` passes 41 tests; `cargo xtask ci` exits 0 |
| M1.4 | `cargo run -p rava-cli -- compat check` | 0 | 2026-11-24 | `compat check` passes all four stages: the target lock and its 35-jar set agree; the manifest regenerates byte-identically at 980 classes, 177 fields, 3,626 methods and 1,048 interface methods; link-check resolves 980 classes and 4,851 member records with `unresolved 0`; a temporary regeneration of `stdlib/classes` is byte-identical across all 980 class files and the committed tree is not rewritten; `cargo test -p rava-cli` passes 54 tests; `cargo xtask ci` exits 0 |
| M2.1 | `cargo test -p rava-classfile --test metadata_model` | 0 | 2026-11-24 | `rava-classfile`'s `metadata` module decodes the C1 attributes ADR-0006 lists into a typed model: access flags, binary name, superclass, interfaces, `Signature`, visible/invisible annotations with every element-value tag and repeatable containers as encoded, parameter and type annotations (all `target_info` variants and all four `type_path` kinds), nest host/members, permitted subclasses, inner and enclosing classes, `Record` components with their own annotations, `SourceFile`, `ConstantValue`, `Deprecated`, `Exceptions`, `MethodParameters`, `AnnotationDefault`; every attribute payload is still copied verbatim, so an undecoded attribute is retained; malformed payloads return `Error::BadAttribute` with a structured cause; a normalized dump with committed golden files for `Kitchen` and `Defaults`; 29 new tests, 43 `rava-classfile` tests; `cargo xtask ci` exits 0 |
| M2.2 | `cargo run -p rava-cli -- compat metadata --check` | 0 | 2026-11-24 | `stdlib/c1/classes/` holds 980 committed class files: 952 projected from the host JDK 25 `lib/ct.sym` release-17 view (1,104 fields, 13,008 methods, class-file version 61.0) and 28 header-only placeholders for manifest owners Java 17 does not define, listed in `compat/surface/c1-unreferenced.txt`; the normalized M2.1 metadata dump has 0 differences across the 952 referenced classes, the only normalization being the class-file version line (ct.sym stores 69); `stdlib/classes` is untouched and `compat check` still resolves 980 classes and 4,851 member records with `unresolved 0`; `cargo test -p rava-cli` passes 67 tests; `cargo xtask ci` exits 0 |
| M2.3 | `cargo test -p rava-vm --test c1_runtime` | 0 | 2026-11-24 | `stdlib/c1/classes` is the bootstrap class source through the existing `DirectorySource` and `(loader, name)` identity; `Shared::with_library_dir`, `Shared::load_library_class`, `Shared::library_metadata` and `ClassEntry::metadata()` decode C1 metadata without a `Code` attribute; the test loads `java.lang.String`, `java.lang.Class`, `java.util.List`, sealed `java.lang.reflect.Executable` and annotation type `java.lang.Deprecated`, checks superclass/interfaces/signatures/field and method descriptors/annotations/permitted subclasses, a second loader's distinct identity and no `Code` on any stub method; 7 tests; `cargo test -p rava-vm` passes 42 tests; `cargo xtask ci` exits 0 |

M0.6 boundary: the reference and class-unloading hooks are registration points only.
M0 has no collector, so no weak/soft/phantom reference is cleared and no class is
unloaded; M5 (ADR-0008) implements them on this seam. `MemoryRegion` is a VM-owned
byte region with a fake address, not an off-heap allocator; S2-raw replaces the
backing store without changing the address/bounds API.

M0 gate review (2026-11-24): `cargo xtask ci` exits 0; Hello and the integer corpus
run through the interpreter; the committed metadata dump matches `javap`'s structural
view of `Hello.class`; loader identity has six tests; the safepoint/no-lock suite has
nine tests. Reflection-level JDK 17 metadata diff remains the M2/C1 gate.

M0.7 boundary: the verifier tracks operand-stack slots and definitely assigned local
slots, not Java types. It covers straight-line code, conditional and unconditional
branches, and exception-handler entry; `jsr`/`ret`, `wide`, `tableswitch` and
`lookupswitch` return `UnsupportedOpcode`. Type states, stack-map frames and
generated-class verification land at M3 (ADR-0012). The metadata dump is a stable text
fixture, not a reflection API.

M1.1 boundary: `compat target` validates that the target lock parses, that its
`[jars].count` agrees with `compat/jars.lock`, and that `compat/jars/` holds the
declared number of jars. It does not hash the jars (that is
`bash compat/fetch-jars.sh --check`) and does not scan the manifest; M1.2 added the
static manifest and M1.3 the link-check.

M1.2 boundary: the manifest is the static half of ADR-0003 only. It parses every
`.class` entry in the pinned jars and records `CONSTANT_Class`, `CONSTANT_Fieldref`,
`CONSTANT_Methodref` and `CONSTANT_InterfaceMethodref` entries whose owner has a
`java/`, `javax/`, `jdk/` or `sun/` prefix; array class descriptors are skipped and
non-platform owners are filtered out. Types that appear only inside a descriptor are
not recorded unless the same class also appears as a `CONSTANT_Class`. The dynamic
load, reflective access and metadata steps of ADR-0003 are later slices, so the
manifest is a lower bound on the surface, not the full contract. Multi-release
`META-INF/versions/` entries are scanned as they appear in the jar; no version
selection is applied.

M1.3 boundary: `stdlib/classes/` is a signature-only stub library, not the runtime
library. Each manifest owner gets one class file; every field, method and
interface-method record is declared directly on its owner with the exact name and
descriptor, so inherited members are declared rather than resolved through a
superclass. Every class is public and abstract, an owner named by an
`interface-method` record is written as a public interface, and non-constructor
methods are `public abstract` with no `Code` attribute. `<init>` records carry a
minimal concrete body because JVMS §4.6 forbids an abstract constructor; `<clinit>`
is a no-op. The class header is synthetic: every class extends `java/lang/Object`,
and JDK 17 access flags, real superclasses and interface lists from `ct.sym` are not
preserved, so C1 metadata is not claimed. M2.2 adds the JDK 17 metadata stubs in a
separate tree (`stdlib/c1/classes/`), and M2.3 promotes that tree to the runtime
library; this M1 tree is not rewritten. `compat link-check` resolves each record against
the declarations on its owner; it does not walk superclasses or interfaces. It parses
the committed manifest and the stub class files only; it never reads the pinned jars,
never loads or executes a class, and never generates a missing stub.

M1 gate review (2026-11-24): the target is pinned (Java 17, Boot 3.5.16, Spring
6.2.19, Tomcat 10.1.55) and the jar set is checked as a set of 35 jars. The manifest
is reproducible: a fresh scan of the pinned jars regenerates the committed file
byte-for-byte at 980 classes, 177 fields, 3,626 methods and 1,048 interface methods.
Every manifest record resolves against the stub library with `unresolved 0` across
4,851 member records, and a temporary regeneration of `stdlib/classes` matches the
committed 980 class files byte-for-byte. The stubs are signature-only: abstract
classes and interfaces without JDK 17 superclasses, real interface lists or C1
metadata. `rava compat check` runs all four stages and exits 0; `cargo xtask ci`
exits 0. M2 (C1 metadata, the minimal library and the Spring context) is next.

M2.1 boundary: the model decodes the C1 attributes from a parsed `ClassFile`. A generic
`Signature` is retained as the encoded string; the generic `Type` graph and the
reflective API that reads the model are later M2 work, so no reflection native exists
yet.
Every attribute payload is also copied verbatim onto the class, field, method and record
component, so an attribute the model does not decode is never lost; `Code` stays the
interpreter's view and is not metadata. Code-level type annotations (`localvar`, `catch`,
`offset`, `type_argument`) have decoders but no fixture exercises them, because they
live inside `Code` and reflection does not expose them. The dump is a stable text
fixture, not a reflection API: it keeps class-file order for members, interfaces and
annotations, sorts nest members, permitted subclasses and inner classes, and emits no
host path, timestamp or hash.

M2.2 boundary: `stdlib/c1/classes/` is a second committed tree, separate from
`stdlib/classes/`; the M1 tree is unchanged and its gate still passes. 952 classes are
projected from the host JDK's `lib/ct.sym` release-17 entries: the signature is parsed
with `rava-classfile`, the class-file version is set to the pinned 61.0, and the class is
re-emitted with the constant pool at its original indices and every attribute payload
verbatim. The 28 manifest owners Java 17 does not define (the `Thread$Builder` classes,
JCache, Moneta, WSDL and the Java EE `javax.annotation.Resource`) are listed in
`compat/surface/c1-unreferenced.txt` and get header-only placeholders; the generator
fails, naming the owners, when the archive and that list disagree. `rava compat metadata
--check` re-reads the release-17 signature and compares the M2.1 normalized dump with
the committed stub: 952 classes compared, 0 differences. The only normalization is the
class-file version line, because ct.sym stores the host JDK's version (69) while the tree
is pinned to 61.0; ordering, annotations, signatures and raw attributes are not
normalized. There is no `Code`, no verifier work and no reflection native: this slice
establishes the metadata shape and the diff, not behaviour. Generic signatures are
retained as strings, not turned into a `Type` graph. The writer round-trips every
committed M2.1 fixture byte-for-byte; a lone surrogate in a string constant would not
round-trip, because the model decodes it lossily. M2.3 promoted this tree to the runtime
library.

M2.3 boundary: `stdlib/c1/classes` is the bootstrap class source. It is read through the
existing `DirectorySource` and `(loader, name)` identity, so there is no second class
table and no second identity model. `Shared::with_library_dir` is a constructor over that
source; `Shared::load_library_class`, `Shared::library_metadata` and
`ClassEntry::metadata()` decode metadata on demand and do not cache it. Loading parses a
class file and metadata decoding reads attributes, so neither requires a `Code`
attribute: the signature-only stubs load without an execution body. This slice does not
initialize or execute a class, does not build `Class`, `Field`, `Method` or `Constructor`
objects, and adds no reflection native or Spring path. The committed tree has no record
class, so record metadata is exercised by the M2.1 fixtures rather than here.

## Decision log

| ADR | Decision | Status |
| :--- | :--- | :--- |
| [0001](docs/adr/0001-compatibility-first.md) | Compatibility-first; success is a pinned external application | accepted |
| [0002](docs/adr/0002-target-platform-and-versions.md) | Java 17, Spring 6.2.19, Boot 3.5.16, Tomcat 10.1.55 | accepted |
| [0003](docs/adr/0003-compatibility-contract.md) | Observed surface manifest plus C0–C4 gates | accepted |
| [0004](docs/adr/0004-runtime-class-library.md) | Full replacement runtime library, owned by us | accepted |
| [0005](docs/adr/0005-class-loading-and-dynamic-code.md) | Loaders, `defineClass`, `Proxy`, `Lookup`, `Unsafe` capability/fidelity tiers U0–U4/S0–S2 | accepted |
| [0006](docs/adr/0006-reflection-and-metadata.md) | Reflection reproduces JDK 17 metadata | accepted |
| [0007](docs/adr/0007-concurrency-and-memory-model.md) | Real OS threads and the Java memory model | accepted |
| [0008](docs/adr/0008-references-and-cleaner.md) | References, `Cleaner`, class unloading | accepted |
| [0009](docs/adr/0009-resources-jars-and-modules.md) | Jars, nested jars, resources, `ServiceLoader`, `-jar` | accepted |
| [0010](docs/adr/0010-execution-and-performance.md) | Measure first; choose interpreter/JIT against a budget | accepted |
| [0011](docs/adr/0011-scope-ordering-and-native-boundary.md) | Scope ordering, promotion rule and native/internal boundary | accepted |
| [0012](docs/adr/0012-runtime-foundation-and-upgrade-seams.md) | M0 foundation seams and additive/conditional/architecture-fork classification | accepted |

## Resume

A fresh session should:

1. Read `README.md`, `docs/requirements.md` and `docs/open-questions.md`.
2. Read the ADRs in order; 0001–0005 are the project-defining decisions.
3. M0 and M1 are complete. M2.1 landed the typed C1 metadata model and its normalized
   dump (`cargo test -p rava-classfile --test metadata_model`). M2.2 landed the JDK 17
   signature stubs in `stdlib/c1/classes/` and the zero-diff `rava compat metadata
   --check` gate. M2.3 landed the runtime loading path: that tree is the bootstrap class
   source and its metadata is readable without `Code` (`cargo test -p rava-vm --test
   c1_runtime`). M2.4 is next.
4. Do not write runtime code outside a defined slice.

## Open decisions

No open decisions. [`docs/open-questions.md`](docs/open-questions.md) records all
answers; new questions get new ids and an ADR when they change a decision.
