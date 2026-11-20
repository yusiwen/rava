//! Implementations of the native methods the hosted class library declares.
//!
//! The class library is the reference one (JDK 21): its class files contain `native` method
//! declarations and no bodies, exactly as a user class would. This crate is where the Rust
//! side lives, keyed by the `(owner, name, descriptor)` triple the class file stores at the
//! call site. The registry itself is in `rava-vm`; nothing here is JNI, and no object layout
//! is visible to C.
//!
//! S7 starts with the output path: `System`'s setters, `FileDescriptor`'s and
//! `FileOutputStream`'s identification hooks, and the two writes that end at file descriptor
//! 1 or 2. Those descriptors are the VM's buffers (`Shared::out`/`Shared::err`), flushed by
//! the CLI at the end of the run; any other descriptor is refused, because opening files is a
//! later slice.

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rava_vm::native::expect_args;
use rava_vm::{ClassId, ClassTarget, NativeCtx, Shared, Value, VmError};

/// Installs the JCL natives this VM implements.
pub fn register_jcl_natives(shared: &mut Shared) {
    // `System.<clinit>` registers its natives with the JVM; here the table is built ahead of
    // time, so the call is a no-op that exists because the class file calls it.
    shared.register_native("java/lang/System", "registerNatives", "()V", no_op);

    // `System.initPhase1` installs the standard streams through these.
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

    // The class files call these once, from their static initializers, to cache field offsets
    // in the JVM. The VM resolves fields by name, so there is nothing to cache.
    // `Class.<clinit>` registers the reflection natives the same way; the VM has no JNI to
    // wire, so these are no-ops that keep the initializer honest.
    shared.register_native("java/lang/Class", "registerNatives", "()V", no_op);
    shared.register_native("java/io/FileDescriptor", "initIDs", "()V", no_op);
    shared.register_native("java/io/FileOutputStream", "initIDs", "()V", no_op);

    // `FileDescriptor(int)` caches the OS handle and the append flag through these.
    shared.register_native(
        "java/io/FileDescriptor",
        "getHandle",
        "(I)J",
        descriptor_handle,
    );
    shared.register_native(
        "java/io/FileDescriptor",
        "getAppend",
        "(I)Z",
        descriptor_append,
    );

    shared.register_native("java/io/FileOutputStream", "write", "(IZ)V", write_byte);
    shared.register_native(
        "java/io/FileOutputStream",
        "writeBytes",
        "([BIIZ)V",
        write_bytes,
    );

    // The class object. `getClass` is the only way a program sees it; the `Class` methods the
    // VM must answer are the ones that read VM-side facts rather than the object's fields.
    shared.register_native(
        "java/lang/Object",
        "getClass",
        "()Ljava/lang/Class;",
        get_class,
    );
    shared.register_native(
        "java/lang/Class",
        "initClassName",
        "()Ljava/lang/String;",
        init_class_name,
    );
    shared.register_native(
        "java/lang/Class",
        "desiredAssertionStatus0",
        "(Ljava/lang/Class;)Z",
        assertions_disabled,
    );
    shared.register_native(
        "jdk/internal/reflect/Reflection",
        "getCallerClass",
        "()Ljava/lang/Class;",
        get_caller_class,
    );
    shared.register_native("java/lang/Class", "isPrimitive", "()Z", is_primitive);
    shared.register_native("java/lang/Class", "isArray", "()Z", is_array);
    shared.register_native("java/lang/Class", "isInterface", "()Z", is_interface);

    // CDS asks whether the VM is dumping an archive; this VM never is.
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
        "java/lang/Class",
        "getPrimitiveClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        get_primitive_class,
    );
    // No archived heap: the class is left as loaded and verified.
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

    // The bit-level float/double conversions the library declares native.
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

    // Time. `nanoTime` has an arbitrary origin but must be monotonic; the VM's start is that
    // origin, so the first call is 0 and the clock never goes backwards.
    shared.register_native("java/lang/Object", "hashCode", "()I", identity_hash);
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
}

