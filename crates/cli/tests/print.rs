//! S21 acceptance: G2 — `System.out.println` reaches the VM's output sink.
//!
//! `run_main` installs the standard streams before `main`: the host's `FileOutputStream`,
//! `BufferedOutputStream` and `PrintStream` are constructed over `FileDescriptor.out` with the
//! UTF-8 charset, and `System.lineSeparator` is set. The test reads the sink the VM writes to.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, Shared, ThreadState};

fn jdk_home() -> std::ffi::OsString {
    std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    })
}

fn fixture_dir(name: &str) -> PathBuf {
    // One directory per test: the suite is parallel and a shared one races between
    // `remove_dir_all` and `create_dir_all` (issue #33's shape).
    let dir = std::env::temp_dir().join(format!("rava-print-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn build_print(dir: &Path) {
    let mut builder = ClassBuilder::new("Print", "java/lang/Object", 65);
    let out = builder.field_ref("java/lang/System", "out", "Ljava/io/PrintStream;");
    let println_string =
        builder.method_ref("java/io/PrintStream", "println", "(Ljava/lang/String;)V");
    let println_int = builder.method_ref("java/io/PrintStream", "println", "(I)V");
    let hi = builder.string("hi");

    let mut code = Vec::new();
    let call = |opcode: u8, index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
    };
    call(0xb2, out, &mut code); // getstatic System.out
    call(0x13, hi, &mut code); // ldc "hi"
    call(0xb6, println_string, &mut code);
    call(0xb2, out, &mut code);
    code.extend_from_slice(&[0x10, 42]); // bipush 42
    call(0xb6, println_int, &mut code);
    code.push(0xb1); // return

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((2, 1, code)));
    std::fs::write(dir.join("Print.class"), builder.build()).expect("fixture class");
}

fn run(dir: &Path) -> Shared {
    run_class(dir, "Print")
}

fn run_class(dir: &Path, class: &str) -> Shared {
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, class, &[])
        .unwrap_or_else(|error| panic!("{class} failed: {error}"));
    shared
}

#[test]
fn println_reaches_the_vm_output_sink() {
    let dir = fixture_dir("println");
    build_print(&dir);
    let shared = run(&dir);
    assert_eq!(
        String::from_utf8_lossy(&shared.out),
        "hi\n42\n",
        "println(String) and println(int), each with a newline"
    );
}

/// `printStackTrace` is the first real consumer of the installed streams: the throwable renders
/// its frames through `PrintStream`, and the output lands in the VM sink.
#[test]
fn print_stack_trace_renders_through_the_streams() {
    let mut builder = ClassBuilder::new("PrintTrace", "java/lang/Object", 65);
    let exception = builder.class("java/lang/IllegalStateException");
    let init = builder.method_ref("java/lang/IllegalStateException", "<init>", "()V");
    let print_trace = builder.method_ref("java/lang/Throwable", "printStackTrace", "()V");
    let mut code = Vec::new();
    code.extend_from_slice(&[0xbb, (exception >> 8) as u8, exception as u8, 0x59]);
    code.extend_from_slice(&[0xb7, (init >> 8) as u8, init as u8]);
    code.push(0x4c); // astore_1
    code.push(0x2b); // aload_1
    code.extend_from_slice(&[0xb6, (print_trace >> 8) as u8, print_trace as u8]);
    code.push(0xb1); // return
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((2, 2, code)));
    let dir = fixture_dir("trace");
    std::fs::write(dir.join("PrintTrace.class"), builder.build()).expect("fixture class");
    let shared = run_class(&dir, "PrintTrace");
    // `printStackTrace()` targets `System.err`, which is the fd-2 sink.
    let output = String::from_utf8_lossy(&shared.err);
    assert!(
        output.contains("java.lang.IllegalStateException"),
        "{output:?}"
    );
    assert!(output.contains("\tat PrintTrace.main"), "{output:?}");
}
