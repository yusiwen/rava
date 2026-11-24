//! `java.text.MessageFormat` natives (M2.29).
//!
//! The runtime class is [`crate::HeapObject::MessageFormat`], a dedicated heap shape that
//! holds the pattern text; the C1 stub declares no instance field for it and this slice
//! does not invent one. A `Locale` is accepted by the two-argument constructor but not
//! stored, because the supported formatting subset is locale-independent.
//!
//! The implemented surface is the smallest one Spring's `MessageSourceSupport` static
//! initializer and its `formatMessage` path need:
//!
//! - `<init>(String)`, `<init>(String, Locale)`, `applyPattern(String)` and
//!   `toPattern()`;
//! - `Format.format(Object)` (the inherited `final` method Spring calls), which treats its
//!   argument as a `java.lang.Object[]`;
//! - `MessageFormat.format(Object, StringBuffer, FieldPosition)` and its `Object[]`
//!   overload, both returning the same `StringBuffer` with the formatted text appended;
//! - the static `MessageFormat.format(String, Object...)`.
//!
//! The pattern parser supports the argument-only form `{index}`, repeated indices,
//! literal text and the JDK's single-quote quoting rules (`''` is one quote, a lone quote
//! starts or ends a quoted section that keeps `{`/`}` literal). A `number`, `date`,
//! `time`, `choice` or other subformat is a typed [`crate::VmError::Native`] when a
//! non-null argument reaches it; an absent argument keeps the JDK's literal `{index}`
//! text, and a null argument formats as `"null"`. `parse`, `setFormat(s)`,
//! `getFormats`, `formatToCharacterIterator`, `equals`/`hashCode`/`clone` and
//! `setLocale`/`getLocale` have no native, so they keep failing loudly rather than
//! guessing (the M2.23 acceptance never calls them).
//!
//! An argument's text is the VM's `String.valueOf`-shaped conversion, not a virtual
//! `toString()` dispatch: a `String` is itself, a box is its boxed text, and every other
//! object is the M2.9 `ClassName@hex` shape. The JDK would call the argument's Java
//! `toString` override for a `{index}` placeholder.
//!
//! The `StringBuffer` parameter needs a receiver shape, so this slice adds
//! [`crate::HeapObject::StringBuffer`] and a minimal surface: both constructors,
//! `append(String)`, `toString` and `length`. Other `StringBuffer` methods have no native.
//!
//! Pattern errors (an unmatched `{`, a non-numeric argument index) are raised by the
//! constructor and `applyPattern` as typed errors, not as the JDK's
//! `IllegalArgumentException`; the VM has no exception-object synthesis.

