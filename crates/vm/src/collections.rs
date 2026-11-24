//! `java.lang.StringBuilder` and `java.util.ArrayList` natives (M2.12).
//!
//! Both classes are VM-level heap objects rather than `HeapObject::Instance`s:
//! [`crate::HeapObject::StringBuilder`] holds the builder's text and
//! [`crate::HeapObject::List`] holds the list's elements. [`crate::Shared::allocate_instance`]
//! recognizes the two class identities, so `new` and reflective construction allocate the
//! right representation without a second class table.
//!
//! The builder stores valid UTF-8 (the same representation `java.lang.String` uses), and
//! every Java-visible operation works on UTF-16 code units through
//! [`crate::string::utf16_units`]. `length`, `charAt`, `setLength`, `deleteCharAt`,
//! `insert`, `reverse` and `append(String)` therefore count and index units, not Rust
//! `char`s, and `reverse` keeps a surrogate pair intact the way the JDK does. An
//! operation that would leave a lone surrogate (for example `setLength(1)` over a
//! surrogate pair) is a typed [`crate::VmError::UnpairedSurrogate`], because the stored
//! text cannot represent one.
//!
//! `contains`, `indexOf` and `remove(Object)` use reference identity, not `Object.equals`:
//! invoking a Java `equals` is not available to a VM native, and a silent value-equality
//! comparison would be wrong for every class that does not override `equals`. The
//! limitation is documented in the slice boundary; `null` matches a null element, as on
//! the JDK.
//!
//! There is no Java exception-object synthesis yet, so a null receiver and a negative
//! capacity are typed [`crate::VmError::Native`] errors, and an out-of-range index is
//! `StringIndexOutOfBounds` for the builder or
//! [`crate::VmError::CollectionIndexOutOfBounds`] for a list.

