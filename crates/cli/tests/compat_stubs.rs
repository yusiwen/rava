//! End-to-end tests for `rava compat stubs` and `rava compat link-check`.
//!
//! The fixture is a throwaway repository root with a target lock and a manifest but
//! deliberately no `compat/jars/` directory: both commands must work from the committed
//! manifest and stub tree alone.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use rava_classfile::ClassBuilder;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const MANIFEST: &str = "\
# rava surface manifest - test fixture
# classes: 3
# fields: 1
# methods: 3
# interface-methods: 1
class java/lang/Object
class java/lang/Runnable
class java/lang/System
field java/lang/System out Ljava/io/PrintStream;
method java/lang/Object <init> ()V
method java/lang/Object toString ()Ljava/lang/String;
method java/lang/System arraycopy (Ljava/lang/Object;ILjava/lang/Object;II)V
interface-method java/lang/Runnable run ()V
";

const SYSTEM_ARRAYCOPY: &str = "(Ljava/lang/Object;ILjava/lang/Object;II)V";

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        Self::with_manifest(MANIFEST)
    }

    fn with_manifest(manifest: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rava-cli-stubs-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("compat/surface")).expect("create surface directory");
        fs::write(root.join("compat/targets.toml"), targets()).expect("write target lock");
        fs::write(root.join("compat/surface/test.txt"), manifest).expect("write manifest");
        // No compat/jars and no compat/jars.lock: both commands must not need them.
        Self { root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join("compat/surface/test.txt")
    }

    fn stub_path(&self, owner: &str) -> PathBuf {
        self.root
            .join("stdlib/classes")
            .join(format!("{owner}.class"))
    }

    fn run(&self, command: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rava-cli"))
            .args(["compat", command, "--root"])
            .arg(&self.root)
            .output()
            .expect("run rava-cli")
    }

    fn stubs(&self) -> Output {
        self.run("stubs")
    }

    fn link_check(&self) -> Output {
        self.run("link-check")
    }

    /// Every stub file, keyed by its path relative to the stub directory.
    fn snapshot(&self) -> BTreeMap<String, Vec<u8>> {
        let root = self.root.join("stdlib/classes");
        let mut files = BTreeMap::new();
        collect_files(&root, &root, &mut files);
        files
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn collect_files(root: &Path, directory: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
    let entries = fs::read_dir(directory).expect("read stub directory");
    for entry in entries {
        let path = entry.expect("stub entry").path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            let name = path
                .strip_prefix(root)
                .expect("stub path")
                .to_string_lossy()
                .into_owned();
            files.insert(name, fs::read(&path).expect("read stub"));
        }
    }
}

fn targets() -> String {
    format!(
        r#"schema = 1

[platform]
java = "17"
class_file = "61.0"
spring_boot = "3.5.16"
spring_framework = "6.2.19"
tomcat = "10.1.55"

[reference_app]
path = "reference-app"
version = "0.1.0"
main_class = "dev.rava.reference.ReferenceApplication"
source_sha256 = "{}"

[jars]
directory = "compat/jars"
lock = "compat/jars.lock"
count = 1

[manifest]
path = "compat/surface/test.txt"

[stubs]
path = "stdlib/classes"
"#,
        "a".repeat(64)
    )
}