use crate::VmError;
use crate::heap::{ArrayKind, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::string::{object_text, utf16_units};
use crate::thread::ThreadState;
use crate::value::Value;

/// `java.text.MessageFormat`'s binary-stable internal name.
pub const MESSAGE_FORMAT_CLASS: &str = "java/text/MessageFormat";

/// `java.text.Format`'s binary-stable internal name: the class Spring's
/// `Format.format(Object)` call resolves to.
pub const FORMAT_CLASS: &str = "java/text/Format";

/// `java.lang.StringBuffer`'s binary-stable internal name.
pub const STRING_BUFFER_CLASS: &str = "java/lang/StringBuffer";

/// Registers the `MessageFormat` and `StringBuffer` natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        MESSAGE_FORMAT_CLASS,
        "<init>",
        "(Ljava/lang/String;)V",
        message_format_init,
    );
    shared.register_native(
        MESSAGE_FORMAT_CLASS,
        "<init>",
        "(Ljava/lang/String;Ljava/util/Locale;)V",
        message_format_init_locale,
    );
    shared.register_native(
        MESSAGE_FORMAT_CLASS,
        "applyPattern",
        "(Ljava/lang/String;)V",
        message_format_apply_pattern,
    );
    shared.register_native(
        MESSAGE_FORMAT_CLASS,
        "toPattern",
        "()Ljava/lang/String;",
        message_format_to_pattern,
    );
    shared.register_native(
        FORMAT_CLASS,
        "format",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        message_format_format_object,
    );
    shared.register_native(
        MESSAGE_FORMAT_CLASS,
        "format",
        "(Ljava/lang/Object;Ljava/lang/StringBuffer;Ljava/text/FieldPosition;)Ljava/lang/StringBuffer;",
        message_format_format_object_buffer,
    );
    shared.register_native(
        MESSAGE_FORMAT_CLASS,
        "format",
        "([Ljava/lang/Object;Ljava/lang/StringBuffer;Ljava/text/FieldPosition;)Ljava/lang/StringBuffer;",
        message_format_format_array_buffer,
    );
    shared.register_native(
        MESSAGE_FORMAT_CLASS,
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        message_format_format_static,
    );

    shared.register_native(STRING_BUFFER_CLASS, "<init>", "()V", string_buffer_init);
    shared.register_native(
        STRING_BUFFER_CLASS,
        "<init>",
        "(Ljava/lang/String;)V",
        string_buffer_init_string,
    );
    shared.register_native(
        STRING_BUFFER_CLASS,
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuffer;",
        string_buffer_append,
    );
    shared.register_native(
        STRING_BUFFER_CLASS,
        "toString",
        "()Ljava/lang/String;",
        string_buffer_to_string,
    );
    shared.register_native(STRING_BUFFER_CLASS, "length", "()I", string_buffer_length);
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
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

/// The receiver's pattern text.
fn receiver_pattern(
    shared: &Shared,
    receiver: ObjRef,
    method: &'static str,
) -> Result<String, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(format!(
            "{method} called with a null receiver"
        )));
    }
    let pattern = shared
        .heap()
        .message_format(receiver)
        .map_err(heap_error)?
        .pattern
        .clone();
    Ok(pattern)
}

/// The argument array of a format call.
///
/// `null` is the JDK's empty argument list. A reference value must be an `Object[]`-shaped
/// array; a non-array is the JDK's `ClassCastException`, reported here as a typed error.
fn argument_array(
    shared: &Shared,
    value: Option<Value>,
    method: &'static str,
) -> Result<Vec<Value>, VmError> {
    let value =
        value.ok_or_else(|| VmError::Native(format!("{method} is missing its argument")))?;
    let reference = match value {
        Value::Ref(reference) => reference,
        _ => {
            return Err(VmError::Native(format!(
                "{method} expects an Object[] argument"
            )));
        }
    };
    if reference.is_null() {
        return Ok(Vec::new());
    }
    let elements = {
        let heap = shared.heap();
        let array = heap.array(reference).map_err(heap_error)?;
        if array.kind != ArrayKind::Reference {
            return Err(VmError::Native(format!(
                "{method} expects a reference array, found a {} array",
                array.kind
            )));
        }
        array.elements.clone()
    };
    Ok(elements)
}

/// The `StringBuffer` a formatting call appends to.
fn buffer_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    let reference = match args.get(position).copied() {
        Some(Value::Ref(reference)) => reference,
        Some(_) => {
            return Err(VmError::Native(format!(
                "{method} expects a StringBuffer argument"
            )));
        }
        None => {
            return Err(VmError::Native(format!("{method} is missing its argument")));
        }
    };
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{method} received a null StringBuffer"
        )));
    }
    // Check the shape now, so a wrong buffer object is reported before any text is built.
    shared.heap().string_buffer(reference).map_err(heap_error)?;
    Ok(reference)
}

/// One argument's text for a `{index}` placeholder.
fn argument_text(shared: &Shared, value: Value, method: &'static str) -> Result<String, VmError> {
    match value {
        Value::Ref(reference) => object_text(shared, reference, method),
        // An `Object[]` element is always a reference; a primitive here means the caller
        // handed the native a malformed array.
        _ => Err(VmError::Native(format!(
            "{method} found a non-reference argument element"
        ))),
    }
}

