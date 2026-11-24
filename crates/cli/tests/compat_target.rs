//! End-to-end tests for `rava compat target`.
//!
//! Each test builds a throwaway repository root, so the suite never depends on the
//! fetched `compat/jars/` directory of the real tree.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(jar_count: usize) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("rava-cli-compat-{}-{id}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("compat/jars")).expect("create jar directory");
        for index in 0..jar_count {
            fs::write(
                root.join(format!("compat/jars/dependency-{index}.jar")),
                b"jar",
            )
            .expect("write fixture jar");
        }

        let mut lock = format!("count = {jar_count}\n\n");
        for index in 0..jar_count {
            lock.push_str(&format!("{:064x}  dependency-{index}.jar\n", index + 1));
        }
        fs::write(root.join("compat/jars.lock"), lock).expect("write jar lock");
        fs::write(root.join("compat/targets.toml"), targets(jar_count)).expect("write target lock");

        Self { root }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rava-cli"))
            .args(["compat", "target", "--root"])
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
"#,
        "a".repeat(64)
    )
}

fn row(label: &str, value: &str) -> String {
    format!("  {label:<18}{value}")
}

#[test]
fn prints_the_pinned_target_and_exits_zero() {
    let fixture = Fixture::new(3);
    let output = fixture.run();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        row("schema", "1"),
        row("java", "17"),
        row("class file", "61.0"),
        row("spring boot", "3.5.16"),
        row("spring framework", "6.2.19"),
        row("tomcat", "10.1.55"),
        row("reference app", "reference-app 0.1.0"),
        row("main class", "dev.rava.reference.ReferenceApplication"),
        row("jar directory", "compat/jars"),
        row("jar lock", "compat/jars.lock"),
        row("jar count", "3"),
        "target lock: ok".to_owned(),
    ] {
        assert!(
            stdout.contains(&expected),
            "missing `{expected}` in:\n{stdout}"
        );
    }
}

#[test]
fn finds_the_repository_root_from_the_current_directory() {
    let fixture = Fixture::new(1);
    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .args(["compat", "target"])
        .current_dir(fixture.root())
        .output()
        .expect("run rava-cli");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fails_when_the_jar_directory_is_missing() {
    let fixture = Fixture::new(2);
    fs::remove_dir_all(fixture.root().join("compat/jars")).expect("remove jar directory");

    let output = fixture.run();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing jar directory"), "{stderr}");
}

#[test]
fn fails_when_the_target_lock_is_missing() {
    let fixture = Fixture::new(1);
    fs::remove_file(fixture.root().join("compat/targets.toml")).expect("remove target lock");

    let output = fixture.run();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing target lock"), "{stderr}");
}

#[test]
fn fails_when_the_pinned_count_disagrees_with_the_jar_lock() {
    let fixture = Fixture::new(3);
    fs::write(fixture.root().join("compat/targets.toml"), targets(2)).expect("rewrite target lock");

    let output = fixture.run();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("pins 2 jars") && stderr.contains("declares 3"),
        "{stderr}"
    );
}

#[test]
fn fails_when_the_jar_lock_is_internally_inconsistent() {
    let fixture = Fixture::new(3);
    let lock_path = fixture.root().join("compat/jars.lock");
    let lock = fs::read_to_string(&lock_path).expect("read jar lock");
    fs::write(&lock_path, lock.replace("count = 3", "count = 4")).expect("rewrite jar lock");

    let output = fixture.run();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("count = 4") && stderr.contains("3 entries"),
        "{stderr}"
    );
}

#[test]
fn fails_when_a_pinned_jar_is_missing() {
    let fixture = Fixture::new(2);
    fs::remove_file(fixture.root().join("compat/jars/dependency-0.jar")).expect("remove jar");

    let output = fixture.run();
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("contains 1 jar files") && stderr.contains("declares 2"),
        "{stderr}"
    );
}

#[test]
fn rejects_an_unknown_argument_with_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .args(["compat", "target", "--bogus"])
        .output()
        .expect("run rava-cli");
    assert_eq!(output.status.code(), Some(2));
}
