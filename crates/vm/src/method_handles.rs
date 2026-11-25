//! `java.lang.invoke` natives (M3.4): `MethodHandles` and `MethodHandles.Lookup`, plus the
//! `MethodType` and `MethodHandle` queries a lookup- or reflection-built handle needs.
//!
//! The VM implements a narrow, caller-insensitive core of the method-handle API. The
//! natives here have two jobs:
//!
//! 1. `MethodHandles.lookup` / `publicLookup` / `privateLookupIn`, `Lookup.lookupClass`,
//!    `Lookup.findStatic` / `findVirtual` / `findSpecial` / `findConstructor`,
//!    `Lookup.unreflect` / `unreflectConstructor` and `Lookup.defineClass` build the
//!    handle and lookup objects; and
//! 2. the `MethodType` constructors and accessors describe a signature structurally, so a
//!    handle carries a method type instead of a second copy of the class metadata.
//!
//! Invocation is not here: `MethodHandle.invokeExact` / `invoke` /
//! `invokeWithArguments` re-enter the interpreter, so they live in
//! `rava_interp::method_handles`, exactly as `Method.invoke` lives in
//! `rava_interp::reflect_invoke`.
//!
//! # Documented limitations
//!
//! - **No caller-sensitive lookup.** The native ABI does not carry the class that called a
//!   native, so `MethodHandles.lookup()` uses the stable placeholder class
//!   [`LOOKUP_PLACEHOLDER_CLASS`] (`java/lang/Object`). `privateLookupIn(Class, Lookup)`
//!   replaces it with the target class, which is the only lookup CGLIB's
//!   `ReflectUtils.defineClass` uses.
//! - **No access checks.** Every lookup is full-access unless it is the public lookup, and
//!   `findStatic` / `findVirtual` / `findSpecial` / `findConstructor` ignore the caller's
//!   access to the member. `findSpecial` accepts its special-caller argument and ignores
//!   it.
//! - **No signature-polymorphic call sites.** The interpreter resolves a call by the
//!   descriptor in the class file, so `handle.invoke(...)`/`invokeExact(...)` at a
//!   polymorphic call site is not dispatchable; the tested surface is the real
//!   `invokeExact(Object[])` / `invoke(Object[])` / `invokeWithArguments(Object[])`
//!   declarations, which `rava_interp::method_handles` implements.
//! - **No `asType` conversion.** `MethodHandle.asType` returns the receiver only when the
//!   requested type's descriptor equals the handle's; any other conversion is a typed
//!   error.
//! - **No varargs, spreaders, collectors, `bindTo`, field handles, `VarHandle` or hidden
//!   classes.** Those method names resolve in the C1 stubs but have no native, so a call
//!   reports "no native implementation" rather than doing something approximate.
//! - **No default-method resolution for a `Virtual` handle.** Invocation starts at the
//!   receiver's runtime class and walks its superclass chain; a method that is only
//!   inherited from an interface is a typed "not found" at invocation time.
//! - **`MethodType` is not interned.** Two structurally equal method types are two heap
//!   objects; `equals` compares them structurally and `toString` prints the JDK's simple
//!   form (`(int,int)int`). `MethodHandle.type()` builds a fresh `MethodType` object on
//!   every call, so `handle.type() == handle.type()` is false here.
//! - **`Lookup.previousLookupClass` is null**, as this VM never tracks a previous lookup.
//! - **`Lookup.ensureInitialized` is absent.** Class initialization is an interpreter
//!   operation (it runs `<clinit>`), so a `vm` native cannot implement it.

use rava_classfile::ClassFile;

use crate::VmError;
use crate::exceptions;
use crate::heap::{
    ArrayKind, ClassRef, HandleKind, LookupObject, MethodHandleObject, MethodTypeObject, ObjRef,
    PrimitiveKind,
};
use crate::loader::{ClassId, LoaderId};
use crate::reflection::{self, class_object_of};
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `MethodHandles`'s internal name.
const METHOD_HANDLES_CLASS: &str = "java/lang/invoke/MethodHandles";

/// `MethodHandles.Lookup`'s internal name.
const LOOKUP_CLASS: &str = "java/lang/invoke/MethodHandles$Lookup";

/// `MethodType`'s internal name.
const METHOD_TYPE_CLASS: &str = "java/lang/invoke/MethodType";

/// `MethodHandle`'s internal name.
const METHOD_HANDLE_CLASS: &str = "java/lang/invoke/MethodHandle";

/// The stable placeholder class `MethodHandles.lookup()` reports.
///
/// The native ABI carries the declaring class's loader, never the caller's class, so a
/// caller-sensitive lookup cannot name the real caller. `java.lang.Object` is stable
/// across runs and loaders, and `privateLookupIn` immediately replaces it with the target
/// class for the one path that matters (CGLIB's `ReflectUtils.defineClass`).
pub const LOOKUP_PLACEHOLDER_CLASS: &str = "java/lang/Object";

/// `MethodHandles.Lookup.PUBLIC` from the committed C1 stub.
pub const LOOKUP_PUBLIC: i32 = 0x01;
/// `MethodHandles.Lookup.PRIVATE` from the committed C1 stub.
pub const LOOKUP_PRIVATE: i32 = 0x02;
/// `MethodHandles.Lookup.PROTECTED` from the committed C1 stub.
pub const LOOKUP_PROTECTED: i32 = 0x04;
/// `MethodHandles.Lookup.PACKAGE` from the committed C1 stub.
pub const LOOKUP_PACKAGE: i32 = 0x08;
/// `MethodHandles.Lookup.MODULE` from the committed C1 stub.
pub const LOOKUP_MODULE: i32 = 0x10;
/// `MethodHandles.Lookup.UNCONDITIONAL` from the committed C1 stub.
pub const LOOKUP_UNCONDITIONAL: i32 = 0x20;

