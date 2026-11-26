//! The M4.5/M5 boundary, pinned by ordinary Java rather than by code review.
//!
//! `tests/fixtures/m5/` is a committed `javac --release 17` fixture (`demo.M5Probe`,
//! rebuild recipe in its README). It exercises the declared single-threaded boundary from
//! `docs/explanation/class-loading-and-libraries.md` §5e: `Thread.start` runs no work,
//! latch/queue operations fail only when they would block, executor factories need a
//! worker, and Java stack traces and `StackWalker` see no frames.
//!
//! The blocking methods catch `Throwable`. The VM reports a would-block operation as a
//! `VmError`, so the catch body never runs; each test asserts both halves: the operation
//! aborts at the Rust level and the Java handler was skipped.
//!
//! These tests pin the pre-M5 answers on purpose. M5 owns replacing them (the M4.5 slice
//! record in `docs/slices/M4.md` names the split), so a failure here when threads land is
//! the boundary moving, not a regression.
//!
//! No pinned jars are needed; the fixture imports `java.*` only.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_static_method};
use rava_vm::{ClassId, DirectorySource, LoaderId, Shared, ThreadState, Value, VmError};

const PROBE: &str = "demo/M5Probe";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m5/classes")
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
        .expect("demo/M5Probe loads");
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

fn bool_value(fixture: &Fixture, name: &str) -> bool {
    match call(fixture, name, "()Z").unwrap_or_else(|error| panic!("{name} fails: {error}")) {
        Some(Value::Int(value)) => value != 0,
        other => panic!("{name} should return a boolean, found {other:?}"),
    }
}

/// The call aborted with a VM-level native error containing `fragment`.
fn assert_vm_abort(result: Result<Option<Value>, InterpError>, fragment: &str) {
    match result {
        Err(InterpError::Vm(VmError::Native(message))) => assert!(
            message.contains(fragment),
            "the abort should name {fragment:?}, found {message:?}"
        ),
        other => panic!("expected a VmError native abort naming {fragment:?}, found {other:?}"),
    }
}

#[test]
fn thread_start_runs_no_work_and_join_returns() {
    let fixture = fixture();
    assert!(
        !bool_value(&fixture, "threadStartRan"),
        "Thread.start must not run the runnable on the single-threaded VM"
    );
    assert!(
        bool_value(&fixture, "threadIsAlive"),
        "Thread.isAlive is true while threads do not exist (documented M5 boundary)"
    );
}

#[test]
fn latch_get_count_answers_and_await_would_block() {
    let fixture = fixture();
    assert_eq!(
        call(&fixture, "latchCount", "()J").expect("getCount answers"),
        Some(Value::Long(3))
    );
    assert_vm_abort(call(&fixture, "latchAwaitBlocking", "()Z"), "would block");
    assert!(
        !bool_value(&fixture, "wasCaught"),
        "the Java catch (Throwable) must not see the VM abort"
    );
}

#[test]
fn queue_offer_is_non_blocking_while_put_and_take_would_block() {
    let fixture = fixture();
    assert!(
        bool_value(&fixture, "queueOfferSemantics"),
        "offer fits one element and refuses the second without blocking"
    );
    assert_vm_abort(call(&fixture, "queuePutFull", "()Z"), "would block");
    assert!(
        !bool_value(&fixture, "wasCaught"),
        "put's Java catch (Throwable) must not see the VM abort"
    );
    assert_vm_abort(call(&fixture, "queueTakeEmpty", "()Z"), "would block");
    assert!(
        !bool_value(&fixture, "wasCaught"),
        "take's Java catch (Throwable) must not see the VM abort"
    );
}

#[test]
fn executor_factories_need_a_worker_and_abort_at_the_vm_level() {
    let fixture = fixture();
    assert_vm_abort(
        call(&fixture, "executorPool", "()Z"),
        "single-threaded until M5",
    );
    assert!(
        !bool_value(&fixture, "wasCaught"),
        "the executor factory's Java catch (Throwable) must not see the VM abort"
    );
}

#[test]
fn java_stack_traces_and_stack_walker_see_no_frames() {
    let fixture = fixture();
    assert_eq!(
        call(&fixture, "stackTraceLength", "()I").expect("getStackTrace answers"),
        Some(Value::Int(0)),
        "Throwable.getStackTrace is empty on this VM"
    );
    assert_eq!(
        call(&fixture, "stackWalkCount", "()J").expect("StackWalker.walk answers"),
        Some(Value::Long(0)),
        "StackWalker.walk sees an empty stream"
    );
}
