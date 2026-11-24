//! End-to-end tests for `rava compat check`.
//!
//! The fixture is synthetic: a stored-only zip with hand-built class files, a
//! target lock and a jar lock. The suite therefore never needs the downloaded
//! `compat/jars/` directory of the real tree, and it exercises the whole gate:
//! target, manifest, link-check and stub determinism.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use rava_classfile::ClassBuilder;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// One constant-pool reference of a synthetic class.
enum Reference {
    Class(&'static str),
    Field(&'static str, &'static str, &'static str),
    Method(&'static str, &'static str, &'static str),
    InterfaceMethod(&'static str, &'static str, &'static str),
}

/// A constant pool under construction; indices are assigned in insertion order.
struct Pool {
    next: u16,
    bytes: Vec<u8>,
}

impl Pool {
    fn new() -> Self {
        // Slot 0 is unusable, so the first real entry has index 1.
        Self {
            next: 1,
            bytes: Vec::new(),
        }
    }

    fn utf8(&mut self, text: &str) -> u16 {
        let index = self.next;
        self.bytes.push(1);
        self.bytes
            .extend_from_slice(&(text.len() as u16).to_be_bytes());
        self.bytes.extend_from_slice(text.as_bytes());
        self.next += 1;
        index
    }

    fn one(&mut self, tag: u8, a: u16) -> u16 {
        let index = self.next;
        self.bytes.push(tag);
        self.bytes.extend_from_slice(&a.to_be_bytes());
        self.next += 1;
        index
    }

    fn two(&mut self, tag: u8, a: u16, b: u16) -> u16 {
        let index = self.next;
        self.bytes.push(tag);
        self.bytes.extend_from_slice(&a.to_be_bytes());
        self.bytes.extend_from_slice(&b.to_be_bytes());
        self.next += 1;
        index
    }

    fn class(&mut self, name: &str) -> u16 {
        let name_index = self.utf8(name);
        self.one(7, name_index)
    }

    fn name_and_type(&mut self, name: &str, descriptor: &str) -> u16 {
        let name_index = self.utf8(name);
        let descriptor_index = self.utf8(descriptor);
        self.two(12, name_index, descriptor_index)
    }

    fn field_ref(&mut self, owner: &str, name: &str, descriptor: &str) -> u16 {
        let class_index = self.class(owner);
        let nat_index = self.name_and_type(name, descriptor);
        self.two(9, class_index, nat_index)
    }

    fn method_ref(&mut self, owner: &str, name: &str, descriptor: &str) -> u16 {
        let class_index = self.class(owner);
        let nat_index = self.name_and_type(name, descriptor);
        self.two(10, class_index, nat_index)
    }

    fn interface_method_ref(&mut self, owner: &str, name: &str, descriptor: &str) -> u16 {
        let class_index = self.class(owner);
        let nat_index = self.name_and_type(name, descriptor);
        self.two(11, class_index, nat_index)
    }
}

/// Builds a minimal class file: a constant pool and an empty class body.
fn synthetic_class(name: &str, references: &[Reference]) -> Vec<u8> {
    let mut pool = Pool::new();
    let this_class = pool.class(name);
    let super_class = pool.class("java/lang/Object");
    for reference in references {
        match reference {
            Reference::Class(name) => {
                pool.class(name);
            }
            Reference::Field(owner, name, descriptor) => {
                pool.field_ref(owner, name, descriptor);
            }
            Reference::Method(owner, name, descriptor) => {
                pool.method_ref(owner, name, descriptor);
            }
            Reference::InterfaceMethod(owner, name, descriptor) => {
                pool.interface_method_ref(owner, name, descriptor);
            }
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(&0xCAFE_BABEu32.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // minor
    out.extend_from_slice(&61u16.to_be_bytes()); // major, Java 17
    out.extend_from_slice(&pool.next.to_be_bytes());
    out.extend_from_slice(&pool.bytes);
    out.extend_from_slice(&0x0021u16.to_be_bytes()); // public super
    out.extend_from_slice(&this_class.to_be_bytes());
    out.extend_from_slice(&super_class.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // interfaces
    out.extend_from_slice(&0u16.to_be_bytes()); // fields
    out.extend_from_slice(&0u16.to_be_bytes()); // methods
    out.extend_from_slice(&0u16.to_be_bytes()); // attributes
    out
}

/// Builds a zip with stored (uncompressed) entries. The CRC is left zero because
/// `JarSource` validates sizes and structure, not checksums.
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

/// The one synthetic jar: four platform classes, one field, two methods and one
/// interface method, so each gate stage has something to count.
fn fixture_jar() -> Vec<u8> {
    let class = synthetic_class(
        "com/example/App",
        &[
            Reference::Class("java/lang/System"),
            Reference::Class("java/io/PrintStream"),
            Reference::Class("java/lang/Runnable"),
            Reference::Class("[Ljava/lang/String;"),
            Reference::Field("java/lang/System", "out", "Ljava/io/PrintStream;"),
            Reference::Method("java/io/PrintStream", "println", "(Ljava/lang/String;)V"),
            Reference::Method("java/lang/Object", "<init>", "()V"),
            Reference::InterfaceMethod("java/lang/Runnable", "run", "()V"),
        ],
    );
    stored_zip(&[("com/example/App.class", &class)])
}

/// A throwaway repository root with a jar, a target lock and a jar lock.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rava-cli-check-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("compat/jars")).expect("create jar directory");
        fs::write(root.join("compat/jars/app.jar"), fixture_jar()).expect("write fixture jar");
        fs::write(
            root.join("compat/jars.lock"),
            format!("count = 1\n\n{}  app.jar\n", "a".repeat(64)),
        )
        .expect("write jar lock");
        fs::write(root.join("compat/targets.toml"), targets()).expect("write target lock");

        let fixture = Self { root };
        // The manifest and the stub tree are generated deliverables of the slice;
        // build them once so the gate has a valid committed state to check.
        assert!(fixture.run("manifest").status.success());
        assert!(fixture.run("stubs").status.success());
        fixture
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

    fn check(&self) -> Output {
        self.run("check")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn passes_all_four_stages_and_prints_ok() {
    let fixture = Fixture::new();
    let manifest = fs::read(fixture.manifest_path()).expect("read manifest");
    let stub = fs::read(fixture.stub_path("java/lang/System")).expect("read stub");

    let output = fixture.check();
    assert_eq!(output.status.code(), Some(0), "stderr: {}", stderr(&output));
    let report = stdout(&output);
    for expected in [
        "rava compat check",
        "target lock + jars",
        "manifest --check",
        "link-check",
        "stub determinism",
        "java 17, 1 jar",
        "5 classes, 1 field, 2 methods, 1 interface-method",
        "5 classes, 4 member records, unresolved 0",
        "5 class files byte-identical",
        "compat check: ok",
    ] {
        assert!(
            report.contains(expected),
            "missing `{expected}` in:\n{report}"
        );
    }
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));

    // The gate checks reproducibility; it never rewrites the committed artifacts.
    assert_eq!(
        fs::read(fixture.manifest_path()).expect("read manifest"),
        manifest
    );
    assert_eq!(
        fs::read(fixture.stub_path("java/lang/System")).expect("read stub"),
        stub
    );
}

#[test]
fn fails_at_the_target_stage_when_a_jar_is_missing() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.root().join("compat/jars/app.jar")).expect("remove jar");

    let output = fixture.check();
    assert_eq!(output.status.code(), Some(1), "stdout: {}", stdout(&output));
    let report = stdout(&output);
    assert!(report.contains("target lock + jars"), "{report}");
    assert!(report.contains("failed"), "{report}");
    assert!(
        !report.contains("manifest --check"),
        "the gate must stop at the first failure:\n{report}"
    );
    let error = stderr(&output);
    assert!(error.contains("target lock + jars:"), "{error}");
    assert!(error.contains("jar"), "{error}");
}

#[test]
fn fails_at_the_manifest_stage_when_the_manifest_is_stale() {
    let fixture = Fixture::new();
    let mut manifest = fs::read_to_string(fixture.manifest_path()).expect("read manifest");
    manifest.push_str("class java/lang/Stale\n");
    fs::write(fixture.manifest_path(), &manifest).expect("write stale manifest");

    let output = fixture.check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(report.contains("manifest --check"), "{report}");
    assert!(report.contains("failed"), "{report}");
    assert!(!report.contains("link-check"), "{report}");
    let error = stderr(&output);
    assert!(error.contains("manifest --check:"), "{error}");
    assert!(error.contains("out of date"), "{error}");
    assert!(error.contains("rava compat manifest"), "{error}");
    // A failed check never rewrites the manifest.
    assert_eq!(
        fs::read_to_string(fixture.manifest_path()).expect("read manifest"),
        manifest
    );
}

#[test]
fn fails_at_link_check_when_a_stub_is_missing() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.stub_path("java/lang/Runnable")).expect("remove stub");

    let output = fixture.check();
    assert_eq!(output.status.code(), Some(1));
    let report = stdout(&output);
    assert!(report.contains("link-check"), "{report}");
    assert!(report.contains("failed"), "{report}");
    assert!(!report.contains("stub determinism"), "{report}");
    let error = stderr(&output);
    assert!(error.contains("link-check:"), "{error}");
    assert!(error.contains("unresolved"), "{error}");
    assert!(error.contains("java/lang/Runnable"), "{error}");
    // Link-check never generates a missing stub.
    assert!(!fixture.stub_path("java/lang/Runnable").exists());
}

#[test]
fn fails_stub_determinism_without_rewriting_the_committed_tree() {
    let fixture = Fixture::new();

    // Same member, different access flags and no source-file attribute: link-check
    // still resolves it, but the committed bytes are not what `stubs` generates.
    let mut builder = ClassBuilder::new("java/lang/System", "java/lang/Object", 61);
    builder.field(0x0001, "out", "Ljava/io/PrintStream;");
    let tampered = builder.build();
    fs::write(fixture.stub_path("java/lang/System"), &tampered).expect("tamper stub");

    let output = fixture.check();
    assert_eq!(
        output.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        stdout(&output),
        stderr(&output)
    );
    let report = stdout(&output);
    assert!(report.contains("stub determinism"), "{report}");
    assert!(report.contains("failed"), "{report}");
    assert!(
        !report.contains("compat check: ok"),
        "a failed gate must not print the ok line:\n{report}"
    );
    let error = stderr(&output);
    assert!(error.contains("stub determinism:"), "{error}");
    assert!(
        error.contains("differs from the regenerated stub"),
        "{error}"
    );
    assert!(error.contains("rava compat stubs"), "{error}");

    // The gate regenerates into a temporary directory; the tampered bytes stay put
    // so the report points at what is actually committed.
    assert_eq!(
        fs::read(fixture.stub_path("java/lang/System")).expect("read stub"),
        tampered
    );
}

#[test]
fn rejects_an_unknown_argument_with_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .args(["compat", "check", "--bogus"])
        .output()
        .expect("run rava-cli");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        stderr(&output).contains("unknown argument"),
        "{}",
        stderr(&output)
    );
}
