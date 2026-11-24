//! M0.5b acceptance: the real Hello fixture through the interpreter.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::run_static_method;
use rava_vm::{
    DirectorySource, FieldLayout, HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;
const ACC_STATIC: u16 = 0x0008;
const ACC_PUBLIC_NATIVE: u16 = 0x0101;
const ACC_STATIC_NATIVE: u16 = 0x0108;

fn system_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("java/lang/System", "java/lang/Object", 52);
    builder.field(ACC_PUBLIC_STATIC, "out", "Ljava/io/PrintStream;");
    let init_ref = builder.method_ref("java/lang/System", "initializeSystemClass", "()V");

    let mut clinit = Assembler::new();
    clinit.op(0xb8, &init_ref.to_be_bytes());
    clinit.return_();
    builder.method(
        ACC_STATIC,
        "<clinit>",
        "()V",
        Some((0, 0, clinit.finish().unwrap())),
    );
    builder.method(ACC_STATIC_NATIVE, "initializeSystemClass", "()V", None);
    builder.build()
}

fn print_stream_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("java/io/PrintStream", "java/lang/Object", 52);
    builder.method(ACC_PUBLIC_NATIVE, "println", "(Ljava/lang/String;)V", None);
    builder.method(ACC_PUBLIC_NATIVE, "println", "(I)V", None);
    builder.build()
}

fn hello_bytes() -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../classfile/tests/fixtures/Hello.class"),
    )
    .unwrap()
}

fn init_system(
    shared: &Shared,
    _thread: &ThreadState,
    loader: LoaderId,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let print_stream = shared.load_class(loader, "java/io/PrintStream")?;
    let instance = shared
        .heap()
        .allocate_instance(print_stream, &FieldLayout::new());
    let system = shared.load_class(loader, "java/lang/System")?;
    let entry = shared
        .entry(system)
        .ok_or_else(|| VmError::Native("system class vanished".to_owned()))?;
    entry.statics.lock().expect("static lock").insert(
        ("out".to_owned(), "Ljava/io/PrintStream;".to_owned()),
        Value::Ref(instance),
    );
    Ok(None)
}

fn println(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Err(VmError::Native("println expects a String".to_owned()));
    };
    let text = match shared
        .heap()
        .get(reference)
        .map_err(|error| VmError::Native(error.to_string()))?
    {
        HeapObject::Str(text) => text.clone(),
        _ => return Err(VmError::Native("println expects a String".to_owned())),
    };
    shared.write_stdout(format!("{text}\n").as_bytes());
    Ok(None)
}

fn println_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Int(value)) = args.first().copied() else {
        return Err(VmError::Native("println(int) expects an int".to_owned()));
    };
    shared.write_stdout(format!("{value}\n").as_bytes());
    Ok(None)
}

fn fixture() -> (Shared, rava_vm::ClassId, ThreadState) {
    let dir = std::env::temp_dir().join(format!("rava-m05b-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shared = Shared::with_bootstrap(Arc::new(DirectorySource::new(dir)));
    let _ = shared
        .define_class(LoaderId::BOOTSTRAP, "java/lang/System", &system_class())
        .unwrap();
    let _ = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "java/io/PrintStream",
            &print_stream_class(),
        )
        .unwrap();
    let hello = shared
        .define_class(LoaderId::BOOTSTRAP, "Hello", &hello_bytes())
        .unwrap();

    shared.register_native(
        "java/lang/System",
        "initializeSystemClass",
        "()V",
        init_system,
    );
    shared.register_native(
        "java/io/PrintStream",
        "println",
        "(Ljava/lang/String;)V",
        println,
    );
    shared.register_native("java/io/PrintStream", "println", "(I)V", println_int);

    let thread = shared.thread_state(1);
    (shared, hello, thread)
}

#[test]
fn hello_prints_through_the_interpreter() {
    let (shared, hello, thread) = fixture();
    run_static_method(
        &shared,
        &thread,
        hello,
        "main",
        "([Ljava/lang/String;)V",
        vec![Value::Ref(ObjRef::NULL)],
    )
    .unwrap();
    assert_eq!(shared.stdout(), "Hello, rava\n42\n");
}

#[test]
fn system_initialization_runs_once() {
    let (shared, hello, thread) = fixture();
    run_static_method(
        &shared,
        &thread,
        hello,
        "main",
        "([Ljava/lang/String;)V",
        vec![Value::Ref(ObjRef::NULL)],
    )
    .unwrap();
    run_static_method(
        &shared,
        &thread,
        hello,
        "main",
        "([Ljava/lang/String;)V",
        vec![Value::Ref(ObjRef::NULL)],
    )
    .unwrap();
    assert_eq!(shared.stdout(), "Hello, rava\n42\nHello, rava\n42\n");
}
