//! `sun.misc.Unsafe` U0 and the `jdk.internal.misc.Unsafe` initialization queries — M3.5.
//!
//! ADR-0005 opens exactly one `Unsafe` tier for the first target. U0 is the class,
//! instance and cleaner surface and needs no address:
//!
//! - `sun.misc.Unsafe` exposes the private static `theUnsafe` field, `getUnsafe()`,
//!   `allocateInstance(Class)` and `invokeCleaner(ByteBuffer)`. The C1 stub carries no
//!   `Code` for them (and its `<clinit>` delegates to `jdk/internal/reflect/Reflection`,
//!   `jdk/internal/misc/VM` and U1/U3 members), so the registry is the implementation and
//!   registration installs the `theUnsafe` object the initializer would have created.
//! - `jdk.internal.misc.Unsafe` is where JDK 17 keeps `ensureClassInitialized(Class)` and
//!   `shouldBeInitialized(Class)`; its C1 tree has no class file, so the module installs
//!   the shape in the bootstrap loader. `allocateInstance` and `invokeCleaner` are
//!   answered there as well because the surface is the same U0 family.
//!
//! Every served call appends `owner.name descriptor` to
//! [`rava_vm::Shared::unsafe_calls`], which is ADR-0005's acceptance probe. No U1+ method
//! is registered: `objectFieldOffset` is declared `ACC_NATIVE` on the installed
//! `jdk.internal.misc.Unsafe` shape but deliberately remains unregistered, so a call
//! fails with the named "no native implementation" error instead of a missing-method
//! error or a silent success.
//!
//! `ensureClassInitialized` re-enters the interpreter's existing initialization path
//! ([`crate::ensure_initialized`]); `allocateInstance` ensures initialization, then uses
//! [`rava_vm::Shared::allocate_instance`] and never runs a constructor. `shouldBeInitialized`
//! reads the class state without initializing and follows the JDK's meaning: `true` means
//! the class still needs initialization.
//!
//! `invokeCleaner` is a no-op because direct buffers are VM-managed (ADR-0008): the
//! buffer's lifetime belongs to the collector, not to a cleaner call.
//!
//! **Presence guard.** Spring's repackaged Objenesis selects this path because
//! `java.vm.name` is `rava`, not HotSpot; `sun.reflect.ReflectionFactory` is therefore
//! not on the target path and is not faked here.

use rava_classfile::ClassBuilder;
use rava_vm::{
    ClassId, ClassState, HeapError, LoaderId, ObjRef, Shared, ThreadState, Value, VmError,
};

use crate::InterpError;
use crate::ensure_initialized;

/// `sun.misc.Unsafe`'s internal name.
const SUN_UNSAFE: &str = "sun/misc/Unsafe";

/// `jdk.internal.misc.Unsafe`'s internal name.
const JDK_UNSAFE: &str = "jdk/internal/misc/Unsafe";

/// The descriptor of `sun.misc.Unsafe.theUnsafe`.
const SUN_UNSAFE_DESCRIPTOR: &str = "Lsun/misc/Unsafe;";

/// The descriptor of `jdk.internal.misc.Unsafe.theUnsafe`.
const JDK_UNSAFE_DESCRIPTOR: &str = "Ljdk/internal/misc/Unsafe;";

/// `sun.misc.Unsafe.getUnsafe()`.
const SUN_GET_UNSAFE_DESCRIPTOR: &str = "()Lsun/misc/Unsafe;";

/// `jdk.internal.misc.Unsafe.getUnsafe()`.
const JDK_GET_UNSAFE_DESCRIPTOR: &str = "()Ljdk/internal/misc/Unsafe;";

/// `allocateInstance(Class)`.
const ALLOCATE_INSTANCE_DESCRIPTOR: &str = "(Ljava/lang/Class;)Ljava/lang/Object;";

/// `invokeCleaner(ByteBuffer)`.
const INVOKE_CLEANER_DESCRIPTOR: &str = "(Ljava/nio/ByteBuffer;)V";

/// `ensureClassInitialized(Class)`.
const ENSURE_CLASS_INITIALIZED_DESCRIPTOR: &str = "(Ljava/lang/Class;)V";

/// `shouldBeInitialized(Class)`.
const SHOULD_BE_INITIALIZED_DESCRIPTOR: &str = "(Ljava/lang/Class;)Z";

