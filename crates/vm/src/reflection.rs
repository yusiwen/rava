//! `Object`/`Class` reflection natives (M2.4), reflected members (M2.8), field access
//! (M2.10), named member lookup (M2.14), primitive types (M2.15) and array types (M2.16).
//!
//! The surface here is deliberately small. M2.4 supplies the six class-object methods
//! plus `Object.getClass`: names, modifiers, interface/array flags, the superclass and
//! bootstrap `Class.forName`. M2.8 adds the declared-member queries and read-only
//! `Field`, `Method` and `Constructor` objects: the declaring class, name, modifiers,
//! field type, return type, parameter types and parameter count. M2.10 adds `Field.get`
//! and `Field.set`; M2.25 extends both to every primitive kind, boxing a field read
//! through its own wrapper and unboxing a wrapper on write (with Java's widening rule for
//! a numeric value). M2.14 adds `getDeclaredField`/`getField`,
//! `getDeclaredMethod`/`getMethod` and `getDeclaredConstructor`/`getConstructor`: a
//! query matches a field by name and an executable by name plus parameter descriptors,
//! `get*` walks the superclass chain and interfaces while `getDeclared*` stays on the
//! receiver, and every hit is the same cached member object the `getDeclared*`
//! array queries return. M2.30 makes a miss the JDK's typed exception: a field query
//! throws `NoSuchFieldException` and a method or constructor query throws
//! `NoSuchMethodException`, with the receiver class, member name and parameter
//! descriptors in the message (see [`lookup_miss`]). M2.15 adds primitive class objects: `Class.forName("int")`
//! returns the cached [`crate::HeapObject::PrimitiveClass`] object, `getName`,
//! `getModifiers`, `isPrimitive`, `isArray`, `getSuperclass` and `getComponentType`
//! answer for it, a primitive descriptor resolves to it in `Field.getType`,
//! `Method.getReturnType`/`getParameterTypes` and `Constructor.getParameterTypes`, and a
//! `Class[]` of primitive class objects builds the primitive descriptors a named lookup
//! matches. M2.16 adds array class objects: the cached
//! [`crate::HeapObject::ArrayClass`] object for a component, its descriptor-form
//! `getName`, `0x0411` modifiers, `isArray`, its component and `java.lang.Object`
//! superclass, array descriptors in the same member types, and array class objects in a
//! named lookup's `Class[]`. M2.28 adds the class-file predicate flags Spring's startup
//! asks for: `Method.isBridge`, `Method`/`Constructor`/`Executable.isVarArgs` and
//! `Field`/`Method`/`Constructor`/`Executable`/`Member.isSynthetic`, each read from the
//! declaration's `ACC_BRIDGE`/`ACC_VARARGS`/`ACC_SYNTHETIC` bits. `Method.invoke` and
//! `Constructor.newInstance` need the interpreter's execution machinery, so they live in
//! `rava-interp` instead of this crate; annotations, generic types and access checks
//! remain later slices.
//!
//! The natives read the class table rather than caching a second copy of the metadata:
//! a class object is a [`crate::ClassId`], a [`crate::PrimitiveKind`] or an
//! [`crate::ArrayClass`], and every answer comes from that class's parsed `ClassFile`,
//! the primitive kind or the array component. A reflected member is only a
//! [`crate::ReflectMember`] (declaring class, kind, name and descriptor), and its
//! modifiers and types come from the same typed C1 metadata. `Class.forName` loads and
//! allocates the class object; it does not initialize the class, because the C1 stubs
//! have no observable `<clinit>`.

use std::collections::HashSet;

use crate::VmError;
use crate::boxed;
use crate::heap::{
    ArrayClass, ArrayKind, ClassRef, HeapError, HeapObject, MemberKind, ObjRef, PrimitiveKind,
    ReflectMember,
};
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

/// `Modifier.fieldModifiers()`: `public` through `volatile`, plus `synthetic` and `enum`.
const FIELD_MODIFIERS: u16 = 0x50DF;

/// `Modifier.methodModifiers()`: the method bits, including `bridge`, `varargs` and
/// `synthetic`.
const METHOD_MODIFIERS: u16 = 0x1DFF;

/// `Modifier.constructorModifiers()`: only the three access bits are Java modifiers.
const CONSTRUCTOR_MODIFIERS: u16 = 0x0007;

/// `Class.getModifiers()` for an array class: `Modifier.PUBLIC | Modifier.ABSTRACT |
/// Modifier.FINAL`, the same `0x0411` every primitive reports on the reference JDK.
const ARRAY_MODIFIERS: u16 = 0x0411;

/// `ACC_PUBLIC` (JVMS §4.1).
const ACC_PUBLIC: u16 = 0x0001;

/// `ACC_INTERFACE` (JVMS §4.1).
const ACC_INTERFACE: u16 = 0x0200;

/// `ACC_BRIDGE` (JVMS §4.6): a compiler-generated bridge method.
const ACC_BRIDGE: u16 = 0x0040;

/// `ACC_VARARGS` (JVMS §4.6): the executable was declared with a variable arity.
const ACC_VARARGS: u16 = 0x0080;

/// `ACC_SYNTHETIC` (JVMS §4.1, §4.5, §4.6): the member does not appear in source.
const ACC_SYNTHETIC: u16 = 0x1000;

