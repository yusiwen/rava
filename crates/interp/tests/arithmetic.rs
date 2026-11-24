//! M0.5a acceptance: decoded IR, frames and a static call.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::run_static_method;
use rava_vm::{DirectorySource, LoaderId, Shared, Value};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn add_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Adder", "java/lang/Object", 52);
    let add_ref = builder.method_ref("Adder", "add", "(II)I");

    let mut add = Assembler::new();
    add.iload(0);
    add.iload(1);
    add.iadd();
    add.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "add",
        "(II)I",
        Some((2, 2, add.finish().unwrap())),
    );

    let mut twice = Assembler::new();
    twice.iload(0);
    twice.iload(0);
    twice.op(0xb8, &add_ref.to_be_bytes());
    twice.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "twice",
        "(I)I",
        Some((2, 2, twice.finish().unwrap())),
    );

    builder.build()
}

fn fixture() -> (Shared, rava_vm::ClassId, rava_vm::ThreadState) {
    let dir = std::env::temp_dir().join(format!("rava-m05a-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shared = Shared::with_bootstrap(Arc::new(DirectorySource::new(dir)));
    let class = shared
        .define_class(LoaderId::BOOTSTRAP, "Adder", &add_class())
        .unwrap();
    let thread = shared.thread_state(1);
    (shared, class, thread)
}

#[test]
fn runs_integer_arithmetic_through_a_frame() {
    let (shared, class, thread) = fixture();
    let result = run_static_method(
        &shared,
        &thread,
        class,
        "add",
        "(II)I",
        vec![Value::Int(2), Value::Int(3)],
    )
    .unwrap();
    assert_eq!(result, Some(Value::Int(5)));
    assert!(thread.polls() > 0, "the frame polls the safepoint");
}

#[test]
fn invokestatic_resolves_and_returns_a_value() {
    let (shared, class, thread) = fixture();
    let result = run_static_method(
        &shared,
        &thread,
        class,
        "twice",
        "(I)I",
        vec![Value::Int(5)],
    )
    .unwrap();
    assert_eq!(result, Some(Value::Int(10)));
}

#[test]
fn a_missing_method_is_an_error_not_a_panic() {
    let (shared, class, thread) = fixture();
    let error = run_static_method(&shared, &thread, class, "add", "(I)I", vec![Value::Int(1)]);
    assert!(error.is_err());
}

#[allow(dead_code)]
fn _manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
