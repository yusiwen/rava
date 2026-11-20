//! The throwable natives.
//!
//! `Throwable` is the first class library type whose *behaviour* the VM has to supply rather
//! than only its storage: constructing any throwable calls `fillInStackTrace`, a `synchronized`
//! Java method that calls the private native `fillInStackTrace(int)` and then resets
//! `stackTrace` to `UNASSIGNED_STACK`. The trace the VM records is therefore taken here, while
//! the constructor's frames still exist, and kept in [`Shared::traces`] instead of in the
//! host's `StackTraceElement[]` field: the host array would need `StackTraceElement`
//! construction and array filling that belong to the string/array work (M1).
//!
//! The interpreter already records the same trace on `athrow`, and the CLI reports it with
//! [`Shared::describe_throwable`]. The `contains_key` check makes the two paths agree: the
//! first capture wins, so the trace is the place the throwable was created, not the place it
//! was last rethrown or re-filled. That is a deliberate simplification for this slice: the
//! host's own `fillInStackTrace()` re-fills the native `backtrace` on every explicit call
//! (measured: `java Fill.java` reports `first=again` after a second call), which the VM does
//! not model yet — see the S12 note's open threads.

use rava_vm::native::expect_args;
use rava_vm::{NativeCtx, Shared, Value, VmError};

/// Installs the throwable natives.
pub fn register(shared: &mut Shared) {
    shared.register_native(
        "java/lang/Throwable",
        "fillInStackTrace",
        "(I)Ljava/lang/Throwable;",
        fill_in_stack_trace,
    );
}

/// `Throwable.fillInStackTrace(int)`: capture this thread's stack once for the receiver.
///
/// The `int` is a dummy argument in the host's declaration; the VM ignores its value but still
/// checks that the call site passed an `int`, which turns a wrong registration into a type error
/// instead of a silent one.
fn fill_in_stack_trace(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, depth] = expect_args::<2>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a throwable reference",
        found: *this,
    })?;
    depth.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *depth,
    })?;

    // First capture wins: `athrow` records the trace too, and the trace wanted here is the
    // construction site. The JDK instead re-fills its native `backtrace` whenever
    // `fillInStackTrace()` is called explicitly; keeping the first capture is this slice's
    // simplification, and it keeps the interpreter's rethrow rule ("a rethrow keeps the
    // original trace") true for the same map.
    if !ctx.shared.traces.contains_key(&reference) {
        let trace = ctx.shared.capture_trace(ctx.thread);
        ctx.shared.traces.insert(reference, trace);
    }
    Ok(Value::Ref(reference))
}
