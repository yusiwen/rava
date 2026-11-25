//! Issue #53 acceptance: `Class.getFields()` and the observed C1 closure boundary.
//!
//! `demo.FieldsProbe` is a committed `javac --release 17` fixture (see
//! `tests/fixtures/fields/README.md`) with this hierarchy:
//!
//! ```text
//! Object
//!   A                 (a, A_STATIC)
//!     B extends A implements I   (b, a hides A.a, B_STATIC)
//!       C extends B implements J (c, b hides B.b, private secret)
//!   I                 (I_CONST)
//!     J extends I     (J_CONST)
//! ```
//!
//! Its `run()` returns the exact JDK 17 `C.class.getFields()` output: own public fields
//! in class-file order, then `J`/`I` recursively, then the superclass `B`/`A`. The
//! assertions are the stock JDK 17 lines, so a fixture rebuild that changes the order or
//! drops a hidden field fails here.
//!
//! The last test defines one class whose superclass and one whose interface are not in
//! the C1 tree. Each missing edge is a real `ClassNotFound`, and `getFields()` must still
//! answer with the class's own public fields: the documented tolerance, not an accidental
//! success.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ClassId, DirectorySource, LoaderId, Shared, ThreadState, Value, VmError, arrays, boxed,
    collections, exceptions, instance, object, reflection, string,
};

const PROBE: &str = "demo/FieldsProbe";
const ACC_PUBLIC: u16 = 0x0001;
const ACC_PRIVATE: u16 = 0x0002;
const ACC_STATIC: u16 = 0x0008;
const ACC_PUBLIC_STATIC: u16 = ACC_PUBLIC | ACC_STATIC;

/// The exact stock JDK 17 output of `demo.FieldsProbe.run()`.
const GOLDEN: &str = "\
C.getFields()
C.c
C.b
J.J_CONST
I.I_CONST
B.b
B.a
B.B_STATIC
A.a
A.A_STATIC
";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fields/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
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
    exceptions::register_natives(&shared);
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let probe = shared
        .load_class(loader, PROBE)
        .expect("demo/FieldsProbe loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
    }
}

fn call(
    fixture: &Fixture,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        class,
        name,
        descriptor,
        args,
    )
}

fn ok_call(
    fixture: &Fixture,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Option<Value> {
    call(fixture, class, name, descriptor, args)
        .unwrap_or_else(|error| panic!("{name} fails: {error}"))
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn string_value(shared: &Shared, value: Option<Value>) -> String {
    match value {
        Some(Value::Ref(reference)) => shared
            .heap()
            .string(reference)
            .expect("a String result")
            .to_owned(),
        other => panic!("expected a String result, found {other:?}"),
    }
}

fn probe_text(fixture: &Fixture, name: &str) -> String {
    string_value(
        &fixture.shared,
        ok_call(
            fixture,
            fixture.probe,
            name,
            "()Ljava/lang/String;",
            Vec::new(),
        ),
    )
}

#[test]
fn get_fields_matches_the_jdk_order_and_hidden_fields() {
    let fixture = fixture();
    let output = probe_text(&fixture, "run");
    assert_eq!(output, GOLDEN);
}

#[test]
fn get_field_prefers_the_most_derived_declaration_and_reaches_a_superclass() {
    let fixture = fixture();

    // `B.a` hides `A.a`, so the JDK's class-first search (and every other JDK search
    // order) reports the most-derived declaration.
    assert_eq!(probe_text(&fixture, "fieldA"), "B");
    // `A_STATIC` is not hidden, so the inherited lookup reaches the superclass.
    assert_eq!(probe_text(&fixture, "inheritedStatic"), "A");
}

#[test]
fn primitive_and_array_classes_return_empty_field_arrays() {
    let fixture = fixture();

    assert_eq!(
        int_value(ok_call(
            &fixture,
            fixture.probe,
            "primitiveFieldCount",
            "()I",
            Vec::new(),
        )),
        0
    );
    assert_eq!(
        int_value(ok_call(
            &fixture,
            fixture.probe,
            "arrayFieldCount",
            "()I",
            Vec::new(),
        )),
        0
    );
}

fn orphan_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/Orphan", "demo/MissingSuper", 61);
    builder.field(ACC_PUBLIC, "own", "I");
    builder.field(ACC_PRIVATE, "secret", "I");
    builder.field(ACC_PUBLIC | ACC_STATIC, "OWN_STATIC", "I");
    builder.build()
}

fn orphan_interface_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/OrphanInterface", "java/lang/Object", 61);
    builder.interface("demo/MissingInterface");
    builder.field(ACC_PUBLIC, "marked", "I");
    builder.build()
}

