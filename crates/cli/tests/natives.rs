//! Acceptance for issue #2: the declared-versus-registered native report.
//!
//! Fixtures are built with the class-file builder and written to a temporary directory, so the
//! tests need no JDK. The real-image test is gated on `JAVA21_HOME`, like the VM's image tests:
//! a machine without the devShell image is not the machine that acceptance covers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{NativeCtx, NativeFn, Value, VmError};

fn noop(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-natives-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn write_class(dir: &Path, name: &str, bytes: Vec<u8>) {
    std::fs::write(dir.join(format!("{name}.class")), bytes).expect("fixture class");
}

fn class_path(dir: &Path) -> rava_vm::ClassPath {
    rava_cli::class_path(dir, dir).expect("class path")
}

/// A class with a `main` that does nothing: no native references at all.
fn plain(dir: &Path) {
    let mut builder = ClassBuilder::new("Plain", "java/lang/Object", 65);
    builder.method(
        0x0009, // public static
        "main",
        "([Ljava/lang/String;)V",
        Some((0, 1, vec![0xb1])), // return
    );
    write_class(dir, "Plain", builder.build());
}

/// The native a `Needs` class calls.
fn native_owner(dir: &Path) {
    let mut builder = ClassBuilder::new("NativeOwner", "java/lang/Object", 65);
    builder.method(
        0x0109, // public static native
        "ping", "()V", None,
    );
    write_class(dir, "NativeOwner", builder.build());
}

/// A class whose constant pool references `NativeOwner.ping()V`.
fn needs(dir: &Path) {
    let mut builder = ClassBuilder::new("Needs", "java/lang/Object", 65);
    let call = builder.method_ref("NativeOwner", "ping", "()V");
    let code = vec![
        0xb8, // invokestatic
        (call >> 8) as u8,
        call as u8,
        0xb1, // return
    ];
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((0, 0, code)));
    write_class(dir, "Needs", builder.build());
}

#[test]
fn a_class_without_natives_reports_nothing() {
    let dir = fixture_dir("plain");
    plain(&dir);
    let report = rava_cli::natives::collect(&class_path(&dir), "Plain", &HashMap::new(), 100)
        .expect("collect");
    assert!(report.declared.is_empty());
    assert!(report.missing.is_empty());
    assert!(report.dangling.is_empty());
    assert!(report.ok());
    // The walk also follows the superclass reference, which the fixture directory does not
    // contain: two classes visited, one unresolved, and still nothing to report.
    assert_eq!(report.visited, 2);
    assert_eq!(report.unresolved, 1);
}

#[test]
fn a_referenced_native_without_an_implementation_is_missing() {
    let dir = fixture_dir("missing");
    native_owner(&dir);
    needs(&dir);
    let report = rava_cli::natives::collect(&class_path(&dir), "Needs", &HashMap::new(), 100)
        .expect("collect");
    let keys: Vec<String> = report.declared.iter().map(|method| method.key()).collect();
    assert_eq!(keys, vec!["NativeOwner.ping()V".to_owned()]);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.dangling.len(), 0);
    assert!(!report.ok());
    // The walk reached the owner through the call's constant pool; `Object` is referenced too
    // but is not in the fixture directory.
    assert_eq!(report.visited, 3);
}

#[test]
fn a_registered_native_is_reconciled() {
    let dir = fixture_dir("registered");
    native_owner(&dir);
    needs(&dir);
    let registered: HashMap<String, NativeFn> =
        HashMap::from([("NativeOwner.ping()V".to_owned(), noop as NativeFn)]);
    let report =
        rava_cli::natives::collect(&class_path(&dir), "Needs", &registered, 100).expect("collect");
    assert_eq!(report.declared.len(), 1);
    assert!(report.missing.is_empty());
    assert!(report.dangling.is_empty());
    assert!(report.ok());
}

#[test]
fn a_registration_nothing_declares_is_dangling() {
    let dir = fixture_dir("dangling");
    plain(&dir);
    let registered: HashMap<String, NativeFn> =
        HashMap::from([("Ghost.missing()V".to_owned(), noop as NativeFn)]);
    let report =
        rava_cli::natives::collect(&class_path(&dir), "Plain", &registered, 100).expect("collect");
    assert_eq!(report.dangling, vec!["Ghost.missing()V".to_owned()]);
    assert!(!report.ok());
}

#[test]
fn the_cap_is_reported() {
    let dir = fixture_dir("cap");
    native_owner(&dir);
    needs(&dir);
    let report = rava_cli::natives::collect(&class_path(&dir), "Needs", &HashMap::new(), 1)
        .expect("collect");
    assert!(report.truncated, "a cap of one class must truncate");
    assert!(report.visited <= 2);
}

#[test]
fn the_cli_exits_nonzero_when_a_native_is_missing() {
    let dir = fixture_dir("cli");
    native_owner(&dir);
    needs(&dir);
    let bin = env!("CARGO_BIN_EXE_rava");

    let missing = std::process::Command::new(bin)
        .args(["natives", "Needs", "--home"])
        .arg(&dir)
        .args(["--cp"])
        .arg(&dir)
        .output()
        .expect("run rava natives");
    assert_eq!(missing.status.code(), Some(1), "a missing native must fail");
    let stdout = String::from_utf8_lossy(&missing.stdout);
    assert!(
        stdout.contains("NativeOwner") && stdout.contains("ping()V  missing"),
        "report was:\n{stdout}"
    );

    let plain = std::process::Command::new(bin)
        .args(["natives", "Plain", "--home"])
        .arg(&dir)
        .args(["--cp"])
        .arg(&dir)
        .output()
        .expect("run rava natives");
    assert_eq!(plain.status.code(), Some(0), "a clean program must pass");
}

#[test]
fn the_host_image_reports_hundreds_of_missing_natives() {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    let path =
        rava_cli::class_path(Path::new(&home), Path::new("/nonexistent")).expect("class path");
    let report = rava_cli::natives::collect(&path, "java/lang/System", &HashMap::new(), 20_000)
        .expect("collect");
    // The walk is an over-approximation of what boot touches; the VM registers nothing yet, so
    // every declared native is missing.
    assert!(
        report.declared.len() > 600,
        "only {} natives declared",
        report.declared.len()
    );
    assert_eq!(report.missing.len(), report.declared.len());
    assert!(report.visited > 4_000, "visited {}", report.visited);
    assert!(!report.truncated);
    assert!(
        report
            .declared
            .iter()
            .any(|method| method.key() == "java/lang/System.registerNatives()V")
    );
}
