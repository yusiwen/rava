//! S22 acceptance: G2's byte-for-byte comparison.
//!
//! A small Java program is compiled with the host `javac` and run twice: once by the reference
//! `java` launcher and once by `rava run`. The bytes each writes to stdout must be identical.
//! This is the comparison G2's definition asks for; S21 pinned the VM-side sink, this pins it
//! against the launcher the project is measured by.

use std::path::{Path, PathBuf};
use std::process::Command;

fn jdk_home() -> PathBuf {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    PathBuf::from(home)
}

fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-g2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

/// A program that exercises the print paths the VM implements: `println`/`print` overloads,
/// integer and character rendering, Latin-1 and non-Latin-1 text (the charset's two code paths),
/// an empty line and a trailing newline.
const SOURCE: &str = r#"
public class PrintSample {
    public static void main(String[] args) {
        System.out.println("hello");
        System.out.print("no newline");
        System.out.println();
        System.out.println(42);
        System.out.println(-7);
        System.out.println('x');
        System.out.println(true);
        System.out.println("h\u00e9llo");
        System.out.println("\u4f60\u597d");
        for (int i = 0; i < 3; i++) {
            System.out.println(i);
        }
        System.out.println("tab\tstop");
        System.out.println("");
    }
}
"#;

fn compile(dir: &Path) {
    let source = dir.join("PrintSample.java");
    std::fs::write(&source, SOURCE).expect("source file");
    let output = Command::new(jdk_home().join("bin/javac"))
        .arg("-d")
        .arg(dir)
        .arg(&source)
        .output()
        .expect("run javac");
    assert!(
        output.status.success(),
        "javac failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_java(dir: &Path) -> std::process::Output {
    Command::new(jdk_home().join("bin/java"))
        .arg("-cp")
        .arg(dir)
        .arg("PrintSample")
        .output()
        .expect("run java")
}

fn run_rava(dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rava"))
        .arg("run")
        .arg("PrintSample")
        .arg("--home")
        .arg(jdk_home())
        .arg("--cp")
        .arg(dir)
        .output()
        .expect("run rava")
}

#[test]
fn printing_matches_the_reference_launcher_byte_for_byte() {
    let dir = fixture_dir();
    compile(&dir);
    let reference = run_java(&dir);
    let ours = run_rava(&dir);

    assert_eq!(
        ours.status.code(),
        reference.status.code(),
        "exit code: rava {:?}, java {:?}\nstderr: {}",
        ours.status.code(),
        reference.status.code(),
        String::from_utf8_lossy(&ours.stderr)
    );
    assert_eq!(
        ours.stdout,
        reference.stdout,
        "stdout differs\nrava: {}\njava: {}",
        String::from_utf8_lossy(&ours.stdout),
        String::from_utf8_lossy(&reference.stdout)
    );
    assert!(
        !reference.stdout.is_empty(),
        "the reference output is empty, so the comparison proves nothing"
    );
    assert_eq!(
        ours.stderr,
        reference.stderr,
        "stderr differs: {}",
        String::from_utf8_lossy(&ours.stderr)
    );
}
