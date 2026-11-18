//! Native method plumbing (ADR-0002, ADR-0005).
//!
//! An implementation is keyed by `(owner, name, descriptor)` — exactly the triple a class
//! file stores at a call site — so one function matches one signature and overloads
//! cannot collide.

use crate::Shared;
use crate::error::VmError;
use crate::value::Value;

/// What a native implementation is handed.
///
/// A native that must call back into Java will also receive the `JavaCall` seam; S3 has
/// none, and the trait stays defined so the shape is visible.
pub struct NativeCtx<'a> {
    pub shared: &'a mut Shared,
    /// The calling thread's state: needed by natives that read the frame stack, such as
    /// `System.gc()`, which uses it as part of the root set.
    pub thread: &'a mut crate::ThreadState,
    /// Needed by natives that resolve or load classes, such as `Thread.start()`.
    pub path: &'a crate::loader::ClassPath,
    pub args: &'a [Value],
}

pub type NativeFn = for<'a> fn(&mut NativeCtx<'a>) -> Result<Value, VmError>;

/// Rejects a native whose arguments do not have the expected shape.
pub fn expect_args<'a, const N: usize>(ctx: &'a NativeCtx<'_>) -> Result<&'a [Value; N], VmError> {
    ctx.args
        .try_into()
        .map_err(|_| VmError::Native(format!("expected {N} argument(s), got {}", ctx.args.len())))
}

/// The registry key for one native: `owner.name` plus the descriptor, so overloads are
/// distinct entries.
pub fn key(owner: &str, name: &str, descriptor: &str) -> String {
    format!("{owner}.{name}{descriptor}")
}