/// Parse one placeholder's inner text (`0`, `0,number`, `0,number,#.##`).
fn parse_placeholder(segment: &str) -> Result<(usize, String), VmError> {
    let digits = segment
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(segment.len());
    if digits == 0 {
        return Err(VmError::Native(format!(
            "MessageFormat: can't parse argument number: {segment}"
        )));
    }
    let index = segment[..digits].parse::<usize>().map_err(|_| {
        VmError::Native(format!(
            "MessageFormat: can't parse argument number: {segment}"
        ))
    })?;
    let rest = &segment[digits..];
    if rest.is_empty() {
        return Ok((index, String::new()));
    }
    let Some(style) = rest.strip_prefix(',') else {
        return Err(VmError::Native(format!(
            "MessageFormat: can't parse argument number: {segment}"
        )));
    };
    // Only the format type matters here; the style after the next comma is part of an
    // unsupported subformat and is reported when the subformat itself is rejected.
    let format_type = style.split(',').next().unwrap_or_default().trim();
    Ok((index, format_type.to_owned()))
}

/// Format `pattern` over `args`.
fn format_pattern(
    shared: &Shared,
    pattern: &str,
    args: &[Value],
    method: &'static str,
) -> Result<String, VmError> {
    let mut out = String::new();
    let mut in_quote = false;
    let mut characters = pattern.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\'' => {
                if characters.peek() == Some(&'\'') {
                    characters.next();
                    out.push('\'');
                } else {
                    in_quote = !in_quote;
                }
            }
            '{' if !in_quote => {
                let mut segment = String::new();
                let mut closed = false;
                for inner in characters.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                    segment.push(inner);
                }
                if !closed {
                    return Err(VmError::Native(
                        "MessageFormat: Unmatched braces in the pattern".to_owned(),
                    ));
                }
                let (index, format_type) = parse_placeholder(&segment)?;
                match args.get(index) {
                    // The JDK keeps the placeholder text when the argument is absent.
                    None => {
                        out.push('{');
                        out.push_str(&index.to_string());
                        out.push('}');
                    }
                    Some(Value::Ref(reference)) if reference.is_null() => {
                        out.push_str("null");
                    }
                    Some(value) => {
                        if !format_type.is_empty() {
                            return Err(VmError::Native(format!(
                                "MessageFormat: the `{format_type}` subformat is not implemented"
                            )));
                        }
                        out.push_str(&argument_text(shared, *value, method)?);
                    }
                }
            }
            _ => out.push(character),
        }
    }
    Ok(out)
}

/// `MessageFormat.<init>(String)` and `MessageFormat.<init>(String, Locale)`.
fn message_format_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "MessageFormat.<init>")?;
    // Parse once so a malformed pattern fails at construction, as on the JDK.
    format_pattern(shared, &pattern, &[], "MessageFormat.<init>")?;
    set_pattern(shared, receiver, pattern)?;
    Ok(None)
}

/// `MessageFormat.<init>(String, Locale)`: the locale is accepted and ignored.
fn message_format_init_locale(
    shared: &Shared,
    thread: &ThreadState,
    loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    message_format_init(shared, thread, loader, receiver, args)
}

/// `MessageFormat.applyPattern(String)`.
fn message_format_apply_pattern(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "MessageFormat.applyPattern")?;
    format_pattern(shared, &pattern, &[], "MessageFormat.applyPattern")?;
    set_pattern(shared, receiver, pattern)?;
    Ok(None)
}

fn set_pattern(shared: &Shared, receiver: ObjRef, pattern: String) -> Result<(), VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "MessageFormat method called with a null receiver".to_owned(),
        ));
    }
    shared
        .heap()
        .message_format_mut(receiver)
        .map_err(heap_error)?
        .pattern = pattern;
    Ok(())
}

/// `MessageFormat.toPattern()`.
fn message_format_to_pattern(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = receiver_pattern(shared, receiver, "MessageFormat.toPattern")?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(pattern))))
}

