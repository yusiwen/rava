//! M3.1 acceptance: a generated class is verified when it is defined and then runs.
//!
//! Every fixture is assembled in process with [`ClassBuilder`] + [`Assembler`], so the
//! suite needs no JDK, network or files beyond the committed `stdlib/c1/classes`. The
//! classes are defined through [`Shared::define_class`], which now verifies every method
//! that carries a `Code` attribute before inserting the class into the table.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder, ClassFile};
use rava_interp::{run_instance_method, run_static_method};
use rava_vm::{ClassPathSource, LoaderId, Shared, ThreadState, Value, VmError, verify_method};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

/// A library-backed VM plus an empty application loader, with every native registered.
fn fixture() -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let application = shared
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(ClassPathSource::empty()))
        .expect("application loader is added");
    rava_vm::library::register_all(&shared).expect("the class library registers");
    let thread = shared.thread_state(1);
    (shared, application, thread)
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn defined(shared: &Shared, loader: LoaderId, name: &str, bytes: &[u8]) -> rava_vm::ClassId {
    shared
        .define_class(loader, name, bytes)
        .unwrap_or_else(|error| panic!("{name} defines: {error}"))
}

/// `demo/Generated`: an `int` field, a constructor, an instance getter and a static
/// method, i.e. the shape a generated proxy subclass starts from.
fn generated_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/Generated", "java/lang/Object", 61);
    builder.field(ACC_PUBLIC, "value", "I");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let value_field = builder.field_ref("demo/Generated", "value", "I");

    // demo/Generated(int value) { super(); this.value = value; }
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.aload(0);
    init.iload(1);
    init.putfield(value_field);
    init.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "(I)V",
        Some((2, 2, init.finish().expect("assemble <init>"))),
    );

    // int get() { return this.value; }
    let mut get = Assembler::new();
    get.aload(0);
    get.getfield(value_field);
    get.ireturn();
    builder.method(
        ACC_PUBLIC,
        "get",
        "()I",
        Some((1, 1, get.finish().expect("assemble get"))),
    );

    // static int answer() { return 42; }
    let mut answer = Assembler::new();
    answer.iconst(42);
    answer.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "answer",
        "()I",
        Some((1, 0, answer.finish().expect("assemble answer"))),
    );

    builder.build()
}

/// `demo/Switches`: one `tableswitch` and one `lookupswitch`, both dense enough that the
/// selector chooses among several returns.
fn switches_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/Switches", "java/lang/Object", 61);

    // static int classify(int key) {
    //     switch (key) { case 1: return 10; case 2: return 20; default: return 30; }
    // }
    let mut classify = Assembler::new();
    let one = classify.label();
    let two = classify.label();
    let otherwise = classify.label();
    classify.iload(0);
    classify.tableswitch(otherwise, 1, &[one, two]);
    classify.place(one);
    classify.iconst(10);
    classify.ireturn();
    classify.place(two);
    classify.iconst(20);
    classify.ireturn();
    classify.place(otherwise);
    classify.iconst(30);
    classify.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "classify",
        "(I)I",
        Some((1, 1, classify.finish().expect("assemble classify"))),
    );

    // static int sparse(int key) {
    //     switch (key) { case 1: return 11; case 100: return 22; default: return 33; }
    // }
    let mut sparse = Assembler::new();
    let first = sparse.label();
    let hundred = sparse.label();
    let otherwise = sparse.label();
    sparse.iload(0);
    sparse.lookupswitch(otherwise, &[(1, first), (100, hundred)]);
    sparse.place(first);
    sparse.iconst(11);
    sparse.ireturn();
    sparse.place(hundred);
    sparse.iconst(22);
    sparse.ireturn();
    sparse.place(otherwise);
    sparse.iconst(33);
    sparse.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "sparse",
        "(I)I",
        Some((1, 1, sparse.finish().expect("assemble sparse"))),
    );

    builder.build()
}

/// `demo/WideLocal`: stores and loads local slot 256, past the one-byte local index
/// range, so both instructions need the `wide` prefix.
fn wide_local_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/WideLocal", "java/lang/Object", 61);

    // static int wideLocal() { int slot; slot = 7; return slot; }
    let mut wide = Assembler::new();
    wide.iconst(7);
    wide.op(0xc4, &[0x36, 0x01, 0x00]); // wide istore 256
    wide.op(0xc4, &[0x15, 0x01, 0x00]); // wide iload 256
    wide.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "wideLocal",
        "()I",
        Some((1, 257, wide.finish().expect("assemble wideLocal"))),
    );

    builder.build()
}

