//! `String.format` conversions against the JDK (#71): scientific notation, boxed-width
//! integral output, `%c`, positional arguments and the locale decimal separator.
//!
//! `tests/fixtures/format/` is a committed `javac --release 17` fixture whose methods
//! return the JDK 25 answers.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{ClassId, DirectorySource, LoaderId, Shared, ThreadState, Value};

const PROBE: &str = "demo/FormatProbe";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/format/classes")
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
        .expect("demo/FormatProbe loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
    }
}

fn text(fixture: &Fixture, name: &str) -> String {
    let result: Result<Option<Value>, InterpError> = run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        "()Ljava/lang/String;",
        Vec::new(),
    );
    match result.unwrap_or_else(|error| panic!("{name} fails: {error}")) {
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
fn scientific_and_character_conversions_match_the_jdk() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "scientific"), "1.234500e+03");
    assert_eq!(text(&fixture, "scientificZero"), "0.000000e+00");
    assert_eq!(text(&fixture, "character"), "A");
    assert_eq!(text(&fixture, "positional"), "z-z");
    assert_eq!(text(&fixture, "twoStrings"), "a b");
}

#[test]
fn integral_conversions_use_the_boxed_width() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "hexInt"), "ffffffff");
    assert_eq!(text(&fixture, "hexLong"), "ffffffffffffffff");
    assert_eq!(text(&fixture, "hexByte"), "ff");
    assert_eq!(text(&fixture, "hexShort"), "ffff");
    assert_eq!(text(&fixture, "octalByte"), "377");
    assert_eq!(text(&fixture, "decimalInt"), "42");
}

#[test]
fn locale_and_default_decimal_separators_match_the_jdk() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "germanDecimal"), "1,50");
    assert_eq!(text(&fixture, "usDecimal"), "1.50");
    // The host locale in CI is C/en; `%f` without a Locale uses its separator.
    assert_eq!(text(&fixture, "defaultDecimal"), "1.50");
    assert_eq!(text(&fixture, "padded"), "00001.50");
}
