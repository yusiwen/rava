//! Boxed primitive values and the primitive-wrapper natives (M2.10, M2.25).
//!
//! A box is [`crate::HeapObject::Boxed`] holding a [`BoxedPrimitive`]. It is an ordinary
//! heap object rather than a second [`crate::Value`] variant, so it can live in an
//! `Object`-typed slot, `getClass` reports the boxed class, and `java.lang.Object`'s
//! reference-identity methods still apply when they are reached directly. The registry
//! entries here override the C1 stub, which carries neither `Code` nor, for most methods,
//! `ACC_NATIVE`.
//!
//! M2.25 models the whole family: `Byte`, `Short`, `Integer`, `Long`, `Float` and
//! `Double` (the `java.lang.Number` subclasses, with every accessor the JDK defines)
//! plus `Character` and `Boolean`. Each wrapper gets `valueOf`, its `xxxValue` accessors,
//! `parseXxx`, both `toString` forms, `equals`, `hashCode` and `compareTo`/`compare`.
//! There is no small-value cache, so every `valueOf` allocates a new object. A malformed
//! `parseXxx` is a typed [`crate::VmError::Native`], not a synthesized
//! `NumberFormatException`. The float parsers are Rust's: a hex floating literal is
//! rejected, and the case-insensitive `inf`/`infinity`/`nan` spellings are accepted where
//! Java's grammar is `Infinity`/`NaN`; the integer parsers accept ASCII digits only.
//!
//! [`unbox_argument`] and [`box_result`] are the two conversion helpers the reflective
//! invocation path shares: every primitive descriptor is boxed and unboxed through its
//! own wrapper, a numeric parameter accepts a narrower numeric wrapper by Java's widening
//! rule (JLS §5.3), a reference is passed through with its [`crate::ObjRef`] untouched,
//! and a box of the wrong kind is a typed error rather than a guess.

use crate::VmError;
use crate::heap::{BoxedPrimitive, HeapError, ObjRef, canonical_double_bits, canonical_float_bits};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// Registers the primitive wrapper and `java.lang.Number` natives.
///
/// The entries are keyed by the class that declares the Java method. `valueOf`, the
/// `parse*` family and the static `toString`/`hashCode`/`compare` helpers are
/// `invokestatic` targets; `xxxValue`, `toString()`, `equals`, `hashCode` and
/// `compareTo(same box)` are `invokevirtual` targets on a boxed receiver.
///
/// The `Number` accessors are registered twice on purpose: on each numeric wrapper
/// (where the JDK declares an override) and on `java.lang.Number` itself, so a call that
/// resolves to the abstract superclass still finds an implementation.
pub fn register_natives(shared: &Shared) {
    register_integer_natives(shared);
    register_byte_natives(shared);
    register_short_natives(shared);
    register_long_natives(shared);
    register_float_natives(shared);
    register_double_natives(shared);
    register_number_natives(shared);
    register_character_natives(shared);
    register_boolean_natives(shared);
}

/// Register the `java.lang.Integer` natives plus its inherited `Number` accessors.
fn register_integer_natives(shared: &Shared) {
    let class = "java/lang/Integer";
    shared.register_native(class, "valueOf", "(I)Ljava/lang/Integer;", integer_value_of);
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;)Ljava/lang/Integer;",
        integer_value_of_string,
    );
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;I)Ljava/lang/Integer;",
        integer_value_of_string_radix,
    );
    shared.register_native(class, "intValue", "()I", box_int_value);
    shared.register_native(class, "toString", "()Ljava/lang/String;", box_to_string);
    shared.register_native(
        class,
        "toString",
        "(I)Ljava/lang/String;",
        integer_to_string_of,
    );
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", box_equals);
    shared.register_native(class, "hashCode", "()I", box_hash_code);
    shared.register_native(class, "hashCode", "(I)I", integer_hash_code_of);
    shared.register_native(class, "compareTo", "(Ljava/lang/Integer;)I", box_compare_to);
    shared.register_native(class, "compare", "(II)I", integer_compare);
    shared.register_native(
        class,
        "toHexString",
        "(I)Ljava/lang/String;",
        integer_to_hex_string,
    );
    shared.register_native(
        class,
        "toBinaryString",
        "(I)Ljava/lang/String;",
        integer_to_binary_string,
    );
    shared.register_native(
        class,
        "toOctalString",
        "(I)Ljava/lang/String;",
        integer_to_octal_string,
    );
    shared.register_native(
        class,
        "parseInt",
        "(Ljava/lang/String;)I",
        integer_parse_int,
    );
    shared.register_native(
        class,
        "parseInt",
        "(Ljava/lang/String;I)I",
        integer_parse_int_radix,
    );
    register_number_accessors(shared, class);
}

/// Register the `java.lang.Byte` natives.
fn register_byte_natives(shared: &Shared) {
    let class = "java/lang/Byte";
    shared.register_native(class, "valueOf", "(B)Ljava/lang/Byte;", byte_value_of);
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;)Ljava/lang/Byte;",
        byte_value_of_string,
    );
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;I)Ljava/lang/Byte;",
        byte_value_of_string_radix,
    );
    shared.register_native(class, "byteValue", "()B", box_byte_value);
    shared.register_native(class, "toString", "()Ljava/lang/String;", box_to_string);
    shared.register_native(
        class,
        "toString",
        "(B)Ljava/lang/String;",
        byte_to_string_of,
    );
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", box_equals);
    shared.register_native(class, "hashCode", "()I", box_hash_code);
    shared.register_native(class, "hashCode", "(B)I", byte_hash_code_of);
    shared.register_native(class, "compareTo", "(Ljava/lang/Byte;)I", box_compare_to);
    shared.register_native(class, "compare", "(BB)I", byte_compare);
    shared.register_native(class, "parseByte", "(Ljava/lang/String;)B", byte_parse);
    shared.register_native(
        class,
        "parseByte",
        "(Ljava/lang/String;I)B",
        byte_parse_radix,
    );
    register_number_accessors(shared, class);
}

/// Register the `java.lang.Short` natives.
fn register_short_natives(shared: &Shared) {
    let class = "java/lang/Short";
    shared.register_native(class, "valueOf", "(S)Ljava/lang/Short;", short_value_of);
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;)Ljava/lang/Short;",
        short_value_of_string,
    );
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;I)Ljava/lang/Short;",
        short_value_of_string_radix,
    );
    shared.register_native(class, "shortValue", "()S", box_short_value);
    shared.register_native(class, "toString", "()Ljava/lang/String;", box_to_string);
    shared.register_native(
        class,
        "toString",
        "(S)Ljava/lang/String;",
        short_to_string_of,
    );
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", box_equals);
    shared.register_native(class, "hashCode", "()I", box_hash_code);
    shared.register_native(class, "hashCode", "(S)I", short_hash_code_of);
    shared.register_native(class, "compareTo", "(Ljava/lang/Short;)I", box_compare_to);
    shared.register_native(class, "compare", "(SS)I", short_compare);
    shared.register_native(class, "parseShort", "(Ljava/lang/String;)S", short_parse);
    shared.register_native(
        class,
        "parseShort",
        "(Ljava/lang/String;I)S",
        short_parse_radix,
    );
    register_number_accessors(shared, class);
}

