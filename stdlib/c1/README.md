# stdlib/c1/classes — JDK 17 signature stubs (M2.2)

This directory is a **generated, committed** tree of class files, separate from the
M1 `stdlib/classes/` surface stubs. It is the C1 metadata reference the reflection
work will build on. M2.3 promotes it to the runtime class library; until then nothing
loads it.

The committed tree has 980 class files:

- **952** are projected from the host JDK's `lib/ct.sym` release-17 signature of the
  same class. The generator parses the `.sig` entry with `rava-classfile`, sets the
  class-file version to the pinned 61.0, and re-emits the class with the constant pool
  at its original indices and every attribute payload verbatim. No JDK class-file bytes
  are copied: the signature is parsed into the model and written back out by our own
  writer.
- **28** are header-only placeholders for manifest owners Java 17 does not define (the
  `Thread$Builder` classes, JCache, Moneta, WSDL and the Java EE
  `javax.annotation.Resource`). They carry their name, `public abstract`, a
  `java/lang/Object` superclass, version 61.0 and a `RavaC1Unreferenced.java`
  `SourceFile`; they have no members and no metadata claim. The owner list is
  `compat/surface/c1-unreferenced.txt`.

## What is preserved

- The class name, access flags, superclass, interfaces and class-file version 61.0.
- Every field and method with its access flags, name, descriptor and raw attribute
  payloads. `ct.sym` has no `Code` attribute, so the projected methods have none.
- The constant pool at its original indices, including the skipped slot after a
  `long` or `double`, so `Signature`, annotations, `Record`, `InnerClasses`,
  `Exceptions`, `ConstantValue` and every other payload still index the right entries.
- Attributes in the M2.1 set: `Signature`, annotations and type annotations,
  `AnnotationDefault`, `Exceptions`, `ConstantValue`, `InnerClasses`, `NestHost`/
  `NestMembers`, `PermittedSubclasses`, `Record` when present, and `Deprecated`.

## Normalization boundary

`rava compat metadata --check` compares the M2.1 normalized dump of each committed stub
with the release-17 signature. The only normalization is the class-file version line:
`ct.sym` stores the host JDK's class-file version (69 for JDK 25), while this tree is
pinned to 61.0. The release is already encoded in the `.sig` entry name. Ordering,
annotations, signatures and raw attributes are compared as the dump renders them; a
wrong access flag, a dropped annotation or a changed signature is a difference.

## Regenerating

The reference is found through `--ct-sym <path>`, `RAVA_CT_SYM`, or `JAVA_HOME`. The
local acceptance uses the host JDK 25 archive:

```sh
RAVA_CT_SYM=/home/yusiwen/.local/share/mise/installs/java/zulu-25/lib/ct.sym \
  cargo run -p rava-cli -- compat c1-stubs
cargo run -p rava-cli -- compat metadata --check
```

Both commands read the target lock and the committed manifest only; they never read
the pinned jars and never execute a class. The generator fails, naming the owners,
when the archive and `compat/surface/c1-unreferenced.txt` disagree. The test suite and
`cargo xtask ci` never need a JDK: they use committed fixtures and a synthetic
`ct.sym` in a temporary directory.

Do not edit this tree by hand. `rava compat c1-stubs` replaces it wholesale and is
deterministic.