/// The full-access mode set: every mode bit the stub defines.
pub const FULL_ACCESS: i32 = LOOKUP_PUBLIC
    | LOOKUP_PRIVATE
    | LOOKUP_PROTECTED
    | LOOKUP_PACKAGE
    | LOOKUP_MODULE
    | LOOKUP_UNCONDITIONAL;

/// `MethodHandles.publicLookup()`'s mode set: public access only.
pub const PUBLIC_ACCESS: i32 = LOOKUP_PUBLIC;

/// Install the `java.lang.invoke` natives that do not re-enter the interpreter.
pub fn register_natives(shared: &Shared) {
    for (name, descriptor, function) in [
        (
            "lookup",
            "()Ljava/lang/invoke/MethodHandles$Lookup;",
            method_handles_lookup as crate::NativeFn,
        ),
        (
            "publicLookup",
            "()Ljava/lang/invoke/MethodHandles$Lookup;",
            method_handles_public_lookup,
        ),
        (
            "privateLookupIn",
            "(Ljava/lang/Class;Ljava/lang/invoke/MethodHandles$Lookup;)Ljava/lang/invoke/MethodHandles$Lookup;",
            method_handles_private_lookup_in,
        ),
    ] {
        shared.register_native(METHOD_HANDLES_CLASS, name, descriptor, function);
    }

    for (name, descriptor, function) in [
        (
            "lookupClass",
            "()Ljava/lang/Class;",
            lookup_lookup_class as crate::NativeFn,
        ),
        (
            "previousLookupClass",
            "()Ljava/lang/Class;",
            lookup_previous_lookup_class,
        ),
        ("lookupModes", "()I", lookup_lookup_modes),
        ("hasPrivateAccess", "()Z", lookup_has_private_access),
        (
            "hasFullPrivilegeAccess",
            "()Z",
            lookup_has_full_privilege_access,
        ),
        (
            "in",
            "(Ljava/lang/Class;)Ljava/lang/invoke/MethodHandles$Lookup;",
            lookup_in,
        ),
        (
            "dropLookupMode",
            "(I)Ljava/lang/invoke/MethodHandles$Lookup;",
            lookup_drop_lookup_mode,
        ),
        ("toString", "()Ljava/lang/String;", lookup_to_string),
        ("defineClass", "([B)Ljava/lang/Class;", lookup_define_class),
        (
            "findStatic",
            "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
            lookup_find_static,
        ),
        (
            "findVirtual",
            "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
            lookup_find_virtual,
        ),
        (
            "findSpecial",
            "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/Class;)Ljava/lang/invoke/MethodHandle;",
            lookup_find_special,
        ),
        (
            "findConstructor",
            "(Ljava/lang/Class;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
            lookup_find_constructor,
        ),
        (
            "unreflect",
            "(Ljava/lang/reflect/Method;)Ljava/lang/invoke/MethodHandle;",
            lookup_unreflect,
        ),
        (
            "unreflectConstructor",
            "(Ljava/lang/reflect/Constructor;)Ljava/lang/invoke/MethodHandle;",
            lookup_unreflect_constructor,
        ),
        (
            "findClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            lookup_find_class,
        ),
    ] {
        shared.register_native(LOOKUP_CLASS, name, descriptor, function);
    }

    for (name, descriptor, function) in [
        (
            "methodType",
            "(Ljava/lang/Class;[Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            method_type_of_return_and_array as crate::NativeFn,
        ),
        (
            "methodType",
            "(Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            method_type_of_return,
        ),
        (
            "methodType",
            "(Ljava/lang/Class;Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            method_type_of_return_and_one,
        ),
        (
            "methodType",
            "(Ljava/lang/Class;Ljava/lang/Class;[Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            method_type_of_return_and_variadic,
        ),
        ("returnType", "()Ljava/lang/Class;", method_type_return_type),
        (
            "parameterType",
            "(I)Ljava/lang/Class;",
            method_type_parameter_type,
        ),
        ("parameterCount", "()I", method_type_parameter_count),
        (
            "parameterArray",
            "()[Ljava/lang/Class;",
            method_type_parameter_array,
        ),
        (
            "lastParameterType",
            "()Ljava/lang/Class;",
            method_type_last_parameter_type,
        ),
        (
            "changeReturnType",
            "(Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            method_type_change_return_type,
        ),
        (
            "appendParameterTypes",
            "([Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            method_type_append_parameter_types,
        ),
        (
            "toMethodDescriptorString",
            "()Ljava/lang/String;",
            method_type_to_descriptor_string,
        ),
        ("equals", "(Ljava/lang/Object;)Z", method_type_equals),
        ("hashCode", "()I", method_type_hash_code),
        ("toString", "()Ljava/lang/String;", method_type_to_string),
    ] {
        shared.register_native(METHOD_TYPE_CLASS, name, descriptor, function);
    }

    for (name, descriptor, function) in [
        (
            "type",
            "()Ljava/lang/invoke/MethodType;",
            method_handle_type as crate::NativeFn,
        ),
        (
            "asType",
            "(Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
            method_handle_as_type,
        ),
        ("toString", "()Ljava/lang/String;", method_handle_to_string),
        (
            "isVarargsCollector",
            "()Z",
            method_handle_is_varargs_collector,
        ),
    ] {
        shared.register_native(METHOD_HANDLE_CLASS, name, descriptor, function);
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
    operation: &str,
) -> Result<String, VmError> {
    match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => Ok(shared
            .heap()
            .string(reference)
            .map_err(heap_error)?
            .to_owned()),
        _ => Err(VmError::Native(format!(
            "{operation} expects a non-null String argument at position {index}"
        ))),
    }
}

