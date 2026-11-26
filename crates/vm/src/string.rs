//! `java.lang.String` natives (M2.9).
//!
//! A string is the existing [`crate::HeapObject::Str`], a Rust `String` holding valid
//! UTF-8; there is no second string representation. Every Java-visible operation works on
//! UTF-16 code units, which [`utf16_units`] derives from the stored text, so `length`,
//! `charAt`, `substring`, `indexOf`, `compareTo`, `hashCode` and `toCharArray` measure and
//! index UTF-16 units rather than Rust `char`s. A supplementary character (a surrogate
//! pair) therefore counts as two units and `charAt` returns its high and low surrogates.
//!
//! A lone surrogate cannot be stored in a Rust `String`, so an operation that would
//! produce one (for example `substring(0, 1)` over a surrogate pair, or a `replace` that
//! rewrites only half of a pair) fails with [`VmError::UnpairedSurrogate`] instead of
//! corrupting the value. M2.9 has no Java exception-object synthesis, so a null argument
//! and an out-of-range index are typed [`VmError`]s as well.
//!
//! The C1 `String` stubs carry neither `Code` nor, for most methods, `ACC_NATIVE`; the
//! registry is consulted before either, which is what makes these entries the
//! implementations for `invokevirtual`.

use crate::VmError;
use crate::heap::{ClassRef, HeapError, HeapObject, ObjRef, PrimitiveKind};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// Registers the M2.9 `java.lang.String` natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native("java/lang/String", "length", "()I", string_length);
    shared.register_native("java/lang/String", "isEmpty", "()Z", string_is_empty);
    shared.register_native("java/lang/String", "charAt", "(I)C", string_char_at);
    shared.register_native(
        "java/lang/String",
        "equals",
        "(Ljava/lang/Object;)Z",
        string_equals,
    );
    shared.register_native("java/lang/String", "hashCode", "()I", string_hash_code);
    shared.register_native(
        "java/lang/String",
        "toString",
        "()Ljava/lang/String;",
        string_to_string,
    );
    shared.register_native("java/lang/String", "getBytes", "()[B", string_get_bytes);
    shared.register_native(
        "java/lang/String",
        "getBytes",
        "(Ljava/lang/String;)[B",
        string_get_bytes_charset,
    );
    shared.register_native(
        "java/lang/String",
        "getBytes",
        "(Ljava/nio/charset/Charset;)[B",
        string_get_bytes_object,
    );
    shared.register_native(
        "java/lang/String",
        "compareTo",
        "(Ljava/lang/String;)I",
        string_compare_to,
    );
    // The `Comparable` bridge: `String` implements `Comparable<String>`, and a call through
    // `Comparable.compareTo(Object)` resolves to the bridge declaration.
    shared.register_native(
        "java/lang/String",
        "compareTo",
        "(Ljava/lang/Object;)I",
        string_compare_to,
    );
    shared.register_native(
        "java/lang/String",
        "concat",
        "(Ljava/lang/String;)Ljava/lang/String;",
        string_concat,
    );
    shared.register_native(
        "java/lang/String",
        "substring",
        "(I)Ljava/lang/String;",
        string_substring_from,
    );
    shared.register_native(
        "java/lang/String",
        "substring",
        "(II)Ljava/lang/String;",
        string_substring_range,
    );
    shared.register_native(
        "java/lang/String",
        "subSequence",
        "(II)Ljava/lang/CharSequence;",
        string_substring_range,
    );
    shared.register_native(
        "java/lang/String",
        "chars",
        "()Ljava/util/stream/IntStream;",
        string_chars,
    );
    shared.register_native(
        "java/lang/String",
        "toUpperCase",
        "()Ljava/lang/String;",
        string_to_upper,
    );
    shared.register_native(
        "java/lang/String",
        "toUpperCase",
        "(Ljava/util/Locale;)Ljava/lang/String;",
        string_to_upper,
    );
    shared.register_native(
        "java/lang/String",
        "toLowerCase",
        "()Ljava/lang/String;",
        string_to_lower,
    );
    shared.register_native(
        "java/lang/String",
        "toLowerCase",
        "(Ljava/util/Locale;)Ljava/lang/String;",
        string_to_lower,
    );
    shared.register_native(
        "java/lang/String",
        "startsWith",
        "(Ljava/lang/String;)Z",
        string_starts_with,
    );
    shared.register_native(
        "java/lang/String",
        "endsWith",
        "(Ljava/lang/String;)Z",
        string_ends_with,
    );
    shared.register_native(
        "java/lang/String",
        "indexOf",
        "(Ljava/lang/String;)I",
        string_index_of,
    );
    shared.register_native(
        "java/lang/String",
        "replace",
        "(CC)Ljava/lang/String;",
        string_replace,
    );
    shared.register_native(
        "java/lang/String",
        "toCharArray",
        "()[C",
        string_to_char_array,
    );
    shared.register_native(
        "java/lang/String",
        "intern",
        "()Ljava/lang/String;",
        string_intern,
    );
    shared.register_native("java/lang/String", "<init>", "()V", string_init_empty);
    shared.register_native(
        "java/lang/String",
        "<init>",
        "(Ljava/lang/String;)V",
        string_init_string,
    );
    shared.register_native("java/lang/String", "<init>", "([C)V", string_init_chars);
    shared.register_native(
        "java/lang/String",
        "<init>",
        "([CII)V",
        string_init_chars_range,
    );
    shared.register_native("java/lang/String", "indexOf", "(I)I", string_index_of_char);
    shared.register_native(
        "java/lang/String",
        "indexOf",
        "(II)I",
        string_index_of_char_from,
    );
    shared.register_native(
        "java/lang/String",
        "indexOf",
        "(Ljava/lang/String;I)I",
        string_index_of_string_from,
    );
    shared.register_native(
        "java/lang/String",
        "lastIndexOf",
        "(I)I",
        string_last_index_of_char,
    );
    shared.register_native(
        "java/lang/String",
        "lastIndexOf",
        "(II)I",
        string_last_index_of_char_from,
    );
    shared.register_native(
        "java/lang/String",
        "lastIndexOf",
        "(Ljava/lang/String;)I",
        string_last_index_of_string,
    );
    shared.register_native(
        "java/lang/String",
        "lastIndexOf",
        "(Ljava/lang/String;I)I",
        string_last_index_of_string_from,
    );
    shared.register_native(
        "java/lang/String",
        "startsWith",
        "(Ljava/lang/String;I)Z",
        string_starts_with_from,
    );
    shared.register_native(
        "java/lang/String",
        "contains",
        "(Ljava/lang/CharSequence;)Z",
        string_contains,
    );
    shared.register_native(
        "java/lang/String",
        "trim",
        "()Ljava/lang/String;",
        string_trim,
    );
    shared.register_native(
        "java/lang/String",
        "strip",
        "()Ljava/lang/String;",
        string_strip,
    );
    shared.register_native("java/lang/String", "isBlank", "()Z", string_is_blank);
    shared.register_native(
        "java/lang/String",
        "toLowerCase",
        "()Ljava/lang/String;",
        string_to_lower_case,
    );
    shared.register_native(
        "java/lang/String",
        "toUpperCase",
        "()Ljava/lang/String;",
        string_to_upper_case,
    );
    shared.register_native(
        "java/lang/String",
        "equalsIgnoreCase",
        "(Ljava/lang/String;)Z",
        string_equals_ignore_case,
    );
    shared.register_native(
        "java/lang/String",
        "repeat",
        "(I)Ljava/lang/String;",
        string_repeat,
    );
    shared.register_native(
        "java/lang/String",
        "replace",
        "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/lang/String;",
        string_replace_sequence,
    );
    shared.register_native(
        "java/lang/String",
        "split",
        "(Ljava/lang/String;)[Ljava/lang/String;",
        string_split,
    );
    shared.register_native(
        "java/lang/String",
        "join",
        "(Ljava/lang/CharSequence;[Ljava/lang/CharSequence;)Ljava/lang/String;",
        string_join,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "([C)Ljava/lang/String;",
        string_value_of_chars,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "([CII)Ljava/lang/String;",
        string_value_of_chars_range,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        string_value_of_object,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "(I)Ljava/lang/String;",
        string_value_of_int,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "(J)Ljava/lang/String;",
        string_value_of_long,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "(Z)Ljava/lang/String;",
        string_value_of_boolean,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "(C)Ljava/lang/String;",
        string_value_of_char,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "(F)Ljava/lang/String;",
        string_value_of_float,
    );
    shared.register_native(
        "java/lang/String",
        "valueOf",
        "(D)Ljava/lang/String;",
        string_value_of_double,
    );
}