/// Register the `java.lang.Long` natives.
fn register_long_natives(shared: &Shared) {
    let class = "java/lang/Long";
    shared.register_native(class, "valueOf", "(J)Ljava/lang/Long;", long_value_of);
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;)Ljava/lang/Long;",
        long_value_of_string,
    );
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;I)Ljava/lang/Long;",
        long_value_of_string_radix,
    );
    shared.register_native(class, "longValue", "()J", box_long_value);
    shared.register_native(class, "toString", "()Ljava/lang/String;", box_to_string);
    shared.register_native(
        class,
        "toString",
        "(J)Ljava/lang/String;",
        long_to_string_of,
    );
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", box_equals);
    shared.register_native(class, "hashCode", "()I", box_hash_code);
    shared.register_native(class, "hashCode", "(J)I", long_hash_code_of);
    shared.register_native(class, "compareTo", "(Ljava/lang/Long;)I", box_compare_to);
    shared.register_native(class, "compare", "(JJ)I", long_compare);
    shared.register_native(class, "parseLong", "(Ljava/lang/String;)J", long_parse);
    shared.register_native(
        class,
        "parseLong",
        "(Ljava/lang/String;I)J",
        long_parse_radix,
    );
    shared.register_native(
        class,
        "toHexString",
        "(J)Ljava/lang/String;",
        long_to_hex_string,
    );
    register_number_accessors(shared, class);
}

/// Register the `java.lang.Float` natives.
fn register_float_natives(shared: &Shared) {
    let class = "java/lang/Float";
    shared.register_native(class, "valueOf", "(F)Ljava/lang/Float;", float_value_of);
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;)Ljava/lang/Float;",
        float_value_of_string,
    );
    shared.register_native(class, "floatValue", "()F", box_float_value);
    shared.register_native(class, "toString", "()Ljava/lang/String;", box_to_string);
    shared.register_native(
        class,
        "toString",
        "(F)Ljava/lang/String;",
        float_to_string_of,
    );
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", box_equals);
    shared.register_native(class, "hashCode", "()I", box_hash_code);
    shared.register_native(class, "hashCode", "(F)I", float_hash_code_of);
    shared.register_native(class, "compareTo", "(Ljava/lang/Float;)I", box_compare_to);
    shared.register_native(class, "compare", "(FF)I", float_compare);
    shared.register_native(class, "parseFloat", "(Ljava/lang/String;)F", float_parse);
    shared.register_native(class, "isNaN", "()Z", box_is_nan);
    shared.register_native(class, "isNaN", "(F)Z", float_is_nan);
    shared.register_native(class, "isInfinite", "()Z", box_is_infinite);
    shared.register_native(class, "isInfinite", "(F)Z", float_is_infinite);
    shared.register_native(class, "isFinite", "(F)Z", float_is_finite);
    shared.register_native(class, "floatToIntBits", "(F)I", float_to_int_bits);
    register_number_accessors(shared, class);
}

/// Register the `java.lang.Double` natives.
fn register_double_natives(shared: &Shared) {
    let class = "java/lang/Double";
    shared.register_native(class, "valueOf", "(D)Ljava/lang/Double;", double_value_of);
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;)Ljava/lang/Double;",
        double_value_of_string,
    );
    shared.register_native(class, "doubleValue", "()D", box_double_value);
    shared.register_native(class, "toString", "()Ljava/lang/String;", box_to_string);
    shared.register_native(
        class,
        "toString",
        "(D)Ljava/lang/String;",
        double_to_string_of,
    );
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", box_equals);
    shared.register_native(class, "hashCode", "()I", box_hash_code);
    shared.register_native(class, "hashCode", "(D)I", double_hash_code_of);
    shared.register_native(class, "compareTo", "(Ljava/lang/Double;)I", box_compare_to);
    shared.register_native(class, "compare", "(DD)I", double_compare);
    shared.register_native(class, "parseDouble", "(Ljava/lang/String;)D", double_parse);
    shared.register_native(class, "isNaN", "()Z", box_is_nan);
    shared.register_native(class, "isNaN", "(D)Z", double_is_nan);
    shared.register_native(class, "isInfinite", "()Z", box_is_infinite);
    shared.register_native(class, "isInfinite", "(D)Z", double_is_infinite);
    shared.register_native(class, "isFinite", "(D)Z", double_is_finite);
    shared.register_native(class, "doubleToLongBits", "(D)J", double_to_long_bits);
    register_number_accessors(shared, class);
}

/// Register the six `Number` conversion methods on one numeric wrapper class.
fn register_number_accessors(shared: &Shared, class: &str) {
    shared.register_native(class, "byteValue", "()B", box_byte_value);
    shared.register_native(class, "shortValue", "()S", box_short_value);
    shared.register_native(class, "intValue", "()I", box_int_value);
    shared.register_native(class, "longValue", "()J", box_long_value);
    shared.register_native(class, "floatValue", "()F", box_float_value);
    shared.register_native(class, "doubleValue", "()D", box_double_value);
}

/// Register the `java.lang.Number` accessors.
///
/// A wrapper class declares its own overrides, so these entries are the fallback for a
/// class whose stub inherits the method; the receiver-driven implementations work for
/// any numeric box.
fn register_number_natives(shared: &Shared) {
    register_number_accessors(shared, "java/lang/Number");
}

/// Register the `java.lang.Boolean` natives.
fn register_boolean_natives(shared: &Shared) {
    let class = "java/lang/Boolean";
    shared.register_native(class, "valueOf", "(Z)Ljava/lang/Boolean;", boolean_value_of);
    shared.register_native(
        class,
        "valueOf",
        "(Ljava/lang/String;)Ljava/lang/Boolean;",
        boolean_value_of_string,
    );
    shared.register_native(class, "booleanValue", "()Z", boolean_boolean_value);
    shared.register_native(
        class,
        "parseBoolean",
        "(Ljava/lang/String;)Z",
        boolean_parse,
    );
    shared.register_native(
        class,
        "toString",
        "(Z)Ljava/lang/String;",
        boolean_to_string_of,
    );
    shared.register_native(class, "toString", "()Ljava/lang/String;", boolean_to_string);
    shared.register_native(class, "hashCode", "()I", boolean_hash_code);
    shared.register_native(class, "hashCode", "(Z)I", boolean_hash_code_of);
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", boolean_equals);
    shared.register_native(
        class,
        "compareTo",
        "(Ljava/lang/Boolean;)I",
        boolean_compare_to,
    );
    shared.register_native(class, "compare", "(ZZ)I", boolean_compare);
    shared.register_native(
        class,
        "getBoolean",
        "(Ljava/lang/String;)Z",
        boolean_get_boolean,
    );
}

/// Whether a descriptor names a reference type (`L...;` or `[...`).
pub fn is_reference_descriptor(descriptor: &str) -> bool {
    descriptor.starts_with('L') || descriptor.starts_with('[')
}

