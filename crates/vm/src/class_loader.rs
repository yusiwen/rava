//! `java.lang.ClassLoader` natives (M3.3).
//!
//! The C1 stubs carry no `Code`, so every method a loader actually runs is one of these
//! natives. The loader *identity* lives in [`crate::Shared`]: one [`crate::LoaderId`] per
//! source plus one child per user `ClassLoader` instance, and one `java.lang.ClassLoader`
//! object per id. A constructor only checks its receiver: the object is the identity, so
//! the declared fields (`parent`, `name`) are not used.
//!
//! `loadClass` is parent-first through [`crate::Shared::load_class`]; `defineClass` records
//! the class under the receiver's own loader, which is what lets Spring's
//! `OverridingClassLoader` and CGLIB's fallback define a class without a class file on the
//! classpath. Resources and packages are not modelled (M4): `getResource` is null and
//! `getResources` is an empty `java.util.Enumeration`.

use rava_classfile::ClassFile;

use crate::VmError;
use crate::exceptions;
use crate::heap::{ArrayKind, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `java.lang.ClassLoader`'s internal name.
const CLASS_LOADER_CLASS: &str = "java/lang/ClassLoader";

/// Install the `java.lang.ClassLoader` natives.
pub fn register_natives(shared: &Shared) {
    for descriptor in [
        "()V",
        "(Ljava/lang/ClassLoader;)V",
        "(Ljava/lang/String;Ljava/lang/ClassLoader;)V",
    ] {
        shared.register_native(CLASS_LOADER_CLASS, "<init>", descriptor, class_loader_init);
    }
    for (name, descriptor, function) in [
        (
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            class_loader_load_class as crate::NativeFn,
        ),
        (
            "loadClass",
            "(Ljava/lang/String;Z)Ljava/lang/Class;",
            class_loader_load_class,
        ),
        (
            "findLoadedClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            class_loader_find_loaded_class,
        ),
        (
            "findSystemClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            class_loader_find_system_class,
        ),
        (
            "defineClass",
            "(Ljava/lang/String;[BII)Ljava/lang/Class;",
            class_loader_define_class,
        ),
        (
            "defineClass",
            "(Ljava/lang/String;[BIILjava/security/ProtectionDomain;)Ljava/lang/Class;",
            class_loader_define_class_with_domain,
        ),
        (
            "defineClass",
            "([BII)Ljava/lang/Class;",
            class_loader_define_class_unnamed,
        ),
        (
            "resolveClass",
            "(Ljava/lang/Class;)V",
            class_loader_resolve_class,
        ),
        (
            "getParent",
            "()Ljava/lang/ClassLoader;",
            class_loader_get_parent,
        ),
        ("getName", "()Ljava/lang/String;", class_loader_get_name),
        (
            "getSystemClassLoader",
            "()Ljava/lang/ClassLoader;",
            class_loader_get_system,
        ),
        (
            "getPlatformClassLoader",
            "()Ljava/lang/ClassLoader;",
            class_loader_get_system,
        ),
        (
            "getContextClassLoader",
            "()Ljava/lang/ClassLoader;",
            class_loader_get_system,
        ),
        (
            "setContextClassLoader",
            "(Ljava/lang/ClassLoader;)V",
            class_loader_noop_receiver,
        ),
        ("registerAsParallelCapable", "()Z", class_loader_true),
        ("isRegisteredAsParallelCapable", "()Z", class_loader_true),
        (
            "getResource",
            "(Ljava/lang/String;)Ljava/net/URL;",
            class_loader_null,
        ),
        (
            "getResources",
            "(Ljava/lang/String;)Ljava/util/Enumeration;",
            class_loader_empty_enumeration,
        ),
        (
            "getSystemResource",
            "(Ljava/lang/String;)Ljava/net/URL;",
            class_loader_null,
        ),
        (
            "getSystemResources",
            "(Ljava/lang/String;)Ljava/util/Enumeration;",
            class_loader_empty_enumeration,
        ),
        (
            "getResourceAsStream",
            "(Ljava/lang/String;)Ljava/io/InputStream;",
            class_loader_null,
        ),
        (
            "getSystemResourceAsStream",
            "(Ljava/lang/String;)Ljava/io/InputStream;",
            class_loader_null,
        ),
    ] {
        shared.register_native(CLASS_LOADER_CLASS, name, descriptor, function);
    }
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// A non-null `String` argument.
fn string_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    method: &str,
) -> Result<String, VmError> {
    match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            let heap = shared.heap();
            Ok(heap.string(reference).map_err(heap_error)?.to_owned())
        }
        _ => Err(VmError::Native(format!(
            "{method} expects a non-null String argument"
        ))),
    }
}

