//! M2.30 acceptance: JDK-shaped lookup misses.
//!
//! A named `Class` lookup that finds no member throws the JDK's exception instead of
//! returning null:
//!
//! - `getDeclaredField`/`getField` throw `java.lang.NoSuchFieldException`;
//! - `getDeclaredMethod`/`getMethod` and `getDeclaredConstructor`/`getConstructor` throw
//!   `java.lang.NoSuchMethodException`.
//!
//! The message names the receiver class, the member name and, for an executable, the
//! parameter descriptors (`demo.LookupMissProbe.missing(I, Ljava/lang/String;)`, a
//! constructor as `demo.LookupMissProbe.<init>(...)`). The throwable is an ordinary
//! `java.lang` object built through the `exceptions` module, so a compiled
//! `catch (NoSuchMethodException ...)` observes it, `getMessage()` reads the text and
//! `getClass().getName()` reports the JDK class. Unique to M2.30: the miss on a primitive
//! or array class object throws the same way; only the `Class[]`-returning queries stay
//! empty arrays.
//!
//! The fixture `demo.LookupMissProbe` is a committed `javac --release 17` class (see
//! `tests/fixtures/lookup_exceptions/README.md`). Half the assertions are compiled Java
//! catch blocks, half inspect the throwable at the native boundary, so neither the
//! interpreter's handler search nor the native's exception object can hide a null return.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, DirectorySource, LoaderId, ObjRef, Shared, ThreadState, Value, exceptions, instance,
    object, reflection, string,
};

const PROBE: &str = "demo/LookupMissProbe";
const NO_SUCH_FIELD: &str = "java/lang/NoSuchFieldException";
const NO_SUCH_METHOD: &str = "java/lang/NoSuchMethodException";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lookup_exceptions/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
    probe_object: ObjRef,
    class_class: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    object::register_natives(&shared);
    instance::register_natives(&shared);
    string::register_natives(&shared);
    reflection::register_natives(&shared);
    exceptions::register_natives(&shared);
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let probe = shared
        .load_class(loader, PROBE)
        .expect("demo/LookupMissProbe loads");
    let probe_object = shared
        .class_object(probe)
        .expect("the probe has a class object");
    let class_class = shared
        .load_library_class("java/lang/Class")
        .expect("java.lang.Class loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
        probe_object,
        class_class,
    }
}

fn call(fixture: &Fixture, name: &str, descriptor: &str) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        Vec::new(),
    )
}

/// Call a fixture method that returns a `String` and read the text.
fn call_text(fixture: &Fixture, name: &str) -> String {
    let value = call(fixture, name, "()Ljava/lang/String;")
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
    let Some(Value::Ref(reference)) = value else {
        panic!("{name} did not return a String: {value:?}");
    };
    fixture
        .shared
        .heap()
        .string(reference)
        .unwrap_or_else(|error| panic!("{name} returned a non-String: {error}"))
        .to_owned()
}

/// The throwable of an uncaught lookup miss, asserting the interpreter preserved it.
fn thrown(result: Result<Option<Value>, InterpError>) -> ObjRef {
    match result {
        Err(InterpError::Thrown(throwable)) => throwable,
        other => panic!("expected an uncaught lookup exception, found {other:?}"),
    }
}

fn throwable_class(fixture: &Fixture, throwable: ObjRef) -> ClassId {
    fixture
        .shared
        .heap()
        .object_class(throwable)
        .expect("the throwable is an instance")
}

fn throwable_class_name(fixture: &Fixture, throwable: ObjRef) -> String {
    let class = throwable_class(fixture, throwable);
    fixture
        .shared
        .entry(class)
        .expect("the throwable class is loaded")
        .name
        .clone()
}

#[test]
fn a_field_lookup_miss_throws_no_such_field_exception() {
    let fixture = fixture();
    let expected = "java.lang.NoSuchFieldException|demo.LookupMissProbe.missing";
    assert_eq!(call_text(&fixture, "fieldMiss"), expected);
    assert_eq!(call_text(&fixture, "inheritedFieldMiss"), expected);
    assert_eq!(
        call_text(&fixture, "fieldMissKind"),
        "NoSuchFieldException|demo.LookupMissProbe.missing",
        "the miss is a NoSuchFieldException, not a NoSuchMethodException"
    );
}

