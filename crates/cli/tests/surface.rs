//! The M4.5 interpreter-side surface, pinned by ordinary Java.
//!
//! `tests/fixtures/surface/` is a committed `javac --release 17` fixture
//! (`demo.SurfaceProbe`). Before #76 these modules had no direct test; the Boot gate does
//! not call most of them (the M4.5 census found 109 of 585 added natives invoked).
//!
//! The probes cover `collection_equality` (List/Set `contains` calls Java `equals`),
//! `objects` (`Objects.hash`/`hashCode` call Java `hashCode`), `interface_defaults`
//! (`CharSequence.isEmpty`, `Map.getOrDefault`), `functional` (`BiPredicate.or`,
//! `Predicate.and`), `iterables` (`Iterable.spliterator`) and `bean_support`
//! (`PropertyDescriptor(String, Method, Method)`). No pinned jars are needed.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{ClassId, DirectorySource, LoaderId, Shared, ThreadState, Value};

const PROBE: &str = "demo/SurfaceProbe";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/surface/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    rava_vm::library::register_all(&shared).expect("the VM class library registers");
    rava_interp::register_all(&shared).expect("the interpreter natives register");
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let probe = shared
        .load_class(loader, PROBE)
        .expect("demo/SurfaceProbe loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
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

fn bool_value(fixture: &Fixture, name: &str) -> bool {
    match call(fixture, name, "()Z").unwrap_or_else(|error| panic!("{name} fails: {error}")) {
        Some(Value::Int(value)) => value != 0,
        other => panic!("{name} should return a boolean, found {other:?}"),
    }
}

fn int_value(fixture: &Fixture, name: &str) -> i32 {
    match call(fixture, name, "()I").unwrap_or_else(|error| panic!("{name} fails: {error}")) {
        Some(Value::Int(value)) => value,
        other => panic!("{name} should return an int, found {other:?}"),
    }
}

fn long_value(fixture: &Fixture, name: &str) -> i64 {
    match call(fixture, name, "()J").unwrap_or_else(|error| panic!("{name} fails: {error}")) {
        Some(Value::Long(value)) => value,
        other => panic!("{name} should return a long, found {other:?}"),
    }
}

fn string_value(fixture: &Fixture, name: &str) -> String {
    match call(fixture, name, "()Ljava/lang/String;")
        .unwrap_or_else(|error| panic!("{name} fails: {error}"))
    {
        Some(Value::Ref(reference)) => fixture
            .shared
            .heap()
            .string(reference)
            .expect("the result is a string")
            .to_owned(),
        other => panic!("{name} should return a string, found {other:?}"),
    }
}

#[test]
fn collection_membership_calls_java_equals() {
    let fixture = fixture();
    assert!(bool_value(&fixture, "listContainsUsesEquals"));
    assert!(bool_value(&fixture, "setContainsUsesEquals"));
}

#[test]
fn objects_hash_calls_java_hash_code() {
    let fixture = fixture();
    // `Arrays.hashCode` starts at 1: 31 * (31 * 1 + 1) + 2.
    assert_eq!(
        int_value(&fixture, "objectsHash"),
        994,
        "Objects.hash(1, 2)"
    );
    assert_eq!(
        int_value(&fixture, "objectsHashCode"),
        7,
        "Objects.hashCode runs the key's own hashCode"
    );
}

#[test]
fn interface_defaults_dispatch_the_receiver() {
    let fixture = fixture();
    assert!(bool_value(&fixture, "charSequenceEmpty"));
    assert_eq!(string_value(&fixture, "mapGetOrDefault"), "default");
}

#[test]
fn functional_defaults_compose_java_lambdas() {
    let fixture = fixture();
    assert!(bool_value(&fixture, "biPredicateOr"));
    assert!(bool_value(&fixture, "predicateAnd"));
}

#[test]
fn iterable_spliterator_drains_the_iterator() {
    let fixture = fixture();
    assert_eq!(long_value(&fixture, "iterableSpliteratorCount"), 2);
}

#[test]
fn map_membership_calls_java_equals() {
    let fixture = fixture();
    assert!(bool_value(&fixture, "mapGetUsesEquals"));
    assert!(bool_value(&fixture, "mapContainsKeyUsesEquals"));
    assert!(bool_value(&fixture, "mapPutReplacesEqualKey"));
    assert!(bool_value(&fixture, "mapGetOrDefaultUsesEquals"));
    assert!(bool_value(&fixture, "mapRemoveUsesEquals"));
    assert!(bool_value(&fixture, "mapRemovePairUsesEquals"));
    assert!(bool_value(&fixture, "mapContainsValueUsesEquals"));
    assert!(bool_value(&fixture, "mapPutIfAbsentUsesEquals"));
    assert_eq!(int_value(&fixture, "mapComputeIfAbsent"), 84);
}

#[test]
fn set_copy_constructors_and_bulk_operations_call_java_equals() {
    let fixture = fixture();
    assert_eq!(int_value(&fixture, "setCopyDedupes"), 1);
    assert_eq!(int_value(&fixture, "linkedSetCopyDedupes"), 1);
    assert_eq!(int_value(&fixture, "copyOnWriteSetCopyDedupes"), 1);
    assert_eq!(int_value(&fixture, "treeSetCopyDedupes"), 1);
    assert_eq!(string_value(&fixture, "setAddAllDedupes"), "true:2");
    assert!(bool_value(&fixture, "setContainsAllEqual"));
    assert_eq!(string_value(&fixture, "setRemoveAllEqual"), "true:1");
    assert_eq!(string_value(&fixture, "setRetainAllEqual"), "true:1");
    assert_eq!(string_value(&fixture, "listBulkEqual"), "true:true");
}

#[test]
fn identity_map_stays_reference_based() {
    let fixture = fixture();
    assert_eq!(
        int_value(&fixture, "identityMapStoresBoth"),
        2,
        "IdentityHashMap keeps two equal keys apart"
    );
    assert!(
        bool_value(&fixture, "identityMapInterfaceCall"),
        "a Map-typed call on an IdentityHashMap must still compare by identity"
    );
    assert!(
        bool_value(&fixture, "identityMapComputeIfAbsent"),
        "the Map.computeIfAbsent default must keep identity keys apart"
    );
    assert!(
        bool_value(&fixture, "identityMapRemovePair"),
        "the Map.remove(k, v) default must keep identity keys apart"
    );
    assert!(
        bool_value(&fixture, "identityMapEquals"),
        "IdentityHashMap.equals is identity-based and rejects another map type"
    );
}

#[test]
fn text_conversions_call_java_to_string() {
    let fixture = fixture();
    assert!(bool_value(&fixture, "stringValueOfUsesToString"));
    assert!(bool_value(&fixture, "concatUsesToString"));
    assert!(bool_value(&fixture, "formatUsesToString"));
    assert!(bool_value(&fixture, "builderAppendUsesToString"));
    assert!(bool_value(&fixture, "bufferAppendUsesToString"));
    assert!(bool_value(&fixture, "objectsToStringUsesToString"));
    assert!(bool_value(&fixture, "arraysToStringUsesToString"));
    assert!(bool_value(&fixture, "defaultToStringStaysStructural"));
}

#[test]
fn property_descriptor_constructor_uses_the_bean_methods() {
    let fixture = fixture();
    assert!(bool_value(&fixture, "propertyDescriptor"));
}
