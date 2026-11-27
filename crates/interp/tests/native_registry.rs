//! Guard: every native registration for a JDK class names a method the C1 tree declares.
//!
//! The native registry is keyed by `(owner, name, descriptor)`, and the interpreter looks
//! a native up with the owner the bytecode names. A registration whose descriptor does not
//! exist on that owner is unreachable, and the first Java call aborts with
//! `error: method … has no bytecode` — a `VmError`, not a Java exception. This test walks
//! the registry the runner installs and compares it with `stdlib/c1/classes`, so a wrong
//! descriptor fails CI instead of a user program (issue #70).
//!
//! Scope includes constructors: a registration for a constructor the stub does not declare
//! is dead even though `new` fails earlier through the class metadata, and the M4.5
//! registry carried 49 such entries (#80). `$`-prefixed entries are addressed by native
//! lambda targets, not by javac; the tests below pin the lambda targets that matter.
//!
//! Two allowances keep the guard honest without pinning every historical quirk:
//!
//! - an inherited method registered under the receiver's class, where the declaring
//!   supertype is one of the owners the C1 tree omits (the tree is not closed; see
//!   `docs/explanation/c1-metadata.md`), and
//! - a dead alias for which a correct registration of the same `(owner, name)` also
//!   exists; such an entry cannot abort a Java call because the correct lookup wins.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use rava_classfile::ClassFile;
use rava_vm::{HeapObject, LoaderId, ObjRef, Shared, Value};

/// Inherited methods whose declaring supertype the C1 tree omits, and default methods
/// javac addresses through the concrete class.
const INHERITED_OWNER_ALLOWLIST: &[(&str, &str, &str)] = &[
    // `remove(Object)` is declared on `AbstractCollection`, an owner the C1 tree omits.
    ("java/util/AbstractList", "remove", "(Ljava/lang/Object;)Z"),
    // `read(char[])` is inherited from `java/io/Reader`.
    ("java/io/InputStreamReader", "read", "([C)I"),
    // `Queue.remove()` is inherited from `java/util/Queue`; the class also declares
    // `remove(Object)`, which is the descriptor the stub carries.
    (
        "java/util/concurrent/ConcurrentLinkedQueue",
        "remove",
        "()Ljava/lang/Object;",
    ),
    // `Map.remove(Object,Object)` is a default method on `java/util/Map`; javac emits the
    // concrete map class as the owner.
    (
        "java/util/TreeMap",
        "remove",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
    ),
    (
        "java/util/EnumMap",
        "remove",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
    ),
    (
        "java/util/IdentityHashMap",
        "remove",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
    ),
    (
        "java/util/WeakHashMap",
        "remove",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
    ),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/interp is inside the workspace")
        .to_path_buf()
}

fn c1_classes() -> PathBuf {
    repo_root().join("stdlib/c1/classes")
}

/// Every C1 stub's `(name, descriptor)` set, keyed by its internal binary name.
fn stubs() -> BTreeMap<String, BTreeSet<(String, String)>> {
    fn walk(directory: &Path, out: &mut BTreeMap<String, BTreeSet<(String, String)>>) {
        for entry in std::fs::read_dir(directory).expect("the C1 tree is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(&path, out);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("class") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("a C1 stub is readable");
            let file = ClassFile::parse(&bytes).expect("a C1 stub parses");
            let metadata = file.metadata().expect("a C1 stub's metadata decodes");
            let methods = metadata
                .methods
                .iter()
                .map(|method| (method.name.clone(), method.descriptor.clone()))
                .collect();
            out.insert(metadata.binary_name, methods);
        }
    }
    let mut out = BTreeMap::new();
    walk(&c1_classes(), &mut out);
    out
}

/// A `Shared` with exactly the registry the CLI runner installs.
fn fixture() -> Shared {
    let shared = Shared::with_library_dir(c1_classes());
    rava_vm::library::register_all(&shared).expect("the VM class library registers");
    rava_interp::register_all(&shared).expect("the interpreter natives register");
    shared
}

