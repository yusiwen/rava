//! Boxed primitive values and the `java.lang.Integer` natives (M2.10).
//!
//! A box is [`crate::HeapObject::Boxed`] holding a [`BoxedPrimitive::Integer`]. It is an
//! ordinary heap object rather than a second [`crate::Value`] variant, so it can live in
//! an `Object`-typed slot, `getClass` reports `java.lang.Integer`, and
//! `java.lang.Object`'s reference-identity methods still apply when they are reached
//! directly. The registry entries here override the C1 stub, which carries neither
//! `Code` nor, for most methods, `ACC_NATIVE`.
//!
//! M2.10 models one box. `Boolean` and `Long` boxes, the small-integer cache, the
//! `String`-taking `valueOf`/`parseInt` overloads and the unsigned/format families are
//! later slices. [`unbox_argument`] and [`box_result`] are the two conversion helpers the
//! reflective invocation path shares: `int` is boxed and unboxed, a reference is passed
//! through with its [`crate::ObjRef`] untouched, and every other primitive descriptor is
//! a typed error rather than a guess.

use crate::VmError;
use crate::heap::{BoxedPrimitive, HeapError, HeapObject, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// Registers the M2.10 `java.lang.Integer` natives.
///
/// The entries are keyed by the class that declares the Java method. `valueOf`,
/// `parseInt`, the static `toString(int)` and the boxing conversions are
/// `invokestatic` targets; `intValue`, `toString()`, `equals`, `hashCode` and
/// `compareTo(Integer)` are `invokevirtual` targets on a boxed receiver.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        "java/lang/Integer",
        "valueOf",
        "(I)Ljava/lang/Integer;",
        integer_value_of,
    );
    shared.register_native("java/lang/Integer", "intValue", "()I", integer_int_value);
    shared.register_native(
        "java/lang/Integer",
        "toString",
        "()Ljava/lang/String;",
        integer_to_string,
    );
    shared.register_native(
        "java/lang/Integer",
        "toString",
        "(I)Ljava/lang/String;",
        integer_to_string_of,
    );
    shared.register_native(
        "java/lang/Integer",
        "equals",
        "(Ljava/lang/Object;)Z",
        integer_equals,
    );
    shared.register_native("java/lang/Integer", "hashCode", "()I", integer_hash_code);
    shared.register_native(
        "java/lang/Integer",
        "compareTo",
        "(Ljava/lang/Integer;)I",
        integer_compare_to,
    );
    shared.register_native(
        "java/lang/Integer",
        "parseInt",
        "(Ljava/lang/String;)I",
        integer_parse_int,
    );
}

/// Whether a descriptor names a reference type (`L...;` or `[...`).
pub fn is_reference_descriptor(descriptor: &str) -> bool {
    descriptor.starts_with('L') || descriptor.starts_with('[')
}

/// Convert one argument for a parameter of `descriptor`.
///
/// `I` takes a `Value::Ref` to a boxed `Integer` and returns its `Value::Int`; a
/// reference descriptor passes the same handle through, null included. Every other
/// primitive descriptor is an error, because M2.10 has no `Boolean`, `Long`, `Float` or
/// `Double` box.
pub fn unbox_argument(shared: &Shared, descriptor: &str, value: Value) -> Result<Value, VmError> {
    if descriptor == "I" {
        return match value {
            Value::Ref(reference) => Ok(Value::Int(shared.unbox_int(reference)?)),
            other => Err(VmError::Native(format!(
                "expected a boxed Integer for an int parameter, found {other:?}"
            ))),
        };
    }
    if is_reference_descriptor(descriptor) {
        return match value {
            Value::Ref(reference) => Ok(Value::Ref(reference)),
            other => Err(VmError::Native(format!(
                "expected a reference for a {descriptor} parameter, found {other:?}"
            ))),
        };
    }
    Err(VmError::Native(format!(
        "only int and reference values are supported (M2.10): {descriptor}"
    )))
}