/// An `int` argument.
fn int_argument(args: &[Value], index: usize, method: &str) -> Result<i32, VmError> {
    match args.get(index).copied() {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(format!(
            "{method} expects an int argument at position {index}"
        ))),
    }
}

/// A `ClassNotFoundException` carrying `name` as its detail message.
fn class_not_found(shared: &Shared, name: &str) -> VmError {
    match exceptions::new_throwable(shared, "java/lang/ClassNotFoundException", name.to_owned()) {
        Ok(object) => VmError::Thrown(object),
        Err(error) => error,
    }
}

/// The dotted name a `loadClass`/`findLoadedClass` argument carries, as an internal name.
fn internal_name(name: &str) -> String {
    name.replace('.', "/")
}

/// `ClassLoader.<init>(...)`: a no-op on a non-null receiver.
///
/// The receiver object *is* the loader identity; the declared `parent` and `name` fields
/// are not used, so a constructor has nothing to store.
fn class_loader_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "ClassLoader.<init> called with a null receiver".to_owned(),
        ));
    }
    Ok(None)
}

/// `ClassLoader.loadClass(String[, boolean])`: parent-first delegation, then the
/// receiver's own source.
///
/// The `resolve` flag controls linking on the JDK; this VM resolves symbols lazily, so it
/// is accepted and ignored. A miss is a real `ClassNotFoundException` object, so compiled
/// code can catch it.
fn class_loader_load_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let loader = shared.loader_id_for(receiver)?;
    let name = string_argument(shared, args, 0, "ClassLoader.loadClass")?;
    let internal = internal_name(&name);
    let class = match shared.load_class(loader, &internal) {
        Ok(class) => class,
        Err(VmError::ClassNotFound { .. }) => return Err(class_not_found(shared, &name)),
        Err(error) => return Err(error),
    };
    Ok(Some(Value::Ref(shared.class_object(class)?)))
}

/// `ClassLoader.findLoadedClass(String)`: a lookup only, never a delegation or a read.
fn class_loader_find_loaded_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let loader = shared.loader_id_for(receiver)?;
    let name = string_argument(shared, args, 0, "ClassLoader.findLoadedClass")?;
    match shared.lookup(loader, &internal_name(&name)) {
        Some(class) => Ok(Some(Value::Ref(shared.class_object(class)?))),
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

/// `ClassLoader.findSystemClass(String)`: the application loader's parent-first lookup.
fn class_loader_find_system_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    // The receiver must still be a loader even though the lookup uses the system loader.
    shared.loader_id_for(receiver)?;
    let name = string_argument(shared, args, 0, "ClassLoader.findSystemClass")?;
    let internal = internal_name(&name);
    let class = match shared.load_class(shared.application_loader(), &internal) {
        Ok(class) => class,
        Err(VmError::ClassNotFound { .. }) => return Err(class_not_found(shared, &name)),
        Err(error) => return Err(error),
    };
    Ok(Some(Value::Ref(shared.class_object(class)?)))
}

/// `ClassLoader.resolveClass(Class)`: accepted and ignored.
///
/// The VM links a class when it is first used, so there is no second resolution step.
fn class_loader_resolve_class(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

/// `ClassLoader.getParent()`: the parent's loader object, or null for the application
/// loader (this VM has no platform loader) and for the bootstrap loader.
fn class_loader_get_parent(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let loader = shared.loader_id_for(receiver)?;
    let parent = if loader == shared.application_loader() {
        None
    } else {
        shared
            .loader_parent(loader)
            .filter(|parent| *parent != LoaderId::BOOTSTRAP)
    };
    match parent {
        Some(parent) => Ok(Some(Value::Ref(shared.loader_object(parent)?))),
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

/// `ClassLoader.getName()`: the stable loader name from [`Shared::loader_name`].
fn class_loader_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let loader = shared.loader_id_for(receiver)?;
    let name = shared.loader_name(loader);
    Ok(Some(Value::Ref(shared.intern_string(&name))))
}

/// `ClassLoader.getSystemClassLoader()` and `getPlatformClassLoader()`.
///
/// Both return the application loader object: this VM has one application loader and no
/// separate platform loader, and the JDK contract only requires a non-null loader.
fn class_loader_get_system(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(shared.class_loader_object()?)))
}

fn class_loader_noop_receiver(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn class_loader_true(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(1)))
}

fn class_loader_null(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

/// `ClassLoader.getResources(String)`: an empty enumeration.
///
/// There is no resource model until M4, so the only truthful answer is an enumeration
/// with no elements. The C1 `java.util.Enumeration` natives already serve it.
fn class_loader_empty_enumeration(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.heap().allocate_enumeration(Vec::new()),
    )))
}

