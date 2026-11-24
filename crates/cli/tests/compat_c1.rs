//! End-to-end tests for `rava compat c1-stubs` and `rava compat metadata`.
//!
//! The fixture is a throwaway repository root plus a synthetic `ct.sym`: a ZIP whose
//! entry names use the real `H/java.base/...sig` layout. The suite therefore needs no JDK
//! and no `compat/jars/` directory, and it exercises the whole M2.2 path: projection,
//! placeholder handling, the normalized diff and a deliberate metadata mutation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use rava_classfile::{ClassBuilder, ClassFile};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

const MANIFEST: &str = "\
# rava surface manifest - test fixture
# classes: 3
# fields: 2
# methods: 2
# interface-methods: 0
class demo/Widget
class demo/Gadget
class demo/Absent
field demo/Widget value I
field demo/Gadget name Ljava/lang/String;
method demo/Widget get ()I
method demo/Gadget <init> ()V
";

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

[c1]
path = "stdlib/c1/classes"
unreferenced = "compat/surface/c1-unreferenced.txt"
"#,
        "a".repeat(64)
    )
}

/// The signature class for `demo/Widget`.
fn widget_signature() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/Widget", "java/lang/Object", 69).with_access(0x0421);
    builder.field(0x0001, "value", "I");
    builder.method(0x0401, "get", "()I", None);
    builder.build()
}