/// Registers the M2.4 class-object and M2.8 reflected-member natives.
///
/// They are keyed by the class that declares the Java method, not by the receiver's
/// runtime class, so virtual dispatch must resolve to `java/lang/Object` for
/// `getClass`, to `java/lang/Class` for the class queries and to
/// `java/lang/reflect/Field`, `Method` or `Constructor` for the member methods before
/// it looks here.
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
    shared.register_native("java/lang/Class", "isPrimitive", "()Z", class_is_primitive);
    shared.register_native(
        "java/lang/Class",
        "getComponentType",
        "()Ljava/lang/Class;",
        class_get_component_type,
    );
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
    shared.register_native(
        "java/lang/Class",
        "forName",
        "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;",
        class_for_name_initialized,
    );
    shared.register_native(
        "java/lang/Class",
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        class_get_class_loader,
    );
    shared.register_native(
        "java/lang/Class",
        "isAssignableFrom",
        "(Ljava/lang/Class;)Z",
        class_is_assignable_from,
    );
    shared.register_native(
        "java/lang/Class",
        "isInstance",
        "(Ljava/lang/Object;)Z",
        class_is_instance,
    );
    shared.register_native(
        "java/lang/reflect/Array",
        "newInstance",
        "(Ljava/lang/Class;I)Ljava/lang/Object;",
        array_new_instance,
    );
    shared.register_native(
        "java/lang/reflect/Array",
        "getLength",
        "(Ljava/lang/Object;)I",
        array_get_length,
    );
    shared.register_native(
        "java/lang/Class",
        "getResource",
        "(Ljava/lang/String;)Ljava/net/URL;",
        class_null_resource,
    );
    shared.register_native(
        "java/lang/Class",
        "getResourceAsStream",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        class_null_resource,
    );
    register_modifier_natives(shared);
    for owner in [
        "java/lang/reflect/Field",
        "java/lang/reflect/Method",
        "java/lang/reflect/Constructor",
    ] {
        shared.register_native(owner, "hashCode", "()I", member_hash_code);
        shared.register_native(owner, "equals", "(Ljava/lang/Object;)Z", member_equals);
    }
    shared.register_native(
        "java/lang/Class",
        "isAnnotation",
        "()Z",
        class_is_annotation,
    );
    shared.register_native("java/lang/Class", "isEnum", "()Z", class_is_enum);
    shared.register_native("java/lang/Class", "isSynthetic", "()Z", class_is_synthetic);
    shared.register_native("java/lang/Class", "isRecord", "()Z", class_is_false);
    shared.register_native("java/lang/Class", "isSealed", "()Z", class_is_false);
    shared.register_native(
        "java/lang/Class",
        "componentType",
        "()Ljava/lang/Class;",
        class_get_component_type,
    );
    shared.register_native(
        "java/lang/Class",
        "getInterfaces",
        "()[Ljava/lang/Class;",
        class_get_interfaces,
    );
    shared.register_native(
        "java/lang/Class",
        "getSimpleName",
        "()Ljava/lang/String;",
        class_get_simple_name,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredClasses",
        "()[Ljava/lang/Class;",
        class_get_declared_classes,
    );
    shared.register_native(
        "java/lang/Class",
        "getClasses",
        "()[Ljava/lang/Class;",
        class_get_classes,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredFields",
        "()[Ljava/lang/reflect/Field;",
        class_get_declared_fields,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredMethods",
        "()[Ljava/lang/reflect/Method;",
        class_get_declared_methods,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredConstructors",
        "()[Ljava/lang/reflect/Constructor;",
        class_get_declared_constructors,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredField",
        "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
        class_get_declared_field,
    );
    shared.register_native(
        "java/lang/Class",
        "getField",
        "(Ljava/lang/String;)Ljava/lang/reflect/Field;",
        class_get_field,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredMethod",
        "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        class_get_declared_method,
    );
    shared.register_native(
        "java/lang/Class",
        "getMethod",
        "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        class_get_method,
    );
    shared.register_native(
        "java/lang/Class",
        "getDeclaredConstructor",
        "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        class_get_declared_constructor,
    );
    shared.register_native(
        "java/lang/Class",
        "getConstructor",
        "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
        class_get_constructor,
    );

    // `Field` and `Method` declare `getName` themselves; `Constructor` overrides it with
    // the binary class name, exactly as the JDK does.
    for owner in ["java/lang/reflect/Field", "java/lang/reflect/Method"] {
        shared.register_native(owner, "getName", "()Ljava/lang/String;", member_get_name);
    }
    for owner in [
        "java/lang/reflect/Field",
        "java/lang/reflect/Method",
        "java/lang/reflect/Constructor",
    ] {
        shared.register_native(owner, "getModifiers", "()I", member_get_modifiers);
        shared.register_native(
            owner,
            "getDeclaringClass",
            "()Ljava/lang/Class;",
            member_get_declaring_class,
        );
    }
    // The class-file predicate flags. Each declaration site gets its own registration
    // because virtual dispatch resolves to the class that declares the stub method: the
    // JDK declares `isSynthetic` on `Member` and `Executable`, redeclares it on `Field`,
    // `Method` and `Constructor`, and declares `isVarArgs` on `Executable`, `Method`
    // and `Constructor`. The flags themselves come from the parsed class-file metadata,
    // never from the descriptor or the name.
    for owner in [
        "java/lang/reflect/Field",
        "java/lang/reflect/Method",
        "java/lang/reflect/Constructor",
        "java/lang/reflect/Executable",
        "java/lang/reflect/Member",
    ] {
        shared.register_native(owner, "isSynthetic", "()Z", member_is_synthetic);
    }
    for owner in [
        "java/lang/reflect/Method",
        "java/lang/reflect/Constructor",
        "java/lang/reflect/Executable",
    ] {
        shared.register_native(owner, "isVarArgs", "()Z", member_is_var_args);
    }
    shared.register_native(
        "java/lang/reflect/Method",
        "isBridge",
        "()Z",
        method_is_bridge,
    );
    shared.register_native(
        "java/lang/reflect/Field",
        "getType",
        "()Ljava/lang/Class;",
        field_get_type,
    );
    shared.register_native(
        "java/lang/reflect/Field",
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        field_get,
    );
    shared.register_native(
        "java/lang/reflect/Field",
        "set",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
        field_set,
    );
    shared.register_native(
        "java/lang/reflect/Method",
        "getReturnType",
        "()Ljava/lang/Class;",
        method_get_return_type,
    );
    for owner in ["java/lang/reflect/Method", "java/lang/reflect/Constructor"] {
        shared.register_native(
            owner,
            "getParameterTypes",
            "()[Ljava/lang/Class;",
            executable_get_parameter_types,
        );
        shared.register_native(
            owner,
            "getParameterCount",
            "()I",
            executable_get_parameter_count,
        );
    }
    shared.register_native(
        "java/lang/reflect/Constructor",
        "getName",
        "()Ljava/lang/String;",
        constructor_get_name,
    );
    // `AccessibleObject` declares both accessibility methods in the C1 stub, and every
    // concrete member inherits them there. The flag is bookkeeping only: this VM has no
    // access-control check.
    shared.register_native(
        "java/lang/reflect/AccessibleObject",
        "isAccessible",
        "()Z",
        accessible_is_accessible,
    );
    shared.register_native(
        "java/lang/reflect/AccessibleObject",
        "setAccessible",
        "(Z)V",
        accessible_set_accessible,
    );
    // `Field`, `Method` and `Constructor` redeclare `setAccessible` in the C1 stub, so
    // each declaration site needs the entry; `isAccessible` stays inherited.
    for owner in [
        "java/lang/reflect/Field",
        "java/lang/reflect/Method",
        "java/lang/reflect/Constructor",
    ] {
        shared.register_native(owner, "setAccessible", "(Z)V", accessible_set_accessible);
    }
}

/// `AccessibleObject.isAccessible()`: whether `setAccessible(true)` was recorded.
fn accessible_is_accessible(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "AccessibleObject.isAccessible called with a null receiver".to_owned(),
        ));
    }
    Ok(Some(Value::Int(i32::from(shared.is_accessible(receiver)))))
}

/// `AccessibleObject.setAccessible(Z)`: record the flag for this member and return void.
fn accessible_set_accessible(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let accessible = match args.first().copied() {
        Some(Value::Int(value)) => value != 0,
        _ => {
            return Err(VmError::Native(
                "AccessibleObject.setAccessible expects a boolean argument".to_owned(),
            ));
        }
    };
    if receiver.is_null() {
        return Err(VmError::Native(
            "AccessibleObject.setAccessible called with a null receiver".to_owned(),
        ));
    }
    shared.set_accessible(receiver, accessible);
    Ok(None)
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
    access_flags: u16,
    super_name: Option<String>,
}

/// What a `Class` receiver denotes: a loaded class, one of the nine primitives, or an
/// array class.
pub(crate) enum ClassObject {
    /// A loaded class, identified by its `(loader, name)` class id.
    Named(ClassId),
    /// A primitive type, which has no class file and no loader.
    Primitive(PrimitiveKind),
    /// An array class, identified by its component.
    Array(ArrayClass),
}

impl ClassObject {
    /// The class ref this receiver denotes.
    pub(crate) fn class_ref(&self) -> ClassRef {
        match self {
            Self::Named(class) => ClassRef::Named(*class),
            Self::Primitive(kind) => ClassRef::Primitive(*kind),
            Self::Array(array) => ClassRef::Array(Box::new(array.clone())),
        }
    }
}

/// Classify a `Class` receiver.
///
/// A primitive class object and an array class object are distinct heap kinds, not a
/// `ClassId`, so every native starts here instead of calling [`crate::Heap::class`] and
/// failing on `int.class` or `int[].class`.
pub(crate) fn class_object_of(shared: &Shared, receiver: ObjRef) -> Result<ClassObject, VmError> {
    let heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        HeapObject::Class(class) => Ok(ClassObject::Named(*class)),
        HeapObject::PrimitiveClass(kind) => Ok(ClassObject::Primitive(*kind)),
        HeapObject::ArrayClass(array) => Ok(ClassObject::Array(array.clone())),
        _ => Err(VmError::Native(format!("{receiver} is not a Class object"))),
    }
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

/// `Class.getName()`: the dotted binary name of a loaded class, the primitive's Java name
/// (`int`, `void`), or the JDK's descriptor form for an array (`[I`, `[Ljava.lang.String;`).
fn class_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = class_object_of(shared, receiver)?;
    let name = shared.class_ref_name(&class.class_ref())?;
    let name = shared.heap().allocate_string(name);
    Ok(Some(Value::Ref(name)))
}

/// `Class.getModifiers()`: the Java modifier mask of a loaded class, or the fixed
/// `public abstract final` (`0x0411`) every primitive and array class reports on the JDK 17.
fn class_get_modifiers(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let modifiers = match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => {
            i32::from(class_of(shared, class)?.access_flags & CLASS_MODIFIERS)
        }
        ClassObject::Primitive(kind) => i32::from(kind.modifiers()),
        ClassObject::Array(_) => i32::from(ARRAY_MODIFIERS),
    };
    Ok(Some(Value::Int(modifiers)))
}

fn class_is_interface(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let flag = match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => class_of(shared, class)?.access_flags & ACC_INTERFACE != 0,
        ClassObject::Primitive(_) | ClassObject::Array(_) => false,
    };
    Ok(Some(Value::Int(i32::from(flag))))
}

/// `Class.isArray()`: true exactly for an array class object.
fn class_is_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let flag = matches!(class_object_of(shared, receiver)?, ClassObject::Array(_));
    Ok(Some(Value::Int(i32::from(flag))))
}