/// `Format.format(Object)`: format an `Object[]` (or `null`) into a fresh `String`.
fn message_format_format_object(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = receiver_pattern(shared, receiver, "Format.format")?;
    let arguments = argument_array(shared, args.first().copied(), "Format.format")?;
    let text = format_pattern(shared, &pattern, &arguments, "Format.format")?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `MessageFormat.format(Object, StringBuffer, FieldPosition)`.
fn message_format_format_object_buffer(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = receiver_pattern(shared, receiver, "MessageFormat.format")?;
    let arguments = argument_array(shared, args.first().copied(), "MessageFormat.format")?;
    append_to_buffer(shared, &pattern, &arguments, args)
}

/// `MessageFormat.format(Object[], StringBuffer, FieldPosition)`.
fn message_format_format_array_buffer(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = receiver_pattern(shared, receiver, "MessageFormat.format")?;
    let arguments = argument_array(shared, args.first().copied(), "MessageFormat.format")?;
    append_to_buffer(shared, &pattern, &arguments, args)
}

fn append_to_buffer(
    shared: &Shared,
    pattern: &str,
    arguments: &[Value],
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let buffer = buffer_argument(shared, args, 1, "MessageFormat.format")?;
    let text = format_pattern(shared, pattern, arguments, "MessageFormat.format")?;
    shared
        .heap()
        .string_buffer_mut(buffer)
        .map_err(heap_error)?
        .push_str(&text);
    Ok(Some(Value::Ref(buffer)))
}

/// The static `MessageFormat.format(String, Object...)`.
fn message_format_format_static(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "MessageFormat.format")?;
    let arguments = argument_array(shared, args.get(1).copied(), "MessageFormat.format")?;
    let text = format_pattern(shared, &pattern, &arguments, "MessageFormat.format")?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `StringBuffer.<init>()`.
fn string_buffer_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // The heap allocation already starts empty.
    Ok(None)
}

/// `StringBuffer.<init>(String)`: the initial text, or a typed error for null.
fn string_buffer_init_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "StringBuffer.<init>")?;
    set_buffer_text(shared, receiver, text)?;
    Ok(None)
}

/// `StringBuffer.append(String)`: returns the receiver, as on the JDK.
fn string_buffer_append(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "StringBuffer.append called with a null receiver".to_owned(),
        ));
    }
    let text = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => shared
            .heap()
            .string(reference)
            .map_err(heap_error)?
            .to_owned(),
        Some(Value::Ref(_)) => "null".to_owned(),
        Some(_) => {
            return Err(VmError::Native(
                "StringBuffer.append(String) expects a String argument".to_owned(),
            ));
        }
        None => {
            return Err(VmError::Native(
                "StringBuffer.append(String) is missing its argument".to_owned(),
            ));
        }
    };
    shared
        .heap()
        .string_buffer_mut(receiver)
        .map_err(heap_error)?
        .push_str(&text);
    Ok(Some(Value::Ref(receiver)))
}

/// `StringBuffer.toString()`.
fn string_buffer_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = buffer_text(shared, receiver)?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `StringBuffer.length()`: UTF-16 code units, as on the JDK.
fn string_buffer_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = buffer_text(shared, receiver)?;
    let length = i32::try_from(utf16_units(&text).len())
        .map_err(|_| VmError::Native("StringBuffer is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(length)))
}

fn buffer_text(shared: &Shared, receiver: ObjRef) -> Result<String, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "StringBuffer method called with a null receiver".to_owned(),
        ));
    }
    Ok(shared
        .heap()
        .string_buffer(receiver)
        .map_err(heap_error)?
        .to_owned())
}

fn set_buffer_text(shared: &Shared, receiver: ObjRef, text: String) -> Result<(), VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "StringBuffer constructor called with a null receiver".to_owned(),
        ));
    }
    *shared
        .heap()
        .string_buffer_mut(receiver)
        .map_err(heap_error)? = text;
    Ok(())
}
