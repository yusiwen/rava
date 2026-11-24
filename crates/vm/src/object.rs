//! `java.lang.Object` natives (M2.9).
//!
//! The three methods every object answers are here: `equals` (reference identity),
//! `hashCode` (the [`ObjRef`] identity hash) and `toString`
//! (`ClassName@hexHashCode` with the runtime class's dotted binary name). `getClass`
//! stays in [`crate::reflection`]: M2.4 owns it and this module does not replace it.
//!
//! The natives are registry entries on `java/lang/Object`, so they win over the C1 stub's
//! declaration even though `equals` and `toString` are not marked `ACC_NATIVE` and no
//! stub carries a `Code` attribute. A subclass that declares its own `equals`,
//! `hashCode` or `toString` resolves to that declaration first; only a call that reaches
//! `java/lang/Object` reaches these implementations.

use crate::VmError;
use crate::heap::{ClassRef, HeapObject, MemberKind, ObjRef};
use crate::loader::{ClassId, LoaderId};
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;
/// Registers the M2.9 `java.lang.Object` natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        "java/lang/Object",
        "equals",
        "(Ljava/lang/Object;)Z",
        object_equals,
    );
    shared.register_native("java/lang/Object", "hashCode", "()I", object_hash_code);
    shared.register_native(
        "java/lang/Object",
        "toString",
        "()Ljava/lang/String;",
        object_to_string,
    );
}

/// `Object.equals(Object)`: reference identity, the JDK's default for a class that does
/// not override it. A null argument is not equal to anything, including itself.
fn object_equals(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "Object.equals called with a null receiver".to_owned(),
        ));
    }
    let argument = match args.first().copied() {
        Some(Value::Ref(reference)) => reference,
        Some(_) => {
            return Err(VmError::Native(
                "Object.equals expects a reference argument".to_owned(),
            ));
        }
        None => {
            return Err(VmError::Native(
                "Object.equals is missing its argument".to_owned(),
            ));
        }
    };
    Ok(Some(Value::Int(i32::from(receiver == argument))))
}

/// `Object.hashCode()`: the stable identity hash derived from the object's [`ObjRef`].
fn object_hash_code(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "Object.hashCode called with a null receiver".to_owned(),
        ));
    }
    Ok(Some(Value::Int(receiver.identity_hash())))
}

/// `Object.toString()`: `ClassName@hexHashCode`, as on the JDK.
///
/// `ClassName` is the runtime class's binary name with dots (`java.lang.Object`) and the
/// hash is [`ObjRef::identity_hash`] formatted as an unsigned hexadecimal int, which is
/// what `Integer.toHexString(hashCode())` prints for a negative hash.
fn object_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class_name = runtime_binary_name(shared, receiver)?;
    let text = format!("{class_name}@{:x}", receiver.identity_hash() as u32);
    let reference = shared.heap().allocate_string(text);
    Ok(Some(Value::Ref(reference)))
}

/// The dotted binary name of a receiver's runtime class.
///
/// An instance reports its defining class, a string `java.lang.String`, a class object
/// `java.lang.Class`, a reflected member its `java.lang.reflect` class, a box its boxed
/// class (`java.lang.Integer`), a builder `java.lang.StringBuilder`, a list
/// `java.util.ArrayList`, a map `java.util.HashMap` and a lambda its functional interface.
/// An array reports its array class's
/// name (`[I`, `[Ljava.lang.String;`), which is the JDK's default `Object.toString` shape.
/// The three VM-level containers use the bootstrap class library as their identity.
///
/// [`crate::string::object_text`] reuses this for `String.valueOf(Object)` and
/// [`crate::collections`] for `StringBuilder.append(Object)`.
pub(crate) fn runtime_binary_name(shared: &Shared, receiver: ObjRef) -> Result<String, VmError> {
    enum Kind {
        Instance(ClassId),
        Str,
        Class,
        Reflect(MemberKind),
        Boxed(&'static str),
        Annotation,
        Enum(ClassId),
        StringBuilder,
        List,
        Map,
        Array(ClassRef),
        Lambda(ClassId),
    }

    let kind = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            HeapObject::Instance(instance) => Kind::Instance(instance.class),
            HeapObject::Str(_) => Kind::Str,
            HeapObject::Class(_) => Kind::Class,
            HeapObject::PrimitiveClass(_) => Kind::Class,
            HeapObject::ArrayClass(_) => Kind::Class,
            HeapObject::Reflect(member) => Kind::Reflect(member.kind),
            HeapObject::Boxed(boxed) => Kind::Boxed(boxed.binary_name()),
            HeapObject::Annotation(_) => Kind::Annotation,
            HeapObject::Enum(constant) => Kind::Enum(constant.class),
            HeapObject::StringBuilder(_) => Kind::StringBuilder,
            HeapObject::List(_) => Kind::List,
            HeapObject::Map(_) => Kind::Map,
            HeapObject::Array(array) => Kind::Array(array.component.clone()),
            HeapObject::Lambda(lambda) => Kind::Lambda(lambda.interface),
        }
    };

    match kind {
        Kind::Instance(class) => {
            let entry = shared
                .entry(class)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
            Ok(entry.name.replace('/', "."))
        }
        Kind::Lambda(interface) => {
            let entry = shared.entry(interface).ok_or_else(|| {
                VmError::Loader(format!("class id {} is not loaded", interface.0))
            })?;
            Ok(entry.name.replace('/', "."))
        }
        Kind::Str => Ok("java.lang.String".to_owned()),
        Kind::Class => Ok("java.lang.Class".to_owned()),
        Kind::Reflect(kind) => Ok(kind.binary_name().to_owned()),
        Kind::Boxed(name) => Ok(name.to_owned()),
        Kind::Annotation => Ok("java.lang.annotation.Annotation".to_owned()),
        Kind::Enum(class) => {
            let entry = shared
                .entry(class)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
            Ok(entry.name.replace('/', "."))
        }
        Kind::StringBuilder => Ok("java.lang.StringBuilder".to_owned()),
        Kind::List => Ok("java.util.ArrayList".to_owned()),
        Kind::Map => Ok("java.util.HashMap".to_owned()),
        Kind::Array(component) => shared.class_ref_name(&ClassRef::array(component)),
    }
}