/// `Class.isPrimitive()`: true exactly for the nine primitive class objects.
fn class_is_primitive(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let flag = matches!(
        class_object_of(shared, receiver)?,
        ClassObject::Primitive(_)
    );
    Ok(Some(Value::Int(i32::from(flag))))
}

/// `Class.getComponentType()`: the component class object of an array, null otherwise.
fn class_get_component_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match class_object_of(shared, receiver)? {
        ClassObject::Array(array) => {
            let component = shared.class_object_for_ref(&array.component)?;
            Ok(Some(Value::Ref(component)))
        }
        ClassObject::Named(_) | ClassObject::Primitive(_) => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

fn class_get_superclass(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = class_object_of(shared, receiver)?;
    match class {
        ClassObject::Array(_) => {
            // Every array class extends `java.lang.Object`, for primitive and reference
            // component types alike.
            let object = shared.load_library_class("java/lang/Object")?;
            let object = shared.class_object(object)?;
            Ok(Some(Value::Ref(object)))
        }
        ClassObject::Primitive(_) => {
            // A primitive has no superclass on the JDK: `int.class.getSuperclass()` is null.
            Ok(Some(Value::Ref(ObjRef::NULL)))
        }
        ClassObject::Named(class) => {
            let view = class_of(shared, class)?;
            // JDK semantics: an interface, like `Object`, reports no superclass even though
            // the class file names `java/lang/Object` as the interface's superclass.
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
    }
}

/// `Class.forName(String)`: a loaded class object, the cached primitive class object for
/// a primitive name (`int`, `void`), or the cached array class object for an array name
/// (`[I`, `[[I`, `[Ljava.lang.String;`).
///
/// `Class.forName` takes a binary name (`java.util.List`), not an internal one; for an
/// array it takes the JDK's descriptor form with dots in an object component. The native
/// ABI receives the *declaring* class's loader, and `java.lang.Class` is a bootstrap class,
/// so this one-argument form resolves through the application loader.
fn class_for_name(
    shared: &Shared,
    _thread: &ThreadState,
    loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = for_name_argument(shared, args)?;
    let loader = if loader == LoaderId::BOOTSTRAP {
        shared.application_loader()
    } else {
        loader
    };
    resolve_for_name(shared, loader, &name)
}

/// The `String` argument of a `Class.forName` call.
fn for_name_argument(shared: &Shared, args: &[Value]) -> Result<String, VmError> {
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Err(VmError::Native(
            "Class.forName expects a String argument".to_owned(),
        ));
    };
    if reference.is_null() {
        return Err(VmError::Native(
            "Class.forName received a null name".to_owned(),
        ));
    }
    let heap = shared.heap();
    Ok(heap.string(reference).map_err(heap_error)?.to_owned())
}

/// Resolve a `Class.forName` name through one loader.
///
/// Primitive names and array descriptors have no class file, so they resolve from the
/// caches regardless of the loader; a named class goes through the ordinary parent-first
/// lookup. A miss is a real `ClassNotFoundException` object.
fn resolve_for_name(
    shared: &Shared,
    loader: LoaderId,
    name: &str,
) -> Result<Option<Value>, VmError> {
    if let Some(kind) = PrimitiveKind::from_name(name) {
        return Ok(Some(Value::Ref(shared.primitive_class(kind))));
    }
    if name.starts_with('[') {
        // A descriptor's object names are internal; `Class.forName` accepts the dotted
        // spelling, so a `[Ljava.lang.String;` argument becomes `[Ljava/lang/String;`.
        let descriptor = name.replace('.', "/");
        let class_ref = shared.class_ref_for_descriptor(loader, &descriptor)?;
        let object = shared.class_object_for_ref(&class_ref)?;
        return Ok(Some(Value::Ref(object)));
    }
    let internal = name.replace('.', "/");
    let class = match shared.load_class(loader, &internal) {
        Ok(class) => class,
        Err(VmError::ClassNotFound { .. }) => {
            return Err(class_not_found(shared, name));
        }
        Err(error) => return Err(error),
    };
    let object = shared.class_object(class)?;
    Ok(Some(Value::Ref(object)))
}

/// A `ClassNotFoundException` instance as the throwable `Class.forName` raises.
///
/// M2.23 needs this one exception object because `Class.forName` is how libraries probe
/// for optional dependencies and they catch the failure. The constructor is not run (the
/// C1 stub has no `Code`); the object exists so the interpreter's handler search can
/// match it by class, and it carries the requested name as its detail message.
fn class_not_found(shared: &Shared, name: &str) -> VmError {
    match crate::exceptions::new_throwable(
        shared,
        "java/lang/ClassNotFoundException",
        name.to_owned(),
    ) {
        Ok(object) => VmError::Thrown(object),
        Err(error) => error,
    }
}

/// `Class.forName(String, boolean, ClassLoader)`: resolve through the supplied loader.
///
/// The initialize flag is accepted and ignored: this VM initializes a class on first use,
/// not at `forName` time, and the C1 stubs have no initializer `Code` to run. A null
/// `ClassLoader` means the bootstrap loader, exactly as the JDK documents, so an
/// application class is not visible through it.
fn class_for_name_initialized(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = for_name_argument(shared, args)?;
    let loader = match args.get(2).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => shared.loader_id_for(reference)?,
        _ => LoaderId::BOOTSTRAP,
    };
    resolve_for_name(shared, loader, &name)
}

/// `Class.getClassLoader()`: the defining loader's object, or null for a bootstrap class
/// and for a primitive or array class.
fn class_get_class_loader(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let loader = match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => shared
            .entry(class)
            .map(|entry| entry.loader)
            .unwrap_or(LoaderId::BOOTSTRAP),
        ClassObject::Primitive(_) | ClassObject::Array(_) => LoaderId::BOOTSTRAP,
    };
    if loader == LoaderId::BOOTSTRAP {
        Ok(Some(Value::Ref(ObjRef::NULL)))
    } else {
        let object = shared.loader_object(loader)?;
        Ok(Some(Value::Ref(object)))
    }
}

/// `java.lang.reflect.Array.newInstance(Class, int)`.
fn array_new_instance(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(component)) = args.first().copied() else {
        return Err(VmError::Native(
            "Array.newInstance expects a Class component".to_owned(),
        ));
    };
    let length = match args.get(1).copied() {
        Some(Value::Int(length)) if length >= 0 => length as usize,
        _ => {
            return Err(VmError::Native(
                "Array.newInstance expects a non-negative int length".to_owned(),
            ));
        }
    };
    let class = class_object_of(shared, component)?;
    let reference = shared.heap().allocate_array(class.class_ref(), length);
    Ok(Some(Value::Ref(reference)))
}

/// `java.lang.reflect.Array.getLength(Object)`.
fn array_get_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Err(VmError::Native(
            "Array.getLength expects an array argument".to_owned(),
        ));
    };
    let length = shared
        .heap()
        .array(reference)
        .map_err(|error| VmError::Native(error.to_string()))?
        .elements
        .len();
    let length = i32::try_from(length)
        .map_err(|_| VmError::Native("array is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(length)))
}

/// `Class.isAssignableFrom(Class)`: is the argument's type this type or a subtype?
fn class_is_assignable_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = class_object_of(shared, receiver)?;
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Err(VmError::Native(
            "Class.isAssignableFrom expects a Class argument".to_owned(),
        ));
    };
    if reference.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    let candidate = class_object_of(shared, reference)?;
    Ok(Some(Value::Int(i32::from(class_object_assignable(
        shared, &target, &candidate,
    )?))))
}

/// `Class.isInstance(Object)`.
fn class_is_instance(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = class_object_of(shared, receiver)?;
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Ok(Some(Value::Int(0)));
    };
    if reference.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    // An annotation object stores its concrete annotation type even though
    // `get_class` reports the `java.lang.annotation.Annotation` interface (its
    // `invokevirtual` dispatch uses the annotation type); `isInstance` asks for the
    // concrete type, so the stored class is used here.
    let annotation_class = {
        let heap = shared.heap();
        match heap
            .get(reference)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::HeapObject::Annotation(annotation) => Some(annotation.class),
            _ => None,
        }
    };
    let candidate = match annotation_class {
        Some(class) => ClassObject::Named(class),
        None => {
            let object_class = shared.get_class(reference)?;
            class_object_of(shared, object_class)?
        }
    };
    Ok(Some(Value::Int(i32::from(class_object_assignable(
        shared, &target, &candidate,
    )?))))
}

