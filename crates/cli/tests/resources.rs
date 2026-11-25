//! M4.1 acceptance: classpath resources and the URL/stream surface.
//!
//! The committed `demo.ResourceProbe` fixture is a `javac --release 17` class that calls
//! the real Java APIs (`ClassLoader.getResource(s)`, `getResourceAsStream`,
//! `Class.getResource`, `URL.openStream` and `URLConnection.getInputStream`). The
//! classpath is the `classes` directory first and `resources.jar` second; both carry
//! `demo/data.txt` with a different payload, so the exact output distinguishes
//! directory-first from jar and proves the enumeration order.
//!
//! Absolute paths are never printed: the probe reports protocol, booleans, counts and
//! payloads, so the golden text is the same on every host.

use std::path::PathBuf;
use std::process::{Command, Output};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(std::path::Path::parent)
        .expect("the cli crate is inside the workspace")
        .to_path_buf()
}

fn fixture() -> PathBuf {
    manifest_dir().join("tests/fixtures/resources")
}

fn run(classpath: &[PathBuf]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rava-cli"));
    command
        .arg("run")
        .arg("demo.ResourceProbe")
        .arg("--stdlib")
        .arg(repo_root().join("stdlib/c1/classes"));
    for entry in classpath {
        command.arg("--cp").arg(entry);
    }
    command.output().expect("run rava-cli")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

#[test]
fn resource_probe_reports_directory_first() {
    let output = run(&[fixture().join("classes"), fixture().join("resources.jar")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the resource probe failed (exit {:?})\nstderr:\n{stderr}",
        output.status.code()
    );
    assert_eq!(
        stdout(&output),
        "\
getResource.protocol=file
getResource.path.endsWith=true
getResource.payload=directory-payload
getResources.count=2
getResources.0.protocol=file
getResources.0.payload=directory-payload
getResources.1.protocol=jar
getResources.1.path.endsWith=true
getResources.1.payload=jar-payload
class.relative.protocol=file
class.absolute.protocol=file
class.relative.same=true
class.absolute.payload=directory-payload
url.ctor.relative.payload=directory-payload
url.forms.equal=true
url.equals.same=true
url.hash.same=true
class.missing=null
stream.length=17
stream.eof=-1
stream.eof.read=-1
stream.available=0
connection.protocol=file
connection.lengthPositive=true
connection.lengthLongPositive=true
connection.payload=directory-payload
"
    );
}

#[test]
fn resource_order_follows_the_classpath_not_the_extension() {
    let output = run(&[fixture().join("resources.jar"), fixture().join("classes")]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the reversed resource probe failed (exit {:?})\nstderr:\n{stderr}",
        output.status.code()
    );
    let text = stdout(&output);
    for line in [
        "getResource.protocol=jar",
        "getResource.payload=jar-payload",
        "getResources.count=2",
        "getResources.0.protocol=jar",
        "getResources.0.payload=jar-payload",
        "getResources.1.protocol=file",
        "getResources.1.payload=directory-payload",
        "class.absolute.payload=jar-payload",
    ] {
        assert!(
            text.lines().any(|candidate| candidate == line),
            "missing `{line}` in:\n{text}"
        );
    }
}
