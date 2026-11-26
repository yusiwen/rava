//! M4.5 acceptance: a real non-web Spring Boot application starts.
//!
//! The committed fixture `tests/fixtures/boot_nonweb/` is a real
//! `@SpringBootConfiguration` (a Boot context without `@EnableAutoConfiguration`)
//! compiled with `javac --release 17` against the pinned `compat/jars` set. It starts `SpringApplication` with `WebApplicationType.NONE` and its
//! `ApplicationRunner` prints exactly `boot:started`; no servlet container and no external
//! service are involved.
//!
//! The gate runs the built `rava-cli` binary as a user would, with the fixture classes plus
//! every jar in `compat/jars`. The pinned jars are fetched, not committed, so the test
//! skips with a clear message when `compat/jars` is absent. `RAVA_BOOT_ACCEPTANCE=1`
//! turns that skip into a failure: that is the M4 gate mode.
//!
//! The child runs with a 64 MiB stack (`ulimit -s 65536`) because Boot's startup nests the
//! interpreter deeply, and it is killed after [`TIMEOUT`] so a regression that deadlocks
//! fails instead of hanging `cargo test`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// The pinned set's jar count (`compat/jars.lock` and `compat/targets.toml` agree on 35).
const PINNED_JAR_COUNT: usize = 35;

/// How long the Boot startup may take before the gate calls it a hang.
const TIMEOUT: Duration = Duration::from_secs(180);

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

fn jars_dir() -> PathBuf {
    repo_root().join("compat/jars")
}

fn fixture() -> PathBuf {
    manifest_dir().join("tests/fixtures/boot_nonweb")
}

fn acceptance_mode() -> bool {
    std::env::var("RAVA_BOOT_ACCEPTANCE").as_deref() == Ok("1")
}

/// The jar names in the committed lock, in lock order.
fn pinned_jars() -> Vec<String> {
    let lock = repo_root().join("compat/jars.lock");
    let text = fs::read_to_string(&lock)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", lock.display()));
    let mut names = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("count") {
            continue;
        }
        // `<sha256>  <name>`; a line with a single token is not a jar entry.
        if let Some((_, name)) = line.split_once(char::is_whitespace) {
            let name = name.trim();
            if name.ends_with(".jar") {
                names.push(name.to_owned());
            }
        }
    }
    names
}

/// The pinned jars that are present on disk, or `None` when `compat/jars` is absent.
fn present_jars() -> Option<Vec<String>> {
    let names = pinned_jars();
    assert_eq!(
        names.len(),
        PINNED_JAR_COUNT,
        "compat/jars.lock lists {} jar(s), expected {PINNED_JAR_COUNT}",
        names.len()
    );
    let directory = jars_dir();
    if !directory.is_dir() {
        return None;
    }
    let mut present = Vec::with_capacity(names.len());
    for name in names {
        let path = directory.join(&name);
        if !path.is_file() {
            return None;
        }
        present.push(name);
    }
    Some(present)
}

/// Skip (ordinary mode) or fail (gate mode) when the pinned jars are absent.
fn require_jars() -> Option<Vec<String>> {
    match present_jars() {
        Some(jars) => Some(jars),
        None if acceptance_mode() => panic!(
            "RAVA_BOOT_ACCEPTANCE=1 but the pinned jars are absent: run \
             `bash compat/fetch-jars.sh` before the M4.5 gate"
        ),
        None => {
            eprintln!(
                "skipping the M4.5 Boot acceptance: compat/jars is absent (run \
                 `bash compat/fetch-jars.sh`, or set RAVA_BOOT_ACCEPTANCE=1 to make this a \
                 failure)"
            );
            None
        }
    }
}

fn classpath(jars: &[String]) -> String {
    let mut entries = Vec::with_capacity(jars.len() + 1);
    entries.push(fixture().join("classes").to_string_lossy().into_owned());
    for name in jars {
        entries.push(jars_dir().join(name).to_string_lossy().into_owned());
    }
    entries.join(":")
}

#[test]
fn classpath_has_all_pinned_jars() {
    let Some(jars) = require_jars() else {
        return;
    };
    assert_eq!(
        jars.len(),
        PINNED_JAR_COUNT,
        "the Boot gate classpath must carry the whole pinned set"
    );
    let classpath = classpath(&jars);
    let entries: Vec<&str> = classpath.split(':').collect();
    assert_eq!(entries.len(), PINNED_JAR_COUNT + 1);
    for name in &jars {
        assert!(
            jars_dir().join(name).is_file(),
            "pinned jar {name} is missing from compat/jars"
        );
    }
    assert!(
        fixture().join("classes/demo/BootNonWeb.class").is_file(),
        "the committed Boot fixture is missing: {}",
        fixture().join("classes/demo/BootNonWeb.class").display()
    );
}

/// Run the CLI, killing it once [`TIMEOUT`] elapses so a deadlock fails the test.
fn run_with_timeout(mut child: Child, command: &str) -> Output {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() >= TIMEOUT => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("collect the killed child");
                panic!(
                    "{command} did not finish within {}s (deadlock?)\nstdout:\n{}\nstderr:\n{}",
                    TIMEOUT.as_secs(),
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => panic!("waiting for {command} failed: {error}"),
        }
    }
    child.wait_with_output().expect("collect the child output")
}

#[test]
fn boot_nonweb_application_starts_and_prints_the_marker() {
    let Some(jars) = require_jars() else {
        return;
    };
    if std::env::var_os("RAVA_BOOT_ACCEPTANCE").is_none() {
        eprintln!("skipping the M4.5 Boot acceptance run: set RAVA_BOOT_ACCEPTANCE=1 to run it");
        return;
    }
    let classpath = classpath(&jars);
    // The launcher's main thread gets a 64 MiB stack: Boot's startup nests the interpreter
    // deeply, and the test must not fail because the host's default soft limit is small.
    let command =
        "ulimit -s 65536 2>/dev/null; exec \"$0\" run demo.BootNonWeb --stdlib \"$1\" --cp \"$2\"";
    let child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .arg(env!("CARGO_BIN_EXE_rava-cli"))
        .arg(stdlib())
        .arg(classpath)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the rava-cli binary starts");
    let output = run_with_timeout(child, "rava-cli run demo.BootNonWeb");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the non-web Boot application did not start (exit {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    assert!(
        stdout.contains("boot:started"),
        "the application started but did not print the marker\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// The fixture class file is committed and part of the classpath even before the jars exist.
#[test]
fn boot_fixture_class_is_committed() {
    let class = fixture().join("classes/demo/BootNonWeb.class");
    assert!(
        class.is_file(),
        "the committed Boot fixture is missing: {}",
        class.display()
    );
    assert!(
        fixture().join("src/demo/BootNonWeb.java").is_file(),
        "the fixture source is committed so the class file can be rebuilt"
    );
}
