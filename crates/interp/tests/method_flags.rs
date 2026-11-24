//! M2.28 acceptance: the `Executable`/`Member` class-file predicate flags.
//!
//! `demo.FlagsProbe` is a committed `javac --release 17` fixture whose compiler-generated
//! members carry each flag exactly as a Java 17 compiler emits it:
//!
//! - `get()` implements `Getter<String>` and javac adds a bridge `Object get()` with
//!   `ACC_PUBLIC | ACC_BRIDGE | ACC_SYNTHETIC` (`0x1041`).
//! - `sum(int...)` is a varargs method (`0x0089`) and `FlagsProbe(int...)` a varargs
//!   constructor (`0x0081`).
//! - `counter()` returns a lambda, whose body javac emits as the synthetic
//!   `lambda$counter$0` method (`0x1002`), and the nested `Kind` enum owns the synthetic
//!   `$VALUES` field (`0x101A`) and `$values()` method (`0x100A`).
//!
//! The assertions are the flags of those committed class files, not values derived from
//! the metadata the natives read, so a fixture rebuild that changes a flag fails here.
//! The `getModifiers()` assertions pin the JDK masks: `Method.getModifiers()` exposes
//! `bridge`/`varargs`/`synthetic` (`Modifier.methodModifiers() == 0x1DFF`), a field's
//! exposes `synthetic`, and a constructor's exposes only the three access bits, so
//! `Constructor.isVarArgs()` reports `0x0080` that `getModifiers()` hides.
//!
//! The fixture also drives the Spring blockers this slice fixed: a `Set` constructor
//! deduplicates (including `null`), a list constructor copies order and duplicates,
//! `removeAll`/`retainAll`/`containsAll` work over the heap collections, and a
//! `Comparator` whose `compare` is declared on a superclass runs through `List.sort`.
//!
//! A member object whose declaration is absent from its class file has no flags to
//! report, so the predicate natives answer with a typed `VmError::Native` rather than a
//! fabricated `false`; a named lookup miss stays `null`, the M2.14 behavior.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ClassId, DirectorySource, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, arrays, boxed,
    collections, instance, object, reflection, string,
};

const PROBE: &str = "demo/FlagsProbe";
const KIND: &str = "demo/FlagsProbe$Kind";
const METHOD: &str = "Ljava/lang/reflect/Method;";
const CONSTRUCTOR: &str = "Ljava/lang/reflect/Constructor;";
const FIELD: &str = "Ljava/lang/reflect/Field;";
const EXECUTABLE: &str = "Ljava/lang/reflect/Executable;";
const MEMBER: &str = "Ljava/lang/reflect/Member;";

/// The bridge javac adds for `Getter<String>.get()`: public, bridge and synthetic.
const BRIDGE_GET_FLAGS: i32 = 0x1041;
/// The real `String get()` implementation.
const IMPLEMENTATION_GET_FLAGS: i32 = 0x0001;
/// `public static int sum(int...)`: public, static and varargs.
const VARARGS_SUM_FLAGS: i32 = 0x0089;
/// `public FlagsProbe(int...)`: public and varargs in the class file.
const VARARGS_CONSTRUCTOR_FLAGS: i32 = 0x0081;
/// The varargs constructor as `Constructor.getModifiers()` reports it: public only.
const VARARGS_CONSTRUCTOR_MODIFIERS: i32 = 0x0001;
/// `private void lambda$counter$0()`: private and synthetic.
const LAMBDA_METHOD_FLAGS: i32 = 0x1002;
/// `Kind.$VALUES`: private, static, final and synthetic.
const ENUM_VALUES_FIELD_FLAGS: i32 = 0x101A;
/// `Kind.$values()`: private, static and synthetic.
const ENUM_VALUES_METHOD_FLAGS: i32 = 0x100A;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/method_flags/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
    kind: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    object::register_natives(&shared);
    instance::register_natives(&shared);
    string::register_natives(&shared);
    collections::register_natives(&shared);
    boxed::register_natives(&shared);
    arrays::register_natives(&shared);
    reflection::register_natives(&shared);
    rava_interp::callbacks::register_natives(&shared);
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let probe = shared
        .load_class(loader, PROBE)
        .expect("demo/FlagsProbe loads");
    let kind = shared
        .load_class(loader, KIND)
        .expect("demo/FlagsProbe$Kind loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
        kind,
    }
}

fn call(
    fixture: &Fixture,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        args,
    )
}