/// The UTF-16 code units of `text`.
///
/// This is the one conversion the string natives use. An ASCII or BMP character is one
/// unit; a supplementary character (U+10000 and above) becomes a surrogate pair.
pub fn utf16_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Build a Rust `String` from UTF-16 code units, rejecting a lone surrogate.
///
/// `method` names the Java operation for the diagnostic. `String::from_utf16` is the only
/// way back from units to the stored representation, so every result that leaves a native
/// passes through here.
pub fn string_from_utf16(units: &[u16], method: &'static str) -> Result<String, VmError> {
    String::from_utf16(units).map_err(|_| VmError::UnpairedSurrogate { method })
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// The receiver's text, copied out of the heap before any further lock is taken.
fn receiver_text(shared: &Shared, receiver: ObjRef) -> Result<String, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "a String method was called on a null receiver".to_owned(),
        ));
    }
    let text = shared
        .heap()
        .string(receiver)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

/// A non-null `String` argument, copied out of the heap.
fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<String, VmError> {
    let reference = match args.get(position).copied() {
        Some(Value::Ref(reference)) => reference,
        Some(_) => {
            return Err(VmError::Native(format!(
                "{method} expects a String argument"
            )));
        }
        None => {
            return Err(VmError::Native(format!("{method} is missing its argument")));
        }
    };
    if reference.is_null() {
        return Err(VmError::NullStringArgument { method });
    }
    let text = shared
        .heap()
        .string(reference)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects an int argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn char_argument(args: &[Value], position: usize, method: &'static str) -> Result<u16, VmError> {
    let value = int_argument(args, position, method)?;
    u16::try_from(value).map_err(|_| VmError::Native(format!("{method} expects a char argument")))
}

fn long_argument(args: &[Value], position: usize, method: &'static str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects a long argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn float_argument(args: &[Value], position: usize, method: &'static str) -> Result<f32, VmError> {
    match args.get(position).copied() {
        Some(Value::Float(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a float argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn double_argument(args: &[Value], position: usize, method: &'static str) -> Result<f64, VmError> {
    match args.get(position).copied() {
        Some(Value::Double(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a double argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

/// A reference argument, including null.
fn reference_argument(
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

/// A `charAt` position: `0 <= index < length`.
fn unit_index(index: i32, length: usize) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position < length => Ok(position),
        _ => Err(VmError::StringIndexOutOfBounds { index, length }),
    }
}

/// A substring bound: `0 <= index <= length`.
fn range_index(index: i32, length: usize) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position <= length => Ok(position),
        _ => Err(VmError::StringIndexOutOfBounds { index, length }),
    }
}

fn int_length(length: usize) -> Result<i32, VmError> {
    i32::try_from(length)
        .map_err(|_| VmError::Native("String is longer than an int can hold".to_owned()))
}

fn new_string(shared: &Shared, text: String) -> Value {
    Value::Ref(shared.heap().allocate_string(text))
}

fn string_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let length = int_length(text.encode_utf16().count())?;
    Ok(Some(Value::Int(length)))
}

fn string_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    Ok(Some(Value::Int(i32::from(text.is_empty()))))
}

fn string_char_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let index = int_argument(args, 0, "String.charAt")?;
    let units = utf16_units(&text);
    let position = unit_index(index, units.len())?;
    Ok(Some(Value::Int(i32::from(units[position]))))
}

/// `String.equals(Object)`: content equality for another string, false for null and for
/// any other object.
fn string_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let Some(Value::Ref(argument)) = args.first().copied() else {
        return Err(VmError::Native(
            "String.equals expects an Object argument".to_owned(),
        ));
    };
    if argument.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    let equal = {
        let heap = shared.heap();
        match heap.get(argument).map_err(heap_error)? {
            HeapObject::Str(other) => other == &text,
            _ => false,
        }
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `String.hashCode()`: Java's `31*h + c` fold over the UTF-16 units.
fn string_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let mut hash = 0i32;
    for unit in text.encode_utf16() {
        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
    }
    Ok(Some(Value::Int(hash)))
}

/// `String.toString()`: the receiver itself, as on the JDK.
fn string_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // Validate the receiver, then return the same object: JDK `String.toString` has no
    // allocation and callers may rely on the identity.
    receiver_text(shared, receiver)?;
    Ok(Some(Value::Ref(receiver)))
}

/// `String.compareTo(String)`: UTF-16 lexicographic order.
///
/// The result is the first differing unit's difference, or the length difference when one
/// string is a prefix of the other, exactly as on the JDK.
fn string_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = utf16_units(&receiver_text(shared, receiver)?);
    let right = utf16_units(&string_argument(shared, args, 0, "String.compareTo")?);
    for (left_unit, right_unit) in left.iter().zip(right.iter()) {
        if left_unit != right_unit {
            return Ok(Some(Value::Int(
                i32::from(*left_unit) - i32::from(*right_unit),
            )));
        }
    }
    let difference = int_length(left.len())? - int_length(right.len())?;
    Ok(Some(Value::Int(difference)))
}

fn string_concat(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut text = receiver_text(shared, receiver)?;
    text.push_str(&string_argument(shared, args, 0, "String.concat")?);
    Ok(Some(new_string(shared, text)))
}

fn string_substring_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let begin = int_argument(args, 0, "String.substring")?;
    substring(shared, receiver, begin, None)
}

