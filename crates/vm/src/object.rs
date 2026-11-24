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
        "clone",
        "()Ljava/lang/Object;",
        object_clone,
    );
    shared.register_native(
        "java/lang/Object",
        "toString",
        "()Ljava/lang/String;",
        object_to_string,
    );
    // `java.util.Objects` is a small, callback-free surface that Spring's startup path
    // uses for null checks and defensive copies.
    shared.register_native(
        "java/util/Objects",
        "requireNonNull",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        objects_require_non_null,
    );
    shared.register_native(
        "java/util/Objects",
        "requireNonNull",
        "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/Object;",
        objects_require_non_null,
    );
    shared.register_native(
        "java/util/Objects",
        "isNull",
        "(Ljava/lang/Object;)Z",
        objects_is_null,
    );
    shared.register_native(
        "java/util/Objects",
        "nonNull",
        "(Ljava/lang/Object;)Z",
        objects_non_null,
    );
    shared.register_native(
        "java/util/Objects",
        "equals",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        objects_equals,
    );
    shared.register_native(
        "java/util/Objects",
        "hashCode",
        "(Ljava/lang/Object;)I",
        objects_hash_code,
    );
    shared.register_native(
        "java/util/Objects",
        "toString",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        objects_to_string,
    );
    shared.register_native(
        "java/util/Objects",
        "toString",
        "(Ljava/lang/Object;Ljava/lang/String;)Ljava/lang/String;",
        objects_to_string_default,
    );
}

/// `Objects.requireNonNull(Object[, String])`: the same reference, or an error for null.
fn objects_require_non_null(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    if matches!(value, Value::Ref(reference) if reference.is_null()) {
        let message = match args.get(1).copied() {
            Some(Value::Ref(reference)) if !reference.is_null() => {
                format!(" (message {reference})")
            }
            _ => String::new(),
        };
        return Err(VmError::Native(format!(
            "Objects.requireNonNull received null{message}"
        )));
    }
    Ok(Some(value))
}

/// `Objects.isNull(Object)`.
fn objects_is_null(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let is_null = matches!(args.first(), Some(Value::Ref(reference)) if reference.is_null());
    Ok(Some(Value::Int(i32::from(is_null))))
}

/// `Objects.nonNull(Object)`.
fn objects_non_null(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let non_null = !matches!(args.first(), Some(Value::Ref(reference)) if reference.is_null());
    Ok(Some(Value::Int(i32::from(non_null))))
}

/// `Objects.equals(Object, Object)`: null-safe equality with the VM's content rules.
fn objects_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let right = args.get(1).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let (Value::Ref(left_ref), Value::Ref(right_ref)) = (left, right) else {
        return Ok(Some(Value::Int(i32::from(left == right))));
    };
    if left_ref.is_null() || right_ref.is_null() {
        return Ok(Some(Value::Int(i32::from(left_ref == right_ref))));
    }
    let equal = {
        let heap = shared.heap();
        match (
            heap.get(left_ref)
                .map_err(|error| VmError::Native(error.to_string()))?,
            heap.get(right_ref)
                .map_err(|error| VmError::Native(error.to_string()))?,
        ) {
            (HeapObject::Str(left), HeapObject::Str(right)) => left == right,
            (HeapObject::Boxed(left), HeapObject::Boxed(right)) => left.java_equals(*right),
            _ => left_ref == right_ref,
        }
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `Objects.hashCode(Object)`: 0 for null, otherwise the VM's hash for the value.
fn objects_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    match args.first().copied() {
        Some(Value::Ref(reference)) if reference.is_null() => Ok(Some(Value::Int(0))),
        Some(Value::Ref(reference)) => {
            let hash = {
                let heap = shared.heap();
                match heap
                    .get(reference)
                    .map_err(|error| VmError::Native(error.to_string()))?
                {
                    HeapObject::Str(text) => {
                        let mut hash = 0i32;
                        for unit in text.encode_utf16() {
                            hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
                        }
                        hash
                    }
                    HeapObject::Boxed(boxed) => boxed.java_hash_code(),
                    _ => reference.identity_hash(),
                }
            };
            Ok(Some(Value::Int(hash)))
        }
        Some(value) => Ok(Some(Value::Int(value.as_int().unwrap_or(0)))),
        None => Ok(Some(Value::Int(0))),
    }
}