/// `Class.getPrimitiveClass(String)`: the class object for a primitive wrapper's `TYPE`.
fn get_primitive_class(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [name] = expect_args::<1>(ctx)?;
    let reference = name.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a primitive name",
        found: *name,
    })?;
    let text = match ctx.shared.heap.get(reference.index()) {
        Some(rava_vm::HeapObject::Str(text)) => text.clone(),
        other => {
            return Err(VmError::Native(format!(
                "getPrimitiveClass needs a string, got {other:?}"
            )));
        }
    };
    let descriptor = match text.as_str() {
        "int" => "I",
        "long" => "J",
        "float" => "F",
        "double" => "D",
        "boolean" => "Z",
        "byte" => "B",
        "char" => "C",
        "short" => "S",
        "void" => "V",
        other => {
            return Err(VmError::Native(format!(
                "getPrimitiveClass: {other} is not a primitive"
            )));
        }
    };
    let path = ctx.path;
    Ok(Value::Ref(ctx.shared.type_class(path, descriptor)?))
}

/// `Reflection.getCallerClass()`: the class that called the caller-sensitive method. The
/// native has no frame of its own, so the top frame called the method and the one below it is
/// the caller whose class is wanted.
fn get_caller_class(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let caller = ctx
        .thread
        .frames
        .iter()
        .rev()
        .nth(1)
        .ok_or_else(|| VmError::Native("getCallerClass: no caller frame".to_owned()))?;
    let class = caller.class;
    let path = ctx.path;
    Ok(Value::Ref(ctx.shared.class_object(path, class)?))
}

fn identity_hash(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [object] = expect_args::<1>(ctx)?;
    let reference = object.as_ref().ok_or(VmError::TypeMismatch {
        expected: "an object reference",
        found: *object,
    })?;
    Ok(Value::Int(ctx.shared.identity_hash(reference)))
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

fn zero_long(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::Long(0))
}

/// A native whose answer this VM can honestly give as "no".
fn false_flag(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::Int(0))
}

fn no_op(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
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

/// `Object.getClass()`: the class object of the receiver's runtime class.
fn get_class(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "an object reference",
        found: *this,
    })?;
    let class = ctx.shared.class_of(reference).ok_or_else(|| {
        VmError::Native(format!(
            "getClass on a value with no runtime class ({reference:?})"
        ))
    })?;
    let path = ctx.path;
    Ok(Value::Ref(ctx.shared.class_object(path, class)?))
}

/// `Class.initClassName()`: the name `Class.getName` would give, cached in the object's `name`
/// field so the next call is a plain field read (the host's `getName` is exactly
/// `name != null ? name : initClassName()`).
fn init_class_name(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let name = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Loaded(class)) => ctx.shared.class(*class).name.replace('/', "."),
        Some(ClassTarget::Type(descriptor)) => rava_vm::type_name(descriptor),
        None => {
            return Err(VmError::Native(format!(
                "initClassName on {reference:?}, which is not a class object"
            )));
        }
    };
    let string = ctx.shared.intern(&name);
    let class_class = ctx.shared.class_of(reference);
    if let Some(class_class) = class_class
        && let Some(slot) = ctx
            .shared
            .class(class_class)
            .field_slot("name", "Ljava/lang/String;")
    {
        ctx.shared
            .set_instance_field(reference, slot, Value::Ref(string))?;
    }
    Ok(Value::Ref(string))
}

/// Assertions are disabled: the VM has no `-ea`, so every class reports the default.
fn assertions_disabled(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_class] = expect_args::<1>(ctx)?;
    Ok(Value::Int(0))
}

fn is_primitive(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let primitive = matches!(
        ctx.shared.class_targets.get(&reference),
        Some(ClassTarget::Type(descriptor)) if !descriptor.starts_with('[')
    );
    Ok(Value::Int(i32::from(primitive)))
}

fn is_array(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let array = matches!(
        ctx.shared.class_targets.get(&reference),
        Some(ClassTarget::Type(descriptor)) if descriptor.starts_with('[')
    );
    Ok(Value::Int(i32::from(array)))
}

fn is_interface(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let interface = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Loaded(class)) => ctx
            .shared
            .class(*class)
            .file
            .as_ref()
            .is_some_and(|file| file.access_flags & 0x0200 != 0),
        _ => false,
    };
    Ok(Value::Int(i32::from(interface)))
}

/// The OS handle is the descriptor number itself in this VM: the standard streams are
/// buffers, and no other descriptor is open yet.
fn descriptor_handle(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [fd] = expect_args::<1>(ctx)?;
    let fd = fd.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *fd,
    })?;
    Ok(Value::Long(i64::from(fd)))
}

/// Nothing this VM opens in S7 is append-mode.
fn descriptor_append(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [fd] = expect_args::<1>(ctx)?;
    let _ = fd.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *fd,
    })?;
    Ok(Value::Int(0))
}