/// The [`ClassRef`] behind a `Class` argument.
fn class_ref_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    operation: &str,
) -> Result<ClassRef, VmError> {
    match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            Ok(class_object_of(shared, reference)?.class_ref())
        }
        _ => Err(VmError::Native(format!(
            "{operation} expects a non-null Class argument at position {index}"
        ))),
    }
}

/// The loaded class behind a `Class` argument, which must not be primitive or an array.
fn named_class_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    operation: &str,
) -> Result<ClassId, VmError> {
    match class_ref_argument(shared, args, index, operation)? {
        ClassRef::Named(class) => Ok(class),
        ClassRef::Primitive(kind) => Err(VmError::Native(format!(
            "{operation} expects a class, found the primitive class {}",
            kind.name()
        ))),
        ClassRef::Array(_) => Err(VmError::Native(format!(
            "{operation} expects a class, found an array class"
        ))),
    }
}

/// The `MethodType` receiver/argument behind `index`.
fn method_type_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    operation: &str,
) -> Result<MethodTypeObject, VmError> {
    let Some(Value::Ref(reference)) = args.get(index).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects a MethodType at position {index}"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{operation} received a null MethodType"
        )));
    }
    Ok(shared
        .heap()
        .method_type(reference)
        .map_err(heap_error)?
        .clone())
}

/// The `Lookup` receiver of a `Lookup` native.
fn lookup_of(shared: &Shared, receiver: ObjRef, operation: &str) -> Result<LookupObject, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(format!(
            "{operation} called with a null receiver"
        )));
    }
    Ok(*shared.heap().lookup(receiver).map_err(heap_error)?)
}

/// The `Lookup` argument at `index`.
fn lookup_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    operation: &str,
) -> Result<LookupObject, VmError> {
    match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            Ok(*shared.heap().lookup(reference).map_err(heap_error)?)
        }
        _ => Err(VmError::Native(format!(
            "{operation} expects a non-null MethodHandles.Lookup argument at position {index}"
        ))),
    }
}

/// The `MethodHandle` receiver of a `MethodHandle` native.
fn method_handle_of(
    shared: &Shared,
    receiver: ObjRef,
    operation: &str,
) -> Result<MethodHandleObject, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(format!(
            "{operation} called with a null receiver"
        )));
    }
    Ok(shared
        .heap()
        .method_handle(receiver)
        .map_err(heap_error)?
        .clone())
}

/// The reflected member argument at `index`.
fn member_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    operation: &str,
) -> Result<crate::ReflectMember, VmError> {
    match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => Ok(shared
            .heap()
            .reflect(reference)
            .map_err(heap_error)?
            .clone()),
        _ => Err(VmError::Native(format!(
            "{operation} expects a non-null reflected member argument at position {index}"
        ))),
    }
}

/// Allocate a lookup for `class` with `modes`.
fn allocate_lookup(shared: &Shared, class: ClassId, modes: i32) -> Value {
    Value::Ref(shared.heap().allocate_lookup(LookupObject {
        lookup_class: class,
        modes,
    }))
}

/// Allocate a `MethodType` from its parts.
fn allocate_method_type(
    shared: &Shared,
    return_type: ClassRef,
    parameters: Vec<ClassRef>,
) -> Value {
    Value::Ref(shared.heap().allocate_method_type(MethodTypeObject {
        return_type,
        parameters,
    }))
}

/// The JVM method descriptor of a `MethodType` (`(II)I`, `(Ljava/lang/String;)V`).
///
/// This is the string `MethodType.toMethodDescriptorString()` returns and the one the
/// `Lookup.find*` natives resolve a member with.
pub fn method_type_descriptor(
    shared: &Shared,
    method_type: &MethodTypeObject,
) -> Result<String, VmError> {
    let mut descriptor = String::from("(");
    for parameter in &method_type.parameters {
        descriptor.push_str(&shared.class_ref_descriptor(parameter)?);
    }
    descriptor.push(')');
    descriptor.push_str(&shared.class_ref_descriptor(&method_type.return_type)?);
    Ok(descriptor)
}

/// The JDK's `MethodType.toString` form: simple names, `(int,int)int`.
pub fn method_type_text(
    shared: &Shared,
    method_type: &MethodTypeObject,
) -> Result<String, VmError> {
    let mut text = String::from("(");
    for (index, parameter) in method_type.parameters.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&class_ref_simple_name(shared, parameter)?);
    }
    text.push(')');
    text.push_str(&class_ref_simple_name(shared, &method_type.return_type)?);
    Ok(text)
}

/// The simple name of a class ref: `int`, `String[]`, `MethodHandles$Lookup`.
fn class_ref_simple_name(shared: &Shared, class_ref: &ClassRef) -> Result<String, VmError> {
    Ok(match class_ref {
        ClassRef::Primitive(kind) => kind.name().to_owned(),
        ClassRef::Array(array) => {
            format!("{}[]", class_ref_simple_name(shared, &array.component)?)
        }
        ClassRef::Named(class) => {
            let entry = shared
                .entry(*class)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
            entry
                .name
                .rsplit('/')
                .next()
                .unwrap_or(&entry.name)
                .to_owned()
        }
    })
}

/// The dotted binary name of a loaded class, for diagnostics.
fn dotted_class_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.replace('/', "."))
        .unwrap_or_else(|| format!("class {}", class.0))
}

/// `MethodHandles.lookup()`: a full-access lookup on the placeholder class.
fn method_handles_lookup(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(LOOKUP_PLACEHOLDER_CLASS)?;
    Ok(Some(allocate_lookup(shared, class, FULL_ACCESS)))
}

