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
    register_character_natives(shared);
    shared.register_native(
        "java/lang/Boolean",
        "valueOf",
        "(Z)Ljava/lang/Boolean;",
        boolean_value_of,
    );
    shared.register_native(
        "java/lang/Boolean",
        "booleanValue",
        "()Z",
        boolean_boolean_value,
    );
    shared.register_native(
        "java/lang/Boolean",
        "parseBoolean",
        "(Ljava/lang/String;)Z",
        boolean_parse,
    );
    shared.register_native(
        "java/lang/Boolean",
        "toString",
        "(Z)Ljava/lang/String;",
        boolean_to_string_of,
    );
    shared.register_native(
        "java/lang/Boolean",
        "toString",
        "()Ljava/lang/String;",
        boolean_to_string,
    );
    shared.register_native("java/lang/Boolean", "hashCode", "()I", boolean_hash_code);
    shared.register_native(
        "java/lang/Boolean",
        "equals",
        "(Ljava/lang/Object;)Z",
        boolean_equals,
    );
    shared.register_native(
        "java/lang/Character",
        "valueOf",
        "(C)Ljava/lang/Character;",
        character_value_of,
    );
    shared.register_native(
        "java/lang/Character",
        "charValue",
        "()C",
        character_char_value,
    );
    shared.register_native(
        "java/lang/Character",
        "toString",
        "()Ljava/lang/String;",
        character_box_to_string,
    );
    shared.register_native(
        "java/lang/Character",
        "hashCode",
        "()I",
        character_hash_code,
    );
    shared.register_native(
        "java/lang/Character",
        "equals",
        "(Ljava/lang/Object;)Z",
        character_equals,
    );
    shared.register_native(
        "java/lang/Integer",
        "toHexString",
        "(I)Ljava/lang/String;",
        integer_to_hex_string,
    );
    shared.register_native(
        "java/lang/Integer",
        "toBinaryString",
        "(I)Ljava/lang/String;",
        integer_to_binary_string,
    );
    shared.register_native(
        "java/lang/Integer",
        "toOctalString",
        "(I)Ljava/lang/String;",
        integer_to_octal_string,
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
/// `I` takes a `Value::Ref` to a boxed `Integer` and returns its `Value::Int`; `Z` reads
/// a boxed `Boolean`, `C` a boxed `Character` and `B`/`S` a boxed `Integer` (there is no
/// `Byte`/`Short` box yet). A reference descriptor passes the same handle through, null
/// included. `J`, `F` and `D` remain errors: those boxes are not modelled.
pub fn unbox_argument(shared: &Shared, descriptor: &str, value: Value) -> Result<Value, VmError> {
    if descriptor == "I" || descriptor == "B" || descriptor == "S" {
        return match value {
            Value::Ref(reference) => Ok(Value::Int(shared.unbox_int(reference)?)),
            other => Err(VmError::Native(format!(
                "expected a boxed Integer for an {descriptor} parameter, found {other:?}"
            ))),
        };
    }
    if descriptor == "Z" || descriptor == "C" {
        let Value::Ref(reference) = value else {
            return Err(VmError::Native(format!(
                "expected a boxed value for a {descriptor} parameter, found {value:?}"
            )));
        };
        if reference.is_null() {
            return Err(VmError::Native(format!(
                "cannot unbox a null reference as {descriptor}"
            )));
        }
        let boxed = {
            let heap = shared.heap();
            match heap.get(reference) {
                Ok(HeapObject::Boxed(boxed)) => *boxed,
                Ok(_) => {
                    return Err(VmError::Native(format!(
                        "{reference} is not a boxed primitive"
                    )));
                }
                Err(error) => return Err(VmError::Native(error.to_string())),
            }
        };
        return match (descriptor, boxed) {
            ("Z", BoxedPrimitive::Boolean(value)) => Ok(Value::Int(i32::from(value))),
            ("C", BoxedPrimitive::Character(value)) => Ok(Value::Int(value as i32)),
            _ => Err(VmError::Native(format!(
                "{reference} is a boxed {}, not the {descriptor} box",
                boxed.binary_name()
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
/// `boolean`, `byte`, `short`, `char` and `int` are boxed (`byte`/`short` as
/// `java.lang.Integer`, the only integer box this VM has); a reference passes through
/// unchanged, which is what keeps a returned string reference-identical; `void` becomes
/// null, as `Method.invoke` and `Constructor.newInstance` return null for a void method.
/// Annotation element methods return these primitives, so `Method.invoke` on one must box
/// them.
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
    if descriptor == "I" || descriptor == "B" || descriptor == "S" {
        return match value {
            Some(Value::Int(value)) => Ok(Value::Ref(shared.box_int(value))),
            Some(other) => Err(VmError::Native(format!(
                "an {descriptor} method returned {other:?}"
            ))),
            None => Err(VmError::Native(format!(
                "an {descriptor} method returned no value"
            ))),
        };
    }
    if descriptor == "Z" || descriptor == "C" {
        let Some(Value::Int(value)) = value else {
            return Err(match value {
                Some(other) => VmError::Native(format!("a {descriptor} method returned {other:?}")),
                None => VmError::Native(format!("a {descriptor} method returned no value")),
            });
        };
        let boxed_result = if descriptor == "Z" {
            shared.heap().allocate_boxed_boolean(value != 0)
        } else {
            let Some(character) = char::from_u32(value as u32) else {
                return Err(VmError::Native(format!(
                    "{value} is not a UTF-16 code unit for a char result"
                )));
            };
            shared.heap().allocate_boxed_char(character)
        };
        return Ok(Value::Ref(boxed_result));
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

/// Register the `java.lang.Character` classification surface Spring's string utilities use.
fn register_character_natives(shared: &Shared) {
    let class = "java/lang/Character";
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
    let unit = character_unit(args, 0, method)?;
    let result = char::from_u32(u32::from(unit))
        .map(predicate)
        .unwrap_or(false);
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
