//! The remaining natives: flags, bits and processor counts.
//!
//! Small answers the library asks for once or twice: CDS is off, floats convert to their bit patterns, and the processor count is the host's.

use rava_vm::native::expect_args;
use rava_vm::{NativeCtx, Shared, Value, VmError};

use crate::no_op;

/// Installs the remaining natives.
pub fn register(shared: &mut Shared) {
    // CDS asks whether the VM is dumping an archive; this VM never is.
    // `VM.<clinit>` calls `initialize`, which HotSpot uses to publish the boot level and the
    // saved properties. This VM has no property source yet (S21 note); leaving the level at its
    // default is enough for the charset and stream classes, which read it only to branch on
    // boot progress.
    shared.register_native("jdk/internal/misc/VM", "initialize", "()V", no_op);
    // `ScopedMemoryAccess` registers natives in its static initializer; the on-heap buffer paths
    // this VM reaches are the `Unsafe` ones, so registration is all it needs.
    shared.register_native(
        "jdk/internal/misc/ScopedMemoryAccess",
        "registerNatives",
        "()V",
        no_op,
    );
    shared.register_native("java/lang/Thread", "registerNatives", "()V", no_op);
    // `NullPointerException.getMessage` asks the VM for the extended help text HotSpot
    // produces (`-XX:+ShowCodeDetailsInExceptionMessages`). This VM has no such analysis, so
    // the answer is `null` and `getMessage` falls back to the plain constructor message.
    shared.register_native(
        "java/lang/NullPointerException",
        "getExtendedNPEMessage",
        "()Ljava/lang/String;",
        null_string,
    );
    shared.register_native(
        "java/lang/Thread",
        "currentThread",
        "()Ljava/lang/Thread;",
        current_thread,
    );
    shared.register_native(
        "java/lang/Thread",
        "currentCarrierThread",
        "()Ljava/lang/Thread;",
        current_thread,
    );
    shared.register_native(
        "jdk/internal/misc/CDS",
        "isDumpingClassList0",
        "()Z",
        false_flag,
    );
    shared.register_native(
        "jdk/internal/misc/CDS",
        "isDumpingArchive0",
        "()Z",
        false_flag,
    );
    shared.register_native(
        "jdk/internal/misc/CDS",
        "isSharingEnabled0",
        "()Z",
        false_flag,
    );
    shared.register_native(
        "jdk/internal/misc/CDS",
        "getRandomSeedForDumping",
        "()J",
        zero_long,
    );
    shared.register_native(
        "jdk/internal/misc/CDS",
        "initializeFromArchive",
        "(Ljava/lang/Class;)V",
        no_op,
    );
    shared.register_native(
        "java/lang/Runtime",
        "availableProcessors",
        "()I",
        available_processors,
    );
    shared.register_native(
        "java/lang/Float",
        "floatToRawIntBits",
        "(F)I",
        float_to_int_bits,
    );
    shared.register_native(
        "java/lang/Float",
        "intBitsToFloat",
        "(I)F",
        int_bits_to_float,
    );
    shared.register_native(
        "java/lang/Double",
        "doubleToRawLongBits",
        "(D)J",
        double_to_long_bits,
    );
    shared.register_native(
        "java/lang/Double",
        "longBitsToDouble",
        "(J)D",
        long_bits_to_double,
    );
}

/// `Thread.currentThread()`: the VM's one thread, created on first use. The constructor is not
/// run — it reaches `ThreadGroup` and the security context, none of which this VM models — but
/// the fields the library reads for identity and naming are set.
fn current_thread(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    // Static: no receiver on the argument list.
    if let Some(reference) = ctx.shared.main_thread {
        return Ok(Value::Ref(reference));
    }
    let class = ctx.shared.load_class(ctx.path, "java/lang/Thread")?;
    let reference = ctx.shared.new_instance(class);
    let name = ctx.shared.new_string(ctx.path, "main".to_owned())?;
    for (field, descriptor, value) in [
        ("name", "Ljava/lang/String;", Value::Ref(name)),
        ("tid", "J", Value::Long(1)),
        ("eetop", "J", Value::Long(0)),
    ] {
        if let Some(slot) = ctx.shared.class(class).field_slot(field, descriptor) {
            ctx.shared.set_instance_field(reference, slot, value)?;
        }
    }
    ctx.shared.main_thread = Some(reference);
    Ok(Value::Ref(reference))
}

/// A native whose answer is "no string".
fn null_string(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
}

/// A native whose answer this VM can honestly give as "no".
fn false_flag(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::Int(0))
}

fn zero_long(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

fn available_processors(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let count = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1);
    Ok(Value::Int(count as i32))
}

fn float_to_int_bits(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [value] = expect_args::<1>(ctx)?;
    let value = match value {
        Value::Float(value) => *value,
        other => {
            return Err(VmError::TypeMismatch {
                expected: "float",
                found: *other,
            });
        }
    };
    Ok(Value::Int(value.to_bits() as i32))
}

fn int_bits_to_float(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [value] = expect_args::<1>(ctx)?;
    let bits = value.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *value,
    })?;
    Ok(Value::Float(f32::from_bits(bits as u32)))
}

fn double_to_long_bits(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [value] = expect_args::<1>(ctx)?;
    let value = match value {
        Value::Double(value) => *value,
        other => {
            return Err(VmError::TypeMismatch {
                expected: "double",
                found: *other,
            });
        }
    };
    Ok(Value::Long(value.to_bits() as i64))
}

fn long_bits_to_double(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [value] = expect_args::<1>(ctx)?;
    let bits = value.as_long().ok_or(VmError::TypeMismatch {
        expected: "long",
        found: *value,
    })?;
    Ok(Value::Double(f64::from_bits(bits as u64)))
}
