//! M2.20 acceptance: `rava run` from a directory and a jar classpath.
//!
//! The committed `demo/HelloRun` fixture is a `javac --release 17` class file that prints
//! `run ok`, under `tests/fixtures/run/`. The test suite needs no JDK: the directory and
//! jar forms are both committed, and the error-path classes are assembled in Rust.
//!
//! The class-library path is passed explicitly, except for one test that runs from the
//! repository root to check the documented default. Every test executes the built
//! `rava-cli` binary, so it covers argument parsing, the exit code and the captured
//! `System.out` flush exactly as a user sees them.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use rava_classfile::{Assembler, ClassBuilder};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;
const MAIN_DESCRIPTOR: &str = "([Ljava/lang/String;)V";

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(Path::parent)
        .expect("the cli crate is inside the workspace")
        .to_path_buf()
}

fn stdlib() -> PathBuf {
    repo_root().join("stdlib/c1/classes")
}

fn fixture(name: &str) -> PathBuf {
    manifest_dir().join("tests/fixtures/run").join(name)
}

/// A throwaway classpath directory, removed when the test ends.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rava-cli-run-{name}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_class(&self, binary_name: &str, bytes: &[u8]) {
        let path = self
            .path
            .join(format!("{}.class", binary_name.replace('.', "/")));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create class package directory");
        }
        fs::write(path, bytes).expect("write class file");
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Run `rava run <main_class> --stdlib <stdlib> --cp <first> ...`.
fn run(main_class: &str, classpath: &[&Path]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rava-cli"));
    command
        .arg("run")
        .arg(main_class)
        .arg("--stdlib")
        .arg(stdlib());
    for entry in classpath {
        command.arg("--cp").arg(entry);
    }
    command.output().expect("run rava-cli")
}

/// Run with one colon-separated `--cp` value.
fn run_colon(main_class: &str, classpath: &[&Path]) -> Output {
    let list = classpath
        .iter()
        .map(|entry| entry.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .arg("run")
        .arg(main_class)
        .arg("--stdlib")
        .arg(stdlib())
        .arg("--cp")
        .arg(list)
        .output()
        .expect("run rava-cli")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is UTF-8")
}

fn status(output: &Output) -> i32 {
    output.status.code().expect("the process exited normally")
}

/// A `demo.HelloRun` variant whose `main` prints `text` through `System.out`.
fn printing_class(text: &str) -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/HelloRun", "java/lang/Object", 61);
    let out = builder.field_ref("java/lang/System", "out", "Ljava/io/PrintStream;");
    let println = builder.method_ref("java/io/PrintStream", "println", "(Ljava/lang/String;)V");
    let literal = builder.string(text);
    let mut body = Assembler::new();
    body.getstatic(out);
    body.ldc(literal);
    body.invokevirtual(println);
    body.return_();
    builder.method(
        ACC_PUBLIC_STATIC,
        "main",
        MAIN_DESCRIPTOR,
        Some((3, 1, body.finish().expect("assemble main"))),
    );
    builder.build()
}

fn class_without_main() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/NoMain", "java/lang/Object", 61);
    let mut body = Assembler::new();
    body.return_();
    builder.method(
        ACC_PUBLIC_STATIC,
        "helper",
        "()V",
        Some((0, 0, body.finish().expect("assemble helper"))),
    );
    builder.build()
}

fn class_with_non_static_main() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/NonStatic", "java/lang/Object", 61);
    let mut body = Assembler::new();
    body.return_();
    builder.method(
        ACC_PUBLIC,
        "main",
        MAIN_DESCRIPTOR,
        // A non-static `main` has `this` and the arguments array as its two locals.
        Some((0, 2, body.finish().expect("assemble main"))),
    );
    builder.build()
}