fn string_substring_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let begin = int_argument(args, 0, "String.substring")?;
    let end = int_argument(args, 1, "String.substring")?;
    substring(shared, receiver, begin, Some(end))
}

/// The shared body of the two `substring` overloads.
///
/// The JDK returns the receiver when the range is the whole string, and so does this, so a
/// caller can rely on the identity as well as the content.
fn substring(
    shared: &Shared,
    receiver: ObjRef,
    begin_index: i32,
    end_index: Option<i32>,
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let units = utf16_units(&text);
    let length = units.len();
    let begin = range_index(begin_index, length)?;
    let end_index = match end_index {
        Some(index) => index,
        None => int_length(length)?,
    };
    let end = range_index(end_index, length)?;
    if begin > end {
        return Err(VmError::StringIndexOutOfBounds {
            index: begin_index,
            length,
        });
    }
    if begin == 0 && end == length {
        return Ok(Some(Value::Ref(receiver)));
    }
    let text = string_from_utf16(&units[begin..end], "String.substring")?;
    Ok(Some(new_string(shared, text)))
}

fn string_starts_with(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = utf16_units(&receiver_text(shared, receiver)?);
    let prefix = utf16_units(&string_argument(shared, args, 0, "String.startsWith")?);
    Ok(Some(Value::Int(i32::from(text.starts_with(&prefix)))))
}

