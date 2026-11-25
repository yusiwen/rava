//! `java.util.concurrent.FutureTask` natives (M3.6).
//!
//! CGLIB's `org.springframework.cglib.core.internal.LoadingCache` runs each cache entry
//! through a `FutureTask`: `new FutureTask(callable)` stores the callable, `run()`
//! executes it and `get()` returns its outcome. The VM is single-threaded until M5, so
//! the task is a synchronous computation: `run` calls the callable on the calling thread
//! and records the outcome, and `get` reads it. There is no `Executor`, no cancellation
//! and no timeout state — a method this module does not register fails with the named
//! "no native implementation" error rather than guessing.
//!
//! The state lives in [`rava_vm::HeapObject::FutureTask`] because the C1 stub declares no
//! instance field for it. Running the callable re-enters the interpreter (a lambda goes
//! through [`crate::lambda::invoke_sam`], any other object through its own `call`), which
//! is why the natives live here rather than in `rava-vm`.
//!
//! A throwable raised by the callable is stored and rethrown unchanged by `get`, matching
//! the rest of this slice: the JDK's `ExecutionException` wrapper is not synthesized. A
//! `get` before `run` would block forever on the JDK's single thread, so it is a typed
//! error instead of a hang.

use rava_vm::futures::FUTURE_TASK_CLASS;
use rava_vm::{HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::streams::resolve_instance_class;
use crate::{InterpError, lambda, run_instance_method};

/// `FutureTask(Callable)`.
const CONSTRUCTOR_DESCRIPTOR: &str = "(Ljava/util/concurrent/Callable;)V";

/// `Callable.call()`.
const CALL_DESCRIPTOR: &str = "()Ljava/lang/Object;";

/// `FutureTask.get()`.
const GET_DESCRIPTOR: &str = "()Ljava/lang/Object;";

/// Registers the `FutureTask` surface CGLIB's `LoadingCache` reaches.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        FUTURE_TASK_CLASS,
        "<init>",
        CONSTRUCTOR_DESCRIPTOR,
        future_task_init,
    );
    shared.register_native(FUTURE_TASK_CLASS, "run", "()V", future_task_run);
    shared.register_native(FUTURE_TASK_CLASS, "get", GET_DESCRIPTOR, future_task_get);
}

fn heap_error(error: rava_vm::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn reference_argument(args: &[Value], position: usize, method: &str) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
    }
}

fn interp_to_vm(error: InterpError) -> VmError {
    match error {
        InterpError::Vm(error) => error,
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(other.to_string()),
    }
}

/// `FutureTask(Callable)`: keep the callable `run` will execute.
fn future_task_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let callable = reference_argument(args, 0, "FutureTask.<init>")?;
    if callable.is_null() {
        return Err(VmError::Native(
            "FutureTask.<init> received a null Callable".to_owned(),
        ));
    }
    let mut heap = shared.heap();
    heap.future_task_mut(receiver).map_err(heap_error)?.callable = callable;
    Ok(None)
}

/// `FutureTask.run()`: execute the callable once and keep its outcome.
fn future_task_run(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let callable = {
        let heap = shared.heap();
        let task = heap.future_task(receiver).map_err(heap_error)?;
        if task.outcome.is_some() {
            // `FutureTask.run` is a no-op once the task completed.
            return Ok(None);
        }
        task.callable
    };
    let outcome = match invoke_callable(shared, thread, callable) {
        Ok(value) => Ok(value),
        // A throwable the callable raised is the task's outcome, rethrown by `get`.
        Err(VmError::Thrown(throwable)) => Err(throwable),
        // Any other failure is a VM error, not a Java outcome: report it here instead of
        // caching it as if the callable had thrown.
        Err(other) => return Err(other),
    };
    let mut heap = shared.heap();
    let task = heap.future_task_mut(receiver).map_err(heap_error)?;
    if task.outcome.is_none() {
        task.outcome = Some(outcome);
    }
    Ok(None)
}

/// `FutureTask.get()`: the value the callable produced, or the throwable it raised.
fn future_task_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let outcome = {
        let heap = shared.heap();
        heap.future_task(receiver).map_err(heap_error)?.outcome
    };
    match outcome {
        None => Err(VmError::Native(
            "FutureTask.get() was called before run(); the VM's single thread would block"
                .to_owned(),
        )),
        Some(Ok(value)) => Ok(value),
        Some(Err(throwable)) => Err(VmError::Thrown(throwable)),
    }
}

/// Call `Callable.call()` on a lambda object or on any other implementing object.
fn invoke_callable(
    shared: &Shared,
    thread: &ThreadState,
    callable: ObjRef,
) -> Result<Option<Value>, VmError> {
    if callable.is_null() {
        return Err(VmError::Native(
            "FutureTask.run() has no Callable to run".to_owned(),
        ));
    }
    let lambda = {
        let heap = shared.heap();
        match heap.get(callable).map_err(heap_error)? {
            HeapObject::Lambda(lambda) => Some(lambda.clone()),
            _ => None,
        }
    };
    if let Some(lambda) = lambda {
        return lambda::invoke_sam(shared, thread, &lambda, Vec::new()).map_err(interp_to_vm);
    }
    let class = {
        let heap = shared.heap();
        heap.object_class(callable).map_err(heap_error)?
    };
    let class = resolve_instance_class(shared, class, "call", CALL_DESCRIPTOR)?;
    run_instance_method(
        shared,
        thread,
        class,
        "call",
        CALL_DESCRIPTOR,
        callable,
        Vec::new(),
    )
    .map_err(interp_to_vm)
}