/// Convert one argument for a parameter of `descriptor`.
///
/// The conversion follows `java.lang.reflect`'s invocation rules: a numeric parameter
/// accepts any numeric wrapper whose primitive widens to it (JLS §5.3), `Z` requires a
/// boxed `Boolean`, `C` a boxed `Character`, and a reference descriptor passes the same
/// handle through, null included. The returned [`Value`] is the primitive the interpreter
/// expects for that descriptor: `B`/`S`/`I` as `Value::Int`, `J` as `Value::Long`, `F` as
/// `Value::Float` and `D` as `Value::Double`.
pub fn unbox_argument(shared: &Shared, descriptor: &str, value: Value) -> Result<Value, VmError> {
    if descriptor == "Z" || descriptor == "C" {
        let Value::Ref(reference) = value else {
            return Err(VmError::Native(format!(
                "expected a boxed value for a {descriptor} parameter, found {value:?}"
            )));
        };
        let boxed = shared.boxed(reference)?;
        return match (descriptor, boxed) {
            ("Z", BoxedPrimitive::Boolean(value)) => Ok(Value::Int(i32::from(value))),
            ("C", BoxedPrimitive::Character(value)) => Ok(Value::Int(i32::from(value as u16))),
            _ => Err(VmError::Native(format!(
                "{reference} is a boxed {}, not the {descriptor} box",
                boxed.binary_name()
            ))),
        };
    }
    if matches!(descriptor, "B" | "S" | "I" | "J" | "F" | "D") {
        let Value::Ref(reference) = value else {
            return Err(VmError::Native(format!(
                "expected a boxed value for a {descriptor} parameter, found {value:?}"
            )));
        };
        let boxed = shared.boxed(reference)?;
        if !widens_to(boxed, descriptor) {
            return Err(VmError::Native(format!(
                "a boxed {} does not widen to the {descriptor} parameter",
                boxed.binary_name()
            )));
        }
        // A `char` widens to `int`/`long`/`float`/`double` without a `Number` conversion,
        // so read it as its code unit before applying the target conversion.
        let boxed = match boxed {
            BoxedPrimitive::Character(character) => {
                BoxedPrimitive::Integer(i32::from(character as u16))
            }
            other => other,
        };
        return match descriptor {
            "B" => boxed
                .byte_value()
                .map(|value| Value::Int(i32::from(value)))
                .ok_or_else(|| unboxable(boxed, descriptor)),
            "S" => boxed
                .short_value()
                .map(|value| Value::Int(i32::from(value)))
                .ok_or_else(|| unboxable(boxed, descriptor)),
            "I" => boxed
                .int_value()
                .map(Value::Int)
                .ok_or_else(|| unboxable(boxed, descriptor)),
            "J" => boxed
                .long_value()
                .map(Value::Long)
                .ok_or_else(|| unboxable(boxed, descriptor)),
            "F" => boxed
                .float_value()
                .map(Value::Float)
                .ok_or_else(|| unboxable(boxed, descriptor)),
            "D" => boxed
                .double_value()
                .map(Value::Double)
                .ok_or_else(|| unboxable(boxed, descriptor)),
            _ => unreachable!("the descriptor set is closed above"),
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
        "unsupported parameter descriptor: {descriptor}"
    )))
}

fn unboxable(boxed: BoxedPrimitive, descriptor: &str) -> VmError {
    VmError::Native(format!(
        "a boxed {} cannot be read as a {descriptor} parameter",
        boxed.binary_name()
    ))
}

/// Whether `boxed`'s primitive widens to the primitive named by `descriptor` (JLS §5.3).
fn widens_to(boxed: BoxedPrimitive, descriptor: &str) -> bool {
    use BoxedPrimitive::*;
    match descriptor {
        "B" => matches!(boxed, Byte(_)),
        "S" => matches!(boxed, Byte(_) | Short(_)),
        "I" => matches!(boxed, Byte(_) | Short(_) | Character(_) | Integer(_)),
        "J" => matches!(
            boxed,
            Byte(_) | Short(_) | Character(_) | Integer(_) | Long(_)
        ),
        "F" => matches!(
            boxed,
            Byte(_) | Short(_) | Character(_) | Integer(_) | Long(_) | Float(_)
        ),
        "D" => matches!(
            boxed,
            Byte(_) | Short(_) | Character(_) | Integer(_) | Long(_) | Float(_) | Double(_)
        ),
        _ => false,
    }
}