/// The signature class for `demo/Gadget`.
fn gadget_signature() -> Vec<u8> {
    let mut builder = ClassBuilder::new("demo/Gadget", "java/lang/Object", 69).with_access(0x0421);
    builder.field(0x0001, "name", "Ljava/lang/String;");
    builder.method(0x0401, "<init>", "()V", None);
    builder.build()
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(referenced: &[(&str, Vec<u8>)], unreferenced: &[&str]) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("rava-cli-c1-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("compat/surface")).expect("create surface directory");
        fs::write(root.join("compat/targets.toml"), targets()).expect("write target lock");
        fs::write(root.join("compat/surface/test.txt"), MANIFEST).expect("write manifest");
        let mut listed = String::from("# owners Java 17 does not define\n\n");
        for owner in unreferenced {
            listed.push_str(owner);
            listed.push('\n');
        }
        fs::write(root.join("compat/surface/c1-unreferenced.txt"), listed)
            .expect("write unreferenced list");

        let entries: Vec<(&str, &[u8])> = referenced
            .iter()
            .map(|(owner, bytes)| (*owner, bytes.as_slice()))
            .collect();
        fs::write(root.join("fixture-ct.sym"), zip(&entries)).expect("write synthetic ct.sym");
        Self { root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn ct_sym(&self) -> PathBuf {
        self.root.join("fixture-ct.sym")
    }

    fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rava-cli"));
        command
            .args(["compat"])
            .args(args)
            .arg("--root")
            .arg(&self.root);
        if args.contains(&"c1-stubs") || args.contains(&"metadata") {
            command.arg("--ct-sym").arg(self.ct_sym());
        }
        command.output().expect("run rava-cli")
    }

    fn c1_stubs(&self) -> Output {
        self.run(&["c1-stubs"])
    }

    fn metadata_check(&self) -> Output {
        self.run(&["metadata", "--check"])
    }

    fn stub_path(&self, owner: &str) -> PathBuf {
        self.root
            .join("stdlib/c1/classes")
            .join(format!("{owner}.class"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Builds a minimal ZIP in memory with deflated entries.
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (owner, data) in entries {
        let name = format!("H/java.base/{owner}.sig");
        let offset = out.len();
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        std::io::Write::write_all(&mut encoder, data).expect("compress");
        let compressed = encoder.finish().expect("finish compression");
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let crc = crc.sum();

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&compressed);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&(offset as u32).to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let directory_offset = out.len();
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&(directory_offset as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn successful(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn fixture() -> Fixture {
    Fixture::new(
        &[
            ("demo/Widget", widget_signature()),
            ("demo/Gadget", gadget_signature()),
        ],
        &["demo/Absent"],
    )
}

#[test]
fn generates_a_version_61_tree_and_a_placeholder_for_an_owner_java_17_lacks() {
    let fixture = fixture();
    let stdout = successful(&fixture.c1_stubs());
    for expected in [
        "classes           3",
        "from ct.sym       2",
        "unreferenced      1",
        "class files       3",
        "c1 stubs: written",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in:\n{stdout}"
        );
    }

    let widget = fs::read(fixture.stub_path("demo/Widget")).expect("read projected stub");
    let class = ClassFile::parse(&widget).expect("projected stub parses");
    assert_eq!(class.major_version, 61);
    assert_eq!(class.this_class_name().unwrap(), "demo/Widget");
    assert_eq!(class.fields.len(), 1);
    assert_eq!(class.methods.len(), 1);
    assert!(
        class.code(&class.methods[0]).unwrap().is_none(),
        "a ct.sym signature carries no Code attribute"
    );

    let placeholder = fs::read(fixture.stub_path("demo/Absent")).expect("read placeholder");
    let placeholder = ClassFile::parse(&placeholder).expect("placeholder parses");
    assert_eq!(placeholder.major_version, 61);
    assert_eq!(placeholder.this_class_name().unwrap(), "demo/Absent");
    assert!(placeholder.fields.is_empty());
    assert!(placeholder.methods.is_empty());
    assert_eq!(
        placeholder.source_file().unwrap().as_deref(),
        Some("RavaC1Unreferenced.java")
    );
}

#[test]
fn the_metadata_check_is_clean_and_normalizes_only_the_version() {
    let fixture = fixture();
    successful(&fixture.c1_stubs());
    let stdout = successful(&fixture.metadata_check());
    for expected in [
        "classes           3",
        "from ct.sym       2",
        "unreferenced      1",
        "differences       0",
        "metadata: ok",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in:\n{stdout}"
        );
    }
}

#[test]
fn the_summary_does_not_need_ct_sym() {
    let fixture = fixture();
    successful(&fixture.c1_stubs());
    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .args(["compat", "metadata", "--root"])
        .arg(fixture.root())
        .output()
        .expect("run rava-cli");
    let stdout = successful(&output);
    assert!(stdout.contains("metadata: scanned"), "{stdout}");
    assert!(stdout.contains("fields            2"), "{stdout}");
    assert!(stdout.contains("methods           2"), "{stdout}");
}

#[test]
fn a_deliberate_mutation_is_reported_as_a_difference() {
    let fixture = fixture();
    successful(&fixture.c1_stubs());
    let path = fixture.stub_path("demo/Widget");
    let mut class = ClassFile::parse(&fs::read(&path).unwrap()).unwrap();
    // Flip `ACC_FINAL` on the first field: valid class file, wrong C1 metadata.
    class.fields[0].access_flags ^= 0x0010;
    fs::write(&path, class.write()).expect("write mutated stub");

    let output = fixture.metadata_check();
    assert!(!output.status.success(), "the mutation must fail the gate");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("demo/Widget"), "{stdout}");
    assert!(stdout.contains("differences       1"), "{stdout}");
    assert!(stdout.contains("metadata: failed"), "{stdout}");
}

#[test]
fn an_owner_missing_from_ct_sym_and_the_list_fails_with_its_name() {
    let fixture = Fixture::new(
        &[
            ("demo/Widget", widget_signature()),
            ("demo/Gadget", gadget_signature()),
        ],
        &[],
    );
    let output = fixture.c1_stubs();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("demo/Absent"),
        "the failing owner must be named:\n{stderr}"
    );
    assert!(stderr.contains("do not match"), "{stderr}");
}

#[test]
fn a_stale_unreferenced_entry_fails_rather_than_being_ignored() {
    let fixture = Fixture::new(
        &[
            ("demo/Widget", widget_signature()),
            ("demo/Gadget", gadget_signature()),
        ],
        &["demo/Absent", "demo/Gadget"],
    );
    let output = fixture.c1_stubs();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("demo/Gadget"), "{stderr}");
    assert!(stderr.contains("resolved by Java 17"), "{stderr}");
}
