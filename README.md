# rava

A Java 17 runtime in Rust, built to run Spring — not just a corpus.

## Status

**Decisions accepted (2026-11-23). M0 and M1 are complete (2026-11-24):** workspace,
parser, loader identity, heap/seams, the interpreter core, Hello, safepoint/GC-root
and no-lock-across-Java-call invariants, the verifier dataflow skeleton with the
metadata dump fixture, the pinned target lock with the reference application and its
35-jar set, the static surface manifest (`compat/surface/spring-boot-3.5.16.txt`:
980 classes, 177 fields, 3,626 methods, 1,048 interface methods), and the
signature-only stub library under `stdlib/classes/` (980 class files) with a
zero-unresolved link-check. The manifest and stubs regenerate byte-identically, and
`rava compat check` runs the whole M1 gate. **M2.1 is done (2026-11-24):** a typed C1
metadata model in `rava-classfile` (annotations of every element-value kind, parameter
and type annotations, generic signatures, records, sealed permits, nestmates, inner and
enclosing classes, exceptions and parameter names) with every raw attribute payload
retained, a normalized text dump, committed Java 17 fixtures and golden dumps, plus
typed errors for malformed payloads. **M2.2 is done (2026-11-24):** a separate JDK 17
signature stub tree under `stdlib/c1/classes/` (980 class files, 952 projected from the
host JDK's `lib/ct.sym` release-17 signatures and 28 header-only placeholders for
owners Java 17 does not define) with a faithful class-file writer, and a normalized
metadata check whose diff is 0 across the 952 referenced classes. `stdlib/classes/` is
unchanged and the M1 gate still passes. **M2.3 is done (2026-11-24):** the C1 tree is
the bootstrap class source, read through the existing `DirectorySource` with
`(loader, name)` identity; `Shared::with_library_dir`, `Shared::load_library_class`,
`Shared::library_metadata` and `ClassEntry::metadata()` expose its metadata without a
`Code` attribute or executing a class. **M2.4 is done (2026-11-24):** class objects are
heap objects cached once per `(loader, name)` identity and enumerated as GC roots;
`Object.getClass` and `Class.getName`, `getModifiers`, `isInterface`, `isArray`,
`getSuperclass` and `forName` are native registry entries, and the registry overrides the
C1 stubs that carry neither `Code` nor `ACC_NATIVE`. **M2.5 is done (2026-11-24):**
`new`/`dup`/`invokespecial`/`getfield`/`putfield` and the reference loads/stores execute;
`run_instance_method` puts `this` in local 0 on the existing frame, the VM owns the
superclass-first instance layout and allocation, and instance calls prefer the native
registry, then `Code`, including the registered no-op `Object.<init>()V`. **M2.6 is done
(2026-11-24):** `newarray`, `anewarray`, `multianewarray`, `arraylength` and the array
load/store opcodes allocate and access the existing heap arrays; `goto`, the
`if<cond>`/`if_icmp<cond>` comparisons, `ifnull`/`ifnonnull` and `iinc` execute with
branch targets resolved to instruction indices at decode time; and bounds, null,
negative-size and element-kind failures are typed errors. **M2.7 is done
(2026-11-24):** the `Code` exception table is decoded into instruction ranges,
`athrow` throws, and a throwing instruction or a throwing callee searches the
current frame's table in class-file order for a range that contains it and a catch
type assignable through the superclass chain; a match clears the operand stack,
pushes the throwable and jumps to the handler, while an uncaught throw leaves the
frame as a typed `Thrown` carrying the same object. A catch-all entry
(`catch_type = 0`) decodes, but `finally` duplication, stack traces,
exception-object synthesis and interfaces are not implemented, and nothing checks that
the thrown object or the catch type is a `Throwable`. **M2.8 is done (2026-11-24):**
`HeapObject::Reflect` represents a field, method or constructor by its declaring class,
name and descriptor; one object per member is cached and enumerated as a GC root;
`Class.getDeclaredFields`, `getDeclaredMethods` and `getDeclaredConstructors` return real
heap reference arrays; and the read-only member natives report names, modifiers,
declaring classes, field and return types and parameter counts. Reflective invocation,
annotations and generic types remain later slices. **M2.9 is done (2026-11-24):**
`java.lang.String` keeps the existing `HeapObject::Str` and implements `length`,
`isEmpty`, `charAt`, `equals`, `hashCode`, `compareTo`, `concat`, both `substring`
overloads, `startsWith`, `endsWith`, `indexOf`, `replace`, `toCharArray`, `toString` and
`intern` over UTF-16 code units, including surrogate pairs; `java.lang.Object` gains
reference-identity `equals`, a stable identity `hashCode` and a `ClassName@hash`
`toString`; and interned strings, including `ldc` constants, are permanent roots of a
content-keyed table. A lone
surrogate, a null argument and an out-of-range index are typed errors because exception
objects are not synthesized yet. `String.valueOf`, `StringBuilder`, collections,
annotations and generics remain later slices. **M2.10 is done (2026-11-24):** a boxed
`Integer` is `HeapObject::Boxed(BoxedPrimitive::Integer)`, so `getClass` reports
`java.lang.Integer` and `Integer.valueOf`, `intValue`, `toString`, `equals`, `hashCode`,
`compareTo` and `parseInt` have real value semantics; `Field.get`/`set` read and write
`int` and reference instance fields with boxing and identity-preserving references; and
`Method.invoke`/`Constructor.newInstance` run static and instance methods and
constructors through the existing interpreter, using the reflected name and descriptor
with no overload resolution. A Java throwable from an invoked method propagates as a
`Thrown` rather than being swallowed; `InvocationTargetException` wrapping, varargs,
access checks and `setAccessible` remain later slices. **M2.11 is next.**

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