fn string_ends_with(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = utf16_units(&receiver_text(shared, receiver)?);
    let suffix = utf16_units(&string_argument(shared, args, 0, "String.endsWith")?);
    Ok(Some(Value::Int(i32::from(text.ends_with(&suffix)))))
}

/// `String.indexOf(String)`: the first UTF-16 unit index, or `-1`.
fn string_index_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = utf16_units(&receiver_text(shared, receiver)?);
    let needle = utf16_units(&string_argument(shared, args, 0, "String.indexOf")?);
    let found = if needle.is_empty() {
        0
    } else {
        match text
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
        {
            Some(position) => int_length(position)?,
            None => -1,
        }
    };
    Ok(Some(Value::Int(found)))
}

/// `String.replace(char, char)`: every UTF-16 unit equal to `old` becomes `new`.
fn string_replace(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let old = char_argument(args, 0, "String.replace")?;
    let new = char_argument(args, 1, "String.replace")?;
    if old == new {
        return Ok(Some(Value::Ref(receiver)));
    }
    let mut replaced = false;
    let units: Vec<u16> = text
        .encode_utf16()
        .map(|unit| {
            if unit == old {
                replaced = true;
                new
            } else {
                unit
            }
        })
        .collect();
    if !replaced {
        return Ok(Some(Value::Ref(receiver)));
    }
    let text = string_from_utf16(&units, "String.replace")?;
    Ok(Some(new_string(shared, text)))
}

/// `String.toCharArray()`: a `char[]` holding the UTF-16 units.
fn string_to_char_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let units = utf16_units(&text);
    let mut heap = shared.heap();
    let array = heap.allocate_array(ClassRef::Primitive(PrimitiveKind::Char), units.len());
    {
        let array = heap.array_mut(array).map_err(heap_error)?;
        for (slot, unit) in array.elements.iter_mut().zip(units.iter()) {
            *slot = Value::Int(i32::from(*unit));
        }
    }
    Ok(Some(Value::Ref(array)))
}

/// `String.intern()`: the one canonical object for this text.
fn string_intern(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    Ok(Some(Value::Ref(shared.intern_string(&text))))
}

/// The `String.valueOf(Object)`-shaped text of a reference.
///
/// A string and a builder contribute their content, every wrapper its `toString` text
/// (so a boxed `Double` renders as a Java double), and null the text `"null"`. Any other object uses the VM's `Object.toString` shape
/// (`ClassName@hexIdentityHash`) instead of dispatching a Java `toString` override: a VM
/// native cannot re-enter the interpreter. The shape is the one
/// [`crate::object::runtime_binary_name`] and `ObjRef::identity_hash` produce for
/// `Object.toString`, so the two agree wherever the JDK would call the default `toString`.
/// An array uses its array class's JDK descriptor-form name, so `String.valueOf(int[])`
/// is the default `[I@hex` shape.
///
/// [`crate::collections`] reuses this for `StringBuilder.append(Object)`.
pub(crate) fn object_text(
    shared: &Shared,
    thread: &ThreadState,
    reference: ObjRef,
    method: &str,
) -> Result<String, VmError> {
    if reference.is_null() {
        return Ok("null".to_owned());
    }
    enum Text {
        Known(String),
        Other,
    }
    let text = {
        let heap = shared.heap();
        match heap
            .get(reference)
            .map_err(|error| VmError::Native(error.to_string()))?
        {
            HeapObject::Str(text) => Text::Known(text.clone()),
            HeapObject::StringBuilder(text) => Text::Known(text.clone()),
            // Every wrapper renders through its own `toString`, so a boxed `Long`,
            // `Float` or `Double` uses the decimal/Java-float text rather than the
            // default `ClassName@hex` shape.
            HeapObject::Boxed(boxed) => Text::Known(boxed.text()),
            _ => Text::Other,
        }
    };
    match text {
        Text::Known(text) => Ok(text),
        Text::Other => {
            // A Java object's own `toString` is the JDK's text. The runner installs the
            // resolver from `rava-interp` because running the method re-enters the
            // interpreter; without it (a VM-only test) the structural shape stays.
            if let Some(hook) = shared.hooks().object_text_hook() {
                if let Some(text) = hook(shared, thread, reference)? {
                    return Ok(text);
                }
            }
            let name = crate::object::runtime_binary_name(shared, reference)
                .map_err(|error| VmError::Native(format!("{method}: {error}")))?;
            Ok(format!("{name}@{:x}", reference.identity_hash() as u32))
        }
    }
}