/// Convert an invoked method's result back to the reflective `Object` result.
///
/// Each primitive descriptor becomes its own wrapper (`B` a `Byte`, `S` a `Short`, `J` a
/// `Long`, and so on); a reference passes through unchanged, which is what keeps a
/// returned string reference-identical, and `void` becomes null, as `Method.invoke` and
/// `Constructor.newInstance` return null for a void method. Annotation element methods and
/// reflected field reads return these primitives, so they must be boxed.
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
    if matches!(descriptor, "Z" | "B" | "C" | "S" | "I" | "J" | "F" | "D") {
        let Some(value) = value else {
            return Err(VmError::Native(format!(
                "a {descriptor} method returned no value"
            )));
        };
        return match (descriptor, value) {
            ("Z", Value::Int(value)) => {
                Ok(Value::Ref(shared.heap().allocate_boxed_boolean(value != 0)))
            }
            ("B", Value::Int(value)) => {
                Ok(Value::Ref(shared.heap().allocate_boxed_byte(value as i8)))
            }
            ("S", Value::Int(value)) => {
                Ok(Value::Ref(shared.heap().allocate_boxed_short(value as i16)))
            }
            ("I", Value::Int(value)) => Ok(Value::Ref(shared.box_int(value))),
            ("J", Value::Long(value)) => Ok(Value::Ref(shared.box_long(value))),
            ("F", Value::Float(value)) => Ok(Value::Ref(shared.heap().allocate_boxed_float(value))),
            ("D", Value::Double(value)) => {
                Ok(Value::Ref(shared.heap().allocate_boxed_double(value)))
            }
            ("C", Value::Int(value)) => {
                let Some(character) = char::from_u32(value as u32) else {
                    return Err(VmError::Native(format!(
                        "{value} is not a UTF-16 code unit for a char result"
                    )));
                };
                Ok(Value::Ref(shared.heap().allocate_boxed_char(character)))
            }
            (_, other) => Err(VmError::Native(format!(
                "a {descriptor} method returned {other:?}"
            ))),
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
        "unsupported result descriptor: {descriptor}"
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

fn long_argument(args: &[Value], position: usize, method: &str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects a long argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn float_argument(args: &[Value], position: usize, method: &str) -> Result<f32, VmError> {
    match args.get(position).copied() {
        Some(Value::Float(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a float argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn double_argument(args: &[Value], position: usize, method: &str) -> Result<f64, VmError> {
    match args.get(position).copied() {
        Some(Value::Double(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a double argument"
        ))),
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

/// The box behind a receiver, with the method name for the error message.
fn boxed_receiver(
    shared: &Shared,
    receiver: ObjRef,
    method: &str,
) -> Result<BoxedPrimitive, VmError> {
    shared
        .boxed(receiver)
        .map_err(|error| VmError::Native(format!("{method}: {error}")))
}

/// The box behind a numeric (`java.lang.Number` subclass) receiver.
fn numeric_receiver(
    shared: &Shared,
    receiver: ObjRef,
    method: &str,
) -> Result<BoxedPrimitive, VmError> {
    let boxed = boxed_receiver(shared, receiver, method)?;
    if boxed.is_numeric() {
        Ok(boxed)
    } else {
        Err(VmError::Native(format!(
            "{method} receiver is a {}, not a java.lang.Number",
            boxed.binary_name()
        )))
    }
}

/// The box behind a `compareTo` argument, which must be the same primitive kind.
fn same_kind_argument(
    shared: &Shared,
    receiver: ObjRef,
    args: &[Value],
    method: &str,
) -> Result<(BoxedPrimitive, BoxedPrimitive), VmError> {
    let left = boxed_receiver(shared, receiver, method)?;
    let Some(Value::Ref(argument)) = args.first().copied() else {
        return Err(VmError::Native(format!(
            "{method} expects a boxed argument"
        )));
    };
    let right = shared
        .boxed(argument)
        .map_err(|error| VmError::Native(format!("{method}: {error}")))?;
    if right.descriptor() != left.descriptor() {
        return Err(VmError::Native(format!(
            "{method} expects a {}, found a {}",
            left.binary_name(),
            right.binary_name()
        )));
    }
    Ok((left, right))
}

fn ordering_value(ordering: std::cmp::Ordering) -> i32 {
    match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `Wrapper.toString()`: the receiver's Java text as a new string.
fn box_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = boxed_receiver(shared, receiver, "toString")?.text();
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `Wrapper.equals(Object)`: per-kind value equality; null and other kinds are false.
fn box_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = boxed_receiver(shared, receiver, "equals")?;
    let Some(Value::Ref(argument)) = args.first().copied() else {
        return Err(VmError::Native(
            "equals expects an Object argument".to_owned(),
        ));
    };
    if argument.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    let equal = match shared.boxed(argument) {
        Ok(right) => left.java_equals(right),
        // A non-box reference is simply unequal, as the JDK wrappers return false.
        Err(_) => false,
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `Wrapper.hashCode()`.
fn box_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        boxed_receiver(shared, receiver, "hashCode")?.java_hash_code(),
    )))
}

/// `Wrapper.compareTo(same box)`: -1, 0 or 1 in the wrapper's Java order.
fn box_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (left, right) = same_kind_argument(shared, receiver, args, "compareTo")?;
    let ordering = left.java_compare(right).ok_or_else(|| {
        VmError::Native(format!(
            "compareTo has no order for a {} and a {}",
            left.binary_name(),
            right.binary_name()
        ))
    })?;
    Ok(Some(Value::Int(ordering_value(ordering))))
}

/// `Number.byteValue()`.
fn box_byte_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = numeric_receiver(shared, receiver, "byteValue")?
        .byte_value()
        .ok_or_else(|| VmError::Native("byteValue is not a Number method".to_owned()))?;
    Ok(Some(Value::Int(i32::from(value))))
}

/// `Number.shortValue()`.
fn box_short_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = numeric_receiver(shared, receiver, "shortValue")?
        .short_value()
        .ok_or_else(|| VmError::Native("shortValue is not a Number method".to_owned()))?;
    Ok(Some(Value::Int(i32::from(value))))
}

/// `Number.intValue()`.
fn box_int_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = numeric_receiver(shared, receiver, "intValue")?
        .int_value()
        .ok_or_else(|| VmError::Native("intValue is not a Number method".to_owned()))?;
    Ok(Some(Value::Int(value)))
}

/// `Number.longValue()`.
fn box_long_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = numeric_receiver(shared, receiver, "longValue")?
        .long_value()
        .ok_or_else(|| VmError::Native("longValue is not a Number method".to_owned()))?;
    Ok(Some(Value::Long(value)))
}

/// `Number.floatValue()`.
fn box_float_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = numeric_receiver(shared, receiver, "floatValue")?
        .float_value()
        .ok_or_else(|| VmError::Native("floatValue is not a Number method".to_owned()))?;
    Ok(Some(Value::Float(value)))
}

/// `Number.doubleValue()`.
fn box_double_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = numeric_receiver(shared, receiver, "doubleValue")?
        .double_value()
        .ok_or_else(|| VmError::Native("doubleValue is not a Number method".to_owned()))?;
    Ok(Some(Value::Double(value)))
}

/// `Float.isNaN()`/`Double.isNaN()`.
fn box_is_nan(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let boxed = boxed_receiver(shared, receiver, "isNaN")?;
    let result = match boxed {
        BoxedPrimitive::Float(value) => value.is_nan(),
        BoxedPrimitive::Double(value) => value.is_nan(),
        _ => {
            return Err(VmError::Native(format!(
                "isNaN is not a {} method",
                boxed.binary_name()
            )));
        }
    };
    Ok(Some(Value::Int(i32::from(result))))
}

/// `Float.isInfinite()`/`Double.isInfinite()`.
fn box_is_infinite(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let boxed = boxed_receiver(shared, receiver, "isInfinite")?;
    let result = match boxed {
        BoxedPrimitive::Float(value) => value.is_infinite(),
        BoxedPrimitive::Double(value) => value.is_infinite(),
        _ => {
            return Err(VmError::Native(format!(
                "isInfinite is not a {} method",
                boxed.binary_name()
            )));
        }
    };
    Ok(Some(Value::Int(i32::from(result))))
}

/// `Float.isNaN(float)`.
fn float_is_nan(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Float.isNaN")?;
    Ok(Some(Value::Int(i32::from(value.is_nan()))))
}

/// `Float.isInfinite(float)`.
fn float_is_infinite(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Float.isInfinite")?;
    Ok(Some(Value::Int(i32::from(value.is_infinite()))))
}

/// `Float.isFinite(float)`.
fn float_is_finite(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Float.isFinite")?;
    Ok(Some(Value::Int(i32::from(value.is_finite()))))
}

/// `Double.isNaN(double)`.
fn double_is_nan(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Double.isNaN")?;
    Ok(Some(Value::Int(i32::from(value.is_nan()))))
}

/// `Double.isInfinite(double)`.
fn double_is_infinite(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Double.isInfinite")?;
    Ok(Some(Value::Int(i32::from(value.is_infinite()))))
}

/// `Double.isFinite(double)`.
fn double_is_finite(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Double.isFinite")?;
    Ok(Some(Value::Int(i32::from(value.is_finite()))))
}

/// `Float.floatToIntBits(float)`.
fn float_to_int_bits(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Float.floatToIntBits")?;
    Ok(Some(Value::Int(canonical_float_bits(value) as i32)))
}

/// `Double.doubleToLongBits(double)`.
fn double_to_long_bits(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Double.doubleToLongBits")?;
    Ok(Some(Value::Long(canonical_double_bits(value) as i64)))
}

