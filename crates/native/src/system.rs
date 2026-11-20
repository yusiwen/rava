//! The `System` natives: streams, the clock, and identity.
//!
//! The standard streams are the VM's buffers, installed by `setIn0`/`setOut0`/`setErr0`; the clock and the identity hash live here because `Object.hashCode` and `System.identityHashCode` must answer the same number for the same object.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rava_vm::native::expect_args;
use rava_vm::{NativeCtx, Shared, Value, VmError};

use crate::no_op;

/// Installs the `System` natives.
pub fn register(shared: &mut Shared) {
    shared.register_native("java/lang/System", "registerNatives", "()V", no_op);
    shared.register_native(
        "java/lang/System",
        "setIn0",
        "(Ljava/io/InputStream;)V",
        set_in,
    );
    shared.register_native(
        "java/lang/System",
        "setOut0",
        "(Ljava/io/PrintStream;)V",
        set_out,
    );
    shared.register_native(
        "java/lang/System",
        "setErr0",
        "(Ljava/io/PrintStream;)V",
        set_err,
    );
    shared.register_native(
        "java/lang/System",
        "identityHashCode",
        "(Ljava/lang/Object;)I",
        identity_hash,
    );
    shared.register_native(
        "java/lang/System",
        "currentTimeMillis",
        "()J",
        current_time_millis,
    );
    shared.register_native("java/lang/System", "nanoTime", "()J", nano_time);
    // `Object.hashCode` and `System.identityHashCode` must agree on one value per object.
    shared.register_native("java/lang/Object", "hashCode", "()I", identity_hash);
}

fn set_in(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [stream] = expect_args::<1>(ctx)?;
    set_system_static(ctx, "in", *stream)
}

fn set_out(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [stream] = expect_args::<1>(ctx)?;
    set_system_static(ctx, "out", *stream)
}

fn set_err(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [stream] = expect_args::<1>(ctx)?;
    set_system_static(ctx, "err", *stream)
}

/// `System.in`/`out`/`err` are plain static fields; initialization is the caller's business.
fn set_system_static(ctx: &mut NativeCtx<'_>, field: &str, value: Value) -> Result<Value, VmError> {
    let system = ctx.shared.load_class(ctx.path, "java/lang/System")?;
    ctx.shared.classes[system.index()]
        .statics
        .insert(field.to_owned(), value);
    Ok(Value::NULL)
}

fn identity_hash(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [object] = expect_args::<1>(ctx)?;
    let reference = object.as_ref().ok_or(VmError::TypeMismatch {
        expected: "an object reference",
        found: *object,
    })?;
    Ok(Value::Int(ctx.shared.identity_hash(reference)))
}

fn current_time_millis(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    Ok(Value::Long(millis))
}

fn nano_time(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    Ok(Value::Long(start.elapsed().as_nanos() as i64))
}