/// `MethodHandles.publicLookup()`: a public-access lookup on the placeholder class.
fn method_handles_public_lookup(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(LOOKUP_PLACEHOLDER_CLASS)?;
    Ok(Some(allocate_lookup(shared, class, PUBLIC_ACCESS)))
}

/// `MethodHandles.privateLookupIn(Class, Lookup)`: a full-access lookup for the target.
///
/// The `Lookup` argument is required to be a lookup but its modes and class are not
/// checked: this VM performs no access control (documented), so any lookup can ask for
/// private access to any class. A primitive or array target is a typed error, because
/// `Lookup.lookupClass` and `Lookup.defineClass` are defined only for a loaded class.
fn method_handles_private_lookup_in(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = named_class_argument(shared, args, 0, "MethodHandles.privateLookupIn")?;
    // The lookup argument is validated (it must be a real Lookup) but its access is not
    // consulted.
    lookup_argument(shared, args, 1, "MethodHandles.privateLookupIn")?;
    Ok(Some(allocate_lookup(shared, target, FULL_ACCESS)))
}

/// `Lookup.lookupClass()`: the class this lookup resolves members in.
fn lookup_lookup_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.lookupClass")?;
    Ok(Some(Value::Ref(shared.class_object(lookup.lookup_class)?)))
}

/// `Lookup.previousLookupClass()`: always null, as this VM never chains lookups.
fn lookup_previous_lookup_class(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

/// `Lookup.lookupModes()`: the access-mode bit set.
fn lookup_lookup_modes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.lookupModes")?;
    Ok(Some(Value::Int(lookup.modes)))
}

/// `Lookup.hasPrivateAccess()`: whether `PRIVATE` is in the mode set.
fn lookup_has_private_access(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.hasPrivateAccess")?;
    Ok(Some(Value::Int(i32::from(
        lookup.modes & LOOKUP_PRIVATE != 0,
    ))))
}

/// `Lookup.hasFullPrivilegeAccess()`: `PRIVATE`, `PROTECTED` and `MODULE` are all set.
fn lookup_has_full_privilege_access(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(
        shared,
        receiver,
        "MethodHandles.Lookup.hasFullPrivilegeAccess",
    )?;
    let required = LOOKUP_PRIVATE | LOOKUP_PROTECTED | LOOKUP_MODULE;
    Ok(Some(Value::Int(i32::from(
        lookup.modes & required == required,
    ))))
}

/// `Lookup.in(Class)`: the same modes, a different lookup class.
fn lookup_in(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.in")?;
    let class = named_class_argument(shared, args, 0, "MethodHandles.Lookup.in")?;
    Ok(Some(allocate_lookup(shared, class, lookup.modes)))
}

/// `Lookup.dropLookupMode(int)`: clear the given mode bits.
///
/// Unlike the JDK, dropping every mode is accepted: with no access checks, the resulting
/// lookup is only a name for the class. A `drop` value outside the known bits is masked
/// out by the `!drop` expression, so it cannot set a mode.
fn lookup_drop_lookup_mode(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.dropLookupMode")?;
    let drop = match args.first().copied() {
        Some(Value::Int(drop)) => drop,
        _ => {
            return Err(VmError::Native(
                "MethodHandles.Lookup.dropLookupMode expects an int argument".to_owned(),
            ));
        }
    };
    Ok(Some(allocate_lookup(
        shared,
        lookup.lookup_class,
        lookup.modes & !drop,
    )))
}

/// `Lookup.toString()`: a deterministic description of the class and mode set.
fn lookup_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.toString")?;
    let text = format!(
        "Lookup[class {}, modes={}]",
        dotted_class_name(shared, lookup.lookup_class),
        lookup.modes
    );
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// The package prefix of an internal class name, or the empty string for the unnamed
/// package.
fn package_of(internal_name: &str) -> &str {
    match internal_name.rfind('/') {
        Some(index) => &internal_name[..index],
        None => "",
    }
}

/// The bytes of a heap `byte[]` argument.
///
/// Every failure is a typed [`VmError::Native`]: a null array, an array of another kind, or
/// an element that is not the `int` a `byte` slot stores. Both the sign-extended form
/// `bastore` writes and the unsigned form `String.getBytes` writes are accepted.
fn byte_array_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    operation: &str,
) -> Result<Vec<u8>, VmError> {
    let Some(Value::Ref(reference)) = args.get(index).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects a non-null byte[] argument at position {index}"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{operation} expects a non-null byte[] argument at position {index}"
        )));
    }
    let elements = {
        let heap = shared.heap();
        let array = heap.array(reference).map_err(heap_error)?;
        if array.kind != ArrayKind::Byte {
            return Err(VmError::Native(format!(
                "{operation} expects a byte[], found a {}[]",
                array.kind
            )));
        }
        array.elements.clone()
    };
    let mut bytes = Vec::with_capacity(elements.len());
    for element in elements {
        let Value::Int(value) = element else {
            return Err(VmError::Native(format!(
                "{operation} found a non-byte element in the byte[]"
            )));
        };
        if !(-128..=255).contains(&value) {
            return Err(VmError::Native(format!(
                "{operation} found {value} in the byte[], which is not a byte"
            )));
        }
        bytes.push((value & 0xff) as u8);
    }
    Ok(bytes)
}

