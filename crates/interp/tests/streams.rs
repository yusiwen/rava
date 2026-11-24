//! M2.24 acceptance: an eager `java.util.stream` pipeline through Java 17 bytecode.
//!
//! `demo.StreamProbe` is a committed `javac --release 17` fixture. Every test calls a
//! `static` method on it through the interpreter, so the stream calls are the ones javac
//! really emits: `invokeinterface java/util/stream/Stream.filter`, `invokedynamic`
//! lambdas and constructor references, `Collectors.toList/toSet/joining/toCollection`,
//! `Collector.of` with an accumulator, identity finisher and real finisher, and
//! `StreamSupport.stream` over both a VM `Spliterator` (`words().spliterator()`) and a
//! real Java `Spliterator` (`StreamProbe$WordSpliterator`).
//!
//! The Java spliterator case is the shape Spring's
//! `MergedAnnotationsCollection$AnnotationsSpliterator` has: `tryAdvance` is bytecode that
//! calls `Consumer.accept`, and the VM must pass it a consumer it can call back into.
//! A callback that throws is not swallowed: `caughtThrow` catches the throwable inside the
//! fixture's own exception table, and `uncaughtThrow` crosses the native ABI as
//! [`InterpError::Thrown`].
//!
//! The fixture classes are loaded with no JDK at test time; only the committed class
//! files and `stdlib/c1/classes` are read.

use std::path::PathBuf;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ClassId, LoaderId, ObjRef, Shared, ThreadState, Value, boxed, collections, exceptions,
    instance, object, string,
};

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    object::register_natives(&shared);
    instance::register_natives(&shared);
    string::register_natives(&shared);
    collections::register_natives(&shared);
    boxed::register_natives(&shared);
    exceptions::register_natives(&shared);
    rava_interp::streams::register_natives(&shared);

    let probe = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/StreamProbe",
            include_bytes!("fixtures/streams/classes/demo/StreamProbe.class"),
        )
        .expect("demo/StreamProbe defines");
    let _spliterator = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/StreamProbe$WordSpliterator",
            include_bytes!("fixtures/streams/classes/demo/StreamProbe$WordSpliterator.class"),
        )
        .expect("demo/StreamProbe$WordSpliterator defines");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        probe,
    }
}

fn call(
    fixture: &Fixture,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        args,
    )
}

fn call_ref(fixture: &Fixture, name: &str, descriptor: &str, args: Vec<Value>) -> ObjRef {
    match call(fixture, name, descriptor, args) {
        Ok(Some(Value::Ref(reference))) => reference,
        other => panic!("{name} did not return a reference: {other:?}"),
    }
}

fn text(shared: &Shared, reference: ObjRef) -> String {
    shared
        .heap()
        .string(reference)
        .expect("the reference is a String")
        .to_owned()
}

fn string_argument(fixture: &Fixture, text: &str) -> Value {
    Value::Ref(fixture.shared.intern_string(text))
}

fn texts(shared: &Shared, elements: &[Value]) -> Vec<String> {
    elements
        .iter()
        .map(|value| match value {
            Value::Ref(reference) => text(shared, *reference),
            other => panic!("expected a String element, found {other:?}"),
        })
        .collect()
}

fn list_texts(fixture: &Fixture, reference: ObjRef) -> Vec<String> {
    let elements = fixture
        .shared
        .heap()
        .list(reference)
        .expect("the reference is a list")
        .to_vec();
    texts(&fixture.shared, &elements)
}

fn set_texts(fixture: &Fixture, reference: ObjRef) -> Vec<String> {
    let elements = fixture
        .shared
        .heap()
        .set(reference)
        .expect("the reference is a set")
        .to_vec();
    texts(&fixture.shared, &elements)
}

fn call_text(fixture: &Fixture, name: &str, descriptor: &str, args: Vec<Value>) -> String {
    let reference = call_ref(fixture, name, descriptor, args);
    text(&fixture.shared, reference)
}

#[test]
fn filter_map_collects_to_list() {
    let fixture = fixture();
    let list = call_ref(
        &fixture,
        "filterMap",
        "(I)Ljava/util/List;",
        vec![Value::Int(5)],
    );
    assert_eq!(list_texts(&fixture, list), vec!["CHARLIE"]);

    let list = call_ref(
        &fixture,
        "filterMap",
        "(I)Ljava/util/List;",
        vec![Value::Int(4)],
    );
    assert_eq!(
        list_texts(&fixture, list),
        vec!["DELTA", "ALPHA", "CHARLIE", "BRAVO"]
    );
}

#[test]
fn collect_to_set_deduplicates_and_keeps_insertion_order() {
    let fixture = fixture();
    let set = call_ref(&fixture, "toSet", "()Ljava/util/Set;", Vec::new());
    assert_eq!(set_texts(&fixture, set), vec!["b", "a", "c"]);
}