/// Java's shortest round-trip text for a `float`, with the mandatory `.0`.
///
/// The JDK's `Float.toString` chooses its own shortest representation; Rust's `Display`
/// agrees for the ordinary values this slice covers, but an exponential value is printed
/// in plain decimal here. The slice boundary records that difference.
pub fn float_text(value: f32) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        };
    }
    let mut text = format!("{value}");
    if !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

/// Java's shortest round-trip text for a `double`, with the mandatory `.0`.
pub fn double_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "Infinity".to_owned()
        } else {
            "-Infinity".to_owned()
        };
    }
    let mut text = format!("{value}");
    if !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

/// `String.valueOf(Object)`: the argument's text, with `String` returning itself.
///
/// A null argument is the interned text `"null"`, as on the JDK. A `String` argument is
/// returned unchanged, which preserves reference identity; every other object goes through
/// [`object_text`], so a wrapper becomes its `toString` text and an object without a Java
/// `toString` override gets `ClassName@hex`.
fn string_value_of_object(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = reference_argument(args, 0, "String.valueOf")?;
    if reference.is_null() {
        return Ok(Some(Value::Ref(shared.intern_string("null"))));
    }
    let is_string = {
        let heap = shared.heap();
        matches!(
            heap.get(reference)
                .map_err(|error| VmError::Native(error.to_string()))?,
            HeapObject::Str(_)
        )
    };
    if is_string {
        return Ok(Some(Value::Ref(reference)));
    }
    let text = object_text(shared, thread, reference, "String.valueOf")?;
    Ok(Some(new_string(shared, text)))
}

/// `String.valueOf(int)`: the same decimal text `Integer.toString(int)` produces.
fn string_value_of_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = int_argument(args, 0, "String.valueOf")?.to_string();
    Ok(Some(new_string(shared, text)))
}

/// `String.valueOf(long)`: decimal text.
fn string_value_of_long(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = long_argument(args, 0, "String.valueOf")?.to_string();
    Ok(Some(new_string(shared, text)))
}

/// `String.valueOf(boolean)`: `"true"` for a non-zero `int`, `"false"` for zero.
///
/// The registry sees `boolean` as the `int` a Java verifier would have on the operand
/// stack, so a non-zero value is true, exactly as `StringBuilder.append(boolean)` reads it.
fn string_value_of_boolean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "String.valueOf")?;
    let text = if value == 0 { "false" } else { "true" };
    Ok(Some(new_string(shared, text.to_owned())))
}

/// `String.valueOf(char)`: a one-unit string.
///
/// A lone surrogate is a legal Java `char` but cannot be stored in the heap's UTF-8 text,
/// so it is a typed [`VmError::UnpairedSurrogate`], the same choice the builder's `append`
/// makes.
fn string_value_of_char(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = char_argument(args, 0, "String.valueOf")?;
    let character = char::from_u32(u32::from(unit)).ok_or(VmError::UnpairedSurrogate {
        method: "String.valueOf",
    })?;
    Ok(Some(new_string(shared, character.to_string())))
}

/// `String.valueOf(float)`: the same text `StringBuilder.append(float)` produces.
fn string_value_of_float(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "String.valueOf")?;
    Ok(Some(new_string(shared, float_text(value))))
}

/// `String.valueOf(double)`: the same text `StringBuilder.append(double)` produces.
fn string_value_of_double(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "String.valueOf")?;
    Ok(Some(new_string(shared, double_text(value))))
}

fn text_of(shared: &Shared, receiver: ObjRef) -> Result<String, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "String method on a null receiver".to_owned(),
        ));
    }
    let text = shared
        .heap()
        .string(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?
        .to_owned();
    Ok(text)
}

fn set_receiver_text(shared: &Shared, receiver: ObjRef, text: String) -> Result<(), VmError> {
    *shared
        .heap()
        .string_mut(receiver)
        .map_err(|error| VmError::Native(error.to_string()))? = text;
    Ok(())
}

fn string_init_empty(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn string_init_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = match args.first().copied() {
        Some(Value::Ref(source)) if !source.is_null() => text_of(shared, source)?,
        _ => String::new(),
    };
    set_receiver_text(shared, receiver, source)?;
    Ok(None)
}

fn char_array_text(shared: &Shared, reference: ObjRef) -> Result<Vec<u16>, VmError> {
    let heap = shared.heap();
    let array = heap
        .array(reference)
        .map_err(|error| VmError::Native(error.to_string()))?;
    let mut units = Vec::with_capacity(array.elements.len());
    for element in &array.elements {
        let Value::Int(unit) = *element else {
            return Err(VmError::Native(
                "String constructor expects a char array".to_owned(),
            ));
        };
        units.push(unit as u16);
    }
    Ok(units)
}