/// `Lookup.defineClass(byte[])`: define the class in the lookup class's package and loader.
///
/// The class *name comes from the class bytes*; the JVM method descriptor of the defined
/// class is not consulted. The name must be in the lookup class's package, which is the
/// one access/package semantic CGLIB's `ReflectUtils.defineClass` relies on: it asks for a
/// private lookup on the class that generated the bytes and lets CGLIB name the generated
/// class in that same package. A definition that fails (a parse error, a verifier error, a
/// duplicate or the package mismatch) returns a typed error and leaves no class-table
/// entry, because `Shared::define_class` inserts only after verifying.
fn lookup_define_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.defineClass")?;
    let bytes = byte_array_argument(shared, args, 0, "MethodHandles.Lookup.defineClass")?;
    let file = ClassFile::parse(&bytes)?;
    let internal = file.this_class_name()?.to_owned();
    let lookup_entry = shared.entry(lookup.lookup_class).ok_or_else(|| {
        VmError::Loader(format!("class id {} is not loaded", lookup.lookup_class.0))
    })?;
    let lookup_package = package_of(&lookup_entry.name);
    let defined_package = package_of(&internal);
    if lookup_package != defined_package {
        return Err(VmError::Native(format!(
            "MethodHandles.Lookup.defineClass cannot define {internal} from a lookup on {}: \
             the class must be in the package {}",
            lookup_entry.name,
            if lookup_package.is_empty() {
                "of the unnamed package"
            } else {
                lookup_package
            }
        )));
    }
    let loader = lookup_entry.loader;
    let class = shared.define_class(loader, &internal, &bytes)?;
    Ok(Some(Value::Ref(shared.class_object(class)?)))
}

/// Resolve `name:descriptor` in `owner` or one of its superclasses/interfaces.
///
/// The walk is [`reflection::lookup_order`], which visits the class, its superclass chain
/// and its interfaces, each once. A miss is the JDK's `NoSuchMethodException`, so compiled
/// code can catch it.
fn resolve_method(
    shared: &Shared,
    owner: ClassId,
    name: &str,
    descriptor: &str,
    operation: &str,
) -> Result<ClassId, VmError> {
    for candidate in reflection::lookup_order(shared, owner)? {
        let entry = shared
            .entry(candidate)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", candidate.0)))?;
        if entry
            .file
            .find_method(name, descriptor)
            .map_err(VmError::ClassFile)?
            .is_some()
        {
            return Ok(candidate);
        }
    }
    let message = format!(
        "no such method: {}.{name}{descriptor} ({operation})",
        dotted_class_name(shared, owner)
    );
    let throwable = exceptions::new_throwable(shared, "java/lang/NoSuchMethodException", message)?;
    Err(VmError::Thrown(throwable))
}

/// Whether `name:descriptor` in `class` is a static method.
fn method_is_static(
    shared: &Shared,
    class: ClassId,
    name: &str,
    descriptor: &str,
) -> Result<bool, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let metadata = entry.metadata()?;
    let method = metadata.method(name, descriptor).ok_or_else(|| {
        VmError::Native(format!(
            "class {} has no method {name}{descriptor}",
            entry.name
        ))
    })?;
    Ok(method.is_static())
}

/// The parameter and return class refs of a method descriptor.
fn types_of_descriptor(
    shared: &Shared,
    loader: LoaderId,
    descriptor: &str,
) -> Result<(Vec<ClassRef>, ClassRef), VmError> {
    let parameters = reflection::parameter_descriptors(descriptor)?;
    let mut refs = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        refs.push(shared.class_ref_for_descriptor(loader, parameter)?);
    }
    let return_type =
        shared.class_ref_for_descriptor(loader, reflection::return_descriptor(descriptor)?)?;
    Ok((refs, return_type))
}

/// Build a `MethodHandle` value from its parts.
fn allocate_handle(
    shared: &Shared,
    owner: ClassId,
    name: &str,
    descriptor: &str,
    kind: HandleKind,
    method_type: MethodTypeObject,
) -> Value {
    Value::Ref(shared.heap().allocate_method_handle(MethodHandleObject {
        owner,
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
        kind,
        method_type,
    }))
}

/// `Lookup.findStatic(Class, String, MethodType)`.
fn lookup_find_static(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    lookup_of(shared, receiver, "MethodHandles.Lookup.findStatic")?;
    let owner = named_class_argument(shared, args, 0, "MethodHandles.Lookup.findStatic")?;
    let name = string_argument(shared, args, 1, "MethodHandles.Lookup.findStatic")?;
    let method_type = method_type_argument(shared, args, 2, "MethodHandles.Lookup.findStatic")?;
    let descriptor = method_type_descriptor(shared, &method_type)?;
    let declared = resolve_method(shared, owner, &name, &descriptor, "findStatic")?;
    if !method_is_static(shared, declared, &name, &descriptor)? {
        return Err(VmError::Native(format!(
            "MethodHandles.Lookup.findStatic found an instance method: {}.{name}{descriptor}",
            dotted_class_name(shared, declared)
        )));
    }
    Ok(Some(allocate_handle(
        shared,
        declared,
        &name,
        &descriptor,
        HandleKind::Static,
        method_type,
    )))
}

/// `Lookup.findVirtual(Class, String, MethodType)`.
///
/// The returned handle's type has the requested class as its first parameter, exactly as
/// `MethodHandle.type()` reports it on the JDK; the receiver is resolved virtually at
/// invocation time.
fn lookup_find_virtual(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    lookup_of(shared, receiver, "MethodHandles.Lookup.findVirtual")?;
    let owner = named_class_argument(shared, args, 0, "MethodHandles.Lookup.findVirtual")?;
    let name = string_argument(shared, args, 1, "MethodHandles.Lookup.findVirtual")?;
    let method_type = method_type_argument(shared, args, 2, "MethodHandles.Lookup.findVirtual")?;
    let descriptor = method_type_descriptor(shared, &method_type)?;
    let declared = resolve_method(shared, owner, &name, &descriptor, "findVirtual")?;
    if method_is_static(shared, declared, &name, &descriptor)? {
        return Err(VmError::Native(format!(
            "MethodHandles.Lookup.findVirtual found a static method: {}.{name}{descriptor}",
            dotted_class_name(shared, declared)
        )));
    }
    let mut parameters = Vec::with_capacity(method_type.parameters.len() + 1);
    parameters.push(ClassRef::Named(owner));
    parameters.extend(method_type.parameters.iter().cloned());
    let handle_type = MethodTypeObject {
        return_type: method_type.return_type,
        parameters,
    };
    Ok(Some(allocate_handle(
        shared,
        declared,
        &name,
        &descriptor,
        HandleKind::Virtual,
        handle_type,
    )))
}

