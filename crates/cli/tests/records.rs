//! Record `equals`/`toString`/`hashCode` against the JDK's `ObjectMethods` rules (#72).
//!
//! `tests/fixtures/records/` is a committed `javac --release 17` fixture
//! (`demo.RecordProbe`) whose static methods return the JDK's answers. The rules it pins:
//! `this == o` first, `Float.compare`/`Double.compare` for float components (NaN equals
//! NaN, `0.0` differs from `-0.0`), `true`/`false` for a boolean component, the character
//! for a char component, and the `Wrapper.hashCode` formulas.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{ClassId, DirectorySource, LoaderId, Shared, ThreadState, Value};

const PROBE: &str = "demo/RecordProbe";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/records/classes")
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
        .expect("demo/RecordProbe loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
    }
}

fn call(fixture: &Fixture, name: &str, descriptor: &str) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        Vec::new(),
    )
}

fn boolean(fixture: &Fixture, name: &str) -> bool {
    match call(fixture, name, "()Z").unwrap_or_else(|error| panic!("{name} fails: {error}")) {
        Some(Value::Int(value)) => value != 0,
        other => panic!("{name} should return a boolean, found {other:?}"),
    }
}

fn integer(fixture: &Fixture, name: &str) -> i32 {
    match call(fixture, name, "()I").unwrap_or_else(|error| panic!("{name} fails: {error}")) {
        Some(Value::Int(value)) => value,
        other => panic!("{name} should return an int, found {other:?}"),
    }
}

#[test]
fn record_equals_uses_identity_then_compare() {
    let fixture = fixture();
    assert!(boolean(&fixture, "selfEquals"), "this == o answers true");
    assert!(
        boolean(&fixture, "identityShortcutSkipsComponents"),
        "this == o must skip a component whose equals throws"
    );
    assert!(
        boolean(&fixture, "nanPairEquals"),
        "Float.compare(NaN, NaN) == 0"
    );
    assert!(
        !boolean(&fixture, "zeroEquals"),
        "Float.compare(0.0f, -0.0f) != 0"
    );
    assert!(boolean(&fixture, "doubleNaNEquals"));
    assert!(!boolean(&fixture, "doubleZeroEquals"));
}

#[test]
fn record_to_string_renders_boolean_and_char() {
    let fixture = fixture();
    let text = match call(&fixture, "text", "()Ljava/lang/String;").expect("text answers") {
        Some(Value::Ref(reference)) => fixture
            .shared
            .heap()
            .string(reference)
            .expect("the result is a string")
            .to_owned(),
        other => panic!("text should return a string, found {other:?}"),
    };
    assert_eq!(text, "MixedRecord[letter=x, flag=true]");
}

#[test]
fn record_hash_code_matches_the_wrapper_formulas() {
    let fixture = fixture();
    assert_eq!(integer(&fixture, "floatNaNHash"), 2_143_289_344);
    assert_eq!(integer(&fixture, "doubleNaNHash"), 2_146_959_360);
    assert_eq!(integer(&fixture, "mixedHash"), 4_951);
}