/// `objectFieldOffset(Field)`: declared on the installed jdk shape as the U1 tripwire.
const OBJECT_FIELD_OFFSET_DESCRIPTOR: &str = "(Ljava/lang/reflect/Field;)J";

/// The class-file major version every generated class uses (Java 17).
const CLASS_VERSION: u16 = 61;

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PRIVATE: u16 = 0x0002;
const ACC_STATIC: u16 = 0x0008;
const ACC_FINAL: u16 = 0x0010;
const ACC_SUPER: u16 = 0x0020;
const ACC_NATIVE: u16 = 0x0100;
const ACC_ABSTRACT: u16 = 0x0400;

/// The generated jdk class is `public final super`.
const JDK_CLASS_ACCESS: u16 = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;

/// `theUnsafe` is `private static final`.
const THE_UNSAFE_ACCESS: u16 = ACC_PRIVATE | ACC_STATIC | ACC_FINAL;

/// `getUnsafe` is `public static native`.
const JDK_STATIC_NATIVE: u16 = ACC_PUBLIC | ACC_STATIC | ACC_NATIVE;

/// The remaining U0 members are `public native`.
const JDK_INSTANCE_NATIVE: u16 = ACC_PUBLIC | ACC_NATIVE;

/// Install the U0 `Unsafe` surface and its natives.
///
/// Registration installs one `sun.misc.Unsafe` instance into its `theUnsafe` static and
/// one `jdk.internal.misc.Unsafe` instance into its own, idempotently. It then declares
/// `sun.misc.Unsafe` initialized: the stub's `<clinit>` reads
/// `jdk/internal/reflect/Reflection`, U1 offsets and `addressSize()`, none of which the
/// open tier provides, while the only U0-observable result is the `theUnsafe` object this
/// function installs.
///
/// Returns an error only when the class library cannot supply `sun.misc.Unsafe` or a
/// class cannot be defined; a second call is a no-op apart from re-registering the same
/// native keys.
pub fn register_natives(shared: &Shared) -> Result<(), VmError> {
    let sun = shared.load_library_class(SUN_UNSAFE)?;
    let jdk = ensure_jdk_unsafe(shared)?;
    install_static_instance(shared, sun, SUN_UNSAFE_DESCRIPTOR)?;
    install_static_instance(shared, jdk, JDK_UNSAFE_DESCRIPTOR)?;
    shared.mark_initialized(sun)?;

    shared.register_native(
        SUN_UNSAFE,
        "getUnsafe",
        SUN_GET_UNSAFE_DESCRIPTOR,
        sun_get_unsafe,
    );
    shared.register_native(
        SUN_UNSAFE,
        "allocateInstance",
        ALLOCATE_INSTANCE_DESCRIPTOR,
        sun_allocate_instance,
    );
    shared.register_native(
        SUN_UNSAFE,
        "invokeCleaner",
        INVOKE_CLEANER_DESCRIPTOR,
        sun_invoke_cleaner,
    );
    shared.register_native(
        JDK_UNSAFE,
        "getUnsafe",
        JDK_GET_UNSAFE_DESCRIPTOR,
        jdk_get_unsafe,
    );
    shared.register_native(
        JDK_UNSAFE,
        "allocateInstance",
        ALLOCATE_INSTANCE_DESCRIPTOR,
        jdk_allocate_instance,
    );
    shared.register_native(
        JDK_UNSAFE,
        "ensureClassInitialized",
        ENSURE_CLASS_INITIALIZED_DESCRIPTOR,
        jdk_ensure_class_initialized,
    );
    shared.register_native(
        JDK_UNSAFE,
        "shouldBeInitialized",
        SHOULD_BE_INITIALIZED_DESCRIPTOR,
        jdk_should_be_initialized,
    );
    shared.register_native(
        JDK_UNSAFE,
        "invokeCleaner",
        INVOKE_CLEANER_DESCRIPTOR,
        jdk_invoke_cleaner,
    );
    Ok(())
}

/// The jdk-internal class, loaded from the library when present and generated otherwise.
fn ensure_jdk_unsafe(shared: &Shared) -> Result<ClassId, VmError> {
    if let Some(class) = shared.lookup(LoaderId::BOOTSTRAP, JDK_UNSAFE) {
        return Ok(class);
    }
    match shared.load_class(LoaderId::BOOTSTRAP, JDK_UNSAFE) {
        Ok(class) => Ok(class),
        Err(VmError::ClassNotFound { .. }) => {
            shared.define_class(LoaderId::BOOTSTRAP, JDK_UNSAFE, &jdk_unsafe_class_bytes())
        }
        Err(error) => Err(error),
    }
}

