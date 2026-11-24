//! `Object`/`Class` reflection natives (M2.4), reflected members (M2.8) and field
//! access (M2.10).
//!
//! The surface here is deliberately small. M2.4 supplies the six class-object methods
//! plus `Object.getClass`: names, modifiers, interface/array flags, the superclass and
//! bootstrap `Class.forName`. M2.8 adds the declared-member queries and read-only
//! `Field`, `Method` and `Constructor` objects: the declaring class, name, modifiers,
//! field type, return type, parameter types and parameter count. M2.10 adds `Field.get`
//! and `Field.set` for `int` and reference fields, boxing an `int` on read and unboxing
//! a boxed `Integer` on write. `Method.invoke` and `Constructor.newInstance` need the
//! interpreter's execution machinery, so they live in `rava-interp` instead of this
//! crate; annotations, generic types, inherited-member lookup and access checks remain
//! later slices.
//!
//! The natives read the class table rather than caching a second copy of the metadata:
//! a class object is only a [`crate::ClassId`], and every answer comes from that
//! class's parsed `ClassFile`. A reflected member is only a
//! [`crate::ReflectMember`] (declaring class, kind, name and descriptor), and its
//! modifiers and types come from the same typed C1 metadata. `Class.forName` loads and
//! allocates the class object; it does not initialize the class, because the C1 stubs
//! have no observable `<clinit>`.

use crate::VmError;
use crate::boxed;
use crate::heap::{ArrayKind, HeapError, MemberKind, ObjRef, ReflectMember};
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

fn class_get_declared_fields(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = receiver_class(shared, receiver)?;
    let array = declared_members(shared, class, MemberKind::Field)?;
    Ok(Some(Value::Ref(array)))
}

fn class_get_declared_methods(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = receiver_class(shared, receiver)?;
    let array = declared_members(shared, class, MemberKind::Method)?;
    Ok(Some(Value::Ref(array)))
}

fn class_get_declared_constructors(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = receiver_class(shared, receiver)?;
    let array = declared_members(shared, class, MemberKind::Constructor)?;
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
    allocate_reference_array(shared, &objects)
}

/// Allocate a `java.lang.Object[]`-shaped reference array holding `values`.
pub(crate) fn allocate_reference_array(
    shared: &Shared,
    values: &[ObjRef],
) -> Result<ObjRef, VmError> {
    let mut heap = shared.heap();
    let array = heap.allocate_array(ArrayKind::Reference, values.len());
    {
        let array = heap.array_mut(array).map_err(heap_error)?;
        for (slot, value) in array.elements.iter_mut().zip(values.iter()) {
            *slot = Value::Ref(*value);
        }
    }
    Ok(array)
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
    let array = allocate_reference_array(shared, &classes)?;
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

/// Resolve one field descriptor to a class object from the class library.
///
/// Only object descriptors (`Ljava/lang/String;`) are represented: primitive and array
/// types have no class identity in this slice, so they return a typed error instead of
/// a wrong `Class` object.
fn class_for_descriptor(
    shared: &Shared,
    loader: LoaderId,
    descriptor: &str,
) -> Result<ObjRef, VmError> {
    let Some(internal) = descriptor
        .strip_prefix('L')
        .and_then(|rest| rest.strip_suffix(';'))
    else {
        let kind = if descriptor.starts_with('[') {
            "array class objects"
        } else {
            "primitive class objects"
        };
        return Err(VmError::Native(format!(
            "{kind} are not supported yet (M2.8): {descriptor}"
        )));
    };
    let class = shared.load_class(loader, internal)?;
    shared.class_object(class)
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