use crate::VmError;
use crate::heap::{HeapError, HeapObject, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::string::{double_text, float_text, object_text, string_from_utf16, utf16_units};
use crate::thread::ThreadState;
use crate::value::Value;

/// The internal name of the builder's runtime class.
pub const STRING_BUILDER_CLASS: &str = "java/lang/StringBuilder";

/// The internal name of the list's runtime class.
pub const ARRAY_LIST_CLASS: &str = "java/util/ArrayList";

/// Registers the M2.12 `java.lang.StringBuilder` and `java.util.ArrayList` natives.
///
/// Every entry is keyed by the class that declares the Java method. The C1 stubs carry no
/// `Code` attribute, so the registry is the implementation for `invokevirtual`,
/// `invokespecial` and `invokestatic` alike.
pub fn register_natives(shared: &Shared) {
    // java.lang.StringBuilder
    shared.register_native(STRING_BUILDER_CLASS, "<init>", "()V", string_builder_init);
    shared.register_native(
        STRING_BUILDER_CLASS,
        "<init>",
        "(I)V",
        string_builder_init_capacity,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "<init>",
        "(Ljava/lang/String;)V",
        string_builder_init_string,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        string_builder_append_string,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
        string_builder_append_object,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(I)Ljava/lang/StringBuilder;",
        string_builder_append_int,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(J)Ljava/lang/StringBuilder;",
        string_builder_append_long,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(C)Ljava/lang/StringBuilder;",
        string_builder_append_char,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Z)Ljava/lang/StringBuilder;",
        string_builder_append_boolean,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(F)Ljava/lang/StringBuilder;",
        string_builder_append_float,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(D)Ljava/lang/StringBuilder;",
        string_builder_append_double,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "append",
        "(Ljava/lang/CharSequence;)Ljava/lang/StringBuilder;",
        string_builder_append_char_sequence,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "toString",
        "()Ljava/lang/String;",
        string_builder_to_string,
    );
    shared.register_native(STRING_BUILDER_CLASS, "length", "()I", string_builder_length);
    shared.register_native(
        STRING_BUILDER_CLASS,
        "charAt",
        "(I)C",
        string_builder_char_at,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "setLength",
        "(I)V",
        string_builder_set_length,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "reverse",
        "()Ljava/lang/StringBuilder;",
        string_builder_reverse,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "deleteCharAt",
        "(I)Ljava/lang/StringBuilder;",
        string_builder_delete_char_at,
    );
    shared.register_native(
        STRING_BUILDER_CLASS,
        "insert",
        "(ILjava/lang/String;)Ljava/lang/StringBuilder;",
        string_builder_insert_string,
    );

    // java.util.ArrayList
    shared.register_native(ARRAY_LIST_CLASS, "<init>", "()V", array_list_init);
    shared.register_native(ARRAY_LIST_CLASS, "<init>", "(I)V", array_list_init_capacity);
    shared.register_native(ARRAY_LIST_CLASS, "size", "()I", array_list_size);
    shared.register_native(ARRAY_LIST_CLASS, "isEmpty", "()Z", array_list_is_empty);
    shared.register_native(
        ARRAY_LIST_CLASS,
        "add",
        "(Ljava/lang/Object;)Z",
        array_list_add,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "add",
        "(ILjava/lang/Object;)V",
        array_list_add_at,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "get",
        "(I)Ljava/lang/Object;",
        array_list_get,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
        array_list_set,
    );
    shared.register_native(ARRAY_LIST_CLASS, "clear", "()V", array_list_clear);
    shared.register_native(
        ARRAY_LIST_CLASS,
        "remove",
        "(I)Ljava/lang/Object;",
        array_list_remove_at,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "remove",
        "(Ljava/lang/Object;)Z",
        array_list_remove_object,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "contains",
        "(Ljava/lang/Object;)Z",
        array_list_contains,
    );
    shared.register_native(
        ARRAY_LIST_CLASS,
        "indexOf",
        "(Ljava/lang/Object;)I",
        array_list_index_of,
    );
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn null_receiver(method: &'static str) -> VmError {
    VmError::Native(format!("{method} was called on a null receiver"))
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects an int argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
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

/// A `char` argument: a UTF-16 code unit that must be a valid scalar value.
///
/// A lone surrogate is a legal Java `char` but cannot be stored in the heap's UTF-8 text,
/// so it is a typed [`VmError::UnpairedSurrogate`] rather than a corrupted builder.
fn char_argument(args: &[Value], position: usize, method: &'static str) -> Result<char, VmError> {
    let value = int_argument(args, position, method)?;
    let unit = u16::try_from(value)
        .map_err(|_| VmError::Native(format!("{method} expects a char argument")))?;
    char::from_u32(u32::from(unit)).ok_or(VmError::UnpairedSurrogate { method })
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

/// A `String` argument. A null reference becomes the JDK's `"null"` text.
fn nullable_string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<String, VmError> {
    let reference = reference_argument(args, position, method)?;
    if reference.is_null() {
        return Ok("null".to_owned());
    }
    let text = shared
        .heap()
        .string(reference)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

/// The builder's current text, copied out of the heap before another lock is taken.
fn builder_text(
    shared: &Shared,
    receiver: ObjRef,
    method: &'static str,
) -> Result<String, VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    let text = shared
        .heap()
        .string_builder(receiver)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

/// Replace the builder's text and return the receiver, as the JDK's builder methods do.
fn set_builder_text(
    shared: &Shared,
    receiver: ObjRef,
    text: String,
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    *shared
        .heap()
        .string_builder_mut(receiver)
        .map_err(heap_error)? = text;
    Ok(Some(Value::Ref(receiver)))
}

/// Append `text` to the builder and return the receiver.
fn append_text(
    shared: &Shared,
    receiver: ObjRef,
    text: &str,
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    shared
        .heap()
        .string_builder_mut(receiver)
        .map_err(heap_error)?
        .push_str(text);
    Ok(Some(Value::Ref(receiver)))
}

/// A UTF-16 unit index in `0..length`, or a typed `StringIndexOutOfBounds`.
fn unit_index(index: i32, length: usize) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position < length => Ok(position),
        _ => Err(VmError::StringIndexOutOfBounds { index, length }),
    }
}

/// An insertion point in `0..=length`, or a typed `StringIndexOutOfBounds`.
fn insertion_index(index: i32, length: usize) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position <= length => Ok(position),
        _ => Err(VmError::StringIndexOutOfBounds { index, length }),
    }
}

/// Reverse UTF-16 units the way `AbstractStringBuilder.reverse` does.
///
/// The units are reversed first; when that turns a surrogate pair into a low/high pair,
/// the two halves are swapped back so a supplementary character stays one character.
fn reversed_units(units: &[u16]) -> Vec<u16> {
    let mut reversed: Vec<u16> = units.iter().copied().rev().collect();
    let mut index = 0;
    while index + 1 < reversed.len() {
        let low = (0xDC00..=0xDFFF).contains(&reversed[index]);
        let high = (0xD800..=0xDBFF).contains(&reversed[index + 1]);
        if low && high {
            reversed.swap(index, index + 1);
            index += 2;
        } else {
            index += 1;
        }
    }
    reversed
}

// ---------------------------------------------------------------------------
// java.lang.StringBuilder
// ---------------------------------------------------------------------------

fn string_builder_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    set_builder_text(shared, receiver, String::new(), "StringBuilder.<init>")?;
    Ok(None)
}

fn string_builder_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "StringBuilder.<init>")?;
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "StringBuilder.<init>: negative capacity {capacity}"
        )));
    }
    // The capacity is a performance hint; the UTF-16 text has no separate buffer size.
    set_builder_text(shared, receiver, String::new(), "StringBuilder.<init>")?;
    Ok(None)
}