/// `Class.getResource`/`getResourceAsStream`: no resource model until M4.
fn class_null_resource(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

/// A reflected member's identity hash: the same tuple `equals` compares.
fn member_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let mut hash = member.class.0 as i32;
    hash = hash.wrapping_mul(31).wrapping_add(member.kind as i32);
    for byte in member.name.bytes().chain(member.descriptor.bytes()) {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(byte));
    }
    Ok(Some(Value::Int(hash)))
}

/// A reflected member equals another member of the same kind and declaration.
fn member_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(other)) = args.first().copied() else {
        return Ok(Some(Value::Int(0)));
    };
    if other.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    let left = member_of(shared, receiver)?;
    let right = match shared.heap().get(other) {
        Ok(crate::HeapObject::Reflect(member)) => member.clone(),
        _ => return Ok(Some(Value::Int(0))),
    };
    let equal = left.class == right.class
        && left.kind == right.kind
        && left.name == right.name
        && left.descriptor == right.descriptor;
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `java.lang.reflect.Modifier` predicates over the class-file access bits.
///
/// Each bit gets its own function because a native is selected by `(owner, name,
/// descriptor)` and the ABI does not pass the name to the function.
fn register_modifier_natives(shared: &Shared) {
    macro_rules! modifier {
        ($name:ident, $mask:expr) => {
            fn $name(
                _shared: &Shared,
                _thread: &ThreadState,
                _loader: LoaderId,
                _receiver: ObjRef,
                args: &[Value],
            ) -> Result<Option<Value>, VmError> {
                let flags = match args.first().copied() {
                    Some(Value::Int(flags)) => flags,
                    _ => {
                        return Err(VmError::Native(
                            "Modifier predicate expects an int".to_owned(),
                        ))
                    }
                };
                Ok(Some(Value::Int(i32::from(flags & $mask != 0))))
            }
        };
    }
    modifier!(modifier_public, 0x0001);
    modifier!(modifier_private, 0x0002);
    modifier!(modifier_protected, 0x0004);
    modifier!(modifier_static, 0x0008);
    modifier!(modifier_final, 0x0010);
    modifier!(modifier_synchronized, 0x0020);
    modifier!(modifier_volatile, 0x0040);
    modifier!(modifier_transient, 0x0080);
    modifier!(modifier_native, 0x0100);
    modifier!(modifier_interface, 0x0200);
    modifier!(modifier_abstract, 0x0400);
    modifier!(modifier_strict, 0x0800);
    modifier!(modifier_synthetic, 0x1000);
    modifier!(modifier_annotation, 0x2000);
    modifier!(modifier_enum, 0x4000);
    modifier!(modifier_mandated, 0x8000);
    modifier!(modifier_bridge, 0x0040);
    modifier!(modifier_varargs, 0x0080);

    for (name, function) in [
        ("isPublic", modifier_public as crate::NativeFn),
        ("isPrivate", modifier_private),
        ("isProtected", modifier_protected),
        ("isStatic", modifier_static),
        ("isFinal", modifier_final),
        ("isSynchronized", modifier_synchronized),
        ("isVolatile", modifier_volatile),
        ("isTransient", modifier_transient),
        ("isNative", modifier_native),
        ("isInterface", modifier_interface),
        ("isAbstract", modifier_abstract),
        ("isStrict", modifier_strict),
        ("isSynthetic", modifier_synthetic),
        ("isAnnotation", modifier_annotation),
        ("isEnum", modifier_enum),
        ("isMandated", modifier_mandated),
        ("isBridge", modifier_bridge),
        ("isVarArgs", modifier_varargs),
    ] {
        shared.register_native("java/lang/reflect/Modifier", name, "(I)Z", function);
    }
}

fn class_flags(shared: &Shared, receiver: ObjRef) -> Result<u16, VmError> {
    match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => shared
            .entry(class)
            .map(|entry| entry.file.access_flags)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0))),
        ClassObject::Primitive(_) | ClassObject::Array(_) => Ok(0),
    }
}

const ACC_ANNOTATION: u16 = 0x2000;
const ACC_ENUM: u16 = 0x4000;

fn class_is_annotation(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(i32::from(
        class_flags(shared, receiver)? & ACC_ANNOTATION != 0,
    ))))
}

fn class_is_enum(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(i32::from(
        class_flags(shared, receiver)? & ACC_ENUM != 0,
    ))))
}

fn class_is_synthetic(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(i32::from(
        class_flags(shared, receiver)? & ACC_SYNTHETIC != 0,
    ))))
}

fn class_is_false(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(0)))
}

/// `Class.getInterfaces()`: the direct interfaces, in class-file order.
///
/// The C1 tree is the class library this VM ships; a transitive interface it omits (for
/// example `java.lang.constant.Constable` on `String`) cannot be reported. It is skipped,
/// the same rule M2.19's assignability walk and the interface-method search already use,
/// rather than turning every `String.class.getInterfaces()` into a loader error.
fn class_get_interfaces(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let arrays = match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => {
            let entry = shared
                .entry(class)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
            let mut objects = Vec::new();
            for name in entry.file.interface_names()? {
                match shared.load_class(entry.loader, name) {
                    Ok(interface) => objects.push(Value::Ref(shared.class_object(interface)?)),
                    Err(VmError::ClassNotFound { .. }) => {}
                    Err(error) => return Err(error),
                }
            }
            objects
        }
        ClassObject::Primitive(_) | ClassObject::Array(_) => Vec::new(),
    };
    let class = shared.load_library_class("java/lang/Class")?;
    let array = shared
        .heap()
        .allocate_array(ClassRef::Named(class), arrays.len());
    {
        let mut heap = shared.heap();
        let elements = heap.array_mut(array).expect("a freshly allocated array");
        for (slot, value) in elements.elements.iter_mut().zip(arrays) {
            *slot = value;
        }
    }
    Ok(Some(Value::Ref(array)))
}

/// `Class.getSimpleName()`.
fn class_get_simple_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => simple_name(shared, class)?,
        ClassObject::Primitive(kind) => kind.name().to_owned(),
        ClassObject::Array(array) => {
            let mut name = self::array_component_simple_name(shared, &array)?;
            name.push_str("[]");
            name
        }
    };
    let text = shared.intern_string(&name);
    Ok(Some(Value::Ref(text)))
}

/// The simple name of a loaded class: its binary name without the package and without
/// the enclosing class's `$` prefix. An anonymous class's numeric suffix is kept, which is
/// the same text `Class.getSimpleName` reports for it.
fn simple_name(shared: &Shared, class: ClassId) -> Result<String, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let binary = entry.name.rsplit('/').next().unwrap_or(&entry.name);
    Ok(match binary.rsplit_once('$') {
        Some((_, simple)) => simple.to_owned(),
        None => binary.to_owned(),
    })
}

fn array_component_simple_name(shared: &Shared, array: &ArrayClass) -> Result<String, VmError> {
    match &array.component {
        ClassRef::Named(class) => simple_name(shared, *class),
        ClassRef::Primitive(kind) => Ok(kind.name().to_owned()),
        ClassRef::Array(inner) => {
            let mut name = self::array_component_simple_name(shared, inner)?;
            name.push_str("[]");
            Ok(name)
        }
    }
}

/// Is `candidate` the same as `target`, a subtype of a named `target`, or an array type
/// assignable to `target`?
fn class_object_assignable(
    shared: &Shared,
    target: &ClassObject,
    candidate: &ClassObject,
) -> Result<bool, VmError> {
    match (target, candidate) {
        (ClassObject::Primitive(left), ClassObject::Primitive(right)) => Ok(left == right),
        (ClassObject::Named(left), ClassObject::Named(right)) => {
            named_assignable(shared, *left, *right)
        }
        (ClassObject::Named(target), ClassObject::Array(_)) => {
            let name = class_name(shared, *target);
            Ok(matches!(
                name.as_str(),
                "java/lang/Object" | "java/lang/Cloneable" | "java/io/Serializable"
            ))
        }
        (ClassObject::Array(target), ClassObject::Array(candidate)) => {
            array_assignable(target, candidate)
        }
        _ => Ok(false),
    }
}

