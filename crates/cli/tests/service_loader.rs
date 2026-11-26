//! M4.2 acceptance: `java.util.ServiceLoader` over `META-INF/services`.
//!
//! The committed `demo.ServiceLoaderProbe` fixture is a `javac --release 17` class. The
//! directory classpath carries `META-INF/services/demo.Greeter` for `demo.HelloGreeter`
//! (with comments, a blank line and surrounding spaces) and the committed `providers.jar`
//! carries the same resource for `demo.WorldGreeter`, so `load` has to read both
//! resources in classpath order. Provider static initializers and constructors print, so
//! the exact output also proves that neither ran before the first `Iterator.next()`.
//!
//! The CLI tests run the built binary exactly as a user would. The typed-error tests build
//! a `Shared` and install the same interp-owned natives as `rava run` — including the
//! M4.2 registrations — then call the probe through `run_static_method`, which is the
//! direct equivalent of the binary's path.
//!
//! Absolute host paths never appear in the golden text.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::Arc;

use rava_interp::InterpError;
use rava_interp::run_static_method;
use rava_vm::{ClassPathSource, LoaderId, Shared, ThreadState, Value, VmError, library};

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
    manifest_dir().join("tests/fixtures/service_loader")
}

/// The classpath order the committed jar was built for: directory first, jar second.
fn classpath() -> [PathBuf; 2] {
    [fixture().join("classes"), fixture().join("providers.jar")]
}

fn run(mode: &str, classpath: &[PathBuf]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rava-cli"));
    command
        .arg("run")
        .arg("demo.ServiceLoaderProbe")
        .arg("--stdlib")
        .arg(repo_root().join("stdlib/c1/classes"));
    for entry in classpath {
        command.arg("--cp").arg(entry);
    }
    command.arg(mode);
    command.output().expect("run rava-cli")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is UTF-8")
}

fn assert_success(output: &Output, mode: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the `{mode}` probe failed (exit {:?})\nstderr:\n{stderr}",
        output.status.code()
    );
    stdout(output)
}

#[test]
fn directory_and_jar_providers_iterate_in_classpath_order() {
    let output = run("order", &classpath());
    let text = assert_success(&output, "order");
    // The directory configuration supplies HelloGreeter and the jar supplies
    // WorldGreeter. `clinit:`/`init:` appear only after `after-hasNext`, so load and
    // hasNext constructed nothing; each pair prints inside the following `next`.
    assert_eq!(
        text,
        "\
before-iteration
after-iterator
hasNext=true
after-hasNext
clinit:demo.HelloGreeter
init:demo.HelloGreeter
next:hello
clinit:demo.WorldGreeter
init:demo.WorldGreeter
next:world
"
    );
}

#[test]
fn provider_order_follows_the_classpath() {
    let reversed = [classpath()[1].clone(), classpath()[0].clone()];
    let output = run("order", &reversed);
    let text = assert_success(&output, "order");
    assert_eq!(
        text,
        "\
before-iteration
after-iterator
hasNext=true
after-hasNext
clinit:demo.WorldGreeter
init:demo.WorldGreeter
next:world
clinit:demo.HelloGreeter
init:demo.HelloGreeter
next:hello
"
    );
}

#[test]
fn provider_identity_is_the_internal_name_class() {
    let output = run("identity", &classpath());
    let text = assert_success(&output, "identity");
    // The provider configuration spells `demo.HelloGreeter` with dots; the class must be
    // the same identity as the ordinary internal-name load, not a second class.
    assert_eq!(
        text,
        "\
clinit:demo.HelloGreeter
init:demo.HelloGreeter
identity.name=demo.HelloGreeter
identity.loadClass.same=true
identity.forName.same=true
"
    );
}

#[test]
fn explicit_class_loader_overload_uses_the_given_loader() {
    let output = run("explicit", &classpath());
    let text = assert_success(&output, "explicit");
    assert_eq!(
        text,
        "\
clinit:demo.HelloGreeter
init:demo.HelloGreeter
explicit:hello
clinit:demo.WorldGreeter
init:demo.WorldGreeter
explicit:world
"
    );
}

#[test]
fn missing_service_is_an_empty_iteration() {
    let output = run("missing", &classpath());
    let text = assert_success(&output, "missing");
    assert_eq!(text, "missing.count=0\n");
}

#[test]
fn null_class_loader_selects_the_bootstrap_loader() {
    let output = run("bootstrap", &classpath());
    let text = assert_success(&output, "bootstrap");
    assert_eq!(text, "bootstrap.count=0\n");
}

/// The provider file is what loads each provider: a missing provider class, a
/// non-assignable one and a bad service class are typed native errors. These tests call
/// the probe through the interpreter directly so they can assert the error value rather
/// than a process exit code.
fn direct_fixture() -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(repo_root().join("stdlib/c1/classes"));
    let source = ClassPathSource::from_paths(classpath())
        .expect("the committed fixture classpath is readable");
    let loader = shared
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(source))
        .expect("the application loader is added");
    library::register_all(&shared).expect("the class library registers");
    rava_interp::reflect_invoke::register_natives(&shared);
    rava_interp::method_handles::register_natives(&shared);
    rava_interp::unsafe_natives::register_natives(&shared).expect("the Unsafe natives register");
    rava_interp::proxy::register_natives(&shared);
    rava_interp::callbacks::register_natives(&shared);
    rava_interp::streams::register_natives(&shared);
    rava_interp::futures::register_natives(&shared);
    rava_interp::object_methods::register_natives(&shared);
    // M4.2: the same interp-owned registrations `rava run` installs.
    rava_interp::service_loader::register_natives(&shared);
    let thread = shared.thread_state(1);
    (shared, loader, thread)
}

/// Run `demo.ServiceLoaderProbe.run(String)` for one mode and return the typed error.
fn run_direct_for_error(mode: &str) -> InterpError {
    let (shared, loader, thread) = direct_fixture();
    let class = shared
        .load_class(loader, "demo/ServiceLoaderProbe")
        .expect("the committed probe loads");
    let mode = shared.intern_string(mode);
    run_static_method(
        &shared,
        &thread,
        class,
        "run",
        "(Ljava/lang/String;)V",
        vec![Value::Ref(mode)],
    )
    .expect_err("the probe mode fails")
}

fn native_detail(error: InterpError) -> String {
    match error {
        InterpError::Vm(VmError::Native(detail)) => detail,
        other => panic!("expected a typed VmError::Native, got {other:?}"),
    }
}

#[test]
fn non_assignable_provider_is_a_typed_error_naming_the_provider() {
    let detail = native_detail(run_direct_for_error("invalid"));
    assert!(
        detail.contains("demo.NotAGreeter"),
        "the error must name the provider: {detail}"
    );
    assert!(
        detail.contains("not assignable"),
        "the error must be the assignability error: {detail}"
    );
}

#[test]
fn primitive_service_type_is_rejected_with_a_typed_error() {
    let detail = native_detail(run_direct_for_error("primitive"));
    assert!(
        detail.contains("primitive"),
        "the error must reject the primitive service type: {detail}"
    );
}

#[test]
fn array_service_type_is_rejected_with_a_typed_error() {
    let detail = native_detail(run_direct_for_error("array"));
    assert!(
        detail.contains("array"),
        "the error must reject the array service type: {detail}"
    );
}
