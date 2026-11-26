//! Runtime values against the JDK 17 answers (#71): uptime, `Instant`/`Duration` text, the
//! `Random` LCG sequence and `BigInteger` constants/hash/arithmetic.
//!
//! `tests/fixtures/values/` is a committed `javac --release 17` fixture whose methods
//! return the JDK's answers (captured from a JDK 25 run).

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{ClassId, DirectorySource, LoaderId, Shared, ThreadState, Value};

const PROBE: &str = "demo/ValueProbe";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/values/classes")
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
        .expect("demo/ValueProbe loads");
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

fn string(fixture: &Fixture, name: &str) -> String {
    match call(fixture, name, "()Ljava/lang/String;")
        .unwrap_or_else(|error| panic!("{name} fails: {error}"))
    {
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
fn runtime_mx_bean_reports_a_real_start_and_uptime() {
    let fixture = fixture();
    assert!(boolean(&fixture, "uptimePositive"));
    assert!(boolean(&fixture, "startTimePositive"));
}

#[test]
fn instant_and_duration_to_string_match_the_jdk() {
    let fixture = fixture();
    assert_eq!(string(&fixture, "instantZero"), "1970-01-01T00:00:00Z");
    assert_eq!(
        string(&fixture, "instantMillis"),
        "2025-01-01T00:00:00.123Z"
    );
    assert_eq!(string(&fixture, "durationText"), "PT25H1M1S");
    assert_eq!(string(&fixture, "durationFraction"), "PT1.5S");
}

#[test]
fn named_zones_use_the_tzif_rules_at_the_instant() {
    if !std::path::Path::new("/usr/share/zoneinfo/America/New_York").exists() {
        eprintln!("skipping the named-zone test: /usr/share/zoneinfo is absent");
        return;
    }
    let fixture = fixture();
    assert_eq!(string(&fixture, "newYorkTime"), "2024-12-31 19:00");
    assert_eq!(string(&fixture, "newYorkSummerTime"), "2025-06-30 20:00");
    assert_eq!(string(&fixture, "shanghaiTime"), "2025-01-01 08:00");
    assert_eq!(string(&fixture, "utcTime"), "2025-01-01 00:00");
}

#[test]
fn random_with_a_fixed_seed_follows_the_java_lcg() {
    let fixture = fixture();
    assert_eq!(
        string(&fixture, "randomSequence"),
        "-1155484576,-723955400,29"
    );
}

#[test]
fn big_integer_constants_hash_and_arithmetic_match_the_jdk() {
    let fixture = fixture();
    assert_eq!(string(&fixture, "bigZero"), "0");
    assert!(boolean(&fixture, "bigConstants"));
    assert_eq!(integer(&fixture, "bigHash"), 31);
    assert_eq!(string(&fixture, "bigAdd"), "5");
}
