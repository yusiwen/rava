//! M0.6: nested `invokestatic` frames register their roots with the thread.

use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::run_static_method;
use rava_vm::{DirectorySource, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

const ACC_PUBLIC_STATIC: u16 = 0x0009;
const ACC_PUBLIC_STATIC_NATIVE: u16 = 0x0109;

fn nested_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Nested", "java/lang/Object", 52);
    let outer_ref = builder.method_ref("Nested", "outer", "()I");
    let inner_ref = builder.method_ref("Nested", "inner", "()I");
    let probe_ref = builder.method_ref("Nested", "probe", "()I");

    let mut main = Assembler::new();
    main.op(0xb8, &outer_ref.to_be_bytes());
    main.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "main",
        "(Ljava/lang/String;)I",
        Some((1, 1, main.finish().unwrap())),
    );

    let mut outer = Assembler::new();
    outer.op(0xb8, &inner_ref.to_be_bytes());
    outer.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "outer",
        "()I",
        Some((1, 0, outer.finish().unwrap())),
    );

    let mut inner = Assembler::new();
    inner.op(0xb8, &probe_ref.to_be_bytes());
    inner.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "inner",
        "()I",
        Some((1, 0, inner.finish().unwrap())),
    );

    builder.method(ACC_PUBLIC_STATIC_NATIVE, "probe", "()I", None);
    builder.build()
}

/// Reports the thread's frame depth and root count at the native boundary:
/// `depth * 100 + roots`.
fn probe(
    _shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let depth = thread.frame_depth() as i32;
    let roots = thread.frame_roots().len() as i32;
    Ok(Some(Value::Int(depth * 100 + roots)))
}

fn fixture() -> (Shared, rava_vm::ClassId, ThreadState) {
    let dir = std::env::temp_dir().join(format!("rava-m06-interp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shared = Shared::with_bootstrap(Arc::new(DirectorySource::new(dir)));
    let class = shared
        .define_class(LoaderId::BOOTSTRAP, "Nested", &nested_class())
        .unwrap();
    shared.register_native("Nested", "probe", "()I", probe);
    let thread = shared.thread_state(1);
    (shared, class, thread)
}

#[test]
fn nested_invokestatic_frames_publish_their_roots_and_clean_up() {
    let (shared, class, thread) = fixture();
    let argument = shared.heap().allocate_string("root");

    let result = run_static_method(
        &shared,
        &thread,
        class,
        "main",
        "(Ljava/lang/String;)I",
        vec![Value::Ref(argument)],
    )
    .unwrap();

    // main, outer and inner are active at the native boundary; only main holds a
    // reference (its String argument).
    assert_eq!(result, Some(Value::Int(301)));
    assert_eq!(thread.frame_depth(), 0);
    assert!(thread.frame_roots().is_empty());
    assert!(shared.roots(&[&thread]).is_empty());
    assert!(shared.heap().get(argument).is_ok());
}

#[test]
fn a_failed_call_boundary_probe_is_an_error_not_a_panic() {
    let (shared, class, thread) = fixture();
    let _heap = shared.heap();
    let error = run_static_method(
        &shared,
        &thread,
        class,
        "main",
        "(Ljava/lang/String;)I",
        vec![Value::Ref(ObjRef::NULL)],
    );
    assert!(matches!(error, Err(rava_interp::InterpError::HeldLock(_))));
}
