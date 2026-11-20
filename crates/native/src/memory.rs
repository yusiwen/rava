//! The `Unsafe` natives this VM can answer honestly.
//!
//! `Unsafe`'s static initializer asks for the array base offset and element scale of each
//! primitive array type; those are the numbers `jdk.internal.util.ArraysSupport` turns into
//! shift counts. This VM's arrays are Rust vectors, so the base offset is zero and the scale is
//! the element width — enough for the offset arithmetic the library does before it touches an
//! array, and nothing more. Memory access through `Unsafe` (`getIntUnaligned` and friends) is
//! not implemented yet: it returns the descriptor-based scale only.

use rava_vm::native::expect_args;
use rava_vm::{ClassTarget, NativeCtx, Shared, Value, VmError};

use crate::no_op;

/// Installs the `Unsafe` natives.
pub fn register(shared: &mut Shared) {
    shared.register_native("jdk/internal/misc/Unsafe", "registerNatives", "()V", no_op);
    shared.register_native(
        "jdk/internal/misc/Unsafe",
        "arrayBaseOffset0",
        "(Ljava/lang/Class;)I",
        array_base_offset,
    );
    shared.register_native(
        "jdk/internal/misc/Unsafe",
        "arrayIndexScale0",
        "(Ljava/lang/Class;)I",
        array_index_scale,
    );
}

/// Arrays start at offset zero: an offset is an element index times the scale.
fn array_base_offset(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, _array_class] = expect_args::<2>(ctx)?;
    Ok(Value::Int(0))
}

/// The element width of a primitive array class, from its descriptor.
fn array_index_scale(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, array_class] = expect_args::<2>(ctx)?;
    let reference = array_class.as_ref().ok_or(VmError::TypeMismatch {
        expected: "an array class",
        found: *array_class,
    })?;
    let descriptor = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Type(descriptor)) => descriptor.as_str(),
        _ => {
            return Err(VmError::Native(
                "arrayIndexScale0 needs a primitive array class".to_owned(),
            ));
        }
    };
    // Primitive arrays report their element width. A reference array reports 1: this VM's
    // offsets are element indices, and `Unsafe` reference access is not implemented yet, so the
    // number only has to keep the library's offset arithmetic coherent.
    let scale = match descriptor {
        "[Z" | "[B" => 1,
        "[C" | "[S" => 2,
        "[I" | "[F" => 4,
        "[J" | "[D" => 8,
        other if other.starts_with("[L") => 1,
        other => {
            return Err(VmError::Native(format!(
                "arrayIndexScale0: {other} is not an array type"
            )));
        }
    };
    Ok(Value::Int(scale))
}