/// `Objects.toString(Object)`: `"null"` for null, the value's text otherwise.
fn objects_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = match args.first().copied() {
        Some(Value::Ref(reference)) => {
            crate::string::object_text(shared, reference, "Objects.toString")?
        }
        Some(Value::Int(value)) => value.to_string(),
        Some(Value::Long(value)) => value.to_string(),
        Some(Value::Float(value)) => crate::string::float_text(value),
        Some(Value::Double(value)) => crate::string::double_text(value),
        None => "null".to_owned(),
    };
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `Objects.toString(Object, String)`: the default for null.
fn objects_to_string_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    if matches!(args.first(), Some(Value::Ref(reference)) if reference.is_null()) {
        return Ok(Some(
            args.get(1).copied().unwrap_or(Value::Ref(ObjRef::NULL)),
        ));
    }
    objects_to_string(shared, _thread, _loader, _receiver, args)
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
    // A `java.lang.reflect.Type` object prints its type name, as the JDK's
    // `ParameterizedTypeImpl`/`TypeVariableImpl` overrides do; a class object keeps the
    // `ClassName@hash` shape.
    if let Some(text) = crate::generic::type_text(shared, receiver)? {
        let reference = shared.heap().allocate_string(text);
        return Ok(Some(Value::Ref(reference)));
    }
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
        StringBuffer,
        MessageFormat,
        Named(ClassId),
        Array(ClassRef),
        Lambda(ClassId),
        Type(&'static str),
        LogLevel,
        Atomic(&'static str),
        Lock,
        Condition,
        Pattern,
        Matcher,
        Enumeration,
        Stream,
        Spliterator,
        Collector,
        Optional,
        Sink,
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
            HeapObject::StringBuffer(_) => Kind::StringBuffer,
            HeapObject::MessageFormat(_) => Kind::MessageFormat,
            HeapObject::List(list) => Kind::Named(list.class),
            HeapObject::Set(set) => Kind::Named(set.class),
            HeapObject::Map(map) => Kind::Named(map.class),
            HeapObject::Iterator(iterator) => Kind::Named(iterator.class),
            HeapObject::MapEntry(_) => Kind::Named(
                shared
                    .load_library_class(crate::maps::MAP_ENTRY_CLASS)
                    .map_err(|error| VmError::Native(error.to_string()))?,
            ),
            HeapObject::LogLevel(_) => Kind::LogLevel,
            HeapObject::Atomic(cell) => Kind::Atomic(cell.class_name()),
            HeapObject::Lock(_) => Kind::Lock,
            HeapObject::Condition(_) => Kind::Condition,
            HeapObject::Pattern(_) => Kind::Pattern,
            HeapObject::Matcher(_) => Kind::Matcher,
            HeapObject::Enumeration(_) => Kind::Enumeration,
            HeapObject::Stream(_) => Kind::Stream,
            HeapObject::Spliterator(_) => Kind::Spliterator,
            HeapObject::Collector(_) => Kind::Collector,
            HeapObject::Optional(_) => Kind::Optional,
            HeapObject::Sink(_) => Kind::Sink,
            HeapObject::Array(array) => Kind::Array(array.component.clone()),
            HeapObject::Lambda(lambda) => Kind::Lambda(lambda.interface),
            HeapObject::Type(ty) => Kind::Type(ty.class_name()),
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
        Kind::StringBuffer => Ok("java.lang.StringBuffer".to_owned()),
        Kind::MessageFormat => Ok("java.text.MessageFormat".to_owned()),
        Kind::Named(class) => {
            let entry = shared
                .entry(class)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
            Ok(entry.name.replace('/', "."))
        }
        Kind::Array(component) => shared.class_ref_name(&ClassRef::array(component)),
        Kind::Type(name) => Ok(name.replace('/', ".")),
        Kind::LogLevel => Ok("java.util.logging.Level".to_owned()),
        Kind::Atomic(name) => Ok(name.replace('/', ".")),
        Kind::Lock => Ok("java.util.concurrent.locks.ReentrantLock".to_owned()),
        Kind::Condition => Ok("java.util.concurrent.locks.Condition".to_owned()),
        Kind::Pattern => Ok("java.util.regex.Pattern".to_owned()),
        Kind::Matcher => Ok("java.util.regex.Matcher".to_owned()),
        Kind::Enumeration => Ok("java.util.Enumeration".to_owned()),
        Kind::Stream => Ok("java.util.stream.Stream".to_owned()),
        Kind::Spliterator => Ok("java.util.Spliterator".to_owned()),
        Kind::Collector => Ok("java.util.stream.Collector".to_owned()),
        Kind::Optional => Ok("java.util.Optional".to_owned()),
        Kind::Sink => Ok("java.util.function.Consumer".to_owned()),
    }
}

/// `Object.clone()`: a shallow copy of an instance or an array.
///
/// The JDK's `CloneNotSupportedException` for a class that does not implement
/// `Cloneable` is not synthesized; a receiver that is neither an instance nor an array is
/// a typed error instead.
fn object_clone(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "Object.clone called with a null receiver".to_owned(),
        ));
    }
    enum Shape {
        Instance(crate::ClassId, Vec<Value>),
        Array(crate::ClassRef, Vec<Value>),
        Other,
    }
    let shape = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::HeapObject::Instance(instance) => {
                Shape::Instance(instance.class, instance.fields.clone())
            }
            crate::HeapObject::Array(array) => {
                Shape::Array(array.component.clone(), array.elements.clone())
            }
            _ => Shape::Other,
        }
    };
    match shape {
        Shape::Instance(class, fields) => {
            let copy = shared.allocate_instance(class)?;
            {
                let mut heap = shared.heap();
                let slots = heap
                    .object_fields_mut(copy)
                    .map_err(|error| VmError::Native(error.to_string()))?;
                slots.clone_from(&fields);
            }
            Ok(Some(Value::Ref(copy)))
        }
        Shape::Array(component, elements) => {
            let copy = shared.heap().allocate_array(component, elements.len());
            {
                let mut heap = shared.heap();
                let slots = &mut heap
                    .array_mut(copy)
                    .map_err(|error| VmError::Native(error.to_string()))?
                    .elements;
                slots.clone_from(&elements);
            }
            Ok(Some(Value::Ref(copy)))
        }
        Shape::Other => Err(VmError::Native(
            "Object.clone receiver is not an instance or an array".to_owned(),
        )),
    }
}