fn string_init_chars(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = match args.first().copied() {
        Some(Value::Ref(source)) if !source.is_null() => source,
        _ => return Ok(None),
    };
    let units = char_array_text(shared, source)?;
    let text = string_from_utf16(&units, "String.<init>")?;
    set_receiver_text(shared, receiver, text)?;
    Ok(None)
}

fn string_init_chars_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(source)) = args.first().copied() else {
        return Ok(None);
    };
    if source.is_null() {
        return Ok(None);
    }
    let units = char_array_text(shared, source)?;
    let start = match args.get(1).copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        _ => return Err(VmError::Native("String.<init> has a bad offset".to_owned())),
    };
    let count = match args.get(2).copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        _ => return Err(VmError::Native("String.<init> has a bad count".to_owned())),
    };
    let end = start
        .checked_add(count)
        .ok_or_else(|| VmError::Native("String.<init> range overflow".to_owned()))?;
    let range = units.get(start..end).ok_or_else(|| {
        VmError::Native("String.<init> range is outside the char array".to_owned())
    })?;
    let text = string_from_utf16(range, "String.<init>")?;
    set_receiver_text(shared, receiver, text)?;
    Ok(None)
}

fn char_argument_of(args: &[Value], position: usize, method: &'static str) -> Result<u16, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) if (0..=0xffff).contains(&value) => Ok(value as u16),
        _ => Err(VmError::Native(format!("{method} expects a char argument"))),
    }
}

fn string_index_of_char(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = char_argument_of(args, 0, "String.indexOf")?;
    let text = text_of(shared, receiver)?;
    let units = utf16_units(&text);
    let position = units.iter().position(|candidate| *candidate == unit);
    Ok(Some(Value::Int(index_or_minus_one(position)?)))
}

fn string_index_of_char_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = char_argument_of(args, 0, "String.indexOf")?;
    let from = match args.get(1).copied() {
        Some(Value::Int(value)) => value,
        _ => return Err(VmError::Native("String.indexOf expects an int".to_owned())),
    };
    let text = text_of(shared, receiver)?;
    let units = utf16_units(&text);
    let start = from.max(0) as usize;
    if start >= units.len() {
        return Ok(Some(Value::Int(-1)));
    }
    let position = units[start..]
        .iter()
        .position(|candidate| *candidate == unit)
        .map(|position| position + start);
    Ok(Some(Value::Int(index_or_minus_one(position)?)))
}

fn string_index_of_string_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let needle = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => {
            return Err(VmError::Native(
                "String.indexOf expects a String".to_owned(),
            ));
        }
    };
    let from = match args.get(1).copied() {
        Some(Value::Int(value)) => value.max(0),
        _ => return Err(VmError::Native("String.indexOf expects an int".to_owned())),
    };
    let text = text_of(shared, receiver)?;
    Ok(Some(Value::Int(
        text.get(from as usize..)
            .and_then(|tail| tail.find(&needle))
            .map(|position| position as i32 + from)
            .unwrap_or(-1),
    )))
}

fn string_last_index_of_char(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = char_argument_of(args, 0, "String.lastIndexOf")?;
    let text = text_of(shared, receiver)?;
    let units = utf16_units(&text);
    let position = units.iter().rposition(|candidate| *candidate == unit);
    Ok(Some(Value::Int(index_or_minus_one(position)?)))
}

fn string_last_index_of_char_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let unit = char_argument_of(args, 0, "String.lastIndexOf")?;
    let from = match args.get(1).copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "String.lastIndexOf expects an int".to_owned(),
            ));
        }
    };
    let text = text_of(shared, receiver)?;
    let units = utf16_units(&text);
    if from < 0 || units.is_empty() {
        return Ok(Some(Value::Int(-1)));
    }
    let end = (from as usize).min(units.len() - 1);
    let position = units[..=end]
        .iter()
        .rposition(|candidate| *candidate == unit);
    Ok(Some(Value::Int(index_or_minus_one(position)?)))
}

fn string_last_index_of_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    string_last_index_of_string_from(
        shared,
        _thread,
        _loader,
        receiver,
        &[args[0], Value::Int(i32::MAX)],
    )
}

fn string_last_index_of_string_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let needle = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => {
            return Err(VmError::Native(
                "String.lastIndexOf expects a String".to_owned(),
            ));
        }
    };
    let from = match args.get(1).copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "String.lastIndexOf expects an int".to_owned(),
            ));
        }
    };
    let text = text_of(shared, receiver)?;
    if from < 0 {
        return Ok(Some(Value::Int(-1)));
    }
    let limit = (from as usize).saturating_add(needle.len()).min(text.len());
    let position = text[..limit].rfind(&needle);
    Ok(Some(Value::Int(
        position.map(|position| position as i32).unwrap_or(-1),
    )))
}

fn index_or_minus_one(position: Option<usize>) -> Result<i32, VmError> {
    match position {
        Some(position) => i32::try_from(position)
            .map_err(|_| VmError::Native("string is longer than an int".to_owned())),
        None => Ok(-1),
    }
}

