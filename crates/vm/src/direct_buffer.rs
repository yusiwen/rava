//! VM-managed direct byte buffers — M3.5.
//!
//! Tomcat's `org.apache.tomcat.util.buf.ByteBufferUtils` calls
//! `ByteBuffer.allocateDirect(0)` from its static initializer and later
//! `Unsafe.invokeCleaner(ByteBuffer)`. The JDK's contract has no observable address here:
//! ADR-0005 keeps direct buffers VM-managed and makes `invokeCleaner` a no-op because the
//! collector owns the storage.
//!
//! This module installs the three natives the path needs:
//!
//! - `java.nio.ByteBuffer.allocateDirect(I)` returns an ordinary [`crate::HeapObject::Instance`]
//!   of class `java.nio.DirectByteBuffer`, allocated through
//!   [`crate::Shared::allocate_instance`]. The inherited `Buffer.position`/`limit`/
//!   `capacity` fields are set when the loaded class carries them; the generated C1 stubs
//!   project only public members, so the private `Buffer` fields are absent and a `0`
//!   capacity is simply the allocator's default. A negative capacity is a typed error.
//! - `java.nio.DirectByteBuffer.isDirect()` answers `true`.
//! - `java.nio.ByteBuffer.isDirect()` answers `false` when a call resolves to the abstract
//!   declaration instead of the direct declaration.
//!
//! There is no second buffer representation: the object is the same instance shape every
//! other class uses, and no address or off-heap storage exists.
//!
//! **Why the class is installed here.** `java.nio.DirectByteBuffer` is a package-private
//! JDK class, and `ct.sym` carries only public signatures, so the generated
//! `stdlib/c1/classes` tree has no class file for it even though the committed
//! `java.nio.ByteBuffer.allocateDirect` stub names it. This module defines the shape in
//! the bootstrap loader when the library does not supply one; a future library class of
//! the same name wins because the lookup is tried first. The class declares `isDirect` as
//! `ACC_NATIVE`, so an unregistered native reports the named "no native implementation"
//! error instead of a missing-bytecode error.
//!
//! Deliberate limits: there are no backing bytes and no off-heap storage, the constructor
//! is not provided because `allocateDirect` never runs one, and only the `isDirect` query
//! is answered. A buffer operation beyond that fails at its own call site. Tomcat's
//! static-initializer probe only needs `allocateDirect(0)` to produce an object and
//! `invokeCleaner` to be a no-op.

use rava_classfile::ClassBuilder;

use crate::VmError;
use crate::heap::{HeapError, ObjRef};
use crate::loader::{ClassId, LoaderId};
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `java.nio.ByteBuffer`'s internal name.
const BYTE_BUFFER_CLASS: &str = "java/nio/ByteBuffer";

/// `java.nio.Buffer`'s internal name: the superclass that owns the capacity fields.
const BUFFER_CLASS: &str = "java/nio/Buffer";

/// The package-private JDK class `allocateDirect` returns.
const DIRECT_BYTE_BUFFER_CLASS: &str = "java/nio/DirectByteBuffer";

/// `ByteBuffer.allocateDirect(int)`.
const ALLOCATE_DIRECT_DESCRIPTOR: &str = "(I)Ljava/nio/ByteBuffer;";

/// `isDirect()`.
const IS_DIRECT_DESCRIPTOR: &str = "()Z";

/// The class-file major version every generated class uses (Java 17).
const CLASS_VERSION: u16 = 61;

const ACC_PUBLIC: u16 = 0x0001;
const ACC_FINAL: u16 = 0x0010;
const ACC_SUPER: u16 = 0x0020;
const ACC_NATIVE: u16 = 0x0100;

/// The generated direct-buffer class is `public final super`.
const DIRECT_CLASS_ACCESS: u16 = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;

/// The direct class's `isDirect` is `public native`.
const DIRECT_METHOD_ACCESS: u16 = ACC_PUBLIC | ACC_NATIVE;

/// Install the direct-buffer shape and its natives.
///
/// `java.nio.ByteBuffer`'s C1 `<clinit>` reads `Buffer.UNSAFE` and calls
/// `jdk.internal.misc.Unsafe.arrayBaseOffset(Class)`, a U1 member this VM does not
/// implement. The direct-buffer path needs none of those constants, so the class is
/// marked initialized and its initializer is not run. The generated stubs have no private
/// `Buffer` fields, so the ordinary allocator's default (capacity 0) is the only capacity
/// the probe can observe.
///
/// Idempotent: the class lookup wins over generation, and the native registry is keyed by
/// `(owner, name, descriptor)`.
pub fn install(shared: &Shared) -> Result<(), VmError> {
    let byte_buffer = shared.load_library_class(BYTE_BUFFER_CLASS)?;
    let buffer = shared.load_library_class(BUFFER_CLASS)?;
    // Both C1 initializers read `Buffer.UNSAFE` and call a U1 accessor; the direct-buffer
    // path needs none of those constants, and `capacity()`/`limit()` read the instance
    // fields the allocator fills.
    shared.mark_initialized(byte_buffer)?;
    shared.mark_initialized(buffer)?;
    ensure_class(shared)?;
    shared.register_native(
        BYTE_BUFFER_CLASS,
        "allocateDirect",
        ALLOCATE_DIRECT_DESCRIPTOR,
        allocate_direct,
    );
    shared.register_native(
        DIRECT_BYTE_BUFFER_CLASS,
        "isDirect",
        IS_DIRECT_DESCRIPTOR,
        direct_is_direct,
    );
    shared.register_native(
        BYTE_BUFFER_CLASS,
        "isDirect",
        IS_DIRECT_DESCRIPTOR,
        byte_buffer_is_direct,
    );
    Ok(())
}