/// Is `candidate` a supertype (class or interface) of `from`?
fn named_assignable(shared: &Shared, target: ClassId, from: ClassId) -> Result<bool, VmError> {
    use std::collections::HashSet;
    let mut stack = vec![from];
    let mut seen = HashSet::new();
    while let Some(current) = stack.pop() {
        if current == target {
            return Ok(true);
        }
        if !seen.insert(current) {
            continue;
        }
        let entry = shared
            .entry(current)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", current.0)))?;
        // The library walk skips a platform superclass the C1 tree does not ship (see
        // `Shared::resolution_superclass`), so assignability still reaches `Object`.
        if let Some(super_class) = shared.resolution_superclass(current)? {
            stack.push(super_class);
        }
        for interface_name in entry.file.interface_names()? {
            match shared.load_class(entry.loader, interface_name) {
                Ok(id) => stack.push(id),
                Err(VmError::ClassNotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }
    }
    Ok(false)
}

/// Array assignability: equal component types, or a reference component whose runtime
/// class is assignable, matching the M2.21 `instanceof` rule.
fn array_assignable(target: &ArrayClass, candidate: &ArrayClass) -> Result<bool, VmError> {
    match (&target.component, &candidate.component) {
        (ClassRef::Primitive(left), ClassRef::Primitive(right)) => Ok(left == right),
        (ClassRef::Array(left), ClassRef::Array(right)) => array_assignable(left, right),
        (ClassRef::Named(_), _) | (_, ClassRef::Named(_)) => {
            // Without a shared `Shared` handle here, the name-level check is left to the
            // interpreter's M2.21 path; equal references are still accepted.
            Ok(target.component == candidate.component)
        }
        _ => Ok(false),
    }
}

fn class_get_declared_fields(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = declared_members_of(shared, receiver, MemberKind::Field)?;
    Ok(Some(Value::Ref(array)))
}

fn class_get_declared_methods(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = declared_members_of(shared, receiver, MemberKind::Method)?;
    Ok(Some(Value::Ref(array)))
}

fn class_get_declared_constructors(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = declared_members_of(shared, receiver, MemberKind::Constructor)?;
    Ok(Some(Value::Ref(array)))
}

/// `Class.getDeclaredClasses()`: the member classes this class declares.
///
/// A member class is an `InnerClasses` row whose `outer_class` is this class. A local or
/// anonymous class stores a zero `outer_class` there, so the same filter excludes it, which
/// is the JDK's "declared as a member" rule. Each row's internal name is resolved through
/// this class's defining loader and returned as its cached class object. A primitive or
/// array class has no class file, so it declares no member class and gets an empty
/// `Class[]`. An empty result is an empty array, never a lookup exception.
fn class_get_declared_classes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let objects = match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => declared_class_objects(shared, class, false)?,
        ClassObject::Primitive(_) | ClassObject::Array(_) => Vec::new(),
    };
    let component = shared.load_library_class("java/lang/Class")?;
    let array = allocate_reference_array(shared, ClassRef::Named(component), &objects)?;
    Ok(Some(Value::Ref(array)))
}

/// `Class.getClasses()`: the public member classes of this class and its superclasses.
///
/// The JDK walks the superclass chain and keeps the public member classes; an interface
/// does not follow its class file's `Object` superclass, so only its own members count.
/// As with [`class_get_declared_classes`], a member class outside the C1 tree is skipped
/// and an empty result is an empty array.
fn class_get_classes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut objects = Vec::new();
    if let ClassObject::Named(class) = class_object_of(shared, receiver)? {
        let is_interface = {
            let entry = shared
                .entry(class)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
            entry.metadata()?.is_interface()
        };
        let mut current = Some(class);
        while let Some(class) = current {
            objects.extend(declared_class_objects(shared, class, true)?);
            current = if is_interface {
                None
            } else {
                shared.resolution_superclass(class)?
            };
        }
    }
    let component = shared.load_library_class("java/lang/Class")?;
    let array = allocate_reference_array(shared, ClassRef::Named(component), &objects)?;
    Ok(Some(Value::Ref(array)))
}

/// The cached class objects of the member classes `class` declares.
///
/// `public_only` filters on the `InnerClasses` row's access flags, which is the
/// declaration's public bit (the loaded class file's own header does not carry a member
/// class's private/protected flags). A row whose class the C1 tree omits is skipped: the
/// tree is the pinned manifest's subset, not the whole JDK.
fn declared_class_objects(
    shared: &Shared,
    class: ClassId,
    public_only: bool,
) -> Result<Vec<ObjRef>, VmError> {
    let (loader, binary_name, inner) = {
        let entry = shared
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let metadata = entry.metadata()?;
        (
            entry.loader,
            metadata.binary_name.clone(),
            metadata.inner_classes,
        )
    };
    let mut objects = Vec::new();
    for row in inner {
        if row.outer_class.as_deref() != Some(binary_name.as_str()) {
            continue;
        }
        if public_only && row.access_flags & ACC_PUBLIC == 0 {
            continue;
        }
        let member = match shared.load_class(loader, &row.inner_class) {
            Ok(member) => member,
            // The C1 tree is the pinned manifest's subset of the JDK, so a member class
            // outside it has no committed class file even though the JDK would return
            // it. Skip only that row rather than failing the whole query.
            Err(VmError::ClassNotFound { .. }) => continue,
            Err(error) => return Err(error),
        };
        objects.push(shared.class_object(member)?);
    }
    Ok(objects)
}

/// Build a `Field[]`, `Method[]` or `Constructor[]` in class-file order.
///
/// The members come from the typed C1 metadata model, so names, descriptors and access
/// flags are read once through the same decoder the metadata gate uses. Fields are every
/// declared field; methods are every declared method except `<init>`/`<clinit>`;
/// constructors are the `<init>` methods. Superclasses and interfaces are not consulted,
/// so this is the declared view only. Each element is the cached member object for its
/// `(class, kind, name, descriptor)` identity, and the array is an ordinary heap
/// reference array.
fn declared_members(shared: &Shared, class: ClassId, kind: MemberKind) -> Result<ObjRef, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    let mut members: Vec<(String, String)> = Vec::new();
    match kind {
        MemberKind::Field => {
            for field in &metadata.fields {
                members.push((field.name.clone(), field.descriptor.clone()));
            }
        }
        MemberKind::Method => {
            for method in &metadata.methods {
                if method.is_constructor() || method.is_class_initializer() {
                    continue;
                }
                members.push((method.name.clone(), method.descriptor.clone()));
            }
        }
        MemberKind::Constructor => {
            for method in &metadata.methods {
                if !method.is_constructor() {
                    continue;
                }
                members.push((method.name.clone(), method.descriptor.clone()));
            }
        }
    }

    let mut objects = Vec::with_capacity(members.len());
    for (name, descriptor) in &members {
        objects.push(shared.reflect_member(class, kind, name, descriptor)?);
    }
    let component = shared.load_library_class(kind.class_name())?;
    allocate_reference_array(shared, ClassRef::Named(component), &objects)
}

/// The declared members of a `Class` receiver.
///
/// A primitive or array class object has no class file, so it declares nothing: the JDK
/// returns an empty array for `int.class.getDeclaredFields()` and for the same query on
/// `int[].class`. A loaded class goes through [`declared_members`].
fn declared_members_of(
    shared: &Shared,
    receiver: ObjRef,
    kind: MemberKind,
) -> Result<ObjRef, VmError> {
    match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => declared_members(shared, class, kind),
        ClassObject::Primitive(_) | ClassObject::Array(_) => {
            // The empty result still has the query's array type: `Field[]`, `Method[]` or
            // `Constructor[]`.
            let component = shared.load_library_class(kind.class_name())?;
            allocate_reference_array(shared, ClassRef::Named(component), &[])
        }
    }
}

/// Allocate a reference array with the given component, holding `values`.
pub(crate) fn allocate_reference_array(
    shared: &Shared,
    component: ClassRef,
    values: &[ObjRef],
) -> Result<ObjRef, VmError> {
    let mut heap = shared.heap();
    let array = heap.allocate_array(component, values.len());
    {
        let array = heap.array_mut(array).map_err(heap_error)?;
        for (slot, value) in array.elements.iter_mut().zip(values.iter()) {
            *slot = Value::Ref(*value);
        }
    }
    Ok(array)
}

/// A member found by the M2.14 named lookup, before it becomes a cached member object.
struct FoundMember {
    class: ClassId,
    name: String,
    descriptor: String,
}

