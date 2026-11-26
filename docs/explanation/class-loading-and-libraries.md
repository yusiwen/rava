# Class loading and libraries: sources, loaders, trees and natives

## Question

How does rava find a class, decide its identity, load its class file, choose between the
two `stdlib` trees, and dispatch a `java.*` method to a Rust native?

## The shape in one picture

```
rava run <Main> --stdlib stdlib/c1/classes --cp <dirs/jars>
        |
        v
  Shared::with_library_dir(stdlib)       bootstrap loader (LoaderId(0))
        |
        +-- add_loader(BOOTSTRAP, ClassPathSource)   application loader
        |
        +-- library::register_all(shared)            native registry
        |
        v
  ClassTable: (LoaderId, binary name) -> ClassId -> ClassEntry(ClassFile, statics, state)
        |
        v
  interpreter: frame + decoded IR; native registry before Code
```

There is one class table and one identity model. The runtime does not maintain a second
name-keyed table or a second array/class-object world.

## 1. Sources: `ClassSource`

The abstraction is in [`crates/vm/src/source.rs`](../../crates/vm/src/source.rs):

```rust
pub trait ClassSource: Send + Sync + Debug {
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError>;
    fn read_resource(&self, name: &str) -> Result<Option<Vec<u8>>, VmError>;
    fn read_resources(&self, name: &str) -> Result<Vec<Vec<u8>>, VmError>;
    fn resource_urls(&self, name: &str) -> Result<Vec<String>, VmError>;
    fn resource_names(&self, prefix: &str) -> Result<Vec<String>, VmError>;
}
```

`Some(bytes)` means the source has the class; `None` means “not here”, so the loader can
try the next source and ultimately report `ClassNotFound`. A malformed jar or an I/O
error is an error, not a miss.

Three implementations exist:

| Source | Behavior |
| :--- | :--- |
| `DirectorySource` | Reads `<root>/<binary/name>.class`; a missing file is `None`, other I/O errors are `VmError::Io`. Resources use a `file:/<absolute-path>` URL and `resource_names` walks files under the prefix. |
| `JarSource` | Reads the zip central directory once; stored and DEFLATE entries are supported; Zip64 is rejected; `read` converts the binary name to `<name>.class`. Resources use `jar:file:/<absolute-jar>!/<entry>`; `entry_names`/`class_entry_names`/`read_entry` are the scanner-facing views used by the manifest tool. |
| `ClassPathSource` | An ordered list of directories and jars; the first entry that returns `Some` wins, matching Java classpath order. Resource URLs/names concatenate in that same order, and `read_resources` returns every matching entry in that order rather than only the first. A nonexistent path fails when the source is built, rather than silently changing the search. |

The manifest tool scans jars through `JarSource::class_entry_names`, not through the
runtime loader; the runner uses `ClassPathSource` as an application source.

## 2. Identity: `(LoaderId, binary name)`

`LoaderId(0)` is the bootstrap loader. `LoaderId` and `ClassId` are distinct newtypes in
[`crates/vm/src/loader.rs`](../../crates/vm/src/loader.rs); class identity is the pair
`(defining loader, binary name)`, never a bare name. The table is:

```
HashMap<(LoaderId, String), ClassId>
```

`ClassEntry` carries the defining loader, the binary name, the parsed `ClassFile`, a
statics table keyed by `(name, descriptor)`, and a `ClassState`
(`Uninitialized`/`Initializing`/`Initialized`/`Erroneous`).

Two loaders can define the same name independently. A class loaded by a child loader is
not the same `ClassId` as the bootstrap class of the same name; this is exercised by the
loader-identity and C1 runtime tests.

## 3. Delegation: parent-first, then define

`ClassTable::load_class(loader, name)`:

1. Validates the requested loader.
2. Builds the parent chain from the requested loader up to bootstrap.
3. Checks already-loaded classes **child first, then parent**, returning the first hit.
4. If nothing is loaded, asks sources **parent first, then child**. This is parent-first
   delegation: an application classpath cannot shadow a bootstrap library class.
