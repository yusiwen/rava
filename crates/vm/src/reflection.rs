//! `Object`/`Class` reflection natives (M2.4), reflected members (M2.8), field access
//! (M2.10), named member lookup (M2.14), primitive types (M2.15) and array types (M2.16).
//!
//! The surface here is deliberately small. M2.4 supplies the six class-object methods
//! plus `Object.getClass`: names, modifiers, interface/array flags, the superclass and
//! bootstrap `Class.forName`. M2.8 adds the declared-member queries and read-only
//! `Field`, `Method` and `Constructor` objects: the declaring class, name, modifiers,
//! field type, return type, parameter types and parameter count. M2.10 adds `Field.get`
//! and `Field.set` for `int` and reference fields, boxing an `int` on read and unboxing
//! a boxed `Integer` on write. M2.14 adds `getDeclaredField`/`getField`,
//! `getDeclaredMethod`/`getMethod` and `getDeclaredConstructor`/`getConstructor`: a
//! query matches a field by name and an executable by name plus parameter descriptors,
//! `get*` walks the superclass chain and interfaces while `getDeclared*` stays on the
//! receiver, and every hit is the same cached member object the `getDeclared*`
//! array queries return. M2.15 adds primitive class objects: `Class.forName("int")`
//! returns the cached [`crate::HeapObject::PrimitiveClass`] object, `getName`,
//! `getModifiers`, `isPrimitive`, `isArray`, `getSuperclass` and `getComponentType`
//! answer for it, a primitive descriptor resolves to it in `Field.getType`,
//! `Method.getReturnType`/`getParameterTypes` and `Constructor.getParameterTypes`, and a
//! `Class[]` of primitive class objects builds the primitive descriptors a named lookup
//! matches. M2.16 adds array class objects: the cached
//! [`crate::HeapObject::ArrayClass`] object for a component, its descriptor-form
//! `getName`, `0x0411` modifiers, `isArray`, its component and `java.lang.Object`
//! superclass, array descriptors in the same member types, and array class objects in a
//! named lookup's `Class[]`. `Method.invoke` and `Constructor.newInstance` need the
//! interpreter's execution machinery, so they live in `rava-interp` instead of this
//! crate; annotations, generic types and access checks remain later slices.
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

/// `ACC_INTERFACE` (JVMS §4.1).
const ACC_INTERFACE: u16 = 0x0200;

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
/// array it takes the JDK's descriptor form with dots in an object component. The named
/// component of an array is resolved through the bootstrap loader, exactly as a named
/// argument is.
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

    if let Some(kind) = PrimitiveKind::from_name(&name) {
        return Ok(Some(Value::Ref(shared.primitive_class(kind))));
    }
    if name.starts_with('[') {
        // A descriptor's object names are internal; `Class.forName` accepts the dotted
        // spelling, so a `[Ljava.lang.String;` argument becomes `[Ljava/lang/String;`.
        let descriptor = name.replace('.', "/");
        let class_ref = shared.class_ref_for_descriptor(LoaderId::BOOTSTRAP, &descriptor)?;
        let object = shared.class_object_for_ref(&class_ref)?;
        return Ok(Some(Value::Ref(object)));
    }
    let internal = name.replace('.', "/");
    let class = shared.load_library_class(&internal)?;
    let object = shared.class_object(class)?;
    Ok(Some(Value::Ref(object)))
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

/// The loaded class behind a member-query receiver, or `None` for a primitive or array.
///
/// A primitive or array class object declares no members, so a named lookup on it is a
/// miss; the JDK's `NoSuchFieldException`/`NoSuchMethodException` is still represented as
/// the documented `null` miss in this slice.
fn named_receiver(shared: &Shared, receiver: ObjRef) -> Result<Option<ClassId>, VmError> {
    Ok(match class_object_of(shared, receiver)? {
        ClassObject::Named(class) => Some(class),
        ClassObject::Primitive(_) | ClassObject::Array(_) => None,
    })
}

fn class_get_declared_field(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = lookup_name(shared, args, 0, "Class.getDeclaredField")?;
    let Some(class) = named_receiver(shared, receiver)? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let found = find_declared_field(shared, class, &name)?;
    member_value(shared, found, MemberKind::Field)
}

fn class_get_field(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = lookup_name(shared, args, 0, "Class.getField")?;
    let Some(class) = named_receiver(shared, receiver)? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    for candidate in lookup_order(shared, class)? {
        if let Some(found) = find_declared_field(shared, candidate, &name)? {
            return member_value(shared, Some(found), MemberKind::Field);
        }
    }
    Ok(Some(Value::Ref(ObjRef::NULL)))
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
    let Some(class) = named_receiver(shared, receiver)? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let found = find_declared_method(shared, class, &name, &parameters)?;
    member_value(shared, found, MemberKind::Method)
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
    let Some(class) = named_receiver(shared, receiver)? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    for candidate in lookup_order(shared, class)? {
        if let Some(found) = find_declared_method(shared, candidate, &name, &parameters)? {
            return member_value(shared, Some(found), MemberKind::Method);
        }
    }
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

fn class_get_declared_constructor(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let parameters = parameter_class_descriptors(shared, args, 0, "Class.getDeclaredConstructor")?;
    let Some(class) = named_receiver(shared, receiver)? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let found = find_declared_constructor(shared, class, &parameters)?;
    member_value(shared, found, MemberKind::Constructor)
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
    let Some(class) = named_receiver(shared, receiver)? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let found = find_declared_constructor(shared, class, &parameters)?;
    member_value(shared, found, MemberKind::Constructor)
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
/// A miss is `null`, which is the result this slice documents for every lookup miss. The
/// JDK throws `NoSuchFieldException`/`NoSuchMethodException`, but this slice does not
/// synthesize exception objects, so `null` is the honest, testable result here.
fn member_value(
    shared: &Shared,
    found: Option<FoundMember>,
    kind: MemberKind,
) -> Result<Option<Value>, VmError> {
    let Some(found) = found else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
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
fn lookup_order(shared: &Shared, start: ClassId) -> Result<Vec<ClassId>, VmError> {
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
        let interface = shared.load_class(loader, &interface)?;
        collect_lookup_order(shared, interface, visited, order)?;
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
    let mask = match member.kind {
        MemberKind::Field => FIELD_MODIFIERS,
        MemberKind::Method => METHOD_MODIFIERS,
        MemberKind::Constructor => CONSTRUCTOR_MODIFIERS,
    };
    Ok(Some(Value::Int(i32::from(flags & mask))))
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

/// `Field.get(Object)`: read an instance field and box an `int` result.
///
/// M2.10 supports instance fields only: a static field is a typed mismatch rather than a
/// silent read with an ignored receiver. The receiver must be an instance assignable to
/// the declaring class, and the value is converted with [`boxed::box_result`], so an
/// `int` becomes a `java.lang.Integer` box and a reference keeps its handle. There is no
/// access check, so a private field reads like a public one.
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

/// `Field.set(Object, Object)`: write an instance field, unboxing an `int` value.
///
/// The descriptor decides the conversion: `I` requires a boxed `Integer`, a reference
/// descriptor passes the handle through (null included), and any other primitive
/// descriptor is a typed error. A static field and a non-assignable receiver are
/// rejected exactly as in [`field_get`].
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
        let entry = shared
            .entry(from)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", from.0)))?;
        let Some(super_name) = entry.file.super_class_name()? else {
            return Ok(false);
        };
        let super_name = super_name.to_owned();
        from = shared.load_class(entry.loader, &super_name)?;
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