/// The exception a named lookup raises when it finds no member.
///
/// The JDK throws `NoSuchFieldException` for a field query and `NoSuchMethodException`
/// for a method or constructor query. The message names the receiver class, the member
/// name and, for an executable, the parameter descriptors. The throwable is a real
/// `java.lang` object built through [`crate::exceptions::new_throwable`], so a compiled
/// catch clause catches it, `getMessage()` reads the message, and a miss on a primitive or
/// array class object (`int.class.getDeclaredMethod(...)`) behaves like the JDK too.
fn lookup_miss(
    shared: &Shared,
    class: &ClassObject,
    kind: MemberKind,
    name: &str,
    parameters: &[String],
) -> Result<Option<Value>, VmError> {
    let class_name = shared.class_ref_name(&class.class_ref())?;
    let (exception, message) = match kind {
        MemberKind::Field => (
            "java/lang/NoSuchFieldException",
            format!("{class_name}.{name}"),
        ),
        MemberKind::Method | MemberKind::Constructor => (
            "java/lang/NoSuchMethodException",
            format!("{class_name}.{name}({})", parameters.join(", ")),
        ),
    };
    let throwable = crate::exceptions::new_throwable(shared, exception, message)?;
    Err(VmError::Thrown(throwable))
}

fn class_get_declared_field(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = lookup_name(shared, args, 0, "Class.getDeclaredField")?;
    let class = class_object_of(shared, receiver)?;
    if let ClassObject::Named(class) = class {
        if let Some(found) = find_declared_field(shared, class, &name)? {
            return member_value(shared, found, MemberKind::Field);
        }
    }
    lookup_miss(shared, &class, MemberKind::Field, &name, &[])
}

fn class_get_field(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = lookup_name(shared, args, 0, "Class.getField")?;
    let class = class_object_of(shared, receiver)?;
    if let ClassObject::Named(class) = class {
        for candidate in lookup_order(shared, class)? {
            if let Some(found) = find_declared_field(shared, candidate, &name)? {
                return member_value(shared, found, MemberKind::Field);
            }
        }
    }
    lookup_miss(shared, &class, MemberKind::Field, &name, &[])
}

fn class_get_declared_method(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = lookup_name(shared, args, 0, "Class.getDeclaredMethod")?;
    let parameters = parameter_class_descriptors(shared, args, 1, "Class.getDeclaredMethod")?;
    let class = class_object_of(shared, receiver)?;
    if let ClassObject::Named(class) = class {
        if let Some(found) = find_declared_method(shared, class, &name, &parameters)? {
            return member_value(shared, found, MemberKind::Method);
        }
    }
    lookup_miss(shared, &class, MemberKind::Method, &name, &parameters)
}

fn class_get_method(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = lookup_name(shared, args, 0, "Class.getMethod")?;
    let parameters = parameter_class_descriptors(shared, args, 1, "Class.getMethod")?;
    let class = class_object_of(shared, receiver)?;
    if let ClassObject::Named(class) = class {
        for candidate in lookup_order(shared, class)? {
            if let Some(found) = find_declared_method(shared, candidate, &name, &parameters)? {
                return member_value(shared, found, MemberKind::Method);
            }
        }
    }
    lookup_miss(shared, &class, MemberKind::Method, &name, &parameters)
}

fn class_get_declared_constructor(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let parameters = parameter_class_descriptors(shared, args, 0, "Class.getDeclaredConstructor")?;
    let class = class_object_of(shared, receiver)?;
    if let ClassObject::Named(class) = class {
        if let Some(found) = find_declared_constructor(shared, class, &parameters)? {
            return member_value(shared, found, MemberKind::Constructor);
        }
    }
    lookup_miss(
        shared,
        &class,
        MemberKind::Constructor,
        "<init>",
        &parameters,
    )
}

/// `Class.getConstructor`: constructors are not inherited, so the lookup is the declared
/// view of the receiver. The JDK additionally filters to public constructors; this slice
/// performs no access check, so every declared constructor is a candidate.
fn class_get_constructor(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let parameters = parameter_class_descriptors(shared, args, 0, "Class.getConstructor")?;
    let class = class_object_of(shared, receiver)?;
    if let ClassObject::Named(class) = class {
        if let Some(found) = find_declared_constructor(shared, class, &parameters)? {
            return member_value(shared, found, MemberKind::Constructor);
        }
    }
    lookup_miss(
        shared,
        &class,
        MemberKind::Constructor,
        "<init>",
        &parameters,
    )
}

/// The `String` name argument of a field lookup.
fn lookup_name(
    shared: &Shared,
    args: &[Value],
    position: usize,
    operation: &str,
) -> Result<String, VmError> {
    let Some(Value::Ref(reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects a String name argument"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{operation} received a null name argument"
        )));
    }
    let name = {
        let heap = shared.heap();
        let name = heap.string(reference).map_err(|error| {
            VmError::Native(format!(
                "{operation} name argument is not a String: {error}"
            ))
        })?;
        name.to_owned()
    };
    Ok(name)
}

/// Build the parameter descriptors of a lookup from its `Class[]` argument.
///
/// A parameter class object is a loaded class, a primitive or an array class. A loaded
/// class becomes its `Linternal/Name;` descriptor; a primitive becomes its one-character
/// descriptor (`I`, `Z`, ..., `V`); an array class becomes its array descriptor (`[I`,
/// `[Ljava/lang/String;`, `[[I`). That is what makes
/// `getDeclaredMethod("sum", new Class[]{int[].class})` match `([I)[I`. A loaded class
/// whose binary name is a primitive keyword or an array descriptor is a bogus class
/// rather than a class object of that shape, so it is a typed error too. A null element,
/// a null array and a non-`Class` element are typed errors, where the JDK would raise
/// `NullPointerException`.
fn parameter_class_descriptors(
    shared: &Shared,
    args: &[Value],
    position: usize,
    operation: &str,
) -> Result<Vec<String>, VmError> {
    let Some(Value::Ref(array_reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects a Class[] argument"
        )));
    };
    if array_reference.is_null() {
        return Err(VmError::Native(format!(
            "{operation} received a null Class[] argument"
        )));
    }
    let classes = {
        let heap = shared.heap();
        let array = heap.array(array_reference).map_err(heap_error)?;
        if array.kind != ArrayKind::Reference {
            return Err(VmError::Native(format!(
                "{operation} expects a Class[] argument, found a primitive array"
            )));
        }
        let mut classes = Vec::with_capacity(array.elements.len());
        for element in &array.elements {
            let Value::Ref(reference) = *element else {
                return Err(VmError::Native(format!(
                    "{operation} parameter classes must be objects"
                )));
            };
            if reference.is_null() {
                return Err(VmError::Native(format!(
                    "{operation} received a null parameter class"
                )));
            }
            let class = match heap.get(reference).map_err(|error| {
                VmError::Native(format!(
                    "{operation} parameter class is not a Class object: {error}"
                ))
            })? {
                HeapObject::Class(class) => ClassObject::Named(*class),
                HeapObject::PrimitiveClass(kind) => ClassObject::Primitive(*kind),
                HeapObject::ArrayClass(array) => ClassObject::Array(array.clone()),
                _ => {
                    return Err(VmError::Native(format!(
                        "{operation} parameter class is not a Class object: {reference} is not a class object"
                    )));
                }
            };
            classes.push(class);
        }
        classes
    };

    let mut descriptors = Vec::with_capacity(classes.len());
    for class in classes {
        let class = match class {
            ClassObject::Primitive(kind) => {
                descriptors.push(kind.descriptor().to_owned());
                continue;
            }
            ClassObject::Array(array) => {
                descriptors.push(shared.array_descriptor(&array)?);
                continue;
            }
            ClassObject::Named(class) => class,
        };
        let entry = shared
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let name = entry.name.clone();
        if name.starts_with('[') {
            return Err(VmError::Native(format!(
                "{operation} does not accept a class named like an array descriptor: {name}"
            )));
        }
        if is_primitive_class_name(&name) {
            return Err(VmError::Native(format!(
                "{operation} does not accept primitive parameter classes by name; use the primitive class object: {name}"
            )));
        }
        descriptors.push(format!("L{name};"));
    }
    Ok(descriptors)
}

/// A binary name that denotes a primitive type rather than a reference type.
fn is_primitive_class_name(name: &str) -> bool {
    matches!(
        name,
        "boolean" | "byte" | "char" | "short" | "int" | "long" | "float" | "double" | "void"
    )
}

/// The declared field `name`, first in class-file order.
fn find_declared_field(
    shared: &Shared,
    class: ClassId,
    name: &str,
) -> Result<Option<FoundMember>, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    for field in &metadata.fields {
        if field.name == name {
            return Ok(Some(FoundMember {
                class,
                name: field.name.clone(),
                descriptor: field.descriptor.clone(),
            }));
        }
    }
    Ok(None)
}