5. Defines the first bytes found under that candidate loader, or returns
   `VmError::ClassNotFound { loader, name }` naming the original request.

`define_class` rejects a duplicate `(loader, name)`, parses the bytes with
`rava-classfile`, assigns the next `ClassId`, and stores the entry. The runtime never
replaces an existing definition.

The bootstrap source is installed at construction. `Shared::with_library_dir(path)` is
the convenience used by `rava run`; `Shared::add_loader(parent, source)` adds the
application loader. The runner adds exactly one application loader under bootstrap.

## 4. `Shared`: the lockable runtime state

`Shared` in [`crates/vm/src/shared.rs`](../../crates/vm/src/shared.rs) owns:

- the class table behind an `RwLock` (loads/definitions take the write lock; lookups and
  entry snapshots take the read lock);
- the heap behind a `Mutex`;
- the native registry behind an `RwLock`;
- captured stdout/stderr, JUL state, system properties and lock/reference state;
- caches that own permanent heap objects: class objects, primitive classes, array
  classes, reflected members, generic `Type` objects, enum constants and interned
  strings. These are reported by `Shared::roots` and are why a class object is stable
  across calls.

The runtime API is intentionally split so a Java call does not hold one shared lock
while calling into another subsystem. `Shared::entry` returns an `Arc<ClassEntry>` and
releases the class-table lock; `Shared::native` copies a function pointer out of the
registry.

## 5. Two committed trees, two jobs

| Tree | Used by | What it is |
| :--- | :--- | :--- |
| `stdlib/classes/` | M1 `compat check` / `link-check` only | 1,047 synthetic signature stubs generated from the manifest. Every member is declared directly on its owner; all classes are public abstract and extend `Object`; no JDK access flags/superclasses/interfaces and no real metadata. |
| `stdlib/c1/classes/` | runtime (`rava run` default `--stdlib`) and C1 diff | 1,047 C1 class files: 1,011 projected from the host JDK's `ct.sym` release-17 signatures and 36 documented placeholders. They carry real class headers, members, annotations, signatures, records/sealed/nest metadata, but no `Code` attributes. |

The trees are **not interchangeable**:

- `stdlib/classes` exists to prove that the observed manifest has a link surface.
- `stdlib/c1/classes` exists to provide metadata and the class shape to the runtime.
- Neither tree contains behaviour. Behaviour is in the native registry.

The C1 tree's construction, normalization and 36-placeholder boundary are owned by
[`stdlib/c1/README.md`](../../stdlib/c1/README.md) and explained in
[`c1-metadata.md`](c1-metadata.md).

## 5a. Resources and URL streams (M4.1)

A resource is a second view of the same `ClassSource`, not a second source model. The
loader asks sources **parent-first** (bootstrap, then application/child) and, inside one
`ClassPathSource`, in classpath order. `ClassLoader.getResource` returns the first URL,
`getResources` returns an `Enumeration` of every URL, and `getResourceAsStream` returns
the first bytes as a `java.io.InputStream`. `Class.getResource` strips a leading `/` for
an absolute name and otherwise prefixes the class's package. The `getSystem*` forms use
the application loader.

`java.net.URL`, `java.net.URLConnection` and `java.io.InputStream` are real Java objects
with Rust state in an `ObjRef`-keyed side table: URL components, stream bytes/cursors,
`ByteArrayOutputStream` buffers and connection state. The table is not a second heap; the
objects are ordinary class instances, the data is not a Java reference, and
`Shared::roots` reports the keys so a future collector sees the attachment. `URL` opens
`file:` and `jar:file:...!/entry` URLs only; there is no network transport. `InputStream`
returns the unsigned 0..255 byte from `read()` and `-1` at EOF.

The committed `crates/cli/tests/fixtures/resources/` fixture puts a directory and a jar
with the same resource on the classpath in both orders. `cargo test -p rava-cli --test
resources` asserts the exact directory-first output, both URL protocols and payloads,
`getResources` order, missing-resource null and the EOF contract. `ByteArrayOutputStream`,
`Iterator.remove`, `URLDecoder.decode`, `Properties.load` and `Arrays.stream(Object[])`
were added because returning real bytes exposed those existing Spring paths.

