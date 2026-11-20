//! S12 acceptance: exception basics — `Throwable.fillInStackTrace` and the trace it captures.
//!
//! The host's `Throwable` constructor calls the `synchronized` Java method `fillInStackTrace()`,
//! which calls the private native `fillInStackTrace(int)`. Until that native existed, every
//! `new RuntimeException()` stopped at it, so the VM could not even create a throwable. The
//! native captures the calling thread's stack once per throwable and stores it in
//! `Shared::traces`; the interpreter and the CLI already use that map for `athrow` and for the
//! uncaught-exception report.
//!
//! The fixtures are built with the class-file builder, so the tests need no JDK sources; the
//! CLI fixture needs `JAVA21_HOME` at run time because the real `Throwable` comes from the
//! host image.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, HeapObject, Shared, ThreadState, Value};

fn host_path(dir: &Path) -> ClassPath {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    rava_cli::class_path(Path::new(&home), dir).expect("class path")
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-exceptions-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn write(dir: &Path, name: &str, bytes: Vec<u8>) {
    std::fs::write(dir.join(format!("{name}.class")), bytes).expect("fixture class");
}

/// Encoder for the three instruction forms the fixtures use; the builder owns constant-pool
/// indices but not opcodes (yet).
struct Code(Vec<u8>);

impl Code {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn op(&mut self, opcode: u8) {
        self.0.push(opcode);
    }

    fn call(&mut self, opcode: u8, index: u16) {
        self.0
            .extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
    }

    fn done(self) -> Vec<u8> {
        self.0
    }
}

/// `MakeException.main`: `STORED = new RuntimeException(); return;`
fn build_make_exception(dir: &Path) {
    let mut builder = ClassBuilder::new("MakeException", "java/lang/Object", 65);
    builder.field(0x0009, "STORED", "Ljava/lang/RuntimeException;");
    let exception = builder.class("java/lang/RuntimeException");
    let init = builder.method_ref("java/lang/RuntimeException", "<init>", "()V");
    let stored = builder.field_ref("MakeException", "STORED", "Ljava/lang/RuntimeException;");

    let mut code = Code::new();
    code.call(0xbb, exception); // new
    code.op(0x59); // dup
    code.call(0xb7, init); // invokespecial
    code.call(0xb3, stored); // putstatic
    code.op(0xb1); // return

    builder.method(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((2, 1, code.done())),
    );
    write(dir, "MakeException", builder.build());
}

/// `ThrowException.main`: `throw new IllegalStateException();` — never caught.
fn build_throw_exception(dir: &Path) {
    let mut builder = ClassBuilder::new("ThrowException", "java/lang/Object", 65);
    let exception = builder.class("java/lang/IllegalStateException");
    let init = builder.method_ref("java/lang/IllegalStateException", "<init>", "()V");

    let mut code = Code::new();
    code.call(0xbb, exception); // new
    code.op(0x59); // dup
    code.call(0xb7, init); // invokespecial
    code.op(0xbf); // athrow

    builder.method(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((2, 1, code.done())),
    );
    write(dir, "ThrowException", builder.build());
}

/// `CatchException.main`: `try { throw new IllegalStateException(); } catch (...) { CAUGHT = 1; }`
///
/// The exception table is the only control flow: `athrow` at offset 7, handler at offset 8,
/// whose first instruction stores the caught object in local 1.
fn build_catch_exception(dir: &Path) {
    let mut builder = ClassBuilder::new("CatchException", "java/lang/Object", 65);
    builder.field(0x0009, "CAUGHT", "I");
    let exception = builder.class("java/lang/IllegalStateException");
    let init = builder.method_ref("java/lang/IllegalStateException", "<init>", "()V");
    let caught = builder.field_ref("CatchException", "CAUGHT", "I");

    let mut code = Code::new();
    code.call(0xbb, exception); // 0: new
    code.op(0x59); // 3: dup
    code.call(0xb7, init); // 4: invokespecial
    code.op(0xbf); // 7: athrow
    code.op(0x4c); // 8: astore_1
    code.op(0x04); // 9: iconst_1
    code.call(0xb3, caught); // 10: putstatic
    code.op(0xb1); // 13: return

    builder.method_with_handlers(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((2, 2, code.done())),
        // [0, 8) covers new/dup/invokespecial/athrow; the handler is the astore.
        vec![(0, 8, 8, exception)],
    );
    write(dir, "CatchException", builder.build());
}

#[test]
fn making_an_exception_captures_a_trace() {
    let dir = fixture_dir("make");
    build_make_exception(&dir);
    let path = host_path(&dir);
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "MakeException", &[])
        .unwrap_or_else(|error| panic!("MakeException failed: {error}"));

    let id = *shared
        .class_ids
        .get("MakeException")
        .expect("MakeException loaded");
    let stored = shared.classes[id.index()]
        .statics
        .get("STORED")
        .expect("STORED was never set");
    let Value::Ref(stored) = stored else {
        panic!("STORED is {stored:?}, not a reference");
    };
    let stored = *stored;

    let runtime_exception = shared
        .load_class(&path, "java/lang/RuntimeException")
        .expect("RuntimeException");
    let HeapObject::Instance(instance) = &shared.heap[stored.index()] else {
        panic!("STORED is not an instance");
    };
    assert_eq!(
        instance.class, runtime_exception,
        "STORED's runtime class is java.lang.RuntimeException"
    );
    assert_eq!(shared.class_of(stored), Some(runtime_exception));

    // This is the property that was missing: constructing the throwable called the native, and
    // the native recorded a trace while the constructor's frames still existed.
    let trace = shared
        .traces
        .get(&stored)
        .expect("fillInStackTrace captured no trace");
    assert!(
        !trace.is_empty(),
        "the trace has at least the constructor's frames"
    );
    assert_eq!(
        (trace[0].class.as_str(), trace[0].method.as_str()),
        ("java/lang/Throwable", "fillInStackTrace"),
        "the innermost frame is the method that called the native"
    );
    assert_eq!(
        (
            trace.last().unwrap().class.as_str(),
            trace.last().unwrap().method.as_str()
        ),
        ("MakeException", "main"),
        "the outermost frame is main, so the trace spans the constructor"
    );
}

