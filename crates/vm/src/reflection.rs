//! Basic `Object`/`Class` reflection natives (M2.4).
//!
//! The surface here is deliberately the six class-object methods plus
//! `Object.getClass`: names, modifiers, interface/array flags, the superclass and
//! bootstrap `Class.forName`. `Field`, `Method` and `Constructor` objects, annotation
//! proxies and generic types are later slices; nothing in this module builds them.
//!
//! The natives read the class table rather than caching a second copy of the metadata:
//! a class object is only a [`crate::ClassId`], and every answer comes from that
//! class's parsed `ClassFile`. `Class.forName` loads and allocates the class object; it
//! does not initialize the class, because the C1 stubs have no observable `<clinit>`.

use crate::VmError;
use crate::heap::{HeapError, ObjRef};
use crate::loader::{ClassId, LoaderId};
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `Modifier.classModifiers()`: the class-level bits `Class.getModifiers()` exposes.
///
/// `ACC_SUPER` (`0x0020`) is a class-file flag rather than a Java modifier, so it is
/// masked out: `String` has class-file flags `0x0031` and reports `0x0011`
/// (`public final`), just as on the reference JDK.
const CLASS_MODIFIERS: u16 = 0x7E1F;

/// `ACC_INTERFACE` (JVMS §4.1).
const ACC_INTERFACE: u16 = 0x0200;

/// Registers the M2.4 class-object natives.
///
/// They are keyed by the class that declares the Java method, not by the receiver's
/// runtime class, so virtual dispatch must resolve to `java/lang/Object` for
/// `getClass` and to `java/lang/Class` for the rest before it looks here.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        "java/lang/Object",
        "getClass",
        "()Ljava/lang/Class;",
        object_get_class,
    );
    shared.register_native(
        "java/lang/Class",
        "getName",
        "()Ljava/lang/String;",
        class_get_name,
    );
    shared.register_native(
        "java/lang/Class",
        "getModifiers",
        "()I",
        class_get_modifiers,
    );
    shared.register_native("java/lang/Class", "isInterface", "()Z", class_is_interface);
    shared.register_native("java/lang/Class", "isArray", "()Z", class_is_array);
    shared.register_native(
        "java/lang/Class",
        "getSuperclass",
        "()Ljava/lang/Class;",
        class_get_superclass,
    );
    shared.register_native(
        "java/lang/Class",
        "forName",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        class_for_name,
    );
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn class_of(shared: &Shared, class: ClassId) -> Result<ClassEntryView, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let view = ClassEntryView {
        loader: entry.loader,
        name: entry.name.replace('/', "."),
        access_flags: entry.file.access_flags,
        super_name: entry
            .file
            .super_class_name()
            .map_err(VmError::ClassFile)?
            .map(str::to_owned),
    };
    Ok(view)
}

/// A copy of the few class-file facts the natives need, taken without holding the
/// class-table lock or a heap guard.
struct ClassEntryView {
    loader: LoaderId,
    name: String,
    access_flags: u16,
    super_name: Option<String>,
}

fn receiver_class(shared: &Shared, receiver: ObjRef) -> Result<ClassId, VmError> {
    let class = {
        let heap = shared.heap();
        heap.class(receiver).map_err(heap_error)?
    };
    Ok(class)
}

fn object_get_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.get_class(receiver)?;
    Ok(Some(Value::Ref(class)))
}

fn class_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let view = class_of(shared, receiver_class(shared, receiver)?)?;
    let name = shared.heap().allocate_string(view.name);
    Ok(Some(Value::Ref(name)))
}

fn class_get_modifiers(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let view = class_of(shared, receiver_class(shared, receiver)?)?;
    Ok(Some(Value::Int(i32::from(
        view.access_flags & CLASS_MODIFIERS,
    ))))
}

fn class_is_interface(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let view = class_of(shared, receiver_class(shared, receiver)?)?;
    let flag = view.access_flags & ACC_INTERFACE != 0;
    Ok(Some(Value::Int(i32::from(flag))))
}

/// `Class.isArray()`: always false today.
///
/// Only named classes have class objects in M2.4; arrays have no class identity yet, so
/// no `Class` object can represent one and this native cannot observe a true case.
fn class_is_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // Validate the receiver even though the answer is always false, so a wrong receiver
    // is an error rather than a silently false answer.
    let _class = receiver_class(shared, receiver)?;
    Ok(Some(Value::Int(0)))
}

fn class_get_superclass(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let view = class_of(shared, receiver_class(shared, receiver)?)?;
    // JDK semantics: an interface, like `Object`, reports no superclass even though the
    // class file names `java/lang/Object` as the interface's superclass.
    let Some(super_name) = view
        .super_name
        .filter(|_| view.access_flags & ACC_INTERFACE == 0)
    else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let super_class = shared.load_class(view.loader, &super_name)?;
    let object = shared.class_object(super_class)?;
    Ok(Some(Value::Ref(object)))
}

fn class_for_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Err(VmError::Native(
            "Class.forName expects a String argument".to_owned(),
        ));
    };
    let name = {
        let heap = shared.heap();
        heap.string(reference).map_err(heap_error)?.to_owned()
    };

    // `Class.forName` takes a binary name (`java.util.List`), not an internal one. Array
    // class names (`[Ljava.lang.String;`, `[I`) need the array identity this slice does
    // not have, so they fail loudly instead of loading a class that does not exist.
    if name.starts_with('[') {
        return Err(VmError::Native(format!(
            "Class.forName does not support array class names yet: {name}"
        )));
    }
    let internal = name.replace('.', "/");
    let class = shared.load_library_class(&internal)?;
    let object = shared.class_object(class)?;
    Ok(Some(Value::Ref(object)))
}