fn string_starts_with_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let prefix = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => {
            return Err(VmError::Native(
                "String.startsWith expects a String".to_owned(),
            ));
        }
    };
    let from = match args.get(1).copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "String.startsWith expects an int".to_owned(),
            ));
        }
    };
    let text = text_of(shared, receiver)?;
    let starts = from >= 0
        && text
            .get(from as usize..)
            .map(|tail| tail.starts_with(&prefix))
            .unwrap_or(false);
    Ok(Some(Value::Int(i32::from(starts))))
}

fn string_contains(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let needle = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            // `CharSequence` is a `String` in this VM for the values Spring passes.
            text_of(shared, reference)?
        }
        _ => return Ok(Some(Value::Int(0))),
    };
    let text = text_of(shared, receiver)?;
    Ok(Some(Value::Int(i32::from(text.contains(&needle)))))
}

fn string_trim(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = text_of(shared, receiver)?;
    let trimmed = text
        .trim_matches(|candidate: char| candidate <= ' ')
        .to_owned();
    Ok(Some(new_string(shared, trimmed)))
}

fn string_strip(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = text_of(shared, receiver)?;
    Ok(Some(new_string(shared, text.trim().to_owned())))
}

fn string_is_blank(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = text_of(shared, receiver)?;
    Ok(Some(Value::Int(i32::from(text.trim().is_empty()))))
}

fn string_to_lower_case(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = text_of(shared, receiver)?;
    Ok(Some(new_string(shared, text.to_lowercase())))
}

fn string_to_upper_case(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = text_of(shared, receiver)?;
    Ok(Some(new_string(shared, text.to_uppercase())))
}

fn string_equals_ignore_case(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => return Ok(Some(Value::Int(0))),
    };
    let text = text_of(shared, receiver)?;
    Ok(Some(Value::Int(i32::from(
        text.eq_ignore_ascii_case(&other),
    ))))
}

fn string_repeat(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = match args.first().copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        _ => return Err(VmError::Native("String.repeat expects a count".to_owned())),
    };
    let text = text_of(shared, receiver)?;
    Ok(Some(new_string(shared, text.repeat(count))))
}

fn string_replace_sequence(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => {
            return Err(VmError::Native(
                "String.replace expects a CharSequence".to_owned(),
            ));
        }
    };
    let replacement = match args.get(1).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => {
            return Err(VmError::Native(
                "String.replace expects a CharSequence".to_owned(),
            ));
        }
    };
    let text = text_of(shared, receiver)?;
    Ok(Some(new_string(
        shared,
        text.replace(&target, &replacement),
    )))
}

fn string_split(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let separator = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => return Err(VmError::Native("String.split expects a String".to_owned())),
    };
    let text = text_of(shared, receiver)?;
    // M2.23 has no regular expressions; the separator is used literally.
    let parts: Vec<&str> = if text.is_empty() {
        vec![text.as_str()]
    } else {
        text.split(separator.as_str()).collect()
    };
    let strings: Vec<ObjRef> = parts
        .iter()
        .map(|part| shared.heap().allocate_string((*part).to_owned()))
        .collect();
    let class = shared.load_library_class("java/lang/String")?;
    let reference = shared
        .heap()
        .allocate_array(crate::heap::ClassRef::Named(class), strings.len());
    {
        let mut heap = shared.heap();
        let array = heap
            .array_mut(reference)
            .expect("a freshly allocated array");
        for (slot, string) in array.elements.iter_mut().zip(strings) {
            *slot = Value::Ref(string);
        }
    }
    Ok(Some(Value::Ref(reference)))
}

fn string_join(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let delimiter = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => text_of(shared, reference)?,
        _ => String::new(),
    };
    let array = match args.get(1).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => return Err(VmError::Native("String.join expects an array".to_owned())),
    };
    let elements = shared
        .heap()
        .array(array)
        .map_err(|error| VmError::Native(error.to_string()))?
        .elements
        .to_vec();
    let mut parts = Vec::with_capacity(elements.len());
    for element in elements {
        if let Value::Ref(reference) = element {
            if !reference.is_null() {
                parts.push(text_of(shared, reference)?);
            } else {
                parts.push("null".to_owned());
            }
        }
    }
    Ok(Some(new_string(shared, parts.join(&delimiter))))
}

fn string_value_of_chars(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "String.valueOf expects a char array".to_owned(),
            ));
        }
    };
    let units = char_array_text(shared, reference)?;
    let text = string_from_utf16(&units, "String.valueOf")?;
    Ok(Some(new_string(shared, text)))
}