// --- Integer -------------------------------------------------------------------------

/// `Integer.valueOf(int)`: a fresh box. There is no small-value cache.
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

/// `Integer.valueOf(String)`.
fn integer_value_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_int_argument(shared, args, 10, "Integer.valueOf")?;
    Ok(Some(Value::Ref(shared.box_int(value))))
}

/// `Integer.valueOf(String, int)`.
fn integer_value_of_string_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Integer.valueOf")?;
    let value = parse_int_argument(shared, args, radix, "Integer.valueOf")?;
    Ok(Some(Value::Ref(shared.box_int(value))))
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

/// `Integer.hashCode(int)`.
fn integer_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(int_argument(args, 0, "Integer.hashCode")?)))
}

/// `Integer.compare(int, int)`.
fn integer_compare(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = int_argument(args, 0, "Integer.compare")?;
    let right = int_argument(args, 1, "Integer.compare")?;
    Ok(Some(Value::Int(ordering_value(left.cmp(&right)))))
}

/// `Integer.parseInt(String)`.
fn integer_parse_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_int_argument(shared, args, 10, "Integer.parseInt")?;
    Ok(Some(Value::Int(value)))
}

/// `Integer.parseInt(String, int)`.
fn integer_parse_int_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Integer.parseInt")?;
    let value = parse_int_argument(shared, args, radix, "Integer.parseInt")?;
    Ok(Some(Value::Int(value)))
}

/// `Integer.toHexString(int)`: the unsigned hexadecimal text of the two's-complement bits.
fn integer_to_hex_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Integer.toHexString")?;
    let text = shared.intern_string(&format!("{:x}", value as u32));
    Ok(Some(Value::Ref(text)))
}

/// `Integer.toBinaryString(int)`.
fn integer_to_binary_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Integer.toBinaryString")?;
    let text = shared.intern_string(&format!("{:b}", value as u32));
    Ok(Some(Value::Ref(text)))
}

/// `Integer.toOctalString(int)`.
fn integer_to_octal_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Integer.toOctalString")?;
    let text = shared.intern_string(&format!("{:o}", value as u32));
    Ok(Some(Value::Ref(text)))
}

// --- Byte ----------------------------------------------------------------------------

/// `Byte.valueOf(byte)`.
fn byte_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Byte.valueOf")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_boxed_byte(value as i8),
    )))
}

/// `Byte.valueOf(String)`.
fn byte_value_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_int_argument(shared, args, 10, "Byte.valueOf")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_boxed_byte(value as i8),
    )))
}

/// `Byte.valueOf(String, int)`.
fn byte_value_of_string_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Byte.valueOf")?;
    let value = parse_int_argument(shared, args, radix, "Byte.valueOf")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_boxed_byte(value as i8),
    )))
}

/// `Byte.parseByte(String)`.
fn byte_parse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_int_argument(shared, args, 10, "Byte.parseByte")?;
    Ok(Some(Value::Int(value)))
}

/// `Byte.parseByte(String, int)`.
fn byte_parse_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Byte.parseByte")?;
    let value = parse_int_argument(shared, args, radix, "Byte.parseByte")?;
    Ok(Some(Value::Int(value)))
}

/// `Byte.toString(byte)`.
fn byte_to_string_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Byte.toString")? as i8;
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(value.to_string()),
    )))
}

/// `Byte.hashCode(byte)`.
fn byte_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        int_argument(args, 0, "Byte.hashCode")? as i8 as i32,
    )))
}

/// `Byte.compare(byte, byte)`.
fn byte_compare(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = int_argument(args, 0, "Byte.compare")? as i8;
    let right = int_argument(args, 1, "Byte.compare")? as i8;
    Ok(Some(Value::Int(ordering_value(left.cmp(&right)))))
}

// --- Short ---------------------------------------------------------------------------

/// `Short.valueOf(short)`.
fn short_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Short.valueOf")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_boxed_short(value as i16),
    )))
}

/// `Short.valueOf(String)`.
fn short_value_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_int_argument(shared, args, 10, "Short.valueOf")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_boxed_short(value as i16),
    )))
}

/// `Short.valueOf(String, int)`.
fn short_value_of_string_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Short.valueOf")?;
    let value = parse_int_argument(shared, args, radix, "Short.valueOf")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_boxed_short(value as i16),
    )))
}

/// `Short.parseShort(String)`.
fn short_parse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_int_argument(shared, args, 10, "Short.parseShort")?;
    Ok(Some(Value::Int(value)))
}

/// `Short.parseShort(String, int)`.
fn short_parse_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Short.parseShort")?;
    let value = parse_int_argument(shared, args, radix, "Short.parseShort")?;
    Ok(Some(Value::Int(value)))
}

/// `Short.toString(short)`.
fn short_to_string_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "Short.toString")? as i16;
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(value.to_string()),
    )))
}

/// `Short.hashCode(short)`.
fn short_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        int_argument(args, 0, "Short.hashCode")? as i16 as i32,
    )))
}

/// `Short.compare(short, short)`.
fn short_compare(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = int_argument(args, 0, "Short.compare")? as i16;
    let right = int_argument(args, 1, "Short.compare")? as i16;
    Ok(Some(Value::Int(ordering_value(left.cmp(&right)))))
}

// --- Long ----------------------------------------------------------------------------

/// `Long.valueOf(long)`.
fn long_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = long_argument(args, 0, "Long.valueOf")?;
    Ok(Some(Value::Ref(shared.box_long(value))))
}

/// `Long.valueOf(String)`.
fn long_value_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_long_argument(shared, args, 10, "Long.valueOf")?;
    Ok(Some(Value::Ref(shared.box_long(value))))
}

/// `Long.valueOf(String, int)`.
fn long_value_of_string_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Long.valueOf")?;
    let value = parse_long_argument(shared, args, radix, "Long.valueOf")?;
    Ok(Some(Value::Ref(shared.box_long(value))))
}

/// `Long.parseLong(String)`.
fn long_parse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = parse_long_argument(shared, args, 10, "Long.parseLong")?;
    Ok(Some(Value::Long(value)))
}

/// `Long.parseLong(String, int)`.
fn long_parse_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let radix = int_argument(args, 1, "Long.parseLong")?;
    let value = parse_long_argument(shared, args, radix, "Long.parseLong")?;
    Ok(Some(Value::Long(value)))
}

/// `Long.toString(long)`.
fn long_to_string_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = long_argument(args, 0, "Long.toString")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(value.to_string()),
    )))
}

/// `Long.hashCode(long)`.
fn long_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = long_argument(args, 0, "Long.hashCode")?;
    let bits = value as u64;
    Ok(Some(Value::Int((value ^ ((bits >> 32) as i64)) as i32)))
}

/// `Long.compare(long, long)`.
fn long_compare(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = long_argument(args, 0, "Long.compare")?;
    let right = long_argument(args, 1, "Long.compare")?;
    Ok(Some(Value::Int(ordering_value(left.cmp(&right)))))
}

