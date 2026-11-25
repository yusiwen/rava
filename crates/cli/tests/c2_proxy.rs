//! M3.2 acceptance: `java.lang.reflect.Proxy` / `InvocationHandler`.
//!
//! The committed `demo.ProxyProbe` fixture is compiled with `javac --release 17` and
//! committed under `tests/fixtures/proxy/classes`, so this suite needs no JDK, network or
//! files beyond that fixture and `stdlib/c1/classes`. The fixture creates a proxy through
//! `Proxy.newProxyInstance`, calls every method shape the interface declares (a reference,
//! an `int` pair, a `long`/`int` pair and a default method) and prints one fact per line,
//! which the first test compares as the whole captured stdout.
//!
//! The proxy class is generated at run time through `ClassBuilder` + `Assembler` and
//! defined through [`Shared::define_class`], so the M3.1 verifier checks it before use; the
//! second test inspects the generated class in the table to prove that and to check the
//! `(loader, interfaces)` cache. The third test covers the typed error for a null interface
//! array.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ClassId, ClassRef, DirectorySource, LoaderId, Shared, ThreadState, Value, VmError, library,
    verify_method,
};

/// `Proxy.getProxyClass(ClassLoader, Class[])`.
const GET_PROXY_CLASS: &str = "(Ljava/lang/ClassLoader;[Ljava/lang/Class;)Ljava/lang/Class;";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn c1_classes() -> PathBuf {
    manifest_dir().join("../../stdlib/c1/classes")
}

fn proxy_fixture_classes() -> PathBuf {
    manifest_dir().join("tests/fixtures/proxy/classes")
}

/// A library-backed VM plus the proxy fixture application loader, with every native the
/// proxy path needs registered.
fn fixture() -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let application = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(proxy_fixture_classes())),
        )
        .expect("the application loader is added");
    library::register_all(&shared).expect("the class library registers");
    rava_interp::reflect_invoke::register_natives(&shared);
    rava_interp::proxy::register_natives(&shared);
    // Load every class the proxy path resolves up front, so a class-count assertion in a
    // test measures the generated proxy class and not an incidental first load.
    for name in [
        "java/lang/Object",
        "java/lang/Class",
        "java/lang/ClassLoader",
        "java/lang/Runnable",
        "java/lang/reflect/Proxy",
        "java/lang/reflect/InvocationHandler",
        "java/lang/reflect/Method",
    ] {
        shared
            .load_library_class(name)
            .unwrap_or_else(|error| panic!("{name} loads: {error}"));
    }
    let thread = shared.thread_state(1);
    (shared, application, thread)
}

/// A `Class[]` holding the given class objects, allocated the way `anewarray` would.
fn class_array(shared: &Shared, classes: &[ClassId]) -> rava_vm::ObjRef {
    let class_class = shared
        .load_library_class("java/lang/Class")
        .expect("java.lang.Class loads");
    // Resolve every class object first: allocating one locks the heap, which must not
    // happen while the array guard is held.
    let objects: Vec<Value> = classes
        .iter()
        .map(|class| {
            Value::Ref(
                shared
                    .class_object(*class)
                    .expect("the class object is allocated"),
            )
        })
        .collect();
    let array = shared
        .heap()
        .allocate_array(ClassRef::Named(class_class), classes.len());
    {
        let mut heap = shared.heap();
        let elements = heap.array_mut(array).expect("a freshly allocated array");
        for (slot, value) in elements.elements.iter_mut().zip(objects) {
            *slot = value;
        }
    }
    array
}

fn run_get_proxy_class(
    shared: &Shared,
    thread: &ThreadState,
    interfaces: rava_vm::ObjRef,
) -> Result<Option<Value>, InterpError> {
    let proxy = shared
        .load_library_class("java/lang/reflect/Proxy")
        .expect("java.lang.reflect.Proxy loads");
    let loader = shared
        .class_loader_object()
        .expect("the application loader object exists");
    run_static_method(
        shared,
        thread,
        proxy,
        "getProxyClass",
        GET_PROXY_CLASS,
        vec![Value::Ref(loader), Value::Ref(interfaces)],
    )
}

#[test]
fn the_proxy_fixture_prints_the_expected_facts() {
    let (shared, _application, thread) = fixture();
    let probe = shared
        .load_class(shared.application_loader(), "demo/ProxyProbe")
        .expect("demo/ProxyProbe loads from the fixture");

    run_static_method(&shared, &thread, probe, "run", "()V", Vec::new())
        .expect("demo.ProxyProbe.run() completes");

    // One fact per line: the handler's name, the summed add/widen arguments, the default
    // greet result, the handler's toString, and the four reflection facts.
    let expected = concat!(
        "rava\n",
        "5\n",
        "9\n",
        "hello rava\n",
        "proxy!\n",
        "true\n",
        "true\n",
        "true\n",
        "true\n",
    );
    assert_eq!(shared.stdout(), expected);
}