#[test]
fn a_method_lookup_miss_throws_no_such_method_exception() {
    let fixture = fixture();
    assert_eq!(
        call_text(&fixture, "methodMiss"),
        "java.lang.NoSuchMethodException|demo.LookupMissProbe.missing(I, Ljava/lang/String;)",
        "the message carries the receiver class, name and parameter descriptors"
    );
    assert_eq!(
        call_text(&fixture, "inheritedMethodMiss"),
        "java.lang.NoSuchMethodException|demo.LookupMissProbe.missing()"
    );
}

#[test]
fn a_constructor_lookup_miss_throws_no_such_method_exception() {
    let fixture = fixture();
    assert_eq!(
        call_text(&fixture, "constructorMiss"),
        "java.lang.NoSuchMethodException|demo.LookupMissProbe.<init>(Ljava/lang/String;, Ljava/lang/String;)",
        "a constructor miss names the class and `<init>` parameters"
    );
    assert_eq!(
        call_text(&fixture, "inheritedConstructorMiss"),
        "java.lang.NoSuchMethodException|demo.LookupMissProbe.<init>(J)"
    );
}

#[test]
fn an_uncaught_miss_is_a_typed_throwable_with_jdk_identity() {
    let fixture = fixture();
    for (name, descriptor, exception, message) in [
        (
            "uncaughtFieldMiss",
            "()Ljava/lang/reflect/Field;",
            NO_SUCH_FIELD,
            "demo.LookupMissProbe.missing",
        ),
        (
            "uncaughtMethodMiss",
            "()Ljava/lang/reflect/Method;",
            NO_SUCH_METHOD,
            "demo.LookupMissProbe.missing(I)",
        ),
        (
            "uncaughtConstructorMiss",
            "()Ljava/lang/reflect/Constructor;",
            NO_SUCH_METHOD,
            "demo.LookupMissProbe.<init>(Ljava/lang/String;, I)",
        ),
    ] {
        let throwable = thrown(call(&fixture, name, descriptor));
        let expected_class = fixture
            .shared
            .load_library_class(exception)
            .expect("the exception class is in the C1 tree");
        assert_eq!(
            throwable_class(&fixture, throwable),
            expected_class,
            "{name} threw a throwable with the wrong class identity"
        );
        assert_eq!(
            fixture.shared.throwable_message(throwable).as_deref(),
            Some(message),
            "{name} carried the wrong message"
        );
        // `Object.getClass()` on the throwable reports the same cached class object a
        // compiled catch uses, so the exception is observable as the JDK class.
        assert_eq!(
            fixture
                .shared
                .get_class(throwable)
                .expect("getClass succeeds"),
            fixture
                .shared
                .class_object(expected_class)
                .expect("the class object exists"),
            "{name} did not report its class through getClass()"
        );
        assert_eq!(throwable_class_name(&fixture, throwable), exception);
    }
}

#[test]
fn the_success_paths_are_unchanged() {
    let fixture = fixture();
    assert_eq!(
        call_text(&fixture, "success"),
        "value|getValue|0",
        "declared field/method/constructor hits are still members"
    );
    assert_eq!(
        call_text(&fixture, "inheritedSuccess"),
        "label|sum|1",
        "getField/getMethod/getConstructor hits are still members"
    );
}

#[test]
fn class_array_queries_stay_empty_arrays() {
    let fixture = fixture();
    // The probe declares no member class and has no superclass members beyond Object's,
    // so both queries are empty arrays rather than lookup exceptions.
    for name in ["getDeclaredClasses", "getClasses"] {
        let value = run_instance_method(
            &fixture.shared,
            &fixture.thread,
            fixture.class_class,
            name,
            "()[Ljava/lang/Class;",
            fixture.probe_object,
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{name} failed: {error}"));
        let Some(Value::Ref(reference)) = value else {
            panic!("{name} did not return a Class[]: {value:?}");
        };
        let heap = fixture.shared.heap();
        let array = heap.array(reference).expect("a reference array");
        assert!(
            array.elements.is_empty(),
            "{name} must stay an empty array for a class with no member classes"
        );
    }
}
