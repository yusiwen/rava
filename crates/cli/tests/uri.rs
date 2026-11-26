//! `java.net.URI` raw/decoded components, opaque forms and syntax errors (#71).
//!
//! `tests/fixtures/uri/` is a committed `javac --release 17` fixture whose methods return
//! the JDK's answers (captured from a JDK 25 run).

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{ClassId, DirectorySource, LoaderId, Shared, ThreadState, Value};

const PROBE: &str = "demo/UriProbe";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/uri/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    rava_vm::library::register_all(&shared).expect("the VM class library registers");
    rava_interp::register_all(&shared).expect("the interpreter natives register");
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let probe = shared
        .load_class(loader, PROBE)
        .expect("demo/UriProbe loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
    }
}

fn call(fixture: &Fixture, name: &str) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        "()Ljava/lang/String;",
        Vec::new(),
    )
}

fn text(fixture: &Fixture, name: &str) -> String {
    match call(fixture, name).unwrap_or_else(|error| panic!("{name} fails: {error}")) {
        Some(Value::Ref(reference)) => fixture
            .shared
            .heap()
            .string(reference)
            .expect("the result is a string")
            .to_owned(),
        other => panic!("{name} should return a string, found {other:?}"),
    }
}

#[test]
fn raw_and_decoded_components_differ() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "rawPath"), "/a%20b");
    assert_eq!(text(&fixture, "path"), "/a b");
    assert_eq!(text(&fixture, "rawQuery"), "q=1%202");
    assert_eq!(text(&fixture, "query"), "q=1 2");
    assert_eq!(text(&fixture, "rawFragment"), "f%203");
    assert_eq!(text(&fixture, "fragment"), "f 3");
    assert_eq!(
        text(&fixture, "schemeSpecificPart"),
        "//example.com/a b?q=1 2"
    );
    assert_eq!(
        text(&fixture, "rawSchemeSpecificPart"),
        "//example.com/a%20b?q=1%202"
    );
}

#[test]
fn opaque_scheme_specific_part_is_decoded() {
    let fixture = fixture();
    assert_eq!(
        text(&fixture, "opaqueSchemeSpecificPart"),
        "a b@example.com"
    );
}

#[test]
fn relative_resolution_follows_rfc_3986() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "resolve"), "http://example.com/a/d");
}

#[test]
fn malformed_uris_are_catchable_uri_syntax_exceptions() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "malformedAuthority"), "URISyntaxException");
    assert_eq!(text(&fixture, "malformedEscape"), "URISyntaxException");
}