#[test]
fn a_generated_class_is_defined_verified_and_runs() {
    let (shared, application, thread) = fixture();
    let bytes = generated_class();
    let class = defined(&shared, application, "demo/Generated", &bytes);

    assert_eq!(
        shared.lookup(application, "demo/Generated"),
        Some(class),
        "the verified class is in the table under its application loader"
    );
    assert_eq!(shared.lookup(LoaderId::BOOTSTRAP, "demo/Generated"), None);

    let answer = run_static_method(&shared, &thread, class, "answer", "()I", Vec::new())
        .expect("answer runs");
    assert_eq!(int_value(answer), 42);

    let instance = shared.allocate_instance(class).expect("instance allocates");
    run_instance_method(
        &shared,
        &thread,
        class,
        "<init>",
        "(I)V",
        instance,
        vec![Value::Int(7)],
    )
    .expect("<init> runs");
    let value = run_instance_method(&shared, &thread, class, "get", "()I", instance, Vec::new())
        .expect("get runs");
    assert_eq!(int_value(value), 7);
}

#[test]
fn generated_tableswitch_and_lookupswitch_dispatch_through_the_interpreter() {
    let (shared, application, thread) = fixture();
    let bytes = switches_class();
    let class = defined(&shared, application, "demo/Switches", &bytes);

    let classify = |key: i32| {
        let result = run_static_method(
            &shared,
            &thread,
            class,
            "classify",
            "(I)I",
            vec![Value::Int(key)],
        )
        .expect("classify runs");
        int_value(result)
    };
    assert_eq!(classify(1), 10);
    assert_eq!(classify(2), 20);
    assert_eq!(classify(3), 30);
    assert_eq!(classify(-1), 30);

    let sparse = |key: i32| {
        let result = run_static_method(
            &shared,
            &thread,
            class,
            "sparse",
            "(I)I",
            vec![Value::Int(key)],
        )
        .expect("sparse runs");
        int_value(result)
    };
    assert_eq!(sparse(1), 11);
    assert_eq!(sparse(100), 22);
    assert_eq!(sparse(2), 33);
}

#[test]
fn wide_local_access_verifies_and_runs() {
    let (shared, application, thread) = fixture();
    let bytes = wide_local_class();

    let file = ClassFile::parse(&bytes).expect("the fixture parses");
    let method = file
        .find_method("wideLocal", "()I")
        .expect("the constant pool resolves")
        .expect("wideLocal exists");
    let verified = verify_method(&file, method).expect("wideLocal verifies");
    assert_eq!(verified.max_locals, 257, "slot 256 needs max_locals 257");

    let class = defined(&shared, application, "demo/WideLocal", &bytes);
    let result = run_static_method(&shared, &thread, class, "wideLocal", "()I", Vec::new())
        .expect("wideLocal runs");
    assert_eq!(int_value(result), 7);
}

/// `demo/Broken`: `static int value() { return 1; }` with the given `max_stack`.
fn broken_class(max_stack: u16) -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/Broken", "java/lang/Object", 61);
    let mut body = Assembler::new();
    body.iconst(1);
    body.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "value",
        "()I",
        Some((max_stack, 0, body.finish().expect("assemble value"))),
    );
    builder.build()
}

#[test]
fn a_class_whose_stack_is_too_small_is_rejected_without_touching_the_table() {
    let (shared, application, thread) = fixture();
    let before = shared.class_count();

    let error = shared
        .define_class(application, "demo/Broken", &broken_class(0))
        .expect_err("the under-sized stack is rejected");
    match error {
        VmError::Verify {
            class,
            method,
            detail,
        } => {
            assert_eq!(class, "demo/Broken");
            assert_eq!(method, "value()I");
            assert!(
                detail.contains("stack overflow"),
                "the verifier detail names the overflow: {detail}"
            );
        }
        other => panic!("expected VmError::Verify, found {other:?}"),
    }

    assert_eq!(
        shared.class_count(),
        before,
        "the rejected class is not inserted"
    );
    assert_eq!(
        shared.lookup(application, "demo/Broken"),
        None,
        "the rejected class did not reserve its name"
    );

    // The rejection left neither a table entry nor a duplicate marker: the corrected
    // class defines under the same name and runs.
    let valid = defined(&shared, application, "demo/Broken", &broken_class(1));
    assert_eq!(shared.class_count(), before + 1);
    let value = run_static_method(&shared, &thread, valid, "value", "()I", Vec::new())
        .expect("the corrected class runs");
    assert_eq!(int_value(value), 1);
}

#[test]
fn defining_the_same_loader_and_name_twice_is_a_duplicate_class() {
    let (shared, application, _thread) = fixture();
    let bytes = generated_class();
    let class = defined(&shared, application, "demo/Generated", &bytes);
    let after_first = shared.class_count();

    let error = shared
        .define_class(application, "demo/Generated", &bytes)
        .expect_err("the second definition is rejected");
    match error {
        VmError::DuplicateClass { loader, name } => {
            assert_eq!(loader, application);
            assert_eq!(name, "demo/Generated");
        }
        other => panic!("expected VmError::DuplicateClass, found {other:?}"),
    }
    assert_eq!(shared.class_count(), after_first);
    assert_eq!(shared.lookup(application, "demo/Generated"), Some(class));
}