/// `Lookup.findSpecial(Class, String, MethodType, Class)`.
///
/// The special-caller argument is accepted and ignored: this VM performs no access
/// control, so there is no caller identity to check. The handle runs the declaration found
/// from the requested class without virtual dispatch.
fn lookup_find_special(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    lookup_of(shared, receiver, "MethodHandles.Lookup.findSpecial")?;
    let owner = named_class_argument(shared, args, 0, "MethodHandles.Lookup.findSpecial")?;
    let name = string_argument(shared, args, 1, "MethodHandles.Lookup.findSpecial")?;
    let method_type = method_type_argument(shared, args, 2, "MethodHandles.Lookup.findSpecial")?;
    // The special caller must still be a Class; its identity is not consulted.
    named_class_argument(shared, args, 3, "MethodHandles.Lookup.findSpecial")?;
    let descriptor = method_type_descriptor(shared, &method_type)?;
    let declared = resolve_method(shared, owner, &name, &descriptor, "findSpecial")?;
    if method_is_static(shared, declared, &name, &descriptor)? {
        return Err(VmError::Native(format!(
            "MethodHandles.Lookup.findSpecial found a static method: {}.{name}{descriptor}",
            dotted_class_name(shared, declared)
        )));
    }
    let mut parameters = Vec::with_capacity(method_type.parameters.len() + 1);
    parameters.push(ClassRef::Named(owner));
    parameters.extend(method_type.parameters.iter().cloned());
    let handle_type = MethodTypeObject {
        return_type: method_type.return_type,
        parameters,
    };
    Ok(Some(allocate_handle(
        shared,
        declared,
        &name,
        &descriptor,
        HandleKind::Special,
        handle_type,
    )))
}

/// `Lookup.findConstructor(Class, MethodType)`.
///
/// The `MethodType` must return `void` (the JDK's rule); the returned handle's type returns
/// the owner class instead, which is what invoking the constructor produces.
fn lookup_find_constructor(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    lookup_of(shared, receiver, "MethodHandles.Lookup.findConstructor")?;
    let owner = named_class_argument(shared, args, 0, "MethodHandles.Lookup.findConstructor")?;
    let method_type =
        method_type_argument(shared, args, 1, "MethodHandles.Lookup.findConstructor")?;
    if method_type.return_type != ClassRef::Primitive(PrimitiveKind::Void) {
        return Err(VmError::Native(
            "MethodHandles.Lookup.findConstructor requires a MethodType returning void".to_owned(),
        ));
    }
    let descriptor = method_type_descriptor(shared, &method_type)?;
    let entry = shared
        .entry(owner)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", owner.0)))?;
    if entry
        .file
        .find_method("<init>", &descriptor)
        .map_err(VmError::ClassFile)?
        .is_none()
    {
        let message = format!(
            "no such constructor: {}.<init>{descriptor}",
            dotted_class_name(shared, owner)
        );
        let throwable =
            exceptions::new_throwable(shared, "java/lang/NoSuchMethodException", message)?;
        return Err(VmError::Thrown(throwable));
    }
    let handle_type = MethodTypeObject {
        return_type: ClassRef::Named(owner),
        parameters: method_type.parameters,
    };
    Ok(Some(allocate_handle(
        shared,
        owner,
        "<init>",
        &descriptor,
        HandleKind::Constructor,
        handle_type,
    )))
}

/// `Lookup.unreflect(Method)`: a handle for the reflected method.
fn lookup_unreflect(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    lookup_of(shared, receiver, "MethodHandles.Lookup.unreflect")?;
    let member = member_argument(shared, args, 0, "MethodHandles.Lookup.unreflect")?;
    if member.kind != crate::MemberKind::Method {
        return Err(VmError::Native(format!(
            "MethodHandles.Lookup.unreflect requires a Method, found a {}",
            member.kind.binary_name()
        )));
    }
    let loader = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?
        .loader;
    let (parameters, return_type) = types_of_descriptor(shared, loader, &member.descriptor)?;
    let is_static = method_is_static(shared, member.class, &member.name, &member.descriptor)?;
    let (kind, method_type) = if is_static {
        (
            HandleKind::Static,
            MethodTypeObject {
                return_type,
                parameters,
            },
        )
    } else {
        let mut with_receiver = Vec::with_capacity(parameters.len() + 1);
        with_receiver.push(ClassRef::Named(member.class));
        with_receiver.extend(parameters);
        (
            HandleKind::Virtual,
            MethodTypeObject {
                return_type,
                parameters: with_receiver,
            },
        )
    };
    Ok(Some(allocate_handle(
        shared,
        member.class,
        &member.name,
        &member.descriptor,
        kind,
        method_type,
    )))
}

