//! M2.23 acceptance: start Spring's `AnnotationConfigApplicationContext` and read a bean.
//!
//! The committed `demo.SpringSmoke` fixture (`javac --release 17`, `proxyBeanMethods =
//! false`) starts a real Spring context from the pinned `compat/jars` set and prints the
//! `String` bean named `hello`. The test runs the built `rava-cli` binary exactly as a
//! user would, so it covers class loading, the context's `refresh()` path, bean
//! definition registration and reflective `@Bean` invocation.
//!
//! The pinned jars are fetched, not committed, so this test skips with a clear message
//! when `compat/jars` is absent (a CI checkout without the fetch step). It never skips
//! when the jars are present: a missing `hello` is a failure, not a skip.
//!
//! The classpath names the Spring modules the context needs and excludes the optional
//! logging adapters (`slf4j`, Log4j, Logback). `spring-jcl` then uses the runtime's
//! `java.util.logging` natives; Boot's logging stack is not part of an M2 Spring-context
//! smoke test. Diagnostics go to stderr and never disturb the asserted stdout.

use std::path::{Path, PathBuf};
use std::process::Command;

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

/// The Spring modules the context needs, in the pinned versions.
const SPRING_JARS: &[&str] = &[
    "jakarta.annotation-api-2.1.1.jar",
    "spring-aop-6.2.19.jar",
    "spring-beans-6.2.19.jar",
    "spring-context-6.2.19.jar",
    "spring-core-6.2.19.jar",
    "spring-expression-6.2.19.jar",
    "spring-jcl-6.2.19.jar",
];

fn classpath() -> Option<String> {
    let jars = repo_root().join("compat/jars");
    let mut entries = Vec::with_capacity(SPRING_JARS.len() + 1);
    for name in SPRING_JARS {
        let path = jars.join(name);
        if !path.is_file() {
            return None;
        }
        entries.push(path.to_string_lossy().into_owned());
    }
    entries.push(
        manifest_dir()
            .join("tests/fixtures/spring/classes")
            .to_string_lossy()
            .into_owned(),
    );
    Some(entries.join(":"))
}

#[test]
fn spring_annotation_context_starts_and_exposes_the_bean() {
    if std::env::var_os("RAVA_SPRING_ACCEPTANCE").is_none() {
        eprintln!("skipping the M2.23 Spring acceptance: set RAVA_SPRING_ACCEPTANCE=1 to run it");
        return;
    }
    let Some(classpath) = classpath() else {
        eprintln!(
            "skipping the M2.23 Spring acceptance: compat/jars is absent (run \
             `bash compat/fetch-jars.sh` to fetch the pinned jars)"
        );
        return;
    };

    let fixture = manifest_dir().join("tests/fixtures/spring/classes/demo/SpringSmoke.class");
    assert!(
        fixture.is_file(),
        "the committed Spring smoke fixture is missing: {}",
        fixture.display()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_rava-cli"))
        .arg("run")
        .arg("demo.SpringSmoke")
        .arg("--stdlib")
        .arg(stdlib())
        .arg("--cp")
        .arg(classpath)
        .output()
        .expect("the rava-cli binary runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "Spring context did not start (exit {:?})\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status.code()
    );
    assert_eq!(
        stdout, "hello\n",
        "the context started but the bean was wrong\nstderr:\n{stderr}"
    );
}
