//! M4.4 acceptance: Spring Boot's nested-jar layout.
//!
//! The committed fixture under `tests/fixtures/nested_jars/` is a real Boot executable
//! jar: its manifest names Boot's `JarLauncher` in `Main-Class` and `demo.NestedMain` in
//! `Start-Class`; the application class sits in `BOOT-INF/classes/`, the helper class,
//! `inner.txt` and the `META-INF/services` provider sit in `BOOT-INF/lib/demo-helper.jar`.
//! CI has no JDK, so the source, classes and both jars are committed (see the fixture
//! README for the rebuild commands).
//!
//! Because the pinned jars carry no Boot loader classes, `rava -jar` substitutes
//! `Start-Class` for the Boot launcher; that substitution, the resources and the nested
//! `ServiceLoader` are all visible in the fixture's exact stdout. The cases build small
//! stored-only Boot jars in a scratch directory: a Boot launcher without `Start-Class`
//! (exit 1), an ordinary `Main-Class` in a Boot layout without `Start-Class` (runs), a
//! malformed nested jar, and a Boot `Class-Path` that must be ignored. A flat jar still
//! takes the M4.3 path.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use rava_classfile::{Assembler, ClassBuilder};

const ACC_PUBLIC_STATIC: u16 = 0x0009;
const MAIN_DESCRIPTOR: &str = "([Ljava/lang/String;)V";
const BOOT_LAUNCHER: &str = "org.springframework.boot.loader.launch.JarLauncher";

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
    manifest_dir().join("tests/fixtures/nested_jars")
}

fn fat_jar() -> PathBuf {
    fixture().join("fat.jar")
}

/// A throwaway directory for synthetic jars, removed when the test ends.
struct Scratch {
    path: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rava-cli-boot-{name}-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
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

const BOOT_OUTPUT: &str = "\
nested:helper
same-loader:true
outer:outer-resource
inner:inner-resource
inner-classroot:inner-resource
resources-outer:1
resources-inner:1
url-outer-protocol:jar
url-outer-path:classes
url-outer-file:classes
url-outer-string:classes
url-outer-open:outer-resource
url-copied-open:outer-resource
url-inner-path:nested
url-inner-open:inner-resource
service:inner-greeter
";

#[test]
fn boot_layout_runs_start_class_with_nested_class_resources_and_services() {
    let output = run(&fat_jar(), &[]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), BOOT_OUTPUT);
    assert_eq!(stderr(&output), "");
}

#[test]
fn both_boot_url_forms_survive_get_resources_open_stream_and_a_new_url_round_trip() {
    let output = run(&fat_jar(), &[]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    for expected in [
        // `getResources` sees one copy of each name, from the classes root and the nested jar.
        "resources-outer:1",
        "resources-inner:1",
        // The classes form `jar:file:<outer>!/BOOT-INF/classes!/<name>`.
        "url-outer-protocol:jar",
        "url-outer-path:classes",
        "url-outer-file:classes",
        "url-outer-string:classes",
        "url-outer-open:outer-resource",
        // `new URL(externalForm)` round trip.
        "url-copied-open:outer-resource",
        // The nested form `jar:file:<outer>!/BOOT-INF/lib/demo-helper.jar!/<name>`.
        "url-inner-path:nested",
        "url-inner-open:inner-resource",
    ] {
        assert!(
            lines.contains(&expected),
            "missing line {expected:?} in:\n{}",
            stdout(&output)
        );
    }
}

#[test]
fn a_boot_layout_without_start_class_is_a_runtime_error() {
    let scratch = Scratch::new("no-start");
    let class = printing_class("demo/Boot", "never printed");
    let manifest = format!("Manifest-Version: 1.0\nMain-Class: {BOOT_LAUNCHER}\n\n");
    let path = scratch.write_jar(
        "app.jar",
        &[
            ("META-INF/MANIFEST.MF", manifest.as_bytes()),
            ("BOOT-INF/classes/demo/Boot.class", &class),
        ],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    assert!(
        stderr(&output).contains("has no Start-Class"),
        "stderr: {}",
        stderr(&output)
    );
}

#[test]
fn a_boot_layout_with_a_normal_main_class_and_no_start_class_runs_that_main_class() {
    let scratch = Scratch::new("normal-main");
    let class = printing_class("demo/Boot", "boot-ok");
    let path = scratch.write_jar(
        "app.jar",
        &[
            (
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\nMain-Class: demo.Boot\n\n",
            ),
            ("BOOT-INF/classes/demo/Boot.class", &class),
        ],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "boot-ok\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn a_malformed_nested_jar_is_an_error_naming_the_entry() {
    let scratch = Scratch::new("broken-lib");
    let class = printing_class("demo/Boot", "never printed");
    let manifest =
        format!("Manifest-Version: 1.0\nMain-Class: {BOOT_LAUNCHER}\nStart-Class: demo.Boot\n\n");
    let path = scratch.write_jar(
        "app.jar",
        &[
            ("META-INF/MANIFEST.MF", manifest.as_bytes()),
            ("BOOT-INF/classes/demo/Boot.class", &class),
            ("BOOT-INF/lib/broken.jar", b"this is not a zip archive"),
        ],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 1);
    assert_eq!(stdout(&output), "");
    let message = stderr(&output);
    assert!(
        message.contains("BOOT-INF/lib/broken.jar"),
        "stderr: {message}"
    );
    assert!(message.contains("not usable"), "stderr: {message}");
}

#[test]
fn a_boot_class_path_is_documented_as_ignored_and_does_not_fail_the_launch() {
    let scratch = Scratch::new("class-path");
    let class = printing_class("demo/Boot", "boot-ok");
    // `Class-Path` points at an entry that does not exist. Boot's launcher builds its
    // classpath from BOOT-INF, so a Boot launch ignores the header instead of skipping or
    // failing on the entry.
    let manifest = format!(
        "Manifest-Version: 1.0\nMain-Class: {BOOT_LAUNCHER}\nStart-Class: demo.Boot\nClass-Path: lib/missing.jar\n\n"
    );
    let path = scratch.write_jar(
        "app.jar",
        &[
            ("META-INF/MANIFEST.MF", manifest.as_bytes()),
            ("BOOT-INF/classes/demo/Boot.class", &class),
        ],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "boot-ok\n");
    assert_eq!(stderr(&output), "");
}

#[test]
fn a_flat_jar_still_uses_the_flat_launch_path() {
    let scratch = Scratch::new("flat");
    let class = printing_class("demo/Flat", "flat-ok");
    let path = scratch.write_jar(
        "app.jar",
        &[
            (
                "META-INF/MANIFEST.MF",
                b"Manifest-Version: 1.0\nMain-Class: demo.Flat\n\n",
            ),
            ("demo/Flat.class", &class),
        ],
    );
    let output = run(&path, &[]);
    assert_eq!(status(&output), 0, "stderr: {}", stderr(&output));
    assert_eq!(stdout(&output), "flat-ok\n");
    assert_eq!(stderr(&output), "");
}