/// A `demo.ArgsRun` whose `main` prints `args[0]`.
fn args_class() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/ArgsRun", "java/lang/Object", 61);
    let out = builder.field_ref("java/lang/System", "out", "Ljava/io/PrintStream;");
    let println = builder.method_ref("java/io/PrintStream", "println", "(Ljava/lang/String;)V");
    let mut body = Assembler::new();
    body.getstatic(out);
    body.aload(0);
    body.iconst(0);
    body.aaload();
    body.invokevirtual(println);
    body.return_();
    builder.method(
        ACC_PUBLIC_STATIC,
        "main",
        MAIN_DESCRIPTOR,
        Some((3, 1, body.finish().expect("assemble main"))),
    );
    builder.build()
}

#[test]
fn runs_a_class_from_a_directory_classpath() {
    let output = run("demo.HelloRun", &[&fixture("classes")]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "run ok\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn runs_a_class_from_a_jar_classpath() {
    let output = run("demo.HelloRun", &[&fixture("HelloRun.jar")]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "run ok\n");
}

#[test]
fn classpath_order_uses_the_first_matching_entry() {
    let shadow = Scratch::new("order");
    shadow.write_class("demo.HelloRun", &printing_class("shadowed"));

    let first = run_colon("demo.HelloRun", &[shadow.path(), &fixture("classes")]);
    assert_eq!(status(&first), 0, "stderr: {}", stderr(&first));
    assert_eq!(stdout(&first), "shadowed\n");

    let second = run_colon("demo.HelloRun", &[&fixture("classes"), shadow.path()]);
    assert_eq!(status(&second), 0, "stderr: {}", stderr(&second));
    assert_eq!(stdout(&second), "run ok\n");
}

#[test]
fn a_repeated_cp_option_appends_to_the_classpath() {
    let shadow = Scratch::new("repeated");
    shadow.write_class("demo.HelloRun", &printing_class("first cp"));
    let fixture_classes = fixture("classes");
    let entries = [shadow.path(), fixture_classes.as_path()];

    let output = run("demo.HelloRun", &entries);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "first cp\n");
}

#[test]
fn a_missing_class_is_a_runtime_error() {
    let output = run("demo.Missing", &[&fixture("classes")]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("class demo.Missing not found on the classpath"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_class_without_main_is_a_runtime_error() {
    let scratch = Scratch::new("no-main");
    scratch.write_class("demo.NoMain", &class_without_main());

    let output = run("demo.NoMain", &[scratch.path()]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("class demo.NoMain has no main([Ljava/lang/String;)V method"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_non_static_main_is_a_runtime_error() {
    let scratch = Scratch::new("non-static");
    scratch.write_class("demo.NonStatic", &class_with_non_static_main());

    let output = run("demo.NonStatic", &[scratch.path()]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("class demo.NonStatic main([Ljava/lang/String;)V is not static"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn missing_arguments_and_unknown_options_are_usage_errors() {
    let missing_main = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .arg("run")
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&missing_main), 2);
    assert!(stdout(&missing_main).contains("usage: rava run"));
    assert!(stderr(&missing_main).contains("missing <MainClass>"));

    let missing_value = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .args(["run", "demo.HelloRun", "--cp"])
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&missing_value), 2);
    assert!(stderr(&missing_value).contains("`--cp` needs a path list"));

    let unknown = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .args(["run", "demo.HelloRun", "--bogus"])
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&unknown), 2);
    assert!(stderr(&unknown).contains("unknown option `--bogus`"));
}

#[test]
fn the_default_stdlib_is_used_from_the_repository_root() {
    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .current_dir(repo_root())
        .arg("run")
        .arg("demo.HelloRun")
        .arg("--cp")
        .arg(fixture("classes"))
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "run ok\n");
}

#[test]
fn program_arguments_fill_the_string_array() {
    let scratch = Scratch::new("args");
    scratch.write_class("demo.ArgsRun", &args_class());

    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .arg("run")
        .arg("demo.ArgsRun")
        .arg("--stdlib")
        .arg(stdlib())
        .arg("--cp")
        .arg(scratch.path())
        .arg("hello")
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "hello\n");
}
