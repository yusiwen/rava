//! M4.3 acceptance: `rava -jar` over the manifest, `Main-Class` and `Class-Path`.
//!
//! The committed fixture under `tests/fixtures/jar_launch/` is a real executable jar: its
//! manifest names `demo.JarMain` and the `lib/helper.jar` Class-Path entry, `JarMain`
//! calls `demo.Helper` from that second jar, and it reads `data.txt` from its own jar
//! through the M4.1 resource path. CI has no JDK, so the source, classes, manifest and
//! both jars are committed (see the fixture README for the rebuild commands).
//!
//! The error cases build small stored-only jars in a scratch directory: a missing
//! manifest, a missing or empty `Main-Class`, a malformed header, and a `Class-Path` that
//! points at entries which do not open. The last case must be skipped, not fatal, because
//! that is what the JDK does with a classpath it cannot use.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use rava_classfile::{Assembler, ClassBuilder};

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

fn fixture() -> PathBuf {
    manifest_dir().join("tests/fixtures/jar_launch")
}

fn app_jar() -> PathBuf {
    fixture().join("app.jar")
}

/// A throwaway directory for synthetic jars, removed when the test ends.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rava-cli-jar-{name}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create scratch parent directory");
        }
        fs::write(&path, bytes).expect("write scratch file");
        path
    }

    fn write_jar(&self, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
        self.write_file(name, &stored_zip(entries))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Build a stored (uncompressed) zip. `JarSource` checks the structure and sizes, not the
/// checksum, so a zero CRC is fine.
fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, data) in entries {
        let offset = out.len() as u32;
        let size = data.len() as u32;
        let name_len = name.len() as u16;

        out.extend_from_slice(&0x0403_4B50u32.to_le_bytes()); // local header
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&0u16.to_le_bytes()); // date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc
        out.extend_from_slice(&size.to_le_bytes()); // compressed size
        out.extend_from_slice(&size.to_le_bytes()); // uncompressed size
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra length
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        directory.extend_from_slice(&0x0201_4B50u32.to_le_bytes()); // central entry
        directory.extend_from_slice(&20u16.to_le_bytes()); // version made by
        directory.extend_from_slice(&20u16.to_le_bytes()); // version needed
        directory.extend_from_slice(&0u16.to_le_bytes()); // flags
        directory.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        directory.extend_from_slice(&0u16.to_le_bytes()); // time
        directory.extend_from_slice(&0u16.to_le_bytes()); // date
        directory.extend_from_slice(&0u32.to_le_bytes()); // crc
        directory.extend_from_slice(&size.to_le_bytes()); // compressed size
        directory.extend_from_slice(&size.to_le_bytes()); // uncompressed size
        directory.extend_from_slice(&name_len.to_le_bytes());
        directory.extend_from_slice(&0u16.to_le_bytes()); // extra length
        directory.extend_from_slice(&0u16.to_le_bytes()); // comment length
        directory.extend_from_slice(&0u16.to_le_bytes()); // disk start
        directory.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        directory.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        directory.extend_from_slice(&offset.to_le_bytes()); // local header offset
        directory.extend_from_slice(name.as_bytes());
    }

    let directory_offset = out.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4B50u32.to_le_bytes()); // EOCD
    out.extend_from_slice(&0u16.to_le_bytes()); // disk
    out.extend_from_slice(&0u16.to_le_bytes()); // central-directory disk
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
    out
}

/// A `main([Ljava/lang/String;)V` that prints one fixed line.
fn printing_class(binary_name: &str, text: &str) -> Vec<u8> {
    let mut builder = ClassBuilder::new(binary_name, "java/lang/Object", 61);
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

/// Run `rava -jar <jar> --stdlib <stdlib> [arguments...]`.
fn run(jar: &Path, arguments: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rava-cli"));
    command.arg("-jar").arg(jar).arg("--stdlib").arg(stdlib());
    command.args(arguments);
    command.output().expect("run rava-cli")
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

const MAIN_OUTPUT: &str = "jar:hello\nhelper:helper\nresource:jar-resource\n";

#[test]
fn executable_jar_runs_main_with_the_classpath_helper_and_resource() {
    let output = run(&app_jar(), &[]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), MAIN_OUTPUT);
    assert_eq!(stderr(&output), "");
}

#[test]
fn program_arguments_are_passed_in_order() {
    let output = run(&app_jar(), &["one", "two", "three"]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!("{MAIN_OUTPUT}arg:one\narg:two\narg:three\n")
    );
}

#[test]
fn options_after_the_double_dash_are_program_arguments() {
    let output = run(&app_jar(), &["--", "--stdlib", "-x", "literal"]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!("{MAIN_OUTPUT}arg:--stdlib\narg:-x\narg:literal\n")
    );
}

#[test]
fn stdlib_option_is_honored_outside_the_repository_root() {
    // The caller's directory has no `stdlib/c1/classes`, so the launch can only succeed
    // if `--stdlib` selected the absolute directory handed to it.
    let scratch = Scratch::new("stdlib");
    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .current_dir(scratch.path())
        .arg("-jar")
        .arg(app_jar())
        .arg("--stdlib")
        .arg(stdlib())
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), MAIN_OUTPUT);
}

