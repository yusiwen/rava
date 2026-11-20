//! S24 acceptance: `System.in` reads the standard input.
//!
//! The bootstrap installs `new BufferedInputStream(new FileInputStream(FileDescriptor.in))` and
//! the read natives serve the VM's stdin buffer. The in-process tests set that buffer directly;
//! the differential test pipes bytes through the CLI and compares with `java`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, Shared, ThreadState, Value};

fn jdk_home() -> PathBuf {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    PathBuf::from(home)
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-stdin-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn build_read_in(dir: &Path) {
    // A = in.read(); B = in.read(); EOF = in.read()
    let mut builder = ClassBuilder::new("ReadIn", "java/lang/Object", 65);
    for name in ["A", "B", "EOF"] {
        builder.field(0x0009, name, "I");
    }
    let a = builder.field_ref("ReadIn", "A", "I");
    let b = builder.field_ref("ReadIn", "B", "I");
    let eof = builder.field_ref("ReadIn", "EOF", "I");
    let read = builder.method_ref("java/io/InputStream", "read", "()I");
    let sys_in = builder.field_ref("java/lang/System", "in", "Ljava/io/InputStream;");
    let mut code = Vec::new();
    for field in [a, b, eof] {
        code.extend_from_slice(&[0xb2, (sys_in >> 8) as u8, sys_in as u8]);
        code.extend_from_slice(&[0xb6, (read >> 8) as u8, read as u8]);
        code.extend_from_slice(&[0xb3, (field >> 8) as u8, field as u8]);
    }
    code.push(0xb1);
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((1, 1, code)));
    std::fs::write(dir.join("ReadIn.class"), builder.build()).expect("fixture class");
}

fn build_read_chunk(dir: &Path) {
    // byte[] buf = new byte[4]; N = in.read(buf); FIRST = buf[0]
    let mut builder = ClassBuilder::new("ReadChunk", "java/lang/Object", 65);
    builder.field(0x0009, "N", "I");
    builder.field(0x0009, "FIRST", "I");
    let n = builder.field_ref("ReadChunk", "N", "I");
    let first = builder.field_ref("ReadChunk", "FIRST", "I");
    let read = builder.method_ref("java/io/InputStream", "read", "([B)I");
    let sys_in = builder.field_ref("java/lang/System", "in", "Ljava/io/InputStream;");
    let mut code = Vec::new();
    code.extend_from_slice(&[0x07, 0xbc, 0x08, 0x4c]); // iconst_4, newarray byte, astore_1
    code.extend_from_slice(&[0xb2, (sys_in >> 8) as u8, sys_in as u8, 0x2b]); // getstatic, aload_1
    code.extend_from_slice(&[0xb6, (read >> 8) as u8, read as u8]);
    code.extend_from_slice(&[0xb3, (n >> 8) as u8, n as u8]);
    code.extend_from_slice(&[0x2b, 0x03, 0x33]); // aload_1, iconst_0, baload
    code.extend_from_slice(&[0xb3, (first >> 8) as u8, first as u8]);
    code.push(0xb1);
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((2, 2, code)));
    std::fs::write(dir.join("ReadChunk.class"), builder.build()).expect("fixture class");
}

fn run(dir: &Path, class: &str, stdin: &[u8]) -> Shared {
    let path: ClassPath = rava_cli::class_path(&jdk_home(), dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    shared.stdin = Some(stdin.to_vec());
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, class, &[])
        .unwrap_or_else(|error| panic!("{class} failed: {error}"));
    shared
}

fn int(shared: &Shared, class: &str, name: &str) -> i32 {
    let id = shared.class_ids[class];
    match shared.classes[id.index()].statics.get(name) {
        Some(Value::Int(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn read_returns_bytes_then_eof() {
    let dir = fixture_dir("read");
    build_read_in(&dir);
    let shared = run(&dir, "ReadIn", b"AB");
    assert_eq!(int(&shared, "ReadIn", "A"), i32::from(b'A'));
    assert_eq!(int(&shared, "ReadIn", "B"), i32::from(b'B'));
    assert_eq!(int(&shared, "ReadIn", "EOF"), -1);
}

#[test]
fn read_byte_array_returns_the_count() {
    let dir = fixture_dir("chunk");
    build_read_chunk(&dir);
    let shared = run(&dir, "ReadChunk", b"xyz");
    assert_eq!(int(&shared, "ReadChunk", "N"), 3);
    assert_eq!(int(&shared, "ReadChunk", "FIRST"), i32::from(b'x'));
}

/// End to end through the CLI: the native reads the process's stdin, `BufferedReader` decodes it
/// with the host charset classes, and the output matches the reference launcher.
#[test]
fn reading_a_line_matches_the_reference_launcher() {
    let dir = fixture_dir("line");
    let source = dir.join("ReadLine.java");
    std::fs::write(
        &source,
        r#"
import java.io.*;
public class ReadLine {
    public static void main(String[] args) throws Exception {
        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in));
        String line = reader.readLine();
        System.out.println("got: " + line);
    }
}
"#,
    )
    .expect("source file");
    let compiled = Command::new(jdk_home().join("bin/javac"))
        .arg("-d")
        .arg(&dir)
        .arg(&source)
        .output()
        .expect("run javac");
    assert!(
        compiled.status.success(),
        "javac failed: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );

    let reference = run_with_stdin(
        Command::new(jdk_home().join("bin/java"))
            .arg("-cp")
            .arg(&dir)
            .arg("ReadLine"),
        b"hello\n",
    );
    let ours = run_with_stdin(
        Command::new(env!("CARGO_BIN_EXE_rava"))
            .arg("run")
            .arg("ReadLine")
            .arg("--home")
            .arg(jdk_home())
            .arg("--cp")
            .arg(&dir),
        b"hello\n",
    );
    assert_eq!(
        ours.status.code(),
        reference.status.code(),
        "stderr: {}",
        String::from_utf8_lossy(&ours.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&ours.stdout),
        String::from_utf8_lossy(&reference.stdout)
    );
}

fn run_with_stdin(command: &mut Command, input: &[u8]) -> std::process::Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("wait")
}