/// `Lookup.unreflectConstructor(Constructor)`: a constructor handle.
fn lookup_unreflect_constructor(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    lookup_of(
        shared,
        receiver,
        "MethodHandles.Lookup.unreflectConstructor",
    )?;
    let member = member_argument(shared, args, 0, "MethodHandles.Lookup.unreflectConstructor")?;
    if member.kind != crate::MemberKind::Constructor {
        return Err(VmError::Native(format!(
            "MethodHandles.Lookup.unreflectConstructor requires a Constructor, found a {}",
            member.kind.binary_name()
        )));
    }
    let loader = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?
        .loader;
    let (parameters, _) = types_of_descriptor(shared, loader, &member.descriptor)?;
    let method_type = MethodTypeObject {
        return_type: ClassRef::Named(member.class),
        parameters,
    };
    Ok(Some(allocate_handle(
        shared,
        member.class,
        "<init>",
        &member.descriptor,
        HandleKind::Constructor,
        method_type,
    )))
}

/// `Lookup.findClass(String)`: load a class through the lookup class's loader.
fn lookup_find_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let lookup = lookup_of(shared, receiver, "MethodHandles.Lookup.findClass")?;
    let name = string_argument(shared, args, 0, "MethodHandles.Lookup.findClass")?;
    let internal = name.replace('.', "/");
    let loader = shared
        .entry(lookup.lookup_class)
        .ok_or_else(|| {
            VmError::Loader(format!("class id {} is not loaded", lookup.lookup_class.0))
        })?
        .loader;
    let class = match shared.load_class(loader, &internal) {
        Ok(class) => class,
        Err(VmError::ClassNotFound { .. }) => {
            let throwable = exceptions::new_throwable(
                shared,
                "java/lang/ClassNotFoundException",
                name.clone(),
            )?;
            return Err(VmError::Thrown(throwable));
        }
        Err(error) => return Err(error),
    };
    Ok(Some(Value::Ref(shared.class_object(class)?)))
}

/// The `MethodType` a `Class[]` argument names; a null array is an empty list.
fn class_array_argument(
    shared: &Shared,
    args: &[Value],
    index: usize,
    operation: &str,
) -> Result<Vec<ClassRef>, VmError> {
    let Some(Value::Ref(reference)) = args.get(index).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects a Class[] argument at position {index}"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{operation} received a null Class[] argument"
        )));
    }
    let elements = {
        let heap = shared.heap();
        let array = heap.array(reference).map_err(heap_error)?;
        if array.kind != ArrayKind::Reference {
            return Err(VmError::Native(format!(
                "{operation} expects a Class[] argument, found a {}[]",
                array.kind
            )));
        }
        array.elements.clone()
    };
    let mut refs = Vec::with_capacity(elements.len());
    for element in elements {
        let Value::Ref(class) = element else {
            return Err(VmError::Native(format!(
                "{operation} parameter classes must be Class objects"
            )));
        };
        if class.is_null() {
            return Err(VmError::Native(format!(
                "{operation} received a null parameter class"
            )));
        }
        let class_ref = class_object_of(shared, class)?.class_ref();
        if class_ref == ClassRef::Primitive(PrimitiveKind::Void) {
            return Err(VmError::Native(format!(
                "{operation} does not accept void as a parameter type"
            )));
        }
        refs.push(class_ref);
    }
    Ok(refs)
}

/// The return-type argument of a `methodType` factory; `void` is legal.
fn return_type_argument(
    shared: &Shared,
    args: &[Value],
    operation: &str,
) -> Result<ClassRef, VmError> {
    class_ref_argument(shared, args, 0, operation)
}

/// `MethodType.methodType(Class, Class[])`.
fn method_type_of_return_and_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let return_type = return_type_argument(shared, args, "MethodType.methodType")?;
    let parameters = class_array_argument(shared, args, 1, "MethodType.methodType")?;
    Ok(Some(allocate_method_type(shared, return_type, parameters)))
}

/// `MethodType.methodType(Class)`: no parameters.
fn method_type_of_return(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let return_type = return_type_argument(shared, args, "MethodType.methodType")?;
    Ok(Some(allocate_method_type(shared, return_type, Vec::new())))
}

/// `MethodType.methodType(Class, Class)`: one parameter.
fn method_type_of_return_and_one(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let return_type = return_type_argument(shared, args, "MethodType.methodType")?;
    let parameter = class_ref_argument(shared, args, 1, "MethodType.methodType")?;
    if parameter == ClassRef::Primitive(PrimitiveKind::Void) {
        return Err(VmError::Native(
            "MethodType.methodType does not accept void as a parameter type".to_owned(),
        ));
    }
    Ok(Some(allocate_method_type(
        shared,
        return_type,
        vec![parameter],
    )))
}

/// `MethodType.methodType(Class, Class, Class[])`: one parameter plus the rest.
fn method_type_of_return_and_variadic(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let return_type = return_type_argument(shared, args, "MethodType.methodType")?;
    let first = class_ref_argument(shared, args, 1, "MethodType.methodType")?;
    if first == ClassRef::Primitive(PrimitiveKind::Void) {
        return Err(VmError::Native(
            "MethodType.methodType does not accept void as a parameter type".to_owned(),
        ));
    }
    let mut parameters = vec![first];
    parameters.extend(class_array_argument(
        shared,
        args,
        2,
        "MethodType.methodType",
    )?);
    Ok(Some(allocate_method_type(shared, return_type, parameters)))
}

/// The `MethodType` receiver of a `MethodType` native.
fn method_type_receiver(
    shared: &Shared,
    receiver: ObjRef,
    operation: &str,
) -> Result<MethodTypeObject, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(format!(
            "{operation} called with a null receiver"
        )));
    }
    Ok(shared
        .heap()
        .method_type(receiver)
        .map_err(heap_error)?
        .clone())
}

/// `MethodType.returnType()`.
fn method_type_return_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.returnType")?;
    Ok(Some(Value::Ref(
        shared.class_object_for_ref(&method_type.return_type)?,
    )))
}