/// Writes one synthetic stub class, replacing whatever `compat stubs` generated.
fn write_stub(path: &Path, name: &str, fields: &[(&str, &str)], methods: &[(&str, &str)]) {
    let mut builder = ClassBuilder::new(name, "java/lang/Object", 61);
    for (field, descriptor) in fields {
        builder.field(0x0001, field, descriptor); // public
    }
    for (method, descriptor) in methods {
        builder.method(0x0401, method, descriptor, None); // public abstract
    }
    fs::write(path, builder.build()).expect("write synthetic stub");
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn writes_a_stub_for_every_manifest_record_and_resolves_it() {
    let fixture = Fixture::new();
    assert!(!fixture.root().join("compat/jars").exists());

    let output = fixture.stubs();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    for expected in [
        "classes           3",
        "class files       3",
        "fields            1",
        "methods           3",
        "interface-methods 1",
        "stubs: written",
    ] {
        assert!(
            report.contains(expected),
            "missing `{expected}` in:\n{report}"
        );
    }
    assert!(fixture.stub_path("java/lang/Object").is_file());
    assert!(fixture.stub_path("java/lang/Runnable").is_file());
    assert!(fixture.stub_path("java/lang/System").is_file());

    let output = fixture.link_check();
    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    for expected in [
        "classes           3",
        "fields            1",
        "methods           3",
        "interface-methods 1",
        "unresolved        0",
        "link-check: ok",
    ] {
        assert!(
            report.contains(expected),
            "missing `{expected}` in:\n{report}"
        );
    }
}

#[test]
fn regenerating_the_stubs_is_byte_identical() {
    let fixture = Fixture::new();
    assert!(fixture.stubs().status.success());
    let first = fixture.snapshot();
    assert_eq!(first.len(), 3);
    assert!(fixture.stubs().status.success());
    assert_eq!(fixture.snapshot(), first);
}

#[test]
fn a_missing_class_is_unresolved_and_never_generated() {
    let fixture = Fixture::new();
    assert!(fixture.stubs().status.success());
    fs::remove_file(fixture.stub_path("java/lang/Runnable")).expect("remove stub");

    let output = fixture.link_check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(
        report.contains("class java/lang/Runnable (missing stub)"),
        "{report}"
    );
    assert!(
        report.contains("interface-method java/lang/Runnable run ()V (missing class)"),
        "{report}"
    );
    assert!(report.contains("unresolved        2"), "{report}");
    assert!(report.contains("link-check: failed"), "{report}");
    assert!(
        !fixture.stub_path("java/lang/Runnable").exists(),
        "link-check must not generate a missing stub"
    );
    assert!(
        stderr(&output).contains("2 unresolved"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_missing_field_is_unresolved() {
    let fixture = Fixture::new();
    assert!(fixture.stubs().status.success());
    // The stub keeps the method but loses the manifest field.
    write_stub(
        &fixture.stub_path("java/lang/System"),
        "java/lang/System",
        &[],
        &[("arraycopy", SYSTEM_ARRAYCOPY)],
    );

    let output = fixture.link_check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(
        report.contains("field java/lang/System out Ljava/io/PrintStream; (missing member)"),
        "{report}"
    );
    assert!(report.contains("unresolved        1"), "{report}");
}

#[test]
fn a_missing_method_is_unresolved() {
    let fixture = Fixture::new();
    assert!(fixture.stubs().status.success());
    // Keep <init>, drop toString.
    let mut builder = ClassBuilder::root("java/lang/Object", 61);
    builder.method(0x0001, "<init>", "()V", Some((0, 1, vec![0xb1])));
    fs::write(fixture.stub_path("java/lang/Object"), builder.build()).expect("rewrite stub");

    let output = fixture.link_check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(
        report.contains("method java/lang/Object toString ()Ljava/lang/String; (missing member)"),
        "{report}"
    );
    assert!(report.contains("unresolved        1"), "{report}");
}

#[test]
fn duplicate_members_are_reported_as_ambiguous() {
    let fixture = Fixture::new();
    assert!(fixture.stubs().status.success());

    let mut builder = ClassBuilder::new("java/lang/System", "java/lang/Object", 61);
    builder.field(0x0001, "out", "Ljava/io/PrintStream;");
    builder.method(0x0401, "arraycopy", SYSTEM_ARRAYCOPY, None);
    builder.method(0x0401, "arraycopy", SYSTEM_ARRAYCOPY, None);
    fs::write(fixture.stub_path("java/lang/System"), builder.build()).expect("rewrite stub");

    let output = fixture.link_check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(
        report.contains("method java/lang/System arraycopy"),
        "{report}"
    );
    assert!(report.contains("ambiguous: 2 declarations"), "{report}");
    assert!(report.contains("unresolved        1"), "{report}");
}

#[test]
fn a_stub_with_the_wrong_class_name_is_unresolved() {
    let fixture = Fixture::new();
    assert!(fixture.stubs().status.success());
    write_stub(
        &fixture.stub_path("java/lang/System"),
        "java/lang/Other",
        &[("out", "Ljava/io/PrintStream;")],
        &[("arraycopy", SYSTEM_ARRAYCOPY)],
    );

    let output = fixture.link_check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(
        report.contains("class java/lang/System (declares java/lang/Other)"),
        "{report}"
    );
    assert!(report.contains("unresolved        3"), "{report}");
}

#[test]
fn a_failed_link_check_prints_a_bounded_sample() {
    let mut manifest = String::from("# classes: 25\n");
    for index in 0..25 {
        manifest.push_str(&format!("class java/lang/Stub{index}\n"));
    }
    let fixture = Fixture::with_manifest(&manifest);
    assert!(fixture.stubs().status.success());
    fs::remove_dir_all(fixture.root().join("stdlib/classes")).expect("remove stub tree");

    let output = fixture.link_check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(report.contains("unresolved        25"), "{report}");
    assert!(
        report.contains("unresolved records (showing 20 of 25):"),
        "{report}"
    );
    assert!(
        !report.contains("class java/lang/Stub9 (missing stub)"),
        "the sample must stop at 20 records:\n{report}"
    );
}

#[test]
fn rejects_an_unknown_argument_with_usage() {
    for command in ["stubs", "link-check"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
            .args(["compat", command, "--bogus"])
            .output()
            .expect("run rava-cli");
        assert_eq!(output.status.code(), Some(2), "{command}");
    }
}

/// The manifest path is never written by link-check, even when it would be convenient.
#[test]
fn link_check_does_not_modify_the_manifest() {
    let fixture = Fixture::new();
    assert!(fixture.stubs().status.success());
    let before = fs::read(fixture.manifest_path()).expect("read manifest");
    assert!(fixture.link_check().status.success());
    assert_eq!(
        fs::read(fixture.manifest_path()).expect("read manifest"),
        before
    );
}