/// `ClassLoader.defineClass(String, byte[], int, int)`: the named overload.
fn class_loader_define_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    define_class(shared, receiver, args, true)
}

/// `ClassLoader.defineClass(String, byte[], int, int, ProtectionDomain)`.
///
/// The protection domain is ignored: the VM performs no access control, so it adds no
/// identity or behaviour to the defined class.
fn class_loader_define_class_with_domain(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    define_class(shared, receiver, args, true)
}

/// `ClassLoader.defineClass(byte[], int, int)`: derive the name from the class bytes.
fn class_loader_define_class_unnamed(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    define_class(shared, receiver, args, false)
}

/// The shared body of the three `defineClass` overloads.
///
/// `has_name` says whether the first argument is the `String` name.
fn define_class(
    shared: &Shared,
    receiver: ObjRef,
    args: &[Value],
    has_name: bool,
) -> Result<Option<Value>, VmError> {
    let loader = shared.loader_id_for(receiver)?;
    // The named overloads accept a null name, in which case the class-file header supplies
    // it; `defineClass(byte[], int, int)` has no name argument at all.
    let name = if has_name {
        match args.first().copied() {
            Some(Value::Ref(reference)) if !reference.is_null() => Some(
                shared
                    .heap()
                    .string(reference)
                    .map_err(heap_error)?
                    .to_owned(),
            ),
            Some(Value::Ref(_)) => None,
            _ => {
                return Err(VmError::Native(
                    "ClassLoader.defineClass expects a String name or null".to_owned(),
                ));
            }
        }
    } else {
        None
    };
    let (bytes_index, offset_index, length_index) = if has_name { (1, 2, 3) } else { (0, 1, 2) };
    let array = match args.get(bytes_index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "ClassLoader.defineClass expects a non-null byte[]".to_owned(),
            ));
        }
    };
    let offset = int_argument(args, offset_index, "ClassLoader.defineClass")?;
    let length = int_argument(args, length_index, "ClassLoader.defineClass")?;
    let bytes = copy_byte_range(shared, array, offset, length)?;

    let internal = match name {
        // The C1 stubs and the JDK accept an unnamed definition; the class-file header
        // carries the name.
        None => ClassFile::parse(&bytes)?.this_class_name()?.to_owned(),
        Some(name) => internal_name(&name),
    };
    let class = shared.define_class(loader, &internal, &bytes)?;
    Ok(Some(Value::Ref(shared.class_object(class)?)))
}

/// Copy `[offset, offset + length)` out of a heap `byte[]`.
///
/// Every failure is a typed [`VmError::Native`]: a null array, an array of another kind, a
/// negative offset or length, a range that runs past the end, or an element that is not
/// the `int` a `byte` slot stores.
fn copy_byte_range(
    shared: &Shared,
    reference: ObjRef,
    offset: i32,
    length: i32,
) -> Result<Vec<u8>, VmError> {
    let elements = {
        let heap = shared.heap();
        let array = heap.array(reference).map_err(heap_error)?;
        if array.kind != ArrayKind::Byte {
            return Err(VmError::Native(format!(
                "ClassLoader.defineClass expects a byte[], found a {}[]",
                array.kind
            )));
        }
        array.elements.clone()
    };
    let offset = usize::try_from(offset).map_err(|_| {
        VmError::Native(format!(
            "ClassLoader.defineClass offset {offset} is negative"
        ))
    })?;
    let length = usize::try_from(length).map_err(|_| {
        VmError::Native(format!(
            "ClassLoader.defineClass length {length} is negative"
        ))
    })?;
    let end = offset.checked_add(length).ok_or_else(|| {
        VmError::Native("ClassLoader.defineClass range overflows the byte[] length".to_owned())
    })?;
    if end > elements.len() {
        return Err(VmError::Native(format!(
            "ClassLoader.defineClass range {offset}..{end} is outside a byte[] of length {}",
            elements.len()
        )));
    }
    let mut bytes = Vec::with_capacity(length);
    for element in &elements[offset..end] {
        let Value::Int(value) = *element else {
            return Err(VmError::Native(
                "ClassLoader.defineClass found a non-byte element in the byte[]".to_owned(),
            ));
        };
        // `bastore` keeps a Java `byte` sign-extended, while `String.getBytes` stores the
        // unsigned value; both spellings are accepted and masked to the low eight bits.
        if !(-128..=255).contains(&value) {
            return Err(VmError::Native(format!(
                "ClassLoader.defineClass found {value} in the byte[], which is not a byte"
            )));
        }
        bytes.push((value & 0xff) as u8);
    }
    Ok(bytes)
}