/// `Long.toHexString(long)`.
fn long_to_hex_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = long_argument(args, 0, "Long.toHexString")?;
    let text = shared.intern_string(&format!("{:x}", value as u64));
    Ok(Some(Value::Ref(text)))
}

// --- Float ---------------------------------------------------------------------------

/// `Float.valueOf(float)`.
fn float_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Float.valueOf")?;
    Ok(Some(Value::Ref(shared.heap().allocate_boxed_float(value))))
}

/// `Float.valueOf(String)`.
fn float_value_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "Float.valueOf")?;
    let value = parse_float_text(&text, "Float.valueOf")?;
    Ok(Some(Value::Ref(shared.heap().allocate_boxed_float(value))))
}

/// `Float.toString(float)`.
fn float_to_string_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Float.toString")?;
    let text = shared.intern_string(&crate::string::float_text(value));
    Ok(Some(Value::Ref(text)))
}

/// `Float.hashCode(float)`.
fn float_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Float.hashCode")?;
    Ok(Some(Value::Int(canonical_float_bits(value) as i32)))
}

/// `Float.compare(float, float)`.
fn float_compare(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = float_argument(args, 0, "Float.compare")?;
    let right = float_argument(args, 1, "Float.compare")?;
    let ordering = BoxedPrimitive::Float(left)
        .java_compare(BoxedPrimitive::Float(right))
        .ok_or_else(|| VmError::Native("Float.compare has no order".to_owned()))?;
    let _ = shared;
    Ok(Some(Value::Int(ordering_value(ordering))))
}

/// `Float.parseFloat(String)`.
fn float_parse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "Float.parseFloat")?;
    Ok(Some(Value::Float(parse_float_text(
        &text,
        "Float.parseFloat",
    )?)))
}

// --- Double --------------------------------------------------------------------------

/// `Double.valueOf(double)`.
fn double_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Double.valueOf")?;
    Ok(Some(Value::Ref(shared.box_double(value))))
}

/// `Double.valueOf(String)`.
fn double_value_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "Double.valueOf")?;
    let value = parse_double_text(&text, "Double.valueOf")?;
    Ok(Some(Value::Ref(shared.box_double(value))))
}

/// `Double.toString(double)`.
fn double_to_string_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Double.toString")?;
    let text = shared.intern_string(&crate::string::double_text(value));
    Ok(Some(Value::Ref(text)))
}

/// `Double.hashCode(double)`.
fn double_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Double.hashCode")?;
    let bits = canonical_double_bits(value);
    Ok(Some(Value::Int((bits ^ (bits >> 32)) as i32)))
}

/// `Double.compare(double, double)`.
fn double_compare(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = double_argument(args, 0, "Double.compare")?;
    let right = double_argument(args, 1, "Double.compare")?;
    let ordering = BoxedPrimitive::Double(left)
        .java_compare(BoxedPrimitive::Double(right))
        .ok_or_else(|| VmError::Native("Double.compare has no order".to_owned()))?;
    Ok(Some(Value::Int(ordering_value(ordering))))
}

/// `Double.parseDouble(String)`.
fn double_parse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "Double.parseDouble")?;
    Ok(Some(Value::Double(parse_double_text(
        &text,
        "Double.parseDouble",
    )?)))
}

// --- Parsing -------------------------------------------------------------------------

/// Parse a `String` argument as a radix integer in the range of one wrapper.
///
/// The JDK's syntax is an optional `+`/`-` followed by digits of `radix` (2..=36); a
/// malformed value or one outside the wrapper's range is a typed error rather than a
/// synthesized `NumberFormatException`. `Integer.parseInt(String)` and the `valueOf`
/// forms use the same parser, so their errors agree.
fn parse_int_argument(
    shared: &Shared,
    args: &[Value],
    radix: i32,
    method: &str,
) -> Result<i32, VmError> {
    let text = string_argument(shared, args, 0, method)?;
    let (min, max) = match method.split('.').next().unwrap_or_default() {
        "Byte" => (i128::from(i8::MIN), i128::from(i8::MAX)),
        "Short" => (i128::from(i16::MIN), i128::from(i16::MAX)),
        _ => (i128::from(i32::MIN), i128::from(i32::MAX)),
    };
    let value = parse_radix_integer(&text, radix, min, max, method)?;
    Ok(value as i32)
}

/// Parse a `String` argument as a radix `long`.
fn parse_long_argument(
    shared: &Shared,
    args: &[Value],
    radix: i32,
    method: &str,
) -> Result<i64, VmError> {
    let text = string_argument(shared, args, 0, method)?;
    let value = parse_radix_integer(
        &text,
        radix,
        i128::from(i64::MIN),
        i128::from(i64::MAX),
        method,
    )?;
    Ok(value as i64)
}

/// Parse an optional-sign radix integer and check it against `[min, max]`.
fn parse_radix_integer(
    text: &str,
    radix: i32,
    min: i128,
    max: i128,
    method: &str,
) -> Result<i128, VmError> {
    if !(2..=36).contains(&radix) {
        return Err(VmError::Native(format!(
            "{method}: radix {radix} is outside 2..=36"
        )));
    }
    let radix = radix as u32;
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'+') => (false, &text[1..]),
        Some(b'-') => (true, &text[1..]),
        _ => (false, text),
    };
    if digits.is_empty() {
        return Err(VmError::Native(format!(
            "{method}: {text:?} is not a radix {radix} integer"
        )));
    }
    let mut magnitude: i128 = 0;
    for character in digits.chars() {
        let Some(digit) = character.to_digit(radix) else {
            return Err(VmError::Native(format!(
                "{method}: {text:?} is not a radix {radix} integer"
            )));
        };
        magnitude = magnitude
            .checked_mul(i128::from(radix))
            .and_then(|value| value.checked_add(i128::from(digit)))
            .ok_or_else(|| {
                VmError::Native(format!(
                    "{method}: {text:?} is out of range for this wrapper"
                ))
            })?;
    }
    let value = if negative { -magnitude } else { magnitude };
    if value < min || value > max {
        return Err(VmError::Native(format!(
            "{method}: {text:?} is out of range for this wrapper"
        )));
    }
    Ok(value)
}

/// `Float.parseFloat` syntax: optional `f`/`F` suffix, `NaN`, both infinities.
///
/// Java also accepts a hex float literal; Rust's parser does not, so a hex literal is the
/// typed error this slice documents. Rust accepts `inf`/`infinity`/`nan` in any case,
/// which is wider than Java's exact `Infinity`/`NaN` spelling.
fn parse_float_text(text: &str, method: &str) -> Result<f32, VmError> {
    let trimmed = trim_float_text(text);
    let trimmed = strip_float_suffix(trimmed);
    match trimmed {
        "NaN" | "+NaN" => Ok(f32::NAN),
        "Infinity" | "+Infinity" => Ok(f32::INFINITY),
        "-Infinity" => Ok(f32::NEG_INFINITY),
        _ => trimmed
            .parse::<f32>()
            .map_err(|_| VmError::Native(format!("{method}: {text:?} is not a float literal"))),
    }
}