## 5b. `ServiceLoader` and `META-INF/services` (M4.2)

`java.util.ServiceLoader.load` reads the provider-configuration resource
`META-INF/services/<service binary name>`. This is the one caller that must see **every**
copy of a resource, so `ClassSource::read_resources` returns all copies a source holds and
`ClassPathSource` concatenates them in classpath order; `Shared::read_resources` walks the
loader chain parent-first, exactly like `resource_urls`, while `read_resource` keeps its
first-match behavior. A provider name line ignores a `#` comment, surrounding whitespace
and blank lines; the remaining order is the file order across the concatenated resources.
Duplicate provider classes are kept once, as the JDK's provider set does.

`ServiceLoader.load` only loads and validates provider classes: each dotted configuration
name is normalized to the internal `a/b/C` form and loaded through the selected loader,
then checked with `reflection::named_assignable`, so the provider has the same class
identity as an ordinary `ldc`/`Class.forName`/`ClassLoader.loadClass` lookup. A missing
class or one that does not implement the service is a typed `VmError::Native` naming the
provider. No provider is allocated or constructed until `Iterator.next`; the loader and
its iterator carry their service class, defining loader, provider classes and cursor in
an `ObjRef`-keyed side table in `Shared` (its no-lock probe is
`SharedLock::ServiceLoaders` and the values own no Java references, so it adds no roots).
The iterator is an ordinary `java.util.Iterator` heap object.
`rava-interp::service_loader` overrides `hasNext`/`next`:
`hasNext` reads the cursor, and `next` allocates the provider and runs `<init>()V` through
`run_instance_method`, so class initialization and constructor effects happen on first
use. Every iterator without service state falls back to the VM's snapshot iterator, which
keeps ordinary collection iteration unchanged. `remove` on a service iterator stays a
typed error because its snapshot has no mutable source.

The committed `crates/cli/tests/fixtures/service_loader/` fixture puts the
`demo.HelloGreeter` provider file in the directory classpath and the `demo.WorldGreeter`
one in `providers.jar`, so both orders are observable. Provider static initializers and
constructors print `clinit:`/`init:` lines after `after-hasNext`, which is the laziness
proof. `cargo test -p rava-cli --test service_loader` also covers the explicit
`ClassLoader` overload, a null loader selecting bootstrap, a missing service as an empty
iteration, the provider's internal-name class identity, and typed errors for a
primitive/array service type and a non-assignable provider. Remaining limits: one
provider name per line, no module layer or `loadInstalled`, no
`Provider`/`stream`/`findFirst` surface, and no `ServiceConfigurationError` object (the
failures stay typed VM errors).

## 6. Native dispatch: registry before `Code`

C1 methods generally have **no `Code` and no `ACC_NATIVE`**. The registry is keyed by
`NativeKey { owner, name, descriptor }`; every entry is a `NativeFn`. The ABI is:

```rust
type NativeFn = fn(
    &Shared,
    &ThreadState,
    LoaderId,
    ObjRef,      // receiver; ObjRef::NULL for a static call
    &[Value],
) -> Result<Option<Value>, VmError>;
```

The interpreter consults the registry **before** requiring `Code` (and before checking
`ACC_NATIVE`), so a stub method can execute its Rust implementation without a Java
method body. Static calls pass `ObjRef::NULL`; virtual calls pass the receiver. This is
what makes `String`, `Class`, reflection members, collections, streams, logging,
`MessageFormat` and Spring's own `java.*` calls work while the class files remain
metadata-shaped.

`crates/vm/src/library.rs::register_all` installs the class-library natives in one place
(object/instance/string/collections/maps/boxed/reflection/generic/annotation/JUL/system/
atomics/math/arrays/locks/references/exceptions/regex/format/thread natives) and
installs the minimal `System.out`/`System.err`. The runner additionally registers the
interpreter-owned natives (`Method.invoke`/`Constructor.newInstance`, callbacks and
streams), because those re-enter the interpreter.