/// `int count(Class)` and `String firstName(Class)` against a supplied class object.
fn helper_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("FieldsHelper", "java/lang/Object", 61);
    let class_get_fields = builder.method_ref(
        "java/lang/Class",
        "getFields",
        "()[Ljava/lang/reflect/Field;",
    );
    let field_get_name =
        builder.method_ref("java/lang/reflect/Field", "getName", "()Ljava/lang/String;");

    let mut count = Assembler::new();
    count.aload(0);
    count.invokevirtual(class_get_fields);
    count.arraylength();
    count.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "count",
        "(Ljava/lang/Class;)I",
        Some((1, 1, count.finish().unwrap())),
    );

    let mut first_name = Assembler::new();
    first_name.aload(0);
    first_name.invokevirtual(class_get_fields);
    first_name.iconst(0);
    first_name.aaload();
    first_name.invokevirtual(field_get_name);
    first_name.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "firstName",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        Some((2, 1, first_name.finish().unwrap())),
    );

    builder.build()
}

#[test]
fn an_omitted_superclass_or_interface_ends_the_branch_instead_of_failing() {
    let fixture = fixture();
    let orphan = fixture
        .shared
        .define_class(LoaderId::BOOTSTRAP, "demo/Orphan", &orphan_bytes())
        .expect("the orphan defines");
    let orphan_interface = fixture
        .shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/OrphanInterface",
            &orphan_interface_bytes(),
        )
        .expect("the interface orphan defines");
    let helper = fixture
        .shared
        .define_class(LoaderId::BOOTSTRAP, "FieldsHelper", &helper_bytes())
        .expect("the helper defines");

    // Control: the superclass and the interface really are absent, so the tolerance is
    // exercised rather than a hierarchy that happened to resolve.
    match fixture.shared.resolution_superclass(orphan) {
        Err(VmError::ClassNotFound { .. }) => {}
        other => panic!("expected demo/MissingSuper to be absent, found {other:?}"),
    }
    assert!(
        matches!(
            fixture
                .shared
                .load_class(LoaderId::BOOTSTRAP, "demo/MissingInterface"),
            Err(VmError::ClassNotFound { .. })
        ),
        "demo/MissingInterface must be absent from the C1 tree"
    );

    let class_object = fixture
        .shared
        .class_object(orphan)
        .expect("the orphan class object");
    let count = int_value(ok_call(
        &fixture,
        helper,
        "count",
        "(Ljava/lang/Class;)I",
        vec![Value::Ref(class_object)],
    ));
    assert_eq!(count, 2, "only the two public fields are returned");

    let first = string_value(
        &fixture.shared,
        ok_call(
            &fixture,
            helper,
            "firstName",
            "(Ljava/lang/Class;)Ljava/lang/String;",
            vec![Value::Ref(class_object)],
        ),
    );
    assert_eq!(first, "own");

    // The same tolerance on an absent superinterface: `OrphanInterface` implements a
    // class the C1 tree does not ship, and still reports its own public field.
    let interface_object = fixture
        .shared
        .class_object(orphan_interface)
        .expect("the interface orphan class object");
    let interface_count = int_value(ok_call(
        &fixture,
        helper,
        "count",
        "(Ljava/lang/Class;)I",
        vec![Value::Ref(interface_object)],
    ));
    assert_eq!(interface_count, 1, "only the own public field is returned");
    let interface_first = string_value(
        &fixture.shared,
        ok_call(
            &fixture,
            helper,
            "firstName",
            "(Ljava/lang/Class;)Ljava/lang/String;",
            vec![Value::Ref(interface_object)],
        ),
    );
    assert_eq!(interface_first, "marked");
}
