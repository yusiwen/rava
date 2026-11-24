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
use crate::heap::{ArrayKind, HeapError, HeapObject, ObjRef};
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
    shared.register_native(
        "java/lang/String",
        "compareTo",
        "(Ljava/lang/String;)I",
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
    let array = heap.allocate_array(ArrayKind::Char, units.len());
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