fn string_value_of_chars_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "String.valueOf expects a char array".to_owned(),
            ));
        }
    };
    let units = char_array_text(shared, reference)?;
    let start = match args.get(1).copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        _ => {
            return Err(VmError::Native(
                "String.valueOf has a bad offset".to_owned(),
            ));
        }
    };
    let count = match args.get(2).copied() {
        Some(Value::Int(value)) if value >= 0 => value as usize,
        _ => return Err(VmError::Native("String.valueOf has a bad count".to_owned())),
    };
    let end = start
        .checked_add(count)
        .ok_or_else(|| VmError::Native("String.valueOf range overflow".to_owned()))?;
    let range = units
        .get(start..end)
        .ok_or_else(|| VmError::Native("String.valueOf range is outside the array".to_owned()))?;
    let text = string_from_utf16(range, "String.valueOf")?;
    Ok(Some(new_string(shared, text)))
}

/// `String.getBytes()`: the UTF-8 bytes, the `file.encoding` this VM reports.
fn string_get_bytes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    Ok(Some(Value::Ref(crate::resources::allocate_bytes(
        shared,
        text.as_bytes(),
    )?)))
}

/// `String.getBytes(String)`: the named charset must be UTF-8.
fn string_get_bytes_charset(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let charset = string_argument(shared, args, 0, "String.getBytes")?;
    if !charset.eq_ignore_ascii_case("UTF-8") && !charset.eq_ignore_ascii_case("UTF8") {
        return Err(VmError::Native(format!(
            "String.getBytes: charset {charset} is not supported (only UTF-8)"
        )));
    }
    string_get_bytes(shared, _thread, _loader, receiver, &[])
}

/// `String.toUpperCase()`: Unicode upper case.
///
/// The `Locale` overload ignores the locale: the VM has no locale data, and Rust's
/// Unicode mapping matches the root locale. A Turkish dotless-i mapping is the one case a
/// locale changes, and it is documented as a boundary rather than silently approximated.
fn string_to_upper(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?.to_uppercase();
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `String.toLowerCase()`: Unicode lower case; the locale is ignored as for upper case.
fn string_to_lower(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?.to_lowercase();
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `String.chars()`: the UTF-16 code units as an `IntStream`.
///
/// The stream is the VM's eager stream shape, tagged with the `IntStream` interface so the
/// `IntStream` natives receive it. A supplementary character is therefore two elements,
/// exactly as `chars()` documents.
fn string_chars(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = receiver_text(shared, receiver)?;
    let class = shared.load_library_class("java/util/stream/IntStream")?;
    let elements = text
        .encode_utf16()
        .map(|unit| Value::Int(i32::from(unit)))
        .collect();
    Ok(Some(Value::Ref(shared.heap().allocate_stream(
        elements,
        false,
        Some(class),
    ))))
}

/// `String.getBytes(Charset)`: the text in the named charset.
fn string_get_bytes_object(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(charset)) = args.first().copied() else {
        return Err(VmError::Native(
            "String.getBytes expects a Charset".to_owned(),
        ));
    };
    if charset.is_null() {
        return Err(VmError::Native("String.getBytes: null Charset".to_owned()));
    }
    let name = shared
        .charset_state()
        .name(charset)
        .ok_or_else(|| VmError::Native("String.getBytes: unknown Charset".to_owned()))?;
    let text = receiver_text(shared, receiver)?;
    let bytes = match name.as_str() {
        "UTF-8" => text.into_bytes(),
        "US-ASCII" => text
            .chars()
            .map(|character| u8::try_from(character as u32).unwrap_or(b'?'))
            .collect(),
        "ISO-8859-1" => text
            .chars()
            .map(|character| u8::try_from(character as u32).unwrap_or(b'?'))
            .collect(),
        other => {
            return Err(VmError::Native(format!(
                "String.getBytes: charset {other} is not supported"
            )));
        }
    };
    Ok(Some(Value::Ref(crate::resources::allocate_bytes(
        shared, &bytes,
    )?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_units_keep_bmp_characters_and_split_supplementary_ones() {
        assert_eq!(utf16_units(""), Vec::<u16>::new());
        assert_eq!(utf16_units("abc"), vec![0x0061, 0x0062, 0x0063]);
        // U+1F600 GRINNING FACE is one Rust `char` but two UTF-16 units.
        assert_eq!(utf16_units("😀"), vec![0xD83D, 0xDE00]);
        assert_eq!(utf16_units("a😀b"), vec![0x0061, 0xD83D, 0xDE00, 0x0062]);
    }

    #[test]
    fn string_from_utf16_round_trips_a_surrogate_pair() {
        let text = string_from_utf16(&[0xD83D, 0xDE00, 0x0062], "test").unwrap();
        assert_eq!(text, "😀b");
        assert_eq!(utf16_units(&text), vec![0xD83D, 0xDE00, 0x0062]);
    }

    #[test]
    fn string_from_utf16_rejects_a_lone_surrogate() {
        let cases: [&[u16]; 3] = [&[0xD83D], &[0xDE00], &[0x0061, 0xD83D]];
        for lone in cases {
            assert!(matches!(
                string_from_utf16(lone, "test"),
                Err(VmError::UnpairedSurrogate { method: "test" })
            ));
        }
    }
}