fn string_builder_init_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = nullable_string_argument(shared, args, 0, "StringBuilder.<init>")?;
    set_builder_text(shared, receiver, text, "StringBuilder.<init>")?;
    Ok(None)
}

fn string_builder_append_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = nullable_string_argument(shared, args, 0, "StringBuilder.append")?;
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_object(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = reference_argument(args, 0, "StringBuilder.append")?;
    let text = object_text(shared, reference, "StringBuilder.append")?;
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = int_argument(args, 0, "StringBuilder.append")?.to_string();
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_long(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = long_argument(args, 0, "StringBuilder.append")?.to_string();
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_char(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut text = String::new();
    text.push(char_argument(args, 0, "StringBuilder.append")?);
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_boolean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "StringBuilder.append")?;
    let text = if value == 0 { "false" } else { "true" };
    append_text(shared, receiver, text, "StringBuilder.append")
}

fn string_builder_append_float(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = float_text(float_argument(args, 0, "StringBuilder.append")?);
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_append_double(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = double_text(double_argument(args, 0, "StringBuilder.append")?);
    append_text(shared, receiver, &text, "StringBuilder.append")
}

/// `StringBuilder.append(CharSequence)`: the text of a String or another builder.
///
/// Implementing `CharSequence` through `length`/`charAt` would require re-entering the
/// interpreter, so the two character sequences the class library models are accepted and
/// any other reference is a typed error rather than a guessed text.
fn string_builder_append_char_sequence(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = reference_argument(args, 0, "StringBuilder.append")?;
    let text = if reference.is_null() {
        "null".to_owned()
    } else {
        let heap = shared.heap();
        match heap.get(reference).map_err(heap_error)? {
            HeapObject::Str(text) | HeapObject::StringBuilder(text) => text.clone(),
            _ => {
                return Err(VmError::Native(format!(
                    "{reference} is not a String or StringBuilder CharSequence"
                )));
            }
        }
    };
    append_text(shared, receiver, &text, "StringBuilder.append")
}

fn string_builder_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.toString")?;
    let reference = shared.heap().allocate_string(text);
    Ok(Some(Value::Ref(reference)))
}

fn string_builder_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.length")?;
    let length = i32::try_from(text.encode_utf16().count())
        .map_err(|_| VmError::Native("StringBuilder is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(length)))
}

fn string_builder_char_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.charAt")?;
    let units = utf16_units(&text);
    let index = int_argument(args, 0, "StringBuilder.charAt")?;
    let position = unit_index(index, units.len())?;
    Ok(Some(Value::Int(i32::from(units[position]))))
}

/// `StringBuilder.setLength(int)`: shrink, or grow with NUL code units, as on the JDK.
///
/// A negative length is `StringIndexOutOfBounds`, and a shrink that would split a
/// surrogate pair is `UnpairedSurrogate`, because the stored UTF-8 text cannot hold the
/// lone half.
fn string_builder_set_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.setLength")?;
    let units = utf16_units(&text);
    let new_length = int_argument(args, 0, "StringBuilder.setLength")?;
    let position = match usize::try_from(new_length) {
        Ok(position) => position,
        _ => {
            return Err(VmError::StringIndexOutOfBounds {
                index: new_length,
                length: units.len(),
            });
        }
    };
    // `resize` truncates a shrink and grows with NUL code units, which is what
    // `AbstractStringBuilder.setLength` does.
    let mut resized = units;
    resized.resize(position, 0u16);
    let text = string_from_utf16(&resized, "StringBuilder.setLength")?;
    set_builder_text(shared, receiver, text, "StringBuilder.setLength")?;
    Ok(None)
}

/// `StringBuilder.reverse()`: reverse the UTF-16 units and keep surrogate pairs intact.
fn string_builder_reverse(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.reverse")?;
    let units = utf16_units(&text);
    let reversed = reversed_units(&units);
    let text = string_from_utf16(&reversed, "StringBuilder.reverse")?;
    set_builder_text(shared, receiver, text, "StringBuilder.reverse")
}

fn string_builder_delete_char_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.deleteCharAt")?;
    let mut units = utf16_units(&text);
    let index = int_argument(args, 0, "StringBuilder.deleteCharAt")?;
    let position = unit_index(index, units.len())?;
    units.remove(position);
    let text = string_from_utf16(&units, "StringBuilder.deleteCharAt")?;
    set_builder_text(shared, receiver, text, "StringBuilder.deleteCharAt")
}