#[test]
fn default_stdlib_is_used_from_the_repository_root() {
    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .current_dir(repo_root())
        .arg("-jar")
        .arg(app_jar())
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), MAIN_OUTPUT);
}

#[test]
fn missing_executable_jar_is_a_runtime_error() {
    let missing = fixture().join("missing.jar");
    let output = run(&missing, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("cannot open executable jar"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_file_that_is_not_a_jar_is_a_runtime_error() {
    let scratch = Scratch::new("not-a-jar");
    let path = scratch.write_file("app.jar", b"this is not a zip archive");
    let output = run(&path, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("cannot open executable jar"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_jar_without_a_manifest_is_a_runtime_error() {
    let scratch = Scratch::new("no-manifest");
    let class = printing_class("demo/NoManifest", "never printed");
    let path = scratch.write_jar("app.jar", &[("demo/NoManifest.class", &class)]);
    let output = run(&path, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("META-INF/MANIFEST.MF"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_manifest_without_main_class_is_a_runtime_error() {
    let scratch = Scratch::new("no-main");
    let path = scratch.write_jar(
        "app.jar",
        &[("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n\n")],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("has no Main-Class"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn an_empty_main_class_is_a_runtime_error() {
    let scratch = Scratch::new("empty-main");
    let path = scratch.write_jar(
        "app.jar",
        &[(
            "META-INF/MANIFEST.MF",
            b"Manifest-Version: 1.0\nMain-Class: \n\n",
        )],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("has no Main-Class"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_malformed_manifest_names_the_line_and_exits_one() {
    let scratch = Scratch::new("malformed");
    let path = scratch.write_jar(
        "app.jar",
        &[(
            "META-INF/MANIFEST.MF",
            b"Manifest-Version: 1.0\nMain-Class demo.JarMain\n",
        )],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    let message = stderr(&output);
    assert!(
        message.contains("malformed manifest at line 2"),
        "stderr: {message}"
    );
}

#[test]
fn bad_class_path_entries_are_skipped_not_fatal() {
    let scratch = Scratch::new("class-path");
    // Both entries are unusable: one does not exist and one is not a jar. The JDK
    // ignores unusable Class-Path entries, and so does the launcher.
    let scratch_class = printing_class("demo/CpSkip", "cp-ok");
    let manifest = "Manifest-Version: 1.0\nMain-Class: demo.CpSkip\nClass-Path: lib/missing.jar lib/broken.jar\n\n";
    let path = scratch.write_jar(
        "app.jar",
        &[
            ("META-INF/MANIFEST.MF", manifest.as_bytes()),
            ("demo/CpSkip.class", &scratch_class),
        ],
    );
    scratch.write_file("lib/broken.jar", b"not a jar either");
    let output = run(&path, &[]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "cp-ok\n");
}

#[test]
fn missing_arguments_and_unknown_options_are_usage_errors() {
    let missing_jar = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .arg("-jar")
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&missing_jar), 2);
    assert!(stdout(&missing_jar).contains("usage: rava -jar"));
    assert!(stderr(&missing_jar).contains("missing <jar>"));

    let missing_value = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .arg("-jar")
        .arg(app_jar())
        .arg("--stdlib")
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&missing_value), 2);
    assert!(stderr(&missing_value).contains("`--stdlib` needs a directory"));

    let unknown = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .arg("-jar")
        .arg(app_jar())
        .arg("--bogus")
        .output()
        .expect("run rava-cli");
    assert_eq!(status(&unknown), 2);
    assert!(stderr(&unknown).contains("unknown option `--bogus`"));
}