/// The declared method called `name` with `parameters`, first in class-file order.
///
/// A method's return type is not part of a reflective query, so the match compares only
/// the name and the parameter descriptors parsed from each method's full descriptor.
fn find_declared_method(
    shared: &Shared,
    class: ClassId,
    name: &str,
    parameters: &[String],
) -> Result<Option<FoundMember>, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    for method in &metadata.methods {
        if method.is_constructor() || method.is_class_initializer() {
            continue;
        }
        if method.name != name || !parameter_match(&method.descriptor, parameters)? {
            continue;
        }
        return Ok(Some(FoundMember {
            class,
            name: method.name.clone(),
            descriptor: method.descriptor.clone(),
        }));
    }
    Ok(None)
}

/// The `<init>` whose parameter descriptors equal `parameters`, first in class-file order.
fn find_declared_constructor(
    shared: &Shared,
    class: ClassId,
    parameters: &[String],
) -> Result<Option<FoundMember>, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    for method in &metadata.methods {
        if !method.is_constructor() || !parameter_match(&method.descriptor, parameters)? {
            continue;
        }
        return Ok(Some(FoundMember {
            class,
            name: method.name.clone(),
            descriptor: method.descriptor.clone(),
        }));
    }
    Ok(None)
}

/// Does a method descriptor's parameter list equal the lookup's parameter descriptors?
fn parameter_match(descriptor: &str, parameters: &[String]) -> Result<bool, VmError> {
    let declared = parameter_descriptors(descriptor)?;
    Ok(declared.len() == parameters.len()
        && declared
            .iter()
            .zip(parameters)
            .all(|(declared, expected)| *declared == expected.as_str()))
}

/// Turn a found member into its cached `Field`/`Method`/`Constructor` object.
///
/// A miss never reaches this function: the six named lookups raise the JDK's
/// `NoSuchFieldException`/`NoSuchMethodException` through [`lookup_miss`] instead.
fn member_value(
    shared: &Shared,
    found: FoundMember,
    kind: MemberKind,
) -> Result<Option<Value>, VmError> {
    let reference = shared.reflect_member(found.class, kind, &found.name, &found.descriptor)?;
    Ok(Some(Value::Ref(reference)))
}

/// The classes an inherited lookup visits, in order: the class itself, its superclass
/// chain, and then interfaces, with every class visited once.
///
/// An interface's class file names `java/lang/Object` as its superclass but the JDK
/// reports no superclass for an interface, so the chain does not follow that edge; the
/// interface's own `interfaces` list is still visited. [`ClassId`]s are visited once, so
/// a cyclic or diamond interface graph cannot loop.
pub(crate) fn lookup_order(shared: &Shared, start: ClassId) -> Result<Vec<ClassId>, VmError> {
    let mut order = Vec::new();
    let mut visited = HashSet::new();
    collect_lookup_order(shared, start, &mut visited, &mut order)?;
    Ok(order)
}

fn collect_lookup_order(
    shared: &Shared,
    class: ClassId,
    visited: &mut HashSet<ClassId>,
    order: &mut Vec<ClassId>,
) -> Result<(), VmError> {
    if !visited.insert(class) {
        return Ok(());
    }
    order.push(class);
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let loader = entry.loader;
    let metadata = entry.metadata()?;
    let is_interface = metadata.is_interface();
    let interfaces = metadata.interfaces.clone();
    if !is_interface {
        if let Some(super_class) = shared.resolution_superclass(class)? {
            collect_lookup_order(shared, super_class, visited, order)?;
        }
    }
    for interface in interfaces {
        // The C1 tree omits a few transitive interfaces (for example
        // `java.lang.constant.Constable` on `String`); they cannot be part of the
        // inherited-lookup walk, while a genuinely missing class elsewhere is still a
        // loader error.
        match shared.load_class(loader, &interface) {
            Ok(interface) => collect_lookup_order(shared, interface, visited, order)?,
            Err(VmError::ClassNotFound { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// The member identity behind a reflected-member receiver.
fn member_of(shared: &Shared, receiver: ObjRef) -> Result<ReflectMember, VmError> {
    let member = {
        let heap = shared.heap();
        heap.reflect(receiver).map_err(heap_error)?.clone()
    };
    Ok(member)
}

fn member_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let name = shared.heap().allocate_string(member.name);
    Ok(Some(Value::Ref(name)))
}

fn member_get_modifiers(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let flags = member_access_flags(shared, &member)?;
    // The mask is the JDK's `Modifier.*Modifiers()` set for the member kind. A method's
    // mask includes `bridge`, `varargs` and `synthetic`; a field's includes `synthetic`
    // and `enum`; a constructor's is the three access bits only. That asymmetry is the
    // JDK's: `Constructor.getModifiers()` does not expose `ACC_VARARGS`/`ACC_SYNTHETIC`,
    // which `Constructor.isVarArgs()`/`isSynthetic()` report separately.
    let mask = match member.kind {
        MemberKind::Field => FIELD_MODIFIERS,
        MemberKind::Method => METHOD_MODIFIERS,
        MemberKind::Constructor => CONSTRUCTOR_MODIFIERS,
    };
    Ok(Some(Value::Int(i32::from(flags & mask))))
}

/// A class-file predicate flag of a member (`ACC_BRIDGE`, `ACC_VARARGS` or
/// `ACC_SYNTHETIC`) read from the declaration's typed C1 metadata.
///
/// A member object whose declaration is absent from its class file has no flags to
/// report, so that lookup is a typed [`VmError::Native`] rather than a fabricated
/// `false`; a lookup miss is a broken member object, not a member with the flag unset.
fn member_flag(shared: &Shared, receiver: ObjRef, flag: u16) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let flags = member_access_flags(shared, &member)?;
    Ok(Some(Value::Int(i32::from(flags & flag != 0))))
}

fn member_is_synthetic(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    member_flag(shared, receiver, ACC_SYNTHETIC)
}

fn member_is_var_args(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    member_flag(shared, receiver, ACC_VARARGS)
}

fn method_is_bridge(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    member_flag(shared, receiver, ACC_BRIDGE)
}

fn member_get_declaring_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let object = shared.class_object(member.class)?;
    Ok(Some(Value::Ref(object)))
}

/// `Constructor.getName()`: the binary name of the declaring class, as on the JDK.
fn constructor_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let entry = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?;
    let name = shared.heap().allocate_string(entry.name.replace('/', "."));
    Ok(Some(Value::Ref(name)))
}

fn field_get_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let loader = declaring_loader(shared, &member)?;
    let object = class_for_descriptor(shared, loader, &member.descriptor)?;
    Ok(Some(Value::Ref(object)))
}

/// The typed metadata of one field, with the descriptor the member object claims.
struct FieldView {
    descriptor: String,
    is_static: bool,
}

/// Read a reflected field's metadata and check it still matches the member identity.
fn field_view(shared: &Shared, member: &ReflectMember) -> Result<FieldView, VmError> {
    let entry = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?;
    let metadata = entry.metadata()?;
    let field = metadata
        .field(&member.name)
        .filter(|field| field.descriptor == member.descriptor)
        .ok_or_else(|| {
            VmError::Native(format!(
                "class {} has no field {}:{}",
                entry.name, member.name, member.descriptor
            ))
        })?;
    Ok(FieldView {
        descriptor: field.descriptor.clone(),
        is_static: field.is_static(),
    })
}

/// `Field.get(Object)`: read an instance field and box a primitive result.
///
/// Instance fields only: a static field is a typed mismatch rather than a silent read
/// with an ignored receiver. The receiver must be an instance assignable to the declaring
/// class, and the value is converted with [`boxed::box_result`], so each primitive becomes
/// its own wrapper (`boolean` a `Boolean`, `long` a `Long`, ...) and a reference keeps its
/// handle. There is no access check, so a private field reads like a public one.
fn field_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    require_field(&member, "Field.get")?;
    let view = field_view(shared, &member)?;
    require_instance_field(shared, &view, &member, "Field.get")?;
    let (target, actual) = field_receiver(shared, &member, args, 0, "Field.get")?;
    let slot = field_slot(shared, actual, &member)?;
    let value = {
        let heap = shared.heap();
        heap.instance(target)
            .map_err(heap_error)?
            .fields
            .get(slot)
            .copied()
            .ok_or_else(|| {
                VmError::Native(format!("Field.get: slot {slot} is outside the instance"))
            })?
    };
    let result = boxed::box_result(shared, &view.descriptor, Some(value))?;
    Ok(Some(result))
}

