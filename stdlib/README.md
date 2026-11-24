# stdlib — generated class libraries

`stdlib/` holds two generated, committed trees with different jobs:

- `stdlib/classes/` (M1.3, this README): signature-only surface stubs from the
  committed manifest. The M1 gate links the manifest against it. It stays in place
  unchanged; M2.2 added the C1 tree beside it.
- `stdlib/c1/classes/` (M2.2): JDK 17 signature stubs with real metadata, regenerated
  from the host `ct.sym`. See [`c1/README.md`](c1/README.md). M2.3 promotes this tree
  to the runtime library.

The rest of this document describes the M1.3 tree only.

# stdlib/classes — signature-only stubs (M1.3)

This directory is **generated** by `rava compat stubs` from the committed surface
manifest (`compat/surface/spring-boot-3.5.16.txt`). It is a signature-only stub
library, not the rava runtime class library.

The committed tree has 980 class files and declares 177 fields and 4,674 methods,
which is every one of the 3,626 method and 1,048 interface-method records in the
manifest.

What the stubs contain:

- One class file per manifest owner, at the binary-name path (`java/lang/Object.class`
  for `java/lang/Object`).
- Every field, method and interface-method record declared directly on its owner with
  the exact name and descriptor. A member inherited in the real JDK is therefore
  declared here instead of being reached through a superclass. Link-check resolves
  each record against its owner's declarations only; it does not walk a class
  hierarchy.
- Public abstract classes; an owner named by an `interface-method` record is written as
  a public interface. Every non-constructor method has no `Code` attribute.
- `<init>` records carry a minimal concrete body (`aload_0`; `Object.<init>`; `return`)
  because the class-file format forbids an abstract constructor. `<clinit>` is a no-op.
- A `SourceFile` attribute naming `RavaStub.java`, so a disassembly shows that the file
  is not compiled library code.

What the stubs do **not** contain:

- Behaviour, native registration, or verifier-ready method bodies.
- JDK 17 access flags, real superclasses or interface lists. Every class extends
  `java/lang/Object`; interface-ness is inferred only from interface-method records.
- Annotations, generic signatures, records, sealed permits, nestmates, parameter names
  or the C1 metadata contract. Those belong to the M2.2 C1 tree above, not here.

Do not edit this tree by hand. Regenerate it after a manifest change:

```sh
cargo run -p rava-cli -- compat stubs
cargo run -p rava-cli -- compat link-check
```

`compat link-check` never generates files; a missing or ambiguous stub is an unresolved
record. The acceptance report is the count printed by the command.