fn invoke(
    shared: &Shared,
    owner: &str,
    name: &str,
    descriptor: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Option<Value> {
    let function = shared
        .native(owner, name, descriptor)
        .unwrap_or_else(|| panic!("no native is registered for {owner}.{name}{descriptor}"));
    let thread = shared.thread_state(1);
    function(shared, &thread, LoaderId::BOOTSTRAP, receiver, args)
        .unwrap_or_else(|error| panic!("{owner}.{name}{descriptor} failed: {error}"))
}

fn lambda_of(shared: &Shared, value: Option<Value>) -> rava_vm::LambdaObject {
    let Some(Value::Ref(reference)) = value else {
        panic!("expected a lambda reference, found {value:?}");
    };
    let heap = shared.heap();
    match heap.get(reference).expect("the lambda reference resolves") {
        HeapObject::Lambda(lambda) => lambda.clone(),
        other => panic!("expected a lambda object, found {other:?}"),
    }
}

#[test]
fn every_registered_native_matches_a_c1_stub_descriptor() {
    let stubs = stubs();
    assert!(
        stubs.len() > 1_000,
        "the C1 tree should carry the 1,047 manifest owners, found {}",
        stubs.len()
    );
    let shared = fixture();
    let keys = shared.registered_natives();
    assert!(
        keys.len() > 2_000,
        "the runner should install the whole registry, found {} natives",
        keys.len()
    );
    let registered: BTreeSet<(String, String, String)> = keys.iter().cloned().collect();

    let mut failures = Vec::new();
    for (owner, name, descriptor) in &keys {
        let Some(methods) = stubs.get(owner) else {
            // An application or library class (Spring, a user class): the C1 tree has no
            // metadata to compare against.
            continue;
        };
        if name.starts_with('$') {
            continue;
        }
        if methods.contains(&(name.clone(), descriptor.clone())) {
            continue;
        }
        if name == "<init>" {
            // A constructor is never inherited, so a descriptor the owner does not declare
            // has no resolution path at all.
            failures.push(format!(
                "{owner}.<init>{descriptor} (the stub declares {})",
                methods
                    .iter()
                    .filter(|(declared, _)| declared == "<init>")
                    .map(|(_, stub_descriptor)| stub_descriptor.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            continue;
        }
        if !methods.iter().any(|(declared, _)| declared == name) {
            // The owner's stub does not declare the name at all: an inherited method
            // registered under the receiver's class, whose declaring supertype is one of
            // the owners the C1 tree omits.
            continue;
        }
        if INHERITED_OWNER_ALLOWLIST
            .iter()
            .any(|entry| entry == &(owner.as_str(), name.as_str(), descriptor.as_str()))
        {
            continue;
        }
        let correct_registered = methods.iter().any(|(declared, stub_descriptor)| {
            declared == name
                && registered.contains(&(owner.clone(), name.clone(), stub_descriptor.clone()))
        });
        if correct_registered {
            // A dead alias: the lookup resolves the correct descriptor, so the extra
            // registration cannot abort a Java call. `Set.of`/`List.of` are the documented
            // case: both owners register both return descriptors so an erased
            // `invokeinterface` resolves, while each stub declares only its own return type
            // (`register_collection_factories`).
            continue;
        }
        failures.push(format!(
            "{owner}.{name}{descriptor} (the stub declares {})",
            methods
                .iter()
                .filter(|(declared, _)| declared == name)
                .map(|(_, stub_descriptor)| stub_descriptor.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    assert!(
        failures.is_empty(),
        "{} native registration(s) cannot match a C1 stub method and would abort on first \
         use:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn count_down_latch_get_count_returns_a_long() {
    let shared = fixture();
    let class = shared
        .load_library_class("java/util/concurrent/CountDownLatch")
        .expect("CountDownLatch loads");
    let latch = shared
        .allocate_instance(class)
        .expect("a CountDownLatch instance allocates");
    invoke(
        &shared,
        "java/util/concurrent/CountDownLatch",
        "<init>",
        "(I)V",
        latch,
        &[Value::Int(3)],
    );
    assert_eq!(
        invoke(
            &shared,
            "java/util/concurrent/CountDownLatch",
            "getCount",
            "()J",
            latch,
            &[],
        ),
        Some(Value::Long(3))
    );
}

#[test]
fn unary_operator_identity_is_a_unary_operator_lambda() {
    let shared = fixture();
    let lambda = lambda_of(
        &shared,
        invoke(
            &shared,
            "java/util/function/UnaryOperator",
            "identity",
            "()Ljava/util/function/UnaryOperator;",
            ObjRef::NULL,
            &[],
        ),
    );
    let interface = shared
        .entry(lambda.interface)
        .expect("the lambda interface is loaded")
        .name
        .clone();
    assert_eq!(interface, "java/util/function/UnaryOperator");
    assert_eq!(lambda.sam_name, "apply");
    assert!(
        shared
            .native(&interface, &lambda.target.name, &lambda.target.descriptor)
            .is_some(),
        "the lambda target {}.{}{} must be registered",
        interface,
        lambda.target.name,
        lambda.target.descriptor
    );
}

#[test]
fn default_thread_factory_targets_the_thread_factory_owner() {
    let shared = fixture();
    let lambda = lambda_of(
        &shared,
        invoke(
            &shared,
            "java/util/concurrent/Executors",
            "defaultThreadFactory",
            "()Ljava/util/concurrent/ThreadFactory;",
            ObjRef::NULL,
            &[],
        ),
    );
    let owner = shared
        .entry(lambda.target.class)
        .expect("the lambda target class is loaded")
        .name
        .clone();
    assert_eq!(owner, "java/util/concurrent/ThreadFactory");
    assert!(
        shared
            .native(&owner, &lambda.target.name, &lambda.target.descriptor)
            .is_some(),
        "the factory's lambda target {owner}.{}{} must be registered under the owner the \
         lambda names",
        lambda.target.name,
        lambda.target.descriptor
    );
}