#[test]
fn the_generated_proxy_class_is_defined_verified_and_cached() {
    let (shared, application, thread) = fixture();
    let service = shared
        .load_class(application, "demo/Service")
        .expect("demo/Service loads from the fixture");
    let interfaces = class_array(&shared, &[service]);

    let before = shared.class_count();
    let result = run_get_proxy_class(&shared, &thread, interfaces).expect("getProxyClass runs");
    let Some(Value::Ref(class_object)) = result else {
        panic!("Proxy.getProxyClass returned {result:?}");
    };
    let generated = shared
        .heap()
        .class(class_object)
        .expect("the result is a class object");

    assert_eq!(
        shared.class_count(),
        before + 1,
        "one generated class is defined for one interface list"
    );
    let entry = shared
        .entry(generated)
        .expect("the generated class is in the table");
    assert!(
        entry.name.starts_with("rava/proxy/$Proxy"),
        "the generated class is named rava/proxy/$ProxyN, found {}",
        entry.name
    );
    assert_eq!(
        shared.lookup(application, &entry.name),
        Some(generated),
        "the generated class is defined in the application loader"
    );
    let metadata = entry.metadata().expect("the generated class has metadata");
    assert_eq!(
        metadata.super_class.as_deref(),
        Some("java/lang/reflect/Proxy"),
        "the generated class extends java.lang.reflect.Proxy"
    );
    assert_eq!(
        metadata
            .interfaces
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["demo/Service"],
        "the generated class lists the requested interface"
    );
    assert!(
        metadata
            .methods
            .iter()
            .any(|method| method.name == "name" && method.descriptor == "()Ljava/lang/String;"),
        "the generated class declares the interface method"
    );
    for method in &entry.file.methods {
        verify_method(&entry.file, method).unwrap_or_else(|error| {
            panic!(
                "generated method {}{} verifies: {error}",
                entry.file.method_name(method).unwrap_or("<unknown>"),
                entry.file.method_descriptor(method).unwrap_or("<unknown>")
            )
        });
    }

    // The `(loader, interfaces)` cache returns the same class object on a repeated call
    // and does not define the class twice.
    let again = run_get_proxy_class(&shared, &thread, interfaces).expect("getProxyClass reruns");
    assert_eq!(again, result, "the cached class is reference-identical");
    assert_eq!(
        shared.class_count(),
        before + 1,
        "the second call reuses the generated class"
    );

    // A repeated interface is deduplicated, so the same class is returned instead of a
    // class that lists `demo/Service` twice.
    let duplicate_interfaces = class_array(&shared, &[service, service]);
    let same = run_get_proxy_class(&shared, &thread, duplicate_interfaces)
        .expect("getProxyClass with a duplicate interface runs");
    assert_eq!(
        same, result,
        "a repeated interface returns the cached class"
    );
    assert_eq!(shared.class_count(), before + 1);

    // A second, different interface list is a different class.
    let runnable = shared
        .load_library_class("java/lang/Runnable")
        .expect("java.lang.Runnable loads");
    let other_interfaces = class_array(&shared, &[service, runnable]);
    let other = run_get_proxy_class(&shared, &thread, other_interfaces)
        .expect("getProxyClass with a second interface list runs");
    let Some(Value::Ref(other_object)) = other else {
        panic!("Proxy.getProxyClass returned {other:?}");
    };
    let other_class = shared
        .heap()
        .class(other_object)
        .expect("the result is a class object");
    assert_ne!(other_class, generated);
    assert_eq!(shared.class_count(), before + 2);
}

#[test]
fn a_null_interface_array_is_a_typed_error() {
    let (shared, _application, thread) = fixture();
    let before = shared.class_count();
    let error = run_get_proxy_class(&shared, &thread, rava_vm::ObjRef::NULL)
        .expect_err("a null Class[] is rejected");

    match error {
        InterpError::Vm(VmError::Native(detail)) => {
            assert!(
                detail.contains("null Class[]"),
                "the error names the null interface array: {detail}"
            );
        }
        other => panic!("expected a typed native error, found {other:?}"),
    }
    assert_eq!(
        shared.class_count(),
        before,
        "the rejected call defines no class"
    );
}