/// `Double.parseDouble` syntax, with the same normalization as [`parse_float_text`].
fn parse_double_text(text: &str, method: &str) -> Result<f64, VmError> {
    let trimmed = trim_float_text(text);
    let trimmed = strip_float_suffix(trimmed);
    match trimmed {
        "NaN" | "+NaN" => Ok(f64::NAN),
        "Infinity" | "+Infinity" => Ok(f64::INFINITY),
        "-Infinity" => Ok(f64::NEG_INFINITY),
        _ => trimmed
            .parse::<f64>()
            .map_err(|_| VmError::Native(format!("{method}: {text:?} is not a double literal"))),
    }
}

/// `String.trim`'s predicate: strip leading and trailing units `<= ' '`.
fn trim_float_text(text: &str) -> &str {
    text.trim_matches(|character: char| character <= ' ')
}

/// Strip the type suffix Java allows on a floating literal (`1.5f`, `2d`).
fn strip_float_suffix(text: &str) -> &str {
    match text.as_bytes().last() {
        Some(b'f' | b'F' | b'd' | b'D') => &text[..text.len() - 1],
        _ => text,
    }
}

/// Register the `java.lang.Character` boxing and classification surface.
fn register_character_natives(shared: &Shared) {
    let class = "java/lang/Character";
    shared.register_native(
        class,
        "valueOf",
        "(C)Ljava/lang/Character;",
        character_value_of,
    );
    shared.register_native(class, "charValue", "()C", character_char_value);
    shared.register_native(
        class,
        "toString",
        "()Ljava/lang/String;",
        character_box_to_string,
    );
    shared.register_native(class, "hashCode", "()I", character_hash_code);
    shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", character_equals);
    for (name, function) in [
        ("isUpperCase", character_is_upper as crate::NativeFn),
        ("isLowerCase", character_is_lower),
        ("isDigit", character_is_digit),
        ("isLetter", character_is_letter),
        ("isLetterOrDigit", character_is_letter_or_digit),
        ("isWhitespace", character_is_whitespace),
        ("isSpaceChar", character_is_space_char),
        ("isAlphabetic", character_is_alphabetic),
        ("isJavaIdentifierStart", character_is_java_identifier_start),
        ("isJavaIdentifierPart", character_is_java_identifier_part),
        ("isSurrogate", character_is_surrogate),
    ] {
        shared.register_native(class, name, "(C)Z", function);
        shared.register_native(class, name, "(I)Z", function);
    }
    shared.register_native(class, "toLowerCase", "(C)C", character_to_lower);
    shared.register_native(class, "toUpperCase", "(C)C", character_to_upper);
    shared.register_native(
        class,
        "toString",
        "(C)Ljava/lang/String;",
        character_to_string,
    );
    shared.register_native(class, "digit", "(CI)I", character_digit);
    shared.register_native(class, "forDigit", "(II)C", character_for_digit);
    shared.register_native(class, "hashCode", "(C)I", character_hash_code_of);
    shared.register_native(class, "compare", "(CC)I", character_compare);
    shared.register_native(
        class,
        "compareTo",
        "(Ljava/lang/Character;)I",
        character_compare_to,
    );
}

fn character_unit(args: &[Value], position: usize, method: &'static str) -> Result<u16, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) if (0..=0xffff).contains(&value) => Ok(value as u16),
        _ => Err(VmError::Native(format!("{method} expects a char argument"))),
    }
}

fn character_predicate(
    args: &[Value],
    method: &'static str,
    predicate: impl FnOnce(char) -> bool,
) -> Result<Option<Value>, VmError> {
    // The `(I)Z` overloads take a code point; the `(C)Z` callers pass a UTF-16 unit, which
    // is also a valid code point. A value outside Unicode is false rather than an error,
    // exactly as `Character.isDigit(int)` documents for a non-character int.
    let code_point = match args.first().copied() {
        Some(Value::Int(value)) if (0..=0x10ffff).contains(&value) => value as u32,
        _ => return Err(VmError::Native(format!("{method} expects an int argument"))),
    };
    let result = char::from_u32(code_point).map(predicate).unwrap_or(false);
    Ok(Some(Value::Int(i32::from(result))))
}

fn character_is_upper(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isUpperCase", char::is_uppercase)
}

fn character_is_lower(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isLowerCase", char::is_lowercase)
}

fn character_is_digit(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isDigit", |character| {
        character.is_ascii_digit()
    })
}

fn character_is_letter(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isLetter", char::is_alphabetic)
}

fn character_is_letter_or_digit(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isLetterOrDigit", char::is_alphanumeric)
}

fn character_is_whitespace(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isWhitespace", char::is_whitespace)
}

fn character_is_space_char(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isSpaceChar", char::is_whitespace)
}

fn character_is_alphabetic(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isAlphabetic", char::is_alphabetic)
}

fn character_is_java_identifier_start(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isJavaIdentifierStart", |character| {
        character == '$' || character == '_' || character.is_alphabetic()
    })
}

fn character_is_java_identifier_part(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    character_predicate(args, "Character.isJavaIdentifierPart", |character| {
        character == '$' || character == '_' || character.is_alphanumeric()
    })
}

fn character_is_surrogate(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = character_unit(args, 0, "Character.isSurrogate")?;
    Ok(Some(Value::Int(i32::from(
        (0xd800..=0xdfff).contains(&unit),
    ))))
}

fn character_to_lower(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = character_unit(args, 0, "Character.toLowerCase")?;
    let converted = char::from_u32(u32::from(unit))
        .and_then(|character| character.to_lowercase().next())
        .unwrap_or_else(|| char::from_u32(u32::from(unit)).unwrap_or('\0'));
    Ok(Some(Value::Int(i32::from(converted as u16))))
}

fn character_to_upper(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = character_unit(args, 0, "Character.toUpperCase")?;
    let converted = char::from_u32(u32::from(unit))
        .and_then(|character| character.to_uppercase().next())
        .unwrap_or_else(|| char::from_u32(u32::from(unit)).unwrap_or('\0'));
    Ok(Some(Value::Int(i32::from(converted as u16))))
}

fn character_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = character_unit(args, 0, "Character.toString")?;
    let text = String::from_utf16_lossy(&[unit]);
    let text = shared.intern_string(&text);
    Ok(Some(Value::Ref(text)))
}

fn character_digit(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = character_unit(args, 0, "Character.digit")?;
    let radix = match args.get(1).copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "Character.digit expects a radix".to_owned(),
            ));
        }
    };
    let digit = char::from_u32(u32::from(unit))
        .and_then(|character| character.to_digit(radix as u32))
        .map(|digit| digit as i32)
        .unwrap_or(-1);
    Ok(Some(Value::Int(digit)))
}

fn character_for_digit(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let digit = match args.first().copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "Character.forDigit expects a digit".to_owned(),
            ));
        }
    };
    let radix = match args.get(1).copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "Character.forDigit expects a radix".to_owned(),
            ));
        }
    };
    let character = char::from_digit(digit as u32, radix as u32).unwrap_or('\0');
    Ok(Some(Value::Int(i32::from(character as u16))))
}

