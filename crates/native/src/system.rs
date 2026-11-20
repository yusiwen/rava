//! The `System` natives: streams, the clock, identity, and the array copy.
//!
//! The standard streams are the VM's buffers, installed by `setIn0`/`setOut0`/`setErr0`; the clock and the identity hash live here because `Object.hashCode` and `System.identityHashCode` must answer the same number for the same object. `arraycopy` is here because it is the primitive every array-user in the library is built on, and the interpreter's array instructions already model the storage it has to move.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rava_vm::native::expect_args;
use rava_vm::{HeapObject, NativeCtx, ObjRef, Shared, Value, VmError};

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
    shared.register_native(
        "java/lang/System",
        "arraycopy",
        "(Ljava/lang/Object;ILjava/lang/Object;II)V",
        arraycopy,
    );
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

/// `System.arraycopy`: an element-wise copy between two arrays of the same kind.
///
/// The JDK's own contract orders the refusals: a null array is a `NullPointerException`, two
/// different array kinds (including a non-array argument) are an `ArrayStoreException`, and an
/// index or length outside `0..length` is an `IndexOutOfBoundsException` — reported here as
/// [`VmError::IndexOutOfBounds`], the same fault the interpreter's array instructions produce.
/// When the source and destination are the same array the copy goes through `copy_within`, so an
/// overlapping move behaves as if buffered (JVMS has no aliasing exception; `System.arraycopy` is
/// specified to work "as if" the source were first copied to a temporary array).
///
/// What this deliberately does not do: check that an element of a reference array is an instance
/// of the destination's element type. The heap keeps no element descriptor for a reference array
/// yet (S11 owns the array class model), so `Object[] -> String[]` with a non-`String` element is
/// accepted for now; `aastore` already has the same gap.
fn arraycopy(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [src, src_pos, dest, dest_pos, length] = expect_args::<5>(ctx)?;
    let src = array_argument(ctx.shared, *src)?;
    let dest = array_argument(ctx.shared, *dest)?;
    let src_pos = int_argument(*src_pos)?;
    let dest_pos = int_argument(*dest_pos)?;
    let length = int_argument(*length)?;

    // Read the shapes before the copy: the source stays immutably borrowed only long enough to
    // learn its kind and length, which is what lets the mutable borrow below be the only one.
    let (src_kind, src_len) = {
        let array = ctx.shared.array(src)?;
        (array.kind, array.elements.len())
    };
    let (dest_kind, dest_len) = {
        let array = ctx.shared.array(dest)?;
        (array.kind, array.elements.len())
    };

    if src_kind != dest_kind {
        return Err(array_store_exception());
    }
    let src_range = bounds(src_pos, length, src_len)?;
    let dest_range = bounds(dest_pos, length, dest_len)?;

    if src == dest {
        // `copy_within` is `memmove`: the two ranges may overlap in either direction.
        ctx.shared
            .array_mut(src)?
            .elements
            .copy_within(src_range, dest_range.start);
    } else {
        // Two heap slots cannot be borrowed at once (both go through `&mut Shared`), and a
        // temporary is what the specification asks for anyway.
        let copied = ctx.shared.array(src)?.elements[src_range].to_vec();
        ctx.shared.array_mut(dest)?.elements[dest_range].copy_from_slice(&copied);
    }
    Ok(Value::NULL)
}

/// A non-null array reference. `null` is the JVM's `NullPointerException`.
fn array_argument(shared: &Shared, value: Value) -> Result<ObjRef, VmError> {
    let reference = value.as_ref().ok_or(VmError::TypeMismatch {
        expected: "an array reference",
        found: value,
    })?;
    if reference.is_null() {
        return Err(VmError::JavaException {
            class: "java/lang/NullPointerException".to_owned(),
            message: None,
        });
    }
    match shared.heap.get(reference.index()) {
        Some(HeapObject::Array(_)) => Ok(reference),
        _ => Err(array_store_exception()),
    }
}

fn int_argument(value: Value) -> Result<i32, VmError> {
    value.as_int().ok_or(VmError::TypeMismatch {
        expected: "an int",
        found: value,
    })
}

fn array_store_exception() -> VmError {
    VmError::JavaException {
        class: "java/lang/ArrayStoreException".to_owned(),
        message: None,
    }
}

/// The element range `pos..pos + length`, or the bounds fault the copy raises.
///
/// A negative position reports itself; a negative length reports the length (there is no other
/// index to name); a range that runs past the end reports its exclusive end, which is the first
/// index that does not exist.
fn bounds(pos: i32, length: i32, array_len: usize) -> Result<std::ops::Range<usize>, VmError> {
    let out_of_bounds = |index: i32| VmError::IndexOutOfBounds {
        index,
        length: array_len,
    };
    let start = usize::try_from(pos).map_err(|_| out_of_bounds(pos))?;
    let count = usize::try_from(length).map_err(|_| out_of_bounds(length))?;
    let end = start
        .checked_add(count)
        .ok_or_else(|| out_of_bounds(i32::MAX))?;
    if end > array_len {
        return Err(out_of_bounds(end.min(i32::MAX as usize) as i32));
    }
    Ok(start..end)
}
