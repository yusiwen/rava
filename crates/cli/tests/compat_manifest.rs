//! End-to-end tests for `rava compat manifest`.
//!
//! The fixture is synthetic: a stored-only zip with hand-built class files. The suite
//! therefore never needs the downloaded `compat/jars/` directory, and the exact records
//! and counts are known in advance.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A throwaway repository root with a target lock, jar lock and jar directory.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(files: &[(&str, &[u8])]) -> Self {
        let names: Vec<&str> = files.iter().map(|(name, _)| *name).collect();
        Self::with_lock(&names, files)
    }

    /// `lock_names` is what `compat/jars.lock` declares; `files` is what the directory
    /// actually holds. They differ only in the missing-jar test.
    fn with_lock(lock_names: &[&str], files: &[(&str, &[u8])]) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("rava-cli-manifest-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("compat/jars")).expect("create jar directory");
        for (name, bytes) in files {
            fs::write(root.join("compat/jars").join(name), bytes).expect("write fixture jar");
        }

        let mut lock = format!("count = {}\n\n", lock_names.len());
        for (index, name) in lock_names.iter().enumerate() {
            lock.push_str(&format!("{:064x}  {name}\n", index + 1));
        }
        fs::write(root.join("compat/jars.lock"), lock).expect("write jar lock");
        fs::write(root.join("compat/targets.toml"), targets(lock_names.len()))
            .expect("write target lock");

        Self { root }
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join("compat/surface/test.txt")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rava-cli"))
            .args(["compat", "manifest"])
            .args(args)
            .arg("--root")
            .arg(&self.root)
            .output()
            .expect("run rava-cli")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn targets(jar_count: usize) -> String {
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
count = {jar_count}

[manifest]
path = "compat/surface/test.txt"

[stubs]
path = "stdlib/classes"
"#,
        "a".repeat(64)
    )
}

/// Two classes whose platform references overlap, plus non-platform and array
/// references that must be filtered out.
fn fixture_jar() -> Vec<u8> {
    let first = synthetic_class(
        "com/example/First",
        &[
            Reference::Class("java/lang/System"),
            Reference::Class("java/io/PrintStream"),
            Reference::Class("java/lang/Runnable"),
            Reference::Class("[Ljava/lang/String;"),
            Reference::Class("com/example/Other"),
            Reference::Field("java/lang/System", "out", "Ljava/io/PrintStream;"),
            Reference::Field("com/example/Other", "value", "I"),
            Reference::Method("java/io/PrintStream", "println", "(Ljava/lang/String;)V"),
            Reference::Method("com/example/Other", "helper", "()V"),
            Reference::InterfaceMethod("java/lang/Runnable", "run", "()V"),
        ],
    );
    let second = synthetic_class(
        "com/example/Second",
        &[
            Reference::Method("java/io/PrintStream", "println", "(Ljava/lang/String;)V"),
            Reference::Method("java/lang/Object", "<init>", "()V"),
        ],
    );
    stored_zip(&[
        ("com/example/First.class", &first),
        ("com/example/Second.class", &second),
        ("META-INF/MANIFEST.MF", b"Manifest-Version: 1.0\n"),
    ])
}

fn records(text: &str) -> Vec<&str> {
    text.lines().filter(|line| !line.starts_with('#')).collect()
}

#[test]
fn writes_a_sorted_deduplicated_manifest_and_checks_it() {
    let jar = fixture_jar();
    let fixture = Fixture::new(&[("first.jar", &jar)]);

    let output = fixture.run(&[]);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "classes           4",
        "fields            1",
        "methods           2",
        "interface-methods 1",
        "manifest: written",
    ] {
        assert!(
            stdout.contains(expected),
            "missing `{expected}` in:\n{stdout}"
        );
    }

    let text = fs::read_to_string(fixture.manifest_path()).expect("read manifest");
    assert_eq!(
        records(&text),
        vec![
            "class java/io/PrintStream",
            "class java/lang/Object",
            "class java/lang/Runnable",
            "class java/lang/System",
            "field java/lang/System out Ljava/io/PrintStream;",
            "method java/io/PrintStream println (Ljava/lang/String;)V",
            "method java/lang/Object <init> ()V",
            "interface-method java/lang/Runnable run ()V",
        ]
    );
    for header in [
        "# target: java 17 (class file 61.0), spring boot 3.5.16, spring framework 6.2.19, tomcat 10.1.55",
        "# reference app: reference-app 0.1.0 (dev.rava.reference.ReferenceApplication)",
        "# jars: 1",
        "# classes: 4",
        "# fields: 1",
        "# methods: 2",
        "# interface-methods: 1",
    ] {
        assert!(text.contains(header), "missing `{header}` in:\n{text}");
    }

    // Regenerating is byte-identical, and --check accepts the committed bytes.
    let first = fs::read(fixture.manifest_path()).expect("read manifest");
    assert!(fixture.run(&[]).status.success());
    assert_eq!(
        fs::read(fixture.manifest_path()).expect("read manifest"),
        first
    );

    let check = fixture.run(&["--check"]);
    assert!(
        check.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&check.stderr)
    );
    assert!(String::from_utf8_lossy(&check.stdout).contains("manifest: ok"));
}

#[test]
fn check_fails_when_the_committed_manifest_is_stale() {
    let jar = fixture_jar();
    let fixture = Fixture::new(&[("first.jar", &jar)]);
    assert!(fixture.run(&[]).status.success());

    let path = fixture.manifest_path();
    let text = fs::read_to_string(&path).expect("read manifest");
    fs::write(
        &path,
        text.replace("class java/lang/System", "class java/lang/Systems"),
    )
    .expect("rewrite manifest");

    let output = fixture.run(&["--check"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("out of date"), "{stderr}");
    assert!(stderr.contains("line 12"), "{stderr}");
}

#[test]
fn check_fails_when_the_manifest_is_missing() {
    let jar = fixture_jar();
    let fixture = Fixture::new(&[("first.jar", &jar)]);

    let output = fixture.run(&["--check"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing manifest"), "{stderr}");
    assert!(stderr.contains("rava compat manifest"), "{stderr}");
}

#[test]
fn fails_clearly_when_a_pinned_jar_is_missing() {
    let jar = fixture_jar();
    // The lock declares second.jar, but the directory holds other.jar instead, so the
    // jar count still matches and the scan itself has to report the missing pinned jar.
    let fixture = Fixture::with_lock(
        &["first.jar", "second.jar"],
        &[("first.jar", &jar), ("other.jar", &jar)],
    );

    let output = fixture.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing pinned jar"), "{stderr}");
    assert!(stderr.contains("second.jar"), "{stderr}");
}

#[test]
fn rejects_an_unknown_argument_with_usage() {
    let jar = fixture_jar();
    let fixture = Fixture::new(&[("first.jar", &jar)]);
    let output = fixture.run(&["--bogus"]);
    assert_eq!(output.status.code(), Some(2));
}