fn string_builder_insert_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = builder_text(shared, receiver, "StringBuilder.insert")?;
    let mut units = utf16_units(&text);
    let index = int_argument(args, 0, "StringBuilder.insert")?;
    let position = insertion_index(index, units.len())?;
    let inserted = nullable_string_argument(shared, args, 1, "StringBuilder.insert")?;
    units.splice(position..position, inserted.encode_utf16());
    let text = string_from_utf16(&units, "StringBuilder.insert")?;
    set_builder_text(shared, receiver, text, "StringBuilder.insert")
}

// ---------------------------------------------------------------------------
// java.util.ArrayList
// ---------------------------------------------------------------------------

fn array_list_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(null_receiver("ArrayList.<init>"));
    }
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .clear();
    Ok(None)
}

fn array_list_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "ArrayList.<init>")?;
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "ArrayList.<init>: negative capacity {capacity}"
        )));
    }
    if receiver.is_null() {
        return Err(null_receiver("ArrayList.<init>"));
    }
    // The capacity is a performance hint; a Vec has no separate capacity contract here.
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .clear();
    Ok(None)
}

fn array_list_size(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let length = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        list.len()
    };
    let size = i32::try_from(length)
        .map_err(|_| VmError::Native("ArrayList is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(size)))
}

fn array_list_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let empty = {
        let heap = shared.heap();
        heap.list(receiver).map_err(heap_error)?.is_empty()
    };
    Ok(Some(Value::Int(i32::from(empty))))
}

fn array_list_add(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "ArrayList.add")?;
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .push(Value::Ref(value));
    Ok(Some(Value::Int(1)))
}

/// An insertion point in `0..=length`, or a typed `CollectionIndexOutOfBounds`.
fn list_insertion_index(
    operation: &'static str,
    index: i32,
    length: usize,
) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position <= length => Ok(position),
        _ => Err(VmError::CollectionIndexOutOfBounds {
            operation,
            index,
            length,
        }),
    }
}

/// An element index in `0..length`, or a typed `CollectionIndexOutOfBounds`.
fn list_element_index(
    operation: &'static str,
    index: i32,
    length: usize,
) -> Result<usize, VmError> {
    match usize::try_from(index) {
        Ok(position) if position < length => Ok(position),
        _ => Err(VmError::CollectionIndexOutOfBounds {
            operation,
            index,
            length,
        }),
    }
}

fn array_list_add_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.add")?;
    let value = reference_argument(args, 1, "ArrayList.add")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let position = list_insertion_index("ArrayList.add", index, list.len())?;
    list.insert(position, Value::Ref(value));
    Ok(None)
}

fn array_list_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.get")?;
    let value = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        let position = list_element_index("ArrayList.get", index, list.len())?;
        list[position]
    };
    Ok(Some(value))
}

fn array_list_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.set")?;
    let value = reference_argument(args, 1, "ArrayList.set")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let position = list_element_index("ArrayList.set", index, list.len())?;
    let previous = std::mem::replace(&mut list[position], Value::Ref(value));
    Ok(Some(previous))
}

fn array_list_clear(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared
        .heap()
        .list_mut(receiver)
        .map_err(heap_error)?
        .clear();
    Ok(None)
}

fn array_list_remove_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = int_argument(args, 0, "ArrayList.remove")?;
    let removed = {
        let mut heap = shared.heap();
        let list = heap.list_mut(receiver).map_err(heap_error)?;
        let position = list_element_index("ArrayList.remove", index, list.len())?;
        list.remove(position)
    };
    Ok(Some(removed))
}

/// `ArrayList.remove(Object)`: remove the first element with the same reference.
///
/// A null argument removes the first null element, as on the JDK; any other argument is
/// matched by reference identity, not by `Object.equals`.
fn array_list_remove_object(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "ArrayList.remove")?;
    let mut heap = shared.heap();
    let list = heap.list_mut(receiver).map_err(heap_error)?;
    let Some(position) = list
        .iter()
        .position(|element| *element == Value::Ref(target))
    else {
        return Ok(Some(Value::Int(0)));
    };
    list.remove(position);
    Ok(Some(Value::Int(1)))
}

fn array_list_contains(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "ArrayList.contains")?;
    let found = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        list.contains(&Value::Ref(target))
    };
    Ok(Some(Value::Int(i32::from(found))))
}

fn array_list_index_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let target = reference_argument(args, 0, "ArrayList.indexOf")?;
    let position = {
        let heap = shared.heap();
        let list = heap.list(receiver).map_err(heap_error)?;
        list.iter()
            .position(|element| *element == Value::Ref(target))
    };
    let index = match position {
        Some(position) => i32::try_from(position)
            .map_err(|_| VmError::Native("ArrayList is longer than an int can hold".to_owned()))?,
        None => -1,
    };
    Ok(Some(Value::Int(index)))
}
