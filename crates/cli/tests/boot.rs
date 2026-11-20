//! S8 acceptance: a program loads from the host image and runs under `rava run`.
//!
//! The program checks itself: it verifies that `getClass` on a class object is the same object
//! as the `java.lang.Class` class literal, and that `desiredAssertionStatus` reports the default.
//! A wrong answer falls into an `athrow`; the test sees the process exit code, so the program
//! has to run, not merely load.
//!
//! Comparing stdout with the reference launcher is G2 (ADR-0014): printing needs the `Unsafe`
//! and charset work that comes after M4.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;

fn jdk_home() -> std::ffi::OsString {
    std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    })
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-boot-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn write(dir: &Path, name: &str, bytes: Vec<u8>) {
    std::fs::write(dir.join(format!("{name}.class")), bytes).expect("fixture class");
}

/// `Boot.main`: two identity checks, then `return`; a failed check throws.
fn build_boot(dir: &Path) {
    let mut builder = ClassBuilder::new("Boot", "java/lang/Object", 65);
    let boot = builder.class("Boot");
    let class_class = builder.class("java/lang/Class");
    let get_class = builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let desired = builder.method_ref("java/lang/Class", "desiredAssertionStatus", "()Z");
    let exception = builder.class("java/lang/IllegalStateException");
    let exception_init = builder.method_ref("java/lang/IllegalStateException", "<init>", "()V");

    let mut code = Vec::new();
    let ldc = |index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[0x13, (index >> 8) as u8, index as u8]);
    };
    let call = |opcode: u8, index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
    };
    let jump = |opcode: u8, offset: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[opcode, (offset >> 8) as u8, offset as u8]);
    };

    // Boot.class.getClass() must be the java.lang.Class class object.
    ldc(boot, &mut code);
    call(0xb6, get_class, &mut code);
    ldc(class_class, &mut code);
    jump(0xa6, 13, &mut code); // if_acmpne -> fail at 22
    // Assertions are off for every class.
    ldc(boot, &mut code);
    call(0xb6, desired, &mut code);
    jump(0x9a, 4, &mut code); // ifne -> fail at 22
    code.push(0xb1); // return
    // fail:
    code.extend_from_slice(&[0xbb, (exception >> 8) as u8, exception as u8]);
    code.push(0x59); // dup
    call(0xb7, exception_init, &mut code);
    code.push(0xbf); // athrow

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((2, 1, code)));
    write(dir, "Boot", builder.build());
}

/// A class whose branch lands in the middle of an instruction: it must fail to load.
fn build_broken(dir: &Path) {
    let mut builder = ClassBuilder::new("Broken", "java/lang/Object", 65);
    let code = vec![0xa7, 0x00, 0x01, 0xb1]; // goto +1, then return
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((1, 1, code)));
    write(dir, "Broken", builder.build());
}

fn run(dir: &Path, class: &str) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_rava"))
        .arg("run")
        .arg(class)
        .arg("--home")
        .arg(jdk_home())
        .arg("--cp")
        .arg(dir)
        .output()
        .expect("run rava")
}

#[test]
fn a_program_runs_on_the_host_library() {
    let dir = fixture_dir("boot");
    build_boot(&dir);
    let output = run(&dir, "Boot");
    assert!(
        output.status.success(),
        "Boot failed: status {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "the program prints nothing, got {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.stderr.is_empty(),
        "the program reports nothing, got {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_program_that_cannot_load_fails() {
    let dir = fixture_dir("broken");
    build_broken(&dir);
    let output = run(&dir, "Broken");
    assert!(!output.status.success(), "a malformed branch was accepted");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("error:"),
        "the failure was not reported: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