The minimal `PrintStream` and the wrapper `TYPE` fields are examples of state that the
C1 stubs cannot initialize with `<clinit>` code (there is none):
`register_all` writes `System.out`/`System.err` into the statics table, and the
interpreter's `getstatic` falls back to `Shared::static_field_value` for the wrapper
`TYPE` fields.

## 7. Class initialization

`ensure_initialized` lives in the interpreter
([`crates/interp/src/lib.rs`](../../crates/interp/src/lib.rs)). It transitions the
`ClassState` from `Uninitialized` to `Initializing`, runs a `<clinit>()V` if the class
file has one, then records `Initialized` or `Erroneous`. The C1 stubs have no `Code`
initializer, so selected library state is installed by `library::register_all` or by a
native fallback instead. This is a deliberate C1/M2 boundary, not a JDK-equivalent
`<clinit>` path for every library class.

## 8. What the runtime does not do yet

- Resources are `file:` and `jar:file:...!/entry` only. There is no network transport,
  no URLConnection headers/content type/dates, no nested jars (M4.4), no custom
  `URLStreamHandler`, and no charset table (`URLDecoder` decodes UTF-8).
- No module layer, multi-release selection, nested jars, manifest `Class-Path`, `-jar`,
  wildcard/remote classpath or class unloading.
- `ServiceLoader` reads `META-INF/services` only: one provider name per line, no
  `loadInstalled`/module layer, no `Provider`/`stream`/`findFirst`, and a configuration
  failure is a typed VM error rather than a `ServiceConfigurationError` object.
- Parent-first only; there is no child-first/OSGi-style loader yet.
- `link-check` resolves declarations on the owner; it does not walk supertypes or verify
  stubs.
- The C1 tree has 36 placeholders for owners never defined by Java 17; they are not
  evidence of metadata compatibility.
- No access-control checks in class/member lookup or execution; the native and loader
  layers are deliberately permissive where the JDK would enforce access.

## 9. Evidence to re-run

```console
$ cargo test -p rava-vm --test loader_identity      # (loader, name) identity and delegation
$ cargo test -p rava-vm --test c1_runtime           # C1 tree as bootstrap, metadata without Code
$ cargo test -p rava-cli --test classpath_run       # directory/jar/order/missing main
$ cargo test -p rava-cli --test resources          # classpath resources, URLs and streams
$ cargo test -p rava-cli --test service_loader     # META-INF/services and lazy providers
$ cargo run -p rava-cli -- compat check             # manifest + synthetic link tree
$ RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context
```

The first five are regular workspace tests; `compat check` needs the fetched jars and
the Spring test is opt-in for the same reason. The compatibility-level interpretation is
in [`compatibility-gates.md`](compatibility-gates.md).

## Owners

- [`crates/vm/src/source.rs`](../../crates/vm/src/source.rs) — sources/classpath.
- [`crates/vm/src/loader.rs`](../../crates/vm/src/loader.rs) — identity, table, delegation.
- [`crates/vm/src/shared.rs`](../../crates/vm/src/shared.rs) — locks, caches, roots, registry entry points.
- [`crates/vm/src/services.rs`](../../crates/vm/src/services.rs) — `ServiceLoader` side-table state.
- [`crates/interp/src/service_loader.rs`](../../crates/interp/src/service_loader.rs) — `ServiceLoader` natives and lazy provider construction.
- [`crates/cli/src/run.rs`](../../crates/cli/src/run.rs) — startup path.
- [`crates/vm/src/library.rs`](../../crates/vm/src/library.rs) — native installation.
- [`stdlib/README.md`](../../stdlib/README.md), [`stdlib/c1/README.md`](../../stdlib/c1/README.md) — tree state.
- [ADR-0004](../adr/0004-runtime-class-library.md) — full replacement library decision.
- [ADR-0005](../adr/0005-class-loading-and-dynamic-code.md) — loading/identity decisions.
