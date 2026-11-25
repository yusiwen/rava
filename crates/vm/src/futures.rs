//! `java.util.concurrent.FutureTask` state (M3.6).
//!
//! CGLIB's `org.springframework.cglib.core.internal.LoadingCache` wraps each generated
//! class in a `FutureTask`: it runs the task on the calling thread and immediately asks
//! it for the cached value. The VM is single-threaded until M5, so the task is a plain
//! synchronous computation whose state is [`crate::HeapObject::FutureTask`]: the callable
//! and, once `run` has executed it, the outcome.
//!
//! The C1 stub declares no instance field for a `FutureTask` (ct.sym carries no private
//! members), so the state lives in this dedicated heap shape instead of in a field the
//! class file does not declare. The natives that run and read the task need to re-enter
//! the interpreter, so they live in `rava-interp` next to `reflect_invoke`; this module
//! only owns the shape and its class name.

use crate::heap::ObjRef;
use crate::value::Value;

/// `java.util.concurrent.FutureTask`'s binary-stable internal name.
pub const FUTURE_TASK_CLASS: &str = "java/util/concurrent/FutureTask";

/// The outcome of running a task's callable: the call's value, or the throwable it raised.
///
/// A call whose Java method returns `void` (impossible for `Callable.call`, but the shape
/// is shared with the general call machinery) stores `None` inside the `Ok`.
pub type FutureOutcome = Result<Option<Value>, ObjRef>;

/// One `java.util.concurrent.FutureTask`.
///
/// `callable` is the `Callable` the constructor received and `outcome` is `None` until
/// `run` completes. A second `run` keeps the first outcome, as the JDK does; the JDK's
/// cancelled and timed-out states are outside this slice.
#[derive(Debug, Clone)]
pub struct FutureTaskObject {
    pub callable: ObjRef,
    pub outcome: Option<FutureOutcome>,
}

impl FutureTaskObject {
    /// A task that has not run yet.
    pub fn new(callable: ObjRef) -> Self {
        Self {
            callable,
            outcome: None,
        }
    }
}
