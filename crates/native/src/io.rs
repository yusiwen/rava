//! The io natives: `FileDescriptor` and `FileOutputStream`.
//!
//! Descriptors 1 and 2 are the VM's buffers (`Shared::out`/`Shared::err`); any other descriptor is refused because opening files belongs to a later slice.

use rava_vm::native::expect_args;
use rava_vm::{ClassId, NativeCtx, Shared, Value, VmError};

use crate::no_op;

/// Installs the io natives.
pub fn register(shared: &mut Shared) {
    // The class files call these once, from their static initializers, to cache field offsets
    // in the JVM; the VM resolves fields by name, so there is nothing to cache.
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
    // `FileDescriptor.<clinit>` calls `initIDs`; HotSpot caches field IDs there, this VM has
    // nothing to cache.
    shared.register_native("java/io/FileDescriptor", "initIDs", "()V", crate::no_op);
    shared.register_native("java/io/FileOutputStream", "write", "(IZ)V", write_byte);
    shared.register_native(
        "java/io/FileOutputStream",
        "writeBytes",
        "([BIIZ)V",
        write_bytes,
    );
    // `System.in` is a FileInputStream over FileDescriptor.in; these are its read path.
    shared.register_native("java/io/FileInputStream", "initIDs", "()V", no_op);
    shared.register_native("java/io/FileInputStream", "read0", "()I", input_read0);
    shared.register_native(
        "java/io/FileInputStream",
        "readBytes",
        "([BII)I",
        input_read_bytes,
    );
    shared.register_native(
        "java/io/FileInputStream",
        "available0",
        "()I",
        input_available,
    );
    shared.register_native("java/io/FileInputStream", "skip0", "(J)J", input_skip);
    shared.register_native(
        "java/io/FileInputStream",
        "position0",
        "()J",
        input_position,
    );
    shared.register_native("java/io/FileInputStream", "length0", "()J", input_length);
    // Closing a standard descriptor is a no-op: the VM's buffers outlive every stream.
    shared.register_native("java/io/FileDescriptor", "close0", "()V", no_op);
}

/// Fills the stdin buffer from the process on first use. A program that never reads stdin never
/// blocks on it; one that does reads its input to end (the CLI is not interactive).
fn ensure_stdin(shared: &mut Shared) {
    if shared.stdin.is_none() {
        use std::io::Read;
        let mut data = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut data);
        shared.stdin = Some(data);
        shared.stdin_pos = 0;
    }
}

/// The descriptor number of a `FileInputStream`'s `fd` field.
fn input_fd(ctx: &NativeCtx<'_>, stream: Value) -> Result<i32, VmError> {
    let stream = stream.as_ref().ok_or(VmError::TypeMismatch {
        expected: "FileInputStream",
        found: stream,
    })?;
    let class = instance_class(ctx.shared, stream)?;
    let slot = ctx
        .shared
        .class(class)
        .field_slot("fd", "Ljava/io/FileDescriptor;")
        .ok_or_else(|| VmError::Native("input stream has no `fd` field".to_owned()))?;
    let descriptor = ctx.shared.instance_field(stream, slot)?;
    let descriptor = descriptor
        .as_ref()
        .ok_or(VmError::Native("input stream has a null `fd`".to_owned()))?;
    let descriptor_class = instance_class(ctx.shared, descriptor)?;
    let fd_slot = ctx
        .shared
        .class(descriptor_class)
        .field_slot("fd", "I")
        .ok_or_else(|| VmError::Native("FileDescriptor has no `fd` field".to_owned()))?;
    let fd = ctx
        .shared
        .instance_field(descriptor, fd_slot)?
        .as_int()
        .ok_or(VmError::Native(
            "FileDescriptor.fd is not an int".to_owned(),
        ))?;
    Ok(fd)
}

fn input_read0(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let fd = input_fd(ctx, *this)?;
    if fd != 0 {
        return Err(VmError::Native(format!(
            "file descriptor {fd} is not readable; only standard input has a reader"
        )));
    }
    ensure_stdin(ctx.shared);
    let data = ctx.shared.stdin.as_ref().expect("filled above");
    let position = ctx.shared.stdin_pos;
    if position >= data.len() {
        return Ok(Value::Int(-1));
    }
    ctx.shared.stdin_pos = position + 1;
    Ok(Value::Int(i32::from(data[position])))
}

fn input_read_bytes(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, array, offset, length] = expect_args::<4>(ctx)?;
    let fd = input_fd(ctx, *this)?;
    if fd != 0 {
        return Err(VmError::Native(format!(
            "file descriptor {fd} is not readable; only standard input has a reader"
        )));
    }
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
    if offset < 0 || length < 0 {
        return Err(VmError::IndexOutOfBounds {
            index: offset,
            length: 0,
        });
    }
    ensure_stdin(ctx.shared);
    let data = ctx.shared.stdin.as_ref().expect("filled above");
    let available = data.len() - ctx.shared.stdin_pos;
    if available == 0 {
        return Ok(Value::Int(-1));
    }
    let count = usize::try_from(length).unwrap_or(usize::MAX).min(available);
    let bytes: Vec<u8> = data[ctx.shared.stdin_pos..ctx.shared.stdin_pos + count].to_vec();
    {
        let array = ctx.shared.array_mut(array)?;
        let array_length = array.elements.len();
        for (index, byte) in bytes.iter().enumerate() {
            let cell = array
                .elements
                .get_mut(usize::try_from(offset).unwrap_or(usize::MAX) + index)
                .ok_or(VmError::IndexOutOfBounds {
                    index: offset + index as i32,
                    length: array_length,
                })?;
            *cell = Value::Int(i32::from(*byte));
        }
    }
    ctx.shared.stdin_pos += count;
    Ok(Value::Int(count as i32))
}

fn input_available(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let fd = input_fd(ctx, *this)?;
    if fd != 0 {
        return Ok(Value::Int(0));
    }
    ensure_stdin(ctx.shared);
    let data = ctx.shared.stdin.as_ref().expect("filled above");
    Ok(Value::Int((data.len() - ctx.shared.stdin_pos) as i32))
}

fn input_skip(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, amount] = expect_args::<2>(ctx)?;
    let fd = input_fd(ctx, *this)?;
    if fd != 0 {
        return Ok(Value::Long(0));
    }
    let amount = amount.as_long().ok_or(VmError::TypeMismatch {
        expected: "long",
        found: *amount,
    })?;
    ensure_stdin(ctx.shared);
    let data = ctx.shared.stdin.as_ref().expect("filled above");
    let available = (data.len() - ctx.shared.stdin_pos) as i64;
    let skipped = amount.max(0).min(available);
    ctx.shared.stdin_pos += skipped as usize;
    Ok(Value::Long(skipped))
}

fn input_position(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let _ = input_fd(ctx, *this)?;
    Ok(Value::Long(ctx.shared.stdin_pos as i64))
}

fn input_length(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let _ = input_fd(ctx, *this)?;
    ensure_stdin(ctx.shared);
    Ok(Value::Long(
        ctx.shared.stdin.as_ref().expect("filled above").len() as i64,
    ))
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