/// Install one instance into `class`'s `theUnsafe` static, returning the installed object.
///
/// A second call returns the first object, so `getUnsafe()` and the static field stay
/// reference-identical. The instance is allocated through the ordinary allocator and no
/// constructor runs.
fn install_static_instance(
    shared: &Shared,
    class: ClassId,
    descriptor: &str,
) -> Result<ObjRef, VmError> {
    if let Some(object) = static_unsafe(shared, class, descriptor)? {
        return Ok(object);
    }
    let object = shared.allocate_instance(class)?;
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    entry
        .statics
        .lock()
        .expect("static field lock poisoned")
        .insert(
            ("theUnsafe".to_owned(), descriptor.to_owned()),
            Value::Ref(object),
        );
    Ok(object)
}

/// The installed `theUnsafe` object, if registration already stored one.
fn static_unsafe(
    shared: &Shared,
    class: ClassId,
    descriptor: &str,
) -> Result<Option<ObjRef>, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let stored = entry
        .statics
        .lock()
        .expect("static field lock poisoned")
        .get(&("theUnsafe".to_owned(), descriptor.to_owned()))
        .copied();
    match stored {
        Some(Value::Ref(object)) if !object.is_null() => Ok(Some(object)),
        _ => Ok(None),
    }
}

/// `sun.misc.Unsafe.getUnsafe()`: the installed instance.
fn sun_get_unsafe(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    record(shared, SUN_UNSAFE, "getUnsafe", SUN_GET_UNSAFE_DESCRIPTOR);
    let class = shared.load_library_class(SUN_UNSAFE)?;
    Ok(Some(Value::Ref(install_static_instance(
        shared,
        class,
        SUN_UNSAFE_DESCRIPTOR,
    )?)))
}

/// `jdk.internal.misc.Unsafe.getUnsafe()`: the installed instance.
fn jdk_get_unsafe(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    record(shared, JDK_UNSAFE, "getUnsafe", JDK_GET_UNSAFE_DESCRIPTOR);
    let class = shared.load_library_class(JDK_UNSAFE)?;
    Ok(Some(Value::Ref(install_static_instance(
        shared,
        class,
        JDK_UNSAFE_DESCRIPTOR,
    )?)))
}

/// `sun.misc.Unsafe.allocateInstance(Class)`.
fn sun_allocate_instance(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    allocate_instance(shared, thread, SUN_UNSAFE, args)
}

/// `jdk.internal.misc.Unsafe.allocateInstance(Class)`.
fn jdk_allocate_instance(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    allocate_instance(shared, thread, JDK_UNSAFE, args)
}

/// `allocateInstance(Class)`: initialize the class, then allocate without a constructor.
fn allocate_instance(
    shared: &Shared,
    thread: &ThreadState,
    owner: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    record(
        shared,
        owner,
        "allocateInstance",
        ALLOCATE_INSTANCE_DESCRIPTOR,
    );
    let class = class_argument(shared, args, 0, owner, "allocateInstance")?;
    ensure_initialized(shared, thread, class)
        .map_err(|error| initialization_error(owner, "allocateInstance", error))?;
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    if metadata.is_interface() || metadata.access_flags & ACC_ABSTRACT != 0 {
        return Err(VmError::Native(format!(
            "{owner}.allocateInstance cannot instantiate the abstract class {}",
            metadata.binary_name
        )));
    }
    Ok(Some(Value::Ref(shared.allocate_instance(class)?)))
}

/// `sun.misc.Unsafe.invokeCleaner(ByteBuffer)`: a no-op for a VM-managed buffer.
fn sun_invoke_cleaner(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    invoke_cleaner(shared, SUN_UNSAFE, args)
}

/// `jdk.internal.misc.Unsafe.invokeCleaner(ByteBuffer)`: a no-op for a VM-managed buffer.
fn jdk_invoke_cleaner(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    invoke_cleaner(shared, JDK_UNSAFE, args)
}

