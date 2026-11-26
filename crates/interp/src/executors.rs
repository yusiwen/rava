//! `java.util.concurrent.Executors` factories (M4.5 boundary).
//!
//! Logback's `ExecutorServiceUtil` class initializer builds a default thread factory while
//! `LoggerContext.stop()` resets the context. The VM has no threads until M5, so this slice
//! returns a `ThreadFactory` that allocates a `Thread` object and leaves its execution to
//! `Thread.start`, which is still a no-op. An executor service factory that would need a
//! real worker is a typed error naming the M5 boundary instead of guessing.

use rava_vm::{
    LambdaObject, LambdaTarget, LambdaTargetKind, LoaderId, ObjRef, Shared, ThreadState, Value,
    VmError,
};

const EXECUTORS_CLASS: &str = "java/util/concurrent/Executors";
const NEW_THREAD: &str = "(Ljava/lang/Runnable;)Ljava/lang/Thread;";

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        EXECUTORS_CLASS,
        "defaultThreadFactory",
        "()Ljava/util/concurrent/ThreadFactory;",
        executors_default_thread_factory,
    );
    // The lambda target is looked up through the owner stored in its `LambdaTarget`, which
    // is the factory interface, not `Executors`; a registration on `Executors` is
    // unreachable and makes `newThread` fail with a missing target.
    shared.register_native(
        "java/util/concurrent/ThreadFactory",
        "$defaultThreadFactory",
        NEW_THREAD,
        executors_new_thread,
    );
    for (name, descriptor) in [
        (
            "newSingleThreadExecutor",
            "()Ljava/util/concurrent/ExecutorService;",
        ),
        (
            "newCachedThreadPool",
            "()Ljava/util/concurrent/ExecutorService;",
        ),
        (
            "newFixedThreadPool",
            "(I)Ljava/util/concurrent/ExecutorService;",
        ),
        (
            "newScheduledThreadPool",
            "(I)Ljava/util/concurrent/ScheduledExecutorService;",
        ),
        (
            "newSingleThreadScheduledExecutor",
            "()Ljava/util/concurrent/ScheduledExecutorService;",
        ),
    ] {
        shared.register_native(EXECUTORS_CLASS, name, descriptor, executors_needs_threads);
    }
}

fn executors_default_thread_factory(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let factory = shared.load_library_class("java/util/concurrent/ThreadFactory")?;
    let lambda = shared.allocate_lambda(LambdaObject {
        interface: factory,
        sam_name: "newThread".to_owned(),
        sam_descriptor: NEW_THREAD.to_owned(),
        instantiated_descriptor: NEW_THREAD.to_owned(),
        target: LambdaTarget {
            class: factory,
            name: "$defaultThreadFactory".to_owned(),
            descriptor: NEW_THREAD.to_owned(),
            kind: LambdaTargetKind::Native,
        },
        captured: Vec::new(),
    });
    Ok(Some(Value::Ref(lambda)))
}

/// The default factory's `newThread`: a fresh `Thread` object.
///
/// The C1 `Thread` has no fields and every `Thread` constructor native is a no-op, so the
/// runnable passed by the caller cannot be stored; `Thread.start` is a no-op until M5.
/// The factory contract "return a `Thread`" holds for shape and identity, and a caller that
/// only inspects the result proceeds. M5 adds thread state (including the runnable) in one
/// place; this native is not the seam.
fn executors_new_thread(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class("java/lang/Thread")?;
    Ok(Some(Value::Ref(shared.allocate_instance(class)?)))
}

fn executors_needs_threads(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Err(VmError::Native(
        "Executors.new*Pool needs real worker threads; the VM is single-threaded until M5"
            .to_owned(),
    ))
}