#[test]
fn a_caught_exception_resumes_at_the_handler() {
    let dir = fixture_dir("catch");
    build_catch_exception(&dir);
    let path = host_path(&dir);
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "CatchException", &[])
        .unwrap_or_else(|error| panic!("CatchException failed: {error}"));

    let id = *shared
        .class_ids
        .get("CatchException")
        .expect("CatchException loaded");
    assert_eq!(
        shared.classes[id.index()].statics.get("CAUGHT"),
        Some(&Value::Int(1)),
        "the handler ran after the throwable was caught"
    );
}

#[test]
fn an_uncaught_exception_exits_one_and_names_the_class() {
    let dir = fixture_dir("throw");
    build_throw_exception(&dir);
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rava"))
        .arg("run")
        .arg("ThrowException")
        .arg("--home")
        .arg(home)
        .arg("--cp")
        .arg(&dir)
        .output()
        .expect("run rava");
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code(),
        Some(1),
        "an uncaught throwable must exit 1, got {:?}: {stderr}",
        output.status.code()
    );
    assert!(
        stderr.contains("Exception in thread \"main\" java.lang.IllegalStateException"),
        "the report must name the exception class, got: {stderr}"
    );
    assert!(
        stderr.contains("\tat ThrowException.main("),
        "the report must include the frame the trace captured, got: {stderr}"
    );
    assert!(
        output.stdout.is_empty(),
        "the program prints nothing, got {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

/// `ThrowMessage.main`: `var e = new IllegalStateException("boom"); MSG = e.getMessage(); TOSTR = e.toString();`
///
/// `getMessage` reads the `detailMessage` field; `toString` concatenates the class name, `": "`
/// and the message through the host `String` methods (S10/S15), so this also pins the two slices
/// together.
fn build_throw_message(dir: &Path) {
    let mut builder = ClassBuilder::new("ThrowMessage", "java/lang/Object", 65);
    builder.field(0x0009, "MSG", "Ljava/lang/String;");
    builder.field(0x0009, "TOSTR", "Ljava/lang/String;");
    let exception = builder.class("java/lang/IllegalStateException");
    let init = builder.method_ref(
        "java/lang/IllegalStateException",
        "<init>",
        "(Ljava/lang/String;)V",
    );
    let get_message =
        builder.method_ref("java/lang/Throwable", "getMessage", "()Ljava/lang/String;");
    let to_string = builder.method_ref("java/lang/Throwable", "toString", "()Ljava/lang/String;");
    let msg = builder.field_ref("ThrowMessage", "MSG", "Ljava/lang/String;");
    let tostr = builder.field_ref("ThrowMessage", "TOSTR", "Ljava/lang/String;");
    let boom = builder.string("boom");

    let mut code = Code::new();
    code.call(0xbb, exception); // new
    code.op(0x59); // dup
    code.call(0x13, boom); // ldc
    code.call(0xb7, init); // invokespecial
    code.op(0x4c); // astore_1
    code.op(0x2b); // aload_1
    code.call(0xb6, get_message); // invokevirtual
    code.call(0xb3, msg); // putstatic
    code.op(0x2b); // aload_1
    code.call(0xb6, to_string); // invokevirtual
    code.call(0xb3, tostr); // putstatic
    code.op(0xb1); // return

    builder.method(
        0x0009,
        "main",
        "([Ljava/lang/String;)V",
        Some((3, 2, code.done())),
    );
    write(dir, "ThrowMessage", builder.build());
}

#[test]
fn the_message_and_to_string_come_from_the_host_throwable() {
    let dir = fixture_dir("message");
    build_throw_message(&dir);
    let path = host_path(&dir);
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "ThrowMessage", &[])
        .unwrap_or_else(|error| panic!("ThrowMessage failed: {error}"));
    let id = shared.class_ids["ThrowMessage"];
    let statics = &shared.classes[id.index()].statics;
    let text = |name: &str| match statics.get(name) {
        Some(Value::Ref(reference)) => shared.string_text(*reference).expect("a String"),
        other => panic!("{name} is {other:?}"),
    };
    assert_eq!(text("MSG"), "boom", "getMessage");
    assert_eq!(
        text("TOSTR"),
        "java.lang.IllegalStateException: boom",
        "toString"
    );
}