#[test]
fn sorted_limit_and_joining_run_in_pipeline_order() {
    let fixture = fixture();
    assert_eq!(
        call_text(
            &fixture,
            "sortedLimited",
            "()Ljava/lang/String;",
            Vec::new()
        ),
        "alpha+bravo"
    );
}

#[test]
fn a_capturing_lambda_sees_the_call_site_argument() {
    let fixture = fixture();
    let suffix = string_argument(&fixture, "-x");
    assert_eq!(
        call_text(
            &fixture,
            "capturing",
            "(Ljava/lang/String;)Ljava/lang/String;",
            vec![suffix]
        ),
        "alpha-x"
    );
}

#[test]
fn stream_support_drains_a_vm_spliterator() {
    let fixture = fixture();
    let list = call_ref(
        &fixture,
        "fromSpliterator",
        "()Ljava/util/List;",
        Vec::new(),
    );
    assert_eq!(list_texts(&fixture, list), vec!["delta", "alpha", "bravo"]);
}

#[test]
fn stream_support_drains_a_java_spliterator_through_its_own_try_advance() {
    let fixture = fixture();
    assert_eq!(
        call_text(
            &fixture,
            "fromJavaSpliterator",
            "()Ljava/lang/String;",
            Vec::new()
        ),
        "alpha,bravo,charlie,delta"
    );
}

#[test]
fn spliterator_reports_its_size_characteristics_and_drains() {
    let fixture = fixture();
    assert_eq!(
        call_text(
            &fixture,
            "spliteratorSurface",
            "()Ljava/lang/String;",
            Vec::new()
        ),
        // ORDERED | SIZED | SUBSIZED == 16 + 64 + 16384.
        "4/4/16464/delta;alpha;charlie;bravo;"
    );
}

#[test]
fn terminal_queries_cover_count_match_and_find_first() {
    let fixture = fixture();
    let bravo = string_argument(&fixture, "bravo");
    assert_eq!(
        call_text(
            &fixture,
            "terminalQueries",
            "(Ljava/lang/String;)Ljava/lang/String;",
            vec![bravo]
        ),
        "3/true/true/true"
    );
    let zulu = string_argument(&fixture, "zulu");
    assert_eq!(
        call_text(
            &fixture,
            "terminalQueries",
            "(Ljava/lang/String;)Ljava/lang/String;",
            vec![zulu]
        ),
        "3/false/true/true"
    );

    let prefix = string_argument(&fixture, "c");
    assert_eq!(
        call_text(
            &fixture,
            "findFirst",
            "(Ljava/lang/String;)Ljava/lang/String;",
            vec![prefix]
        ),
        "charlie/none"
    );
}

#[test]
fn to_array_and_to_collection_use_the_heap_containers() {
    let fixture = fixture();
    assert_eq!(
        call_text(
            &fixture,
            "toArrayLength",
            "()Ljava/lang/String;",
            Vec::new()
        ),
        "4"
    );

    let set = call_ref(&fixture, "toCollection", "()Ljava/util/Set;", Vec::new());
    assert_eq!(set_texts(&fixture, set), vec!["d", "a", "c", "b"]);
}

#[test]
fn collector_of_runs_its_callbacks_and_finisher() {
    let fixture = fixture();
    assert_eq!(
        call_text(
            &fixture,
            "customCollector",
            "()Ljava/lang/String;",
            Vec::new()
        ),
        "alpha|bravo|"
    );
    assert_eq!(
        call_text(
            &fixture,
            "customFinisher",
            "()Ljava/lang/String;",
            Vec::new()
        ),
        "2ofalpha"
    );
}

#[test]
fn a_throwing_callback_is_caught_inside_the_fixture() {
    let fixture = fixture();
    assert_eq!(
        call_text(&fixture, "caughtThrow", "()Ljava/lang/String;", Vec::new()),
        "caught:boom"
    );
}

#[test]
fn a_throwing_callback_crosses_the_native_as_thrown() {
    let fixture = fixture();
    match call(
        &fixture,
        "uncaughtThrow",
        "()Ljava/lang/String;",
        Vec::new(),
    ) {
        Err(InterpError::Thrown(throwable)) => {
            assert_eq!(
                fixture.shared.throwable_message(throwable).as_deref(),
                Some("kaboom")
            );
        }
        other => panic!("the callback's throwable was swallowed: {other:?}"),
    }
}

/// The empty optional and a populated one behave differently through `orElse`.
#[test]
fn optional_semantics_are_observed_through_find_first() {
    let fixture = fixture();
    let prefix = string_argument(&fixture, "delta");
    assert_eq!(
        call_text(
            &fixture,
            "findFirst",
            "(Ljava/lang/String;)Ljava/lang/String;",
            vec![prefix]
        ),
        "delta/none"
    );
}