fn character_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = character_unit(args, 0, "Character.valueOf")?;
    let character = char::from_u32(u32::from(unit)).unwrap_or('\0');
    let boxed = shared.heap().allocate_boxed_char(character);
    Ok(Some(Value::Ref(boxed)))
}

fn character_char_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let character = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Character(value)) => *value,
            _ => {
                return Err(VmError::Native(
                    "Character.charValue receiver is not a Character".to_owned(),
                ));
            }
        }
    };
    Ok(Some(Value::Int(i32::from(character as u16))))
}

fn character_box_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let character = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Character(value)) => *value,
            _ => {
                return Err(VmError::Native(
                    "Character.toString receiver is not a Character".to_owned(),
                ));
            }
        }
    };
    let text = shared.intern_string(&character.to_string());
    Ok(Some(Value::Ref(text)))
}

fn character_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let character = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Character(value)) => *value,
            _ => {
                return Err(VmError::Native(
                    "Character.hashCode receiver is not a Character".to_owned(),
                ));
            }
        }
    };
    Ok(Some(Value::Int(i32::from(character as u16))))
}

fn character_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => return Ok(Some(Value::Int(0))),
    };
    let equal = {
        let heap = shared.heap();
        match (
            heap.get(receiver)
                .map_err(|error| VmError::Native(error.to_string()))?,
            heap.get(other)
                .map_err(|error| VmError::Native(error.to_string()))?,
        ) {
            (
                crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Character(left)),
                crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Character(right)),
            ) => left == right,
            _ => false,
        }
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `Character.hashCode(char)`.
fn character_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = character_unit(args, 0, "Character.hashCode")?;
    Ok(Some(Value::Int(i32::from(unit))))
}

/// `Character.compare(char, char)`.
fn character_compare(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = character_unit(args, 0, "Character.compare")?;
    let right = character_unit(args, 1, "Character.compare")?;
    Ok(Some(Value::Int(ordering_value(left.cmp(&right)))))
}

/// `Character.compareTo(Character)`.
fn character_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (left, right) = same_kind_argument(shared, receiver, args, "Character.compareTo")?;
    let ordering = left
        .java_compare(right)
        .ok_or_else(|| VmError::Native("Character.compareTo has no order".to_owned()))?;
    Ok(Some(Value::Int(ordering_value(ordering))))
}

fn bool_argument(args: &[Value], position: usize, method: &'static str) -> Result<bool, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value != 0),
        _ => Err(VmError::Native(format!(
            "{method} expects a boolean argument"
        ))),
    }
}

fn boolean_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = bool_argument(args, 0, "Boolean.valueOf")?;
    let boxed = shared.heap().allocate_boxed_boolean(value);
    Ok(Some(Value::Ref(boxed)))
}

fn boolean_boolean_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Boolean(value)) => *value,
            _ => {
                return Err(VmError::Native(
                    "Boolean.booleanValue receiver is not a Boolean".to_owned(),
                ));
            }
        }
    };
    Ok(Some(Value::Int(i32::from(value))))
}

fn boolean_parse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => shared
            .heap()
            .string(reference)
            .map_err(|error| VmError::Native(error.to_string()))?
            .to_owned(),
        _ => String::new(),
    };
    Ok(Some(Value::Int(i32::from(
        text.eq_ignore_ascii_case("true"),
    ))))
}

fn boolean_to_string_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = bool_argument(args, 0, "Boolean.toString")?;
    let text = shared.intern_string(if value { "true" } else { "false" });
    Ok(Some(Value::Ref(text)))
}

fn boolean_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Boolean(value)) => *value,
            _ => {
                return Err(VmError::Native(
                    "Boolean.toString receiver is not a Boolean".to_owned(),
                ));
            }
        }
    };
    let text = shared.intern_string(if value { "true" } else { "false" });
    Ok(Some(Value::Ref(text)))
}

fn boolean_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = {
        let heap = shared.heap();
        match heap
            .get(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Boolean(value)) => *value,
            _ => {
                return Err(VmError::Native(
                    "Boolean.hashCode receiver is not a Boolean".to_owned(),
                ));
            }
        }
    };
    Ok(Some(Value::Int(if value { 1231 } else { 1237 })))
}

fn boolean_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => return Ok(Some(Value::Int(0))),
    };
    let equal = {
        let heap = shared.heap();
        match (
            heap.get(receiver)
                .map_err(|error| VmError::Native(error.to_string()))?,
            heap.get(other)
                .map_err(|error| VmError::Native(error.to_string()))?,
        ) {
            (
                crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Boolean(left)),
                crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Boolean(right)),
            ) => left == right,
            _ => false,
        }
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `Boolean.valueOf(String)`: `true` only for a string equal to `"true"`, ignoring case.
fn boolean_value_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => shared
            .heap()
            .string(reference)
            .map_err(|error| VmError::Native(error.to_string()))?
            .eq_ignore_ascii_case("true"),
        _ => false,
    };
    Ok(Some(Value::Ref(
        shared.heap().allocate_boxed_boolean(value),
    )))
}

/// `Boolean.hashCode(boolean)`.
fn boolean_hash_code_of(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = bool_argument(args, 0, "Boolean.hashCode")?;
    Ok(Some(Value::Int(if value { 1231 } else { 1237 })))
}

/// `Boolean.compareTo(Boolean)`.
fn boolean_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (left, right) = same_kind_argument(shared, receiver, args, "Boolean.compareTo")?;
    let ordering = left
        .java_compare(right)
        .ok_or_else(|| VmError::Native("Boolean.compareTo has no order".to_owned()))?;
    Ok(Some(Value::Int(ordering_value(ordering))))
}

/// `Boolean.compare(boolean, boolean)`.
fn boolean_compare(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = bool_argument(args, 0, "Boolean.compare")?;
    let right = bool_argument(args, 1, "Boolean.compare")?;
    Ok(Some(Value::Int(ordering_value(left.cmp(&right)))))
}

/// `Boolean.getBoolean(String)`: whether the named system property is `"true"`.
fn boolean_get_boolean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "Boolean.getBoolean")?;
    let value = shared
        .property(&name)
        .map(|value| value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    Ok(Some(Value::Int(i32::from(value))))
}

/// Install `Boolean.TRUE`/`Boolean.FALSE`, which the C1 stub cannot initialize itself.
pub fn install_booleans(shared: &Shared) -> Result<(), VmError> {
    let class = shared.load_library_class("java/lang/Boolean")?;
    let mut values = Vec::new();
    for value in [true, false] {
        let object = shared.heap().allocate_boxed_boolean(value);
        values.push((
            if value { "TRUE" } else { "FALSE" }.to_owned(),
            "Ljava/lang/Boolean;".to_owned(),
            Value::Ref(object),
        ));
    }
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let mut statics = entry.statics.lock().expect("static field lock poisoned");
    for (name, descriptor, value) in values {
        statics.insert((name, descriptor), value);
    }
    Ok(())
}