fn ok_call(fixture: &Fixture, name: &str, descriptor: &str, args: Vec<Value>) -> Option<Value> {
    call(fixture, name, descriptor, args).unwrap_or_else(|error| panic!("{name} fails: {error}"))
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn boolean_value(value: Option<Value>) -> bool {
    int_value(value) != 0
}

fn flag_int(fixture: &Fixture, name: &str, argument: &str, member: ObjRef) -> i32 {
    int_value(ok_call(
        fixture,
        name,
        &format!("({argument})I"),
        vec![Value::Ref(member)],
    ))
}

fn flag_bool(fixture: &Fixture, name: &str, argument: &str, member: ObjRef) -> bool {
    boolean_value(ok_call(
        fixture,
        name,
        &format!("({argument})Z"),
        vec![Value::Ref(member)],
    ))
}

/// The raw `Method` `access_flags` from the committed class file.
///
/// `getModifiers()` is the JDK-masked view, so this is the only assertion that sees the
/// class-file bits a constructor's mask hides.
fn class_file_method_flags(fixture: &Fixture, class: ClassId, name: &str, descriptor: &str) -> i32 {
    let entry = fixture.shared.entry(class).expect("the class is loaded");
    let metadata = entry.metadata().expect("the class metadata decodes");
    let method = metadata
        .methods
        .iter()
        .find(|method| method.name == name && method.descriptor == descriptor)
        .unwrap_or_else(|| panic!("{name}{descriptor} is declared"));
    i32::from(method.access_flags)
}

/// The raw `Field` `access_flags` from the committed class file.
fn class_file_field_flags(fixture: &Fixture, class: ClassId, name: &str, descriptor: &str) -> i32 {
    let entry = fixture.shared.entry(class).expect("the class is loaded");
    let metadata = entry.metadata().expect("the class metadata decodes");
    let field = metadata
        .fields
        .iter()
        .find(|field| field.name == name && field.descriptor == descriptor)
        .unwrap_or_else(|| panic!("{name}:{descriptor} is declared"));
    i32::from(field.access_flags)
}

fn member(
    fixture: &Fixture,
    class: ClassId,
    kind: rava_vm::MemberKind,
    name: &str,
    descriptor: &str,
) -> ObjRef {
    fixture
        .shared
        .reflect_member(class, kind, name, descriptor)
        .unwrap_or_else(|error| panic!("member {name}{descriptor} allocates: {error}"))
}

fn method_member(fixture: &Fixture, name: &str, descriptor: &str) -> ObjRef {
    member(
        fixture,
        fixture.probe,
        rava_vm::MemberKind::Method,
        name,
        descriptor,
    )
}

fn constructor_member(fixture: &Fixture, descriptor: &str) -> ObjRef {
    member(
        fixture,
        fixture.probe,
        rava_vm::MemberKind::Constructor,
        "<init>",
        descriptor,
    )
}

fn field_member(fixture: &Fixture, class: ClassId, name: &str, descriptor: &str) -> ObjRef {
    member(fixture, class, rava_vm::MemberKind::Field, name, descriptor)
}

#[test]
fn bridge_method_reports_public_bridge_synthetic_and_not_varargs() {
    let fixture = fixture();
    let bridge = method_member(&fixture, "get", "()Ljava/lang/Object;");
    assert_eq!(
        class_file_method_flags(&fixture, fixture.probe, "get", "()Ljava/lang/Object;"),
        BRIDGE_GET_FLAGS
    );
    assert_eq!(
        flag_int(&fixture, "methodModifiers", METHOD, bridge),
        BRIDGE_GET_FLAGS
    );
    assert!(flag_bool(&fixture, "methodIsBridge", METHOD, bridge));
    assert!(flag_bool(&fixture, "methodIsSynthetic", METHOD, bridge));
    assert!(!flag_bool(&fixture, "methodIsVarArgs", METHOD, bridge));

    let implementation = method_member(&fixture, "get", "()Ljava/lang/String;");
    assert_eq!(
        flag_int(&fixture, "methodModifiers", METHOD, implementation),
        IMPLEMENTATION_GET_FLAGS
    );
    assert!(!flag_bool(
        &fixture,
        "methodIsBridge",
        METHOD,
        implementation
    ));
    assert!(!flag_bool(
        &fixture,
        "methodIsSynthetic",
        METHOD,
        implementation
    ));
}

#[test]
fn varargs_method_and_constructor_report_acc_varargs() {
    let fixture = fixture();
    let method = method_member(&fixture, "sum", "([I)I");
    assert_eq!(
        class_file_method_flags(&fixture, fixture.probe, "sum", "([I)I"),
        VARARGS_SUM_FLAGS
    );
    assert_eq!(
        flag_int(&fixture, "methodModifiers", METHOD, method),
        VARARGS_SUM_FLAGS
    );
    assert!(flag_bool(&fixture, "methodIsVarArgs", METHOD, method));
    assert!(!flag_bool(&fixture, "methodIsBridge", METHOD, method));
    assert!(!flag_bool(&fixture, "methodIsSynthetic", METHOD, method));

    let constructor = constructor_member(&fixture, "([I)V");
    assert_eq!(
        class_file_method_flags(&fixture, fixture.probe, "<init>", "([I)V"),
        VARARGS_CONSTRUCTOR_FLAGS
    );
    assert!(flag_bool(
        &fixture,
        "constructorIsVarArgs",
        CONSTRUCTOR,
        constructor
    ));
    assert!(!flag_bool(
        &fixture,
        "constructorIsSynthetic",
        CONSTRUCTOR,
        constructor
    ));
    // The JDK's `Modifier.constructorModifiers()` mask hides `varargs` and `synthetic`,
    // so the class-file `0x0081` surfaces as `public` only.
    assert_eq!(
        flag_int(&fixture, "constructorModifiers", CONSTRUCTOR, constructor),
        VARARGS_CONSTRUCTOR_MODIFIERS
    );
}

#[test]
fn synthetic_lambda_method_and_enum_members_report_acc_synthetic() {
    let fixture = fixture();
    let lambda = method_member(&fixture, "lambda$counter$0", "()V");
    assert_eq!(
        flag_int(&fixture, "methodModifiers", METHOD, lambda),
        LAMBDA_METHOD_FLAGS
    );
    assert!(flag_bool(&fixture, "methodIsSynthetic", METHOD, lambda));
    assert!(!flag_bool(&fixture, "methodIsBridge", METHOD, lambda));
    assert!(!flag_bool(&fixture, "methodIsVarArgs", METHOD, lambda));

    let values_field = field_member(&fixture, fixture.kind, "$VALUES", "[Ldemo/FlagsProbe$Kind;");
    assert_eq!(
        class_file_field_flags(&fixture, fixture.kind, "$VALUES", "[Ldemo/FlagsProbe$Kind;"),
        ENUM_VALUES_FIELD_FLAGS
    );
    assert_eq!(
        flag_int(&fixture, "fieldModifiers", FIELD, values_field),
        ENUM_VALUES_FIELD_FLAGS
    );
    assert!(flag_bool(&fixture, "fieldIsSynthetic", FIELD, values_field));

    let values_method = member(
        &fixture,
        fixture.kind,
        rava_vm::MemberKind::Method,
        "$values",
        "()[Ldemo/FlagsProbe$Kind;",
    );
    assert_eq!(
        flag_int(&fixture, "methodModifiers", METHOD, values_method),
        ENUM_VALUES_METHOD_FLAGS
    );
    assert!(flag_bool(
        &fixture,
        "methodIsSynthetic",
        METHOD,
        values_method
    ));

    // The non-synthetic enum member stays false, so the flag is read, not inferred.
    let constant = field_member(&fixture, fixture.kind, "FIRST", "Ldemo/FlagsProbe$Kind;");
    assert!(!flag_bool(&fixture, "fieldIsSynthetic", FIELD, constant));
}

#[test]
fn discovery_helpers_match_the_jdk_filter_loops() {
    let fixture = fixture();
    for (name, descriptor, argument, expected) in [
        (
            "bridgeMethod",
            "()Ljava/lang/reflect/Method;",
            METHOD,
            BRIDGE_GET_FLAGS,
        ),
        (
            "varargsMethod",
            "()Ljava/lang/reflect/Method;",
            METHOD,
            VARARGS_SUM_FLAGS,
        ),
        (
            "syntheticMethod",
            "()Ljava/lang/reflect/Method;",
            METHOD,
            LAMBDA_METHOD_FLAGS,
        ),
    ] {
        let found = match ok_call(&fixture, name, descriptor, Vec::new()) {
            Some(Value::Ref(reference)) if !reference.is_null() => reference,
            other => panic!("{name} did not find its member: {other:?}"),
        };
        assert_eq!(
            flag_int(&fixture, "methodModifiers", argument, found),
            expected
        );
    }

    let constructor = match ok_call(
        &fixture,
        "varargsConstructor",
        "()Ljava/lang/reflect/Constructor;",
        Vec::new(),
    ) {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        other => panic!("varargsConstructor did not find its member: {other:?}"),
    };
    assert!(flag_bool(
        &fixture,
        "constructorIsVarArgs",
        CONSTRUCTOR,
        constructor
    ));

    let field = match ok_call(
        &fixture,
        "syntheticField",
        "()Ljava/lang/reflect/Field;",
        Vec::new(),
    ) {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        other => panic!("syntheticField did not find its member: {other:?}"),
    };
    assert_eq!(
        flag_int(&fixture, "fieldModifiers", FIELD, field),
        ENUM_VALUES_FIELD_FLAGS
    );
}

#[test]
fn executable_and_member_declarations_answer_the_same_natives() {
    let fixture = fixture();
    // The concrete classes redeclare `isVarArgs`/`isSynthetic`, so these calls resolve to
    // `Method`/`Constructor`; the `Executable` and `Member` registry entries keep the same
    // answers for a receiver type that inherits the declaration.
    let method = method_member(&fixture, "lambda$counter$0", "()V");
    assert!(flag_bool(
        &fixture,
        "executableIsSynthetic",
        EXECUTABLE,
        method
    ));
    assert!(!flag_bool(
        &fixture,
        "executableIsVarArgs",
        EXECUTABLE,
        method
    ));

    let constructor = constructor_member(&fixture, "([I)V");
    assert!(flag_bool(
        &fixture,
        "executableIsVarArgs",
        EXECUTABLE,
        constructor
    ));
    assert!(!flag_bool(
        &fixture,
        "executableIsSynthetic",
        EXECUTABLE,
        constructor
    ));

    let values_field = field_member(&fixture, fixture.kind, "$VALUES", "[Ldemo/FlagsProbe$Kind;");
    assert!(flag_bool(
        &fixture,
        "memberIsSynthetic",
        MEMBER,
        values_field
    ));
    assert!(!flag_bool(
        &fixture,
        "memberIsSynthetic",
        MEMBER,
        method_member(&fixture, "sum", "([I)I")
    ));
}

#[test]
fn a_member_missing_from_its_class_file_is_a_typed_native_error() {
    let fixture = fixture();
    let absent = member(
        &fixture,
        fixture.probe,
        rava_vm::MemberKind::Method,
        "absentMethod",
        "()V",
    );
    match call(
        &fixture,
        "methodIsBridge",
        &format!("({METHOD})Z"),
        vec![Value::Ref(absent)],
    ) {
        Err(InterpError::Vm(VmError::Native(message))) => {
            assert!(
                message.contains("has no java.lang.reflect.Method absentMethod"),
                "unexpected native error: {message}"
            );
        }
        other => panic!("expected a typed native error, found {other:?}"),
    }
}

#[test]
fn a_named_lookup_miss_stays_null() {
    let fixture = fixture();
    match ok_call(
        &fixture,
        "missingMethod",
        "()Ljava/lang/reflect/Method;",
        Vec::new(),
    ) {
        Some(Value::Ref(reference)) => assert!(reference.is_null()),
        other => panic!("expected a null Method, found {other:?}"),
    }
}

#[test]
fn set_constructor_deduplicates_and_list_constructor_keeps_duplicates() {
    let fixture = fixture();
    assert_eq!(
        int_value(ok_call(&fixture, "setConstructorSize", "()I", Vec::new())),
        3
    );
    assert!(boolean_value(ok_call(
        &fixture,
        "setConstructorContainsNull",
        "()Z",
        Vec::new()
    )));
    assert_eq!(
        int_value(ok_call(&fixture, "listConstructorSize", "()I", Vec::new())),
        3
    );
}

#[test]
fn bulk_collection_operations_work_over_the_heap_collections() {
    let fixture = fixture();
    assert!(boolean_value(ok_call(
        &fixture,
        "bulkRemoveAll",
        "()Z",
        Vec::new()
    )));
    assert!(boolean_value(ok_call(
        &fixture,
        "bulkRetainAll",
        "()Z",
        Vec::new()
    )));
    assert!(boolean_value(ok_call(
        &fixture,
        "bulkContainsAll",
        "()Z",
        Vec::new()
    )));
}

#[test]
fn a_comparator_inheriting_compare_runs_through_list_sort() {
    let fixture = fixture();
    assert_eq!(
        int_value(ok_call(&fixture, "inheritedCompareSort", "()I", Vec::new())),
        1
    );
}
