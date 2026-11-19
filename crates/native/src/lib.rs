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

use rava_vm::native::expect_args;
use rava_vm::{ClassId, NativeCtx, Shared, Value, VmError};

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
