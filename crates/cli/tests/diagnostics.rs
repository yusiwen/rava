//! Diagnostics: separated streams, streaming output and throwable traces (#79).
//!
//! The fixtures under `tests/fixtures/diagnostics/` are committed `javac --release 17`
//! classes. Each test runs the built CLI as a user would, so it observes the real process
//! streams rather than the VM's in-process buffers.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(std::path::Path::parent)
        .expect("crates/cli is inside the workspace")
        .to_path_buf()
}

fn stdlib() -> PathBuf {
    repo_root().join("stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    manifest_dir().join("tests/fixtures/diagnostics/classes")
}

fn command(main: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rava-cli"));
    command
        .arg("run")
        .arg(main)
        .arg("--stdlib")
        .arg(stdlib())
        .arg("--cp")
        .arg(fixture_classes());
    command
}

fn run(main: &str) -> Output {
    command(main).output().expect("the CLI starts")
}

fn spawn_hang() -> Child {
    command("demo.Hang")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the CLI starts")
}

#[test]
fn system_err_and_system_out_stay_separate() {
    let output = run("demo.OutErr");
    assert!(
        output.status.success(),
        "OutErr should exit 0, got {:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "OUT-LINE\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "ERR-LINE\n");
}

#[test]
fn print_stack_trace_prints_the_cause_chain_and_suppressed() {
    let output = run("demo.Trace");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("java.lang.IllegalStateException: outer"),
        "the header is missing: {stderr:?}"
    );
    assert!(
        stderr.contains("Caused by: java.lang.RuntimeException: inner"),
        "the cause chain is missing: {stderr:?}"
    );
    assert!(
        stderr.contains("Suppressed: java.lang.IllegalArgumentException: sup"),
        "the suppressed exception is missing: {stderr:?}"
    );
}

#[test]
fn suppressed_exceptions_round_trip_through_get_suppressed() {
    let output = run("demo.Suppressed");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n");
}

#[test]
fn output_written_before_a_kill_survives() {
    let mut child = spawn_hang();
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let read_stdout = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let read_stderr = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    // The fixture prints immediately and then spins; a short wait is enough for the CLI to
    // reach it. The kill is what makes the test meaningful: a buffered runner loses both
    // lines at that point.
    std::thread::sleep(Duration::from_millis(1_500));
    let _ = child.kill();
    let _ = child.wait();

    let stdout = read_stdout.join().expect("the stdout reader finishes");
    let stderr = read_stderr.join().expect("the stderr reader finishes");
    assert!(
        stdout.contains("OUT-BEFORE-KILL"),
        "stdout lost the pre-kill line: {stdout:?}"
    );
    assert!(
        stderr.contains("ERR-BEFORE-KILL"),
        "stderr lost the pre-kill line: {stderr:?}"
    );
}