/// The direct-buffer class, loaded from the library when present and generated otherwise.
///
/// The bootstrap loader owns the identity so every application loader delegates to the
/// same class, exactly as it does for the rest of the class library.
pub(crate) fn ensure_class(shared: &Shared) -> Result<ClassId, VmError> {
    if let Some(class) = shared.lookup(LoaderId::BOOTSTRAP, DIRECT_BYTE_BUFFER_CLASS) {
        return Ok(class);
    }
    match shared.load_class(LoaderId::BOOTSTRAP, DIRECT_BYTE_BUFFER_CLASS) {
        Ok(class) => Ok(class),
        Err(VmError::ClassNotFound { .. }) => shared.define_class(
            LoaderId::BOOTSTRAP,
            DIRECT_BYTE_BUFFER_CLASS,
            &direct_byte_buffer_class_bytes(),
        ),
        Err(error) => Err(error),
    }
}

/// `java.nio.ByteBuffer.allocateDirect(int)`: allocate a direct buffer of `capacity`.
///
/// The buffer is an ordinary instance of `java.nio.DirectByteBuffer`; no constructor
/// runs and nothing outside the heap is allocated. The inherited `Buffer.position`,
/// `Buffer.limit` and `Buffer.capacity` slots are filled when the loaded class carries
/// them.
fn allocate_direct(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = match args.first().copied() {
        Some(Value::Int(capacity)) => capacity,
        _ => {
            return Err(VmError::Native(
                "java.nio.ByteBuffer.allocateDirect expects an int capacity".to_owned(),
            ));
        }
    };
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "java.nio.ByteBuffer.allocateDirect: negative capacity {capacity}"
        )));
    }
    let class = ensure_class(shared)?;
    let buffer = shared.allocate_instance(class)?;
    initialize_capacity(shared, buffer, capacity)?;
    Ok(Some(Value::Ref(buffer)))
}

/// Fill the inherited `Buffer` fields of a fresh direct buffer.
///
/// A missing slot is skipped rather than invented: the C1 stubs project only public
/// members, so `java.nio.Buffer`'s private `position`/`limit`/`capacity` fields are not
/// present on the committed class, while a library class of the same name may carry them.
/// The names are `int` fields in the JDK's layout.
fn initialize_capacity(shared: &Shared, buffer: ObjRef, capacity: i32) -> Result<(), VmError> {
    let class = {
        let heap = shared.heap();
        heap.object_class(buffer).map_err(heap_error)?
    };
    let layout = shared.field_layout(class)?;
    let mut heap = shared.heap();
    let instance = heap.instance_mut(buffer).map_err(heap_error)?;
    for (name, value) in [("position", 0), ("limit", capacity), ("capacity", capacity)] {
        if let Some(slot) = layout.slot(name, "I") {
            if let Some(field) = instance.fields.get_mut(slot) {
                *field = Value::Int(value);
            }
        }
    }
    Ok(())
}

/// `java.nio.DirectByteBuffer.isDirect()`: a direct buffer is direct.
fn direct_is_direct(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    require_direct_buffer(shared, receiver)?;
    Ok(Some(Value::Int(1)))
}

/// `java.nio.ByteBuffer.isDirect()`: the abstract declaration answers `false`.
///
/// A virtual call on a direct buffer resolves to `DirectByteBuffer.isDirect` first, so
/// this is reached only for a non-direct buffer or a reflected call that names the
/// abstract declaration.
fn byte_buffer_is_direct(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "java.nio.ByteBuffer.isDirect called with a null receiver".to_owned(),
        ));
    }
    // Touch the heap so a stale handle is a typed error instead of a silent `false`.
    let heap = shared.heap();
    heap.object_class(receiver).map_err(heap_error)?;
    Ok(Some(Value::Int(0)))
}

/// Reject a receiver that is not the installed direct-buffer class.
fn require_direct_buffer(shared: &Shared, receiver: ObjRef) -> Result<(), VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "java.nio.DirectByteBuffer.isDirect called with a null receiver".to_owned(),
        ));
    }
    let class = {
        let heap = shared.heap();
        heap.object_class(receiver).map_err(heap_error)?
    };
    let name = shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_default();
    if name != DIRECT_BYTE_BUFFER_CLASS {
        return Err(VmError::Native(format!(
            "java.nio.DirectByteBuffer.isDirect expects a direct buffer, found {name}"
        )));
    }
    Ok(())
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// The class file of the installed direct-buffer shape.
fn direct_byte_buffer_class_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new(DIRECT_BYTE_BUFFER_CLASS, BYTE_BUFFER_CLASS, CLASS_VERSION)
        .with_access(DIRECT_CLASS_ACCESS);
    builder.method(DIRECT_METHOD_ACCESS, "isDirect", IS_DIRECT_DESCRIPTOR, None);
    builder.build()
}