/// `invokeCleaner(ByteBuffer)`: record the call and leave the buffer untouched.
fn invoke_cleaner(shared: &Shared, owner: &str, args: &[Value]) -> Result<Option<Value>, VmError> {
    record(shared, owner, "invokeCleaner", INVOKE_CLEANER_DESCRIPTOR);
    match args.first().copied() {
        Some(Value::Ref(buffer)) if !buffer.is_null() => {
            // Touch the heap so a stale handle is a typed error; the buffer's storage
            // belongs to the collector, so there is nothing to release.
            shared.heap().object_class(buffer).map_err(heap_error)?;
            Ok(None)
        }
        _ => Err(VmError::Native(format!(
            "{owner}.invokeCleaner expects a non-null ByteBuffer"
        ))),
    }
}

/// `jdk.internal.misc.Unsafe.ensureClassInitialized(Class)`.
fn jdk_ensure_class_initialized(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    record(
        shared,
        JDK_UNSAFE,
        "ensureClassInitialized",
        ENSURE_CLASS_INITIALIZED_DESCRIPTOR,
    );
    let class = class_argument(shared, args, 0, JDK_UNSAFE, "ensureClassInitialized")?;
    ensure_initialized(shared, thread, class)
        .map_err(|error| initialization_error(JDK_UNSAFE, "ensureClassInitialized", error))?;
    Ok(None)
}

/// `jdk.internal.misc.Unsafe.shouldBeInitialized(Class)`.
///
/// The JDK's meaning is kept: `true` means the class still needs initialization. The
/// state is read without initializing the class.
fn jdk_should_be_initialized(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    record(
        shared,
        JDK_UNSAFE,
        "shouldBeInitialized",
        SHOULD_BE_INITIALIZED_DESCRIPTOR,
    );
    let class = class_argument(shared, args, 0, JDK_UNSAFE, "shouldBeInitialized")?;
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let initialized = {
        let state = entry.state.lock().expect("class state lock poisoned");
        matches!(*state, ClassState::Initialized)
    };
    Ok(Some(Value::Int(i32::from(!initialized))))
}

/// Resolve a `Class` argument to the loaded class it denotes.
fn class_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    owner: &str,
    method: &str,
) -> Result<ClassId, VmError> {
    let reference = match args.get(position).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(format!(
                "{owner}.{method} expects a non-null Class argument"
            )));
        }
    };
    shared.heap().class(reference).map_err(|_| {
        VmError::Native(format!(
            "{owner}.{method} expects a Class object for a loaded class"
        ))
    })
}

/// Append one served call to the U0 log.
fn record(shared: &Shared, owner: &str, name: &str, descriptor: &str) {
    shared.record_unsafe_call(format!("{owner}.{name}{descriptor}"));
}

/// Preserve a Java throwable raised by initialization; everything else is a typed error.
fn initialization_error(owner: &str, method: &str, error: InterpError) -> VmError {
    match error {
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(format!("{owner}.{method} failed: {other}")),
    }
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// The class file of the installed `jdk.internal.misc.Unsafe` shape.
///
/// It declares the U0 members as `ACC_NATIVE` and the U1 `objectFieldOffset` as an
/// unregistered tripwire. A future library class of the same name wins because the lookup
/// is tried first.
fn jdk_unsafe_class_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new(JDK_UNSAFE, "java/lang/Object", CLASS_VERSION)
        .with_access(JDK_CLASS_ACCESS);
    builder.field(THE_UNSAFE_ACCESS, "theUnsafe", JDK_UNSAFE_DESCRIPTOR);
    builder.method(
        JDK_STATIC_NATIVE,
        "getUnsafe",
        JDK_GET_UNSAFE_DESCRIPTOR,
        None,
    );
    for (name, descriptor) in [
        ("allocateInstance", ALLOCATE_INSTANCE_DESCRIPTOR),
        (
            "ensureClassInitialized",
            ENSURE_CLASS_INITIALIZED_DESCRIPTOR,
        ),
        ("shouldBeInitialized", SHOULD_BE_INITIALIZED_DESCRIPTOR),
        ("invokeCleaner", INVOKE_CLEANER_DESCRIPTOR),
        ("objectFieldOffset", OBJECT_FIELD_OFFSET_DESCRIPTOR),
    ] {
        builder.method(JDK_INSTANCE_NATIVE, name, descriptor, None);
    }
    builder.build()
}