/// `MethodType.parameterType(int)`.
fn method_type_parameter_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.parameterType")?;
    let index = match args.first().copied() {
        Some(Value::Int(index)) => index,
        _ => {
            return Err(VmError::Native(
                "MethodType.parameterType expects an int argument".to_owned(),
            ));
        }
    };
    let index = usize::try_from(index).map_err(|_| {
        VmError::Native(format!(
            "MethodType.parameterType index {index} is negative"
        ))
    })?;
    let parameter = method_type.parameters.get(index).ok_or_else(|| {
        VmError::Native(format!(
            "MethodType.parameterType index {index} is outside {} parameter(s)",
            method_type.parameters.len()
        ))
    })?;
    Ok(Some(Value::Ref(shared.class_object_for_ref(parameter)?)))
}

/// `MethodType.parameterCount()`.
fn method_type_parameter_count(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.parameterCount")?;
    let count = i32::try_from(method_type.parameters.len()).map_err(|_| {
        VmError::Native("MethodType parameter count does not fit an int".to_owned())
    })?;
    Ok(Some(Value::Int(count)))
}

/// `MethodType.parameterArray()`: a `Class[]` of the parameter types.
fn method_type_parameter_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.parameterArray")?;
    let class_class = shared.load_library_class("java/lang/Class")?;
    let mut objects = Vec::with_capacity(method_type.parameters.len());
    for parameter in &method_type.parameters {
        objects.push(shared.class_object_for_ref(parameter)?);
    }
    let array =
        reflection::allocate_reference_array(shared, ClassRef::Named(class_class), &objects)?;
    Ok(Some(Value::Ref(array)))
}

/// `MethodType.lastParameterType()`.
fn method_type_last_parameter_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.lastParameterType")?;
    let parameter = method_type.parameters.last().ok_or_else(|| {
        VmError::Native("MethodType.lastParameterType on a type with no parameters".to_owned())
    })?;
    Ok(Some(Value::Ref(shared.class_object_for_ref(parameter)?)))
}

/// `MethodType.changeReturnType(Class)`.
fn method_type_change_return_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.changeReturnType")?;
    let return_type = class_ref_argument(shared, args, 0, "MethodType.changeReturnType")?;
    Ok(Some(allocate_method_type(
        shared,
        return_type,
        method_type.parameters,
    )))
}

/// `MethodType.appendParameterTypes(Class[])`.
fn method_type_append_parameter_types(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut method_type =
        method_type_receiver(shared, receiver, "MethodType.appendParameterTypes")?;
    method_type.parameters.extend(class_array_argument(
        shared,
        args,
        0,
        "MethodType.appendParameterTypes",
    )?);
    Ok(Some(allocate_method_type(
        shared,
        method_type.return_type,
        method_type.parameters,
    )))
}

/// `MethodType.toMethodDescriptorString()`.
fn method_type_to_descriptor_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type =
        method_type_receiver(shared, receiver, "MethodType.toMethodDescriptorString")?;
    let descriptor = method_type_descriptor(shared, &method_type)?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(descriptor))))
}

/// `MethodType.equals(Object)`: structural equality, with a non-type argument unequal.
fn method_type_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.equals")?;
    let equal = match args.first().copied() {
        Some(Value::Ref(other)) if !other.is_null() => match shared.heap().method_type(other) {
            Ok(other) => *other == method_type,
            Err(_) => false,
        },
        _ => false,
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `MethodType.hashCode()`: a deterministic hash of the descriptor.
fn method_type_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.hashCode")?;
    let descriptor = method_type_descriptor(shared, &method_type)?;
    let mut hash = 0i32;
    for byte in descriptor.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(byte));
    }
    Ok(Some(Value::Int(hash)))
}

/// `MethodType.toString()`: the JDK's simple-name form.
fn method_type_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let method_type = method_type_receiver(shared, receiver, "MethodType.toString")?;
    let text = method_type_text(shared, &method_type)?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `MethodHandle.type()`: a `MethodType` object for the stored invocation type.
fn method_handle_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let handle = method_handle_of(shared, receiver, "MethodHandle.type")?;
    Ok(Some(allocate_method_type(
        shared,
        handle.method_type.return_type,
        handle.method_type.parameters,
    )))
}

/// `MethodHandle.asType(MethodType)`: the same handle when the descriptor matches.
///
/// The VM does not implement the JDK's conversions (boxing, widening, cast). A handle
/// whose type descriptor equals the requested one is returned unchanged; any other
/// conversion is a typed error instead of a silent approximation.
fn method_handle_as_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let handle = method_handle_of(shared, receiver, "MethodHandle.asType")?;
    let requested = method_type_argument(shared, args, 0, "MethodHandle.asType")?;
    let stored = method_type_descriptor(shared, &handle.method_type)?;
    let requested = method_type_descriptor(shared, &requested)?;
    if stored != requested {
        return Err(VmError::Native(format!(
            "MethodHandle.asType does not convert {stored} to {requested}"
        )));
    }
    Ok(Some(Value::Ref(receiver)))
}

/// `MethodHandle.toString()`: a deterministic description of the target.
fn method_handle_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let handle = method_handle_of(shared, receiver, "MethodHandle.toString")?;
    let kind = match handle.kind {
        HandleKind::Static => "static",
        HandleKind::Virtual => "virtual",
        HandleKind::Special => "special",
        HandleKind::Constructor => "constructor",
    };
    let text = format!(
        "MethodHandle[{kind} {}.{}{}]",
        dotted_class_name(shared, handle.owner),
        handle.name,
        handle.descriptor
    );
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `MethodHandle.isVarargsCollector()`: always false; varargs collectors are unsupported.
fn method_handle_is_varargs_collector(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    method_handle_of(shared, receiver, "MethodHandle.isVarargsCollector")?;
    Ok(Some(Value::Int(0)))
}