fn write_byte(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, byte, _append] = expect_args::<3>(ctx)?;
    let byte = byte.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *byte,
    })?;
    let fd = descriptor_fd(ctx, *this)?;
    write_fd(ctx, fd, &[byte as u8])?;
    Ok(Value::NULL)
}

fn write_bytes(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, array, offset, length, _append] = expect_args::<5>(ctx)?;
    let array = array.as_ref().ok_or(VmError::TypeMismatch {
        expected: "byte[]",
        found: *array,
    })?;
    let offset = offset.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *offset,
    })?;
    let length = length.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *length,
    })?;
    let data = byte_range(ctx.shared, array, offset, length)?;
    let fd = descriptor_fd(ctx, *this)?;
    write_fd(ctx, fd, &data)?;
    Ok(Value::NULL)
}

/// Copies `length` bytes from `array[offset..]`.
fn byte_range(
    shared: &Shared,
    array: rava_vm::ObjRef,
    offset: i32,
    length: i32,
) -> Result<Vec<u8>, VmError> {
    let array = shared.array(array)?;
    if array.kind != rava_vm::ArrayKind::Byte {
        return Err(VmError::Native(format!(
            "writeBytes needs a byte array, got {:?}",
            array.kind
        )));
    }
    let start = usize::try_from(offset).map_err(|_| VmError::IndexOutOfBounds {
        index: offset,
        length: array.elements.len(),
    })?;
    let count = usize::try_from(length).map_err(|_| VmError::IndexOutOfBounds {
        index: length,
        length: array.elements.len(),
    })?;
    let end = start.checked_add(count).ok_or(VmError::IndexOutOfBounds {
        index: offset,
        length: array.elements.len(),
    })?;
    if end > array.elements.len() {
        return Err(VmError::IndexOutOfBounds {
            index: end as i32,
            length: array.elements.len(),
        });
    }
    let mut bytes = Vec::with_capacity(count);
    for element in &array.elements[start..end] {
        bytes.push(element.as_int().ok_or(VmError::TypeMismatch {
            expected: "byte",
            found: *element,
        })? as u8);
    }
    Ok(bytes)
}

/// The `fd` an output stream writes to: `FileOutputStream.fd` -> `FileDescriptor.fd`.
fn descriptor_fd(ctx: &NativeCtx<'_>, stream: Value) -> Result<i32, VmError> {
    let stream = stream.as_ref().ok_or(VmError::TypeMismatch {
        expected: "FileOutputStream",
        found: stream,
    })?;
    let class = instance_class(ctx.shared, stream)?;
    let slot = ctx
        .shared
        .class(class)
        .field_slot("fd", "Ljava/io/FileDescriptor;")
        .ok_or_else(|| {
            VmError::Native("output stream has no `fd` field; is it a FileOutputStream?".to_owned())
        })?;
    let descriptor = ctx.shared.instance_field(stream, slot)?;
    let descriptor = descriptor.as_ref().ok_or(VmError::Native(
        "output stream is closed: its descriptor is null".to_owned(),
    ))?;
    let descriptor_class = instance_class(ctx.shared, descriptor)?;
    let slot = ctx
        .shared
        .class(descriptor_class)
        .field_slot("fd", "I")
        .ok_or_else(|| VmError::Native("descriptor has no `fd` field".to_owned()))?;
    ctx.shared
        .instance_field(descriptor, slot)?
        .as_int()
        .ok_or_else(|| VmError::Native("file descriptor's fd is not an int".to_owned()))
}

fn instance_class(shared: &Shared, reference: rava_vm::ObjRef) -> Result<ClassId, VmError> {
    match shared.heap.get(reference.index()) {
        Some(rava_vm::HeapObject::Instance(instance)) => Ok(instance.class),
        _ => Err(VmError::TypeMismatch {
            expected: "an object reference",
            found: Value::Ref(reference),
        }),
    }
}

/// Writes to the VM's standard streams. Descriptors 1 and 2 are the ones this slice owns.
fn write_fd(ctx: &mut NativeCtx<'_>, fd: i32, bytes: &[u8]) -> Result<(), VmError> {
    match fd {
        1 => {
            ctx.shared.out.extend_from_slice(bytes);
            Ok(())
        }
        2 => {
            ctx.shared.err.extend_from_slice(bytes);
            Ok(())
        }
        other => Err(VmError::Native(format!(
            "file descriptor {other} is not open; only the standard streams exist in this slice"
        ))),
    }
}