/// Convert an invoked method's result back to the reflective `Object` result.
///
/// A primitive `int` is boxed; a reference passes through unchanged, which is what keeps
/// a returned string reference-identical; `void` becomes null, as `Method.invoke` and
/// `Constructor.newInstance` return null for a void method.
pub fn box_result(
    shared: &Shared,
    descriptor: &str,
    value: Option<Value>,
) -> Result<Value, VmError> {
    if descriptor == "V" {
        return match value {
            None => Ok(Value::Ref(ObjRef::NULL)),
            Some(value) => Err(VmError::Native(format!("a void method returned {value:?}"))),
        };
    }
    if descriptor == "I" {
        return match value {
            Some(Value::Int(value)) => Ok(Value::Ref(shared.box_int(value))),
            Some(other) => Err(VmError::Native(format!("an int method returned {other:?}"))),
            None => Err(VmError::Native(
                "an int method returned no value".to_owned(),
            )),
        };
    }
    if is_reference_descriptor(descriptor) {
        return match value {
            Some(Value::Ref(reference)) => Ok(Value::Ref(reference)),
            Some(other) => Err(VmError::Native(format!(
                "a {descriptor} method returned {other:?}"
            ))),
            None => Err(VmError::Native(format!(
                "a {descriptor} method returned no value"
            ))),
        };
    }
    Err(VmError::Native(format!(
        "only int, reference and void results are supported (M2.10): {descriptor}"
    )))
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn int_argument(args: &[Value], position: usize, method: &str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects an int argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &str,
) -> Result<String, VmError> {
    let Some(Value::Ref(reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{method} expects a String argument"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{method} received a null String argument"
        )));
    }
    let text = shared
        .heap()
        .string(reference)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

/// `Integer.valueOf(int)`: a fresh box. There is no small-value cache in M2.10.
fn integer_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Integer.valueOf")?;
    Ok(Some(Value::Ref(shared.box_int(value))))
}

/// `Integer.intValue()`: the primitive inside the receiver's box.
fn integer_int_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(shared.unbox_int(receiver)?)))
}

/// `Integer.toString()`: the decimal text of the receiver's value.
fn integer_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.unbox_int(receiver)?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(value.to_string()),
    )))
}

/// `Integer.toString(int)`: the decimal text of an argument.
fn integer_to_string_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Integer.toString")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(value.to_string()),
    )))
}

/// `Integer.equals(Object)`: value equality against another boxed `Integer`.
///
/// Null and any non-`Integer` reference are false, as on the JDK. A reference to a box
/// of another kind cannot exist in M2.10, so the only matching shape is a boxed `int`.
fn integer_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.unbox_int(receiver)?;
    let Some(Value::Ref(argument)) = args.first().copied() else {
        return Err(VmError::Native(
            "Integer.equals expects an Object argument".to_owned(),
        ));
    };
    if argument.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    let equal = {
        let heap = shared.heap();
        match heap.get(argument).map_err(heap_error)? {
            HeapObject::Boxed(BoxedPrimitive::Integer(other)) => *other == value,
            _ => false,
        }
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `Integer.hashCode()`: the value itself, as the JDK defines it.
fn integer_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(shared.unbox_int(receiver)?)))
}

/// `Integer.compareTo(Integer)`: -1, 0 or 1 by value.
fn integer_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.unbox_int(receiver)?;
    let Some(Value::Ref(argument)) = args.first().copied() else {
        return Err(VmError::Native(
            "Integer.compareTo expects an Integer argument".to_owned(),
        ));
    };
    let other = shared.unbox_int(argument)?;
    let ordering = match value.cmp(&other) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    };
    Ok(Some(Value::Int(ordering)))
}

/// `Integer.parseInt(String)`: a decimal `int` in the JDK's accepted syntax.
///
/// Rust's integer parser accepts an optional sign and ASCII decimal digits, the same
/// shape `Integer.parseInt` accepts for the values M2.10 needs. An out-of-range or
/// malformed value is a typed error, not a synthesized `NumberFormatException`.
fn integer_parse_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "Integer.parseInt")?;
    let value = text
        .parse::<i32>()
        .map_err(|_| VmError::Native(format!("Integer.parseInt: {text:?} is not a decimal int")))?;
    Ok(Some(Value::Int(value)))
}