/// `Field.set(Object, Object)`: write an instance field, unboxing a primitive value.
///
/// The descriptor decides the conversion: a primitive field requires the matching wrapper
/// (a narrower numeric wrapper is accepted by Java's widening rule, JLS §5.3) and a
/// reference descriptor passes the handle through (null included). A static field and a
/// non-assignable receiver are rejected exactly as in [`field_get`].
fn field_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    require_field(&member, "Field.set")?;
    let view = field_view(shared, &member)?;
    require_instance_field(shared, &view, &member, "Field.set")?;
    let (target, actual) = field_receiver(shared, &member, args, 0, "Field.set")?;
    let Some(value) = args.get(1).copied() else {
        return Err(VmError::Native(
            "Field.set is missing its value argument".to_owned(),
        ));
    };
    let value = boxed::unbox_argument(shared, &view.descriptor, value)?;
    let slot = field_slot(shared, actual, &member)?;
    {
        let mut heap = shared.heap();
        let instance = heap.instance_mut(target).map_err(heap_error)?;
        let slot_value = instance.fields.get_mut(slot).ok_or_else(|| {
            VmError::Native(format!("Field.set: slot {slot} is outside the instance"))
        })?;
        *slot_value = value;
    }
    Ok(None)
}

fn require_field(member: &ReflectMember, operation: &str) -> Result<(), VmError> {
    if member.kind == MemberKind::Field {
        Ok(())
    } else {
        Err(VmError::Native(format!(
            "{operation} requires a Field receiver, found a {}",
            member.kind.binary_name()
        )))
    }
}

fn require_instance_field(
    shared: &Shared,
    view: &FieldView,
    member: &ReflectMember,
    operation: &str,
) -> Result<(), VmError> {
    if view.is_static {
        return Err(VmError::Native(format!(
            "{operation} does not support static field {}.{}:{} (M2.10)",
            class_name(shared, member.class),
            member.name,
            member.descriptor
        )));
    }
    Ok(())
}

/// Resolve the receiver argument and require an instance assignable to the declaring
/// class. Returns the handle and its runtime class.
fn field_receiver(
    shared: &Shared,
    member: &ReflectMember,
    args: &[Value],
    position: usize,
    operation: &str,
) -> Result<(ObjRef, ClassId), VmError> {
    let Some(Value::Ref(receiver)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects an Object receiver"
        )));
    };
    if receiver.is_null() {
        return Err(VmError::Native(format!(
            "{operation} received a null receiver"
        )));
    }
    let actual = {
        let heap = shared.heap();
        match heap.instance(receiver) {
            Ok(instance) => instance.class,
            Err(error) => {
                return Err(VmError::Native(format!(
                    "{operation} receiver is not an instance: {error}"
                )));
            }
        }
    };
    if !is_assignable(shared, actual, member.class)? {
        return Err(VmError::Native(format!(
            "{operation} receiver of class {} is not assignable to {}",
            class_name(shared, actual),
            class_name(shared, member.class)
        )));
    }
    Ok((receiver, actual))
}

/// The layout slot of the reflected field on the receiver's runtime class.
fn field_slot(shared: &Shared, actual: ClassId, member: &ReflectMember) -> Result<usize, VmError> {
    shared
        .field_layout(actual)?
        .slot(&member.name, &member.descriptor)
        .ok_or_else(|| {
            VmError::Native(format!(
                "class {} has no slot for field {}:{}",
                class_name(shared, actual),
                member.name,
                member.descriptor
            ))
        })
}

/// Is `from` the same class as `to`, or a subclass of it?
fn is_assignable(shared: &Shared, mut from: ClassId, to: ClassId) -> Result<bool, VmError> {
    loop {
        if from == to {
            return Ok(true);
        }
        let Some(super_class) = shared.resolution_superclass(from)? else {
            return Ok(false);
        };
        from = super_class;
    }
}

fn class_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("class {}", class.0))
}

fn method_get_return_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let return_type = return_descriptor(&member.descriptor)?;
    let loader = declaring_loader(shared, &member)?;
    let object = class_for_descriptor(shared, loader, return_type)?;
    Ok(Some(Value::Ref(object)))
}

fn executable_get_parameter_types(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let descriptors = parameter_descriptors(&member.descriptor)?;
    let loader = declaring_loader(shared, &member)?;
    let mut classes = Vec::with_capacity(descriptors.len());
    for descriptor in descriptors {
        classes.push(class_for_descriptor(shared, loader, descriptor)?);
    }
    let array = allocate_reference_array(
        shared,
        ClassRef::Named(shared.load_library_class("java/lang/Class")?),
        &classes,
    )?;
    Ok(Some(Value::Ref(array)))
}

fn executable_get_parameter_count(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    let count = parameter_descriptors(&member.descriptor)?.len();
    let count = i32::try_from(count)
        .map_err(|_| VmError::Native("parameter count does not fit an int".to_owned()))?;
    Ok(Some(Value::Int(count)))
}

/// The class-file flags of a member, looked up by name and descriptor in the typed C1
/// metadata.
fn member_access_flags(shared: &Shared, member: &ReflectMember) -> Result<u16, VmError> {
    let entry = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?;
    let metadata = entry.metadata()?;
    match member.kind {
        MemberKind::Field => {
            if let Some(field) = metadata.field(&member.name) {
                if field.descriptor == member.descriptor {
                    return Ok(field.access_flags);
                }
            }
        }
        MemberKind::Method | MemberKind::Constructor => {
            if let Some(method) = metadata.method(&member.name, &member.descriptor) {
                return Ok(method.access_flags);
            }
        }
    }
    Err(VmError::Native(format!(
        "class {} has no {} {}:{}",
        entry.name,
        member.kind.binary_name(),
        member.name,
        member.descriptor
    )))
}

/// The loader that defines a member's declaring class, which is the loader its type
/// descriptors are resolved from.
fn declaring_loader(shared: &Shared, member: &ReflectMember) -> Result<LoaderId, VmError> {
    Ok(shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?
        .loader)
}

/// Resolve one type descriptor to a class object.
///
/// A primitive descriptor (`I`, `Z`, ..., `V`) returns the cached primitive class object;
/// an object descriptor (`Ljava/lang/String;`) resolves the named class through the
/// declaring class's loader; an array descriptor (`[I`, `[Ljava/lang/String;`, `[[I`)
/// returns the cached array class object whose components are resolved through that same
/// loader. That is what makes `Field.getType`, `Method.getReturnType`,
/// `Method.getParameterTypes` and `Constructor.getParameterTypes` report array types.
fn class_for_descriptor(
    shared: &Shared,
    loader: LoaderId,
    descriptor: &str,
) -> Result<ObjRef, VmError> {
    let class_ref = shared.class_ref_for_descriptor(loader, descriptor)?;
    shared.class_object_for_ref(&class_ref)
}

/// Split a method descriptor's parameter list into its parameter descriptors.
pub fn parameter_descriptors(descriptor: &str) -> Result<Vec<&str>, VmError> {
    let rest = descriptor
        .strip_prefix('(')
        .ok_or_else(|| bad_descriptor(descriptor))?;
    let (parameters, _return_type) = rest
        .split_once(')')
        .ok_or_else(|| bad_descriptor(descriptor))?;
    let mut descriptors = Vec::new();
    let bytes = parameters.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let start = cursor;
        match bytes[cursor] {
            b'L' => {
                let end = parameters[cursor..]
                    .find(';')
                    .ok_or_else(|| bad_descriptor(descriptor))?;
                cursor += end + 1;
            }
            b'[' => {
                while bytes.get(cursor) == Some(&b'[') {
                    cursor += 1;
                }
                match bytes.get(cursor).copied() {
                    Some(b'L') => {
                        let end = parameters[cursor..]
                            .find(';')
                            .ok_or_else(|| bad_descriptor(descriptor))?;
                        cursor += end + 1;
                    }
                    Some(b'Z' | b'B' | b'C' | b'S' | b'I' | b'J' | b'F' | b'D') => cursor += 1,
                    _ => return Err(bad_descriptor(descriptor)),
                }
            }
            b'Z' | b'B' | b'C' | b'S' | b'I' | b'J' | b'F' | b'D' => cursor += 1,
            _ => return Err(bad_descriptor(descriptor)),
        }
        descriptors.push(&parameters[start..cursor]);
    }
    Ok(descriptors)
}

/// The return descriptor of a method descriptor (`V` included).
pub fn return_descriptor(descriptor: &str) -> Result<&str, VmError> {
    let (_, return_type) = descriptor
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
        .ok_or_else(|| bad_descriptor(descriptor))?;
    if return_type.is_empty() {
        return Err(bad_descriptor(descriptor));
    }
    Ok(return_type)
}

fn bad_descriptor(descriptor: &str) -> VmError {
    VmError::Native(format!("malformed descriptor {descriptor}"))
}
