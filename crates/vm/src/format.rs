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
    shared.register_native(
        STRING_BUFFER_CLASS,
        "setLength",
        "(I)V",
        string_buffer_set_length,
    );
    shared.register_native(STRING_BUFFER_CLASS, "charAt", "(I)C", string_buffer_char_at);
    for (descriptor, function) in [
        (
            "(C)Ljava/lang/StringBuffer;",
            string_buffer_append_char as crate::NativeFn,
        ),
        ("(I)Ljava/lang/StringBuffer;", string_buffer_append_int),
        ("(J)Ljava/lang/StringBuffer;", string_buffer_append_long),
        ("(F)Ljava/lang/StringBuffer;", string_buffer_append_float),
        ("(D)Ljava/lang/StringBuffer;", string_buffer_append_double),
        ("(Z)Ljava/lang/StringBuffer;", string_buffer_append_boolean),
        (
            "(Ljava/lang/CharSequence;)Ljava/lang/StringBuffer;",
            string_buffer_append_object,
        ),
        (
            "(Ljava/lang/Object;)Ljava/lang/StringBuffer;",
            string_buffer_append_object,
        ),
    ] {
        shared.register_native(STRING_BUFFER_CLASS, "append", descriptor, function);
    }

    // `java.lang.String.format`: a formatter subset, not `java.util.Formatter`. The
    // supported conversions are the ones the pinned libraries use at startup (`%s` and
    // `%d`), plus the ordinary `%b`, `%c`, `%o`, `%x`, `%f`, `%e`, `%h`, `%%` and `%n`
    // forms and a width/precision. An unsupported conversion or flag is a typed error.
    shared.register_native(
        "java/lang/String",
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        string_format,
    );
    shared.register_native(
        "java/lang/String",
        "format",
        "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
        string_format_locale,
    );
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
fn argument_text(
    shared: &Shared,
    thread: &ThreadState,
    value: Value,
    method: &'static str,
) -> Result<String, VmError> {
    match value {
        Value::Ref(reference) => object_text(shared, thread, reference, method),
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
    thread: &ThreadState,
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
                        out.push_str(&argument_text(shared, thread, *value, method)?);
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
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "MessageFormat.<init>")?;
    // Parse once so a malformed pattern fails at construction, as on the JDK.
    format_pattern(shared, thread, &pattern, &[], "MessageFormat.<init>")?;
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
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "MessageFormat.applyPattern")?;
    format_pattern(shared, thread, &pattern, &[], "MessageFormat.applyPattern")?;
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
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = receiver_pattern(shared, receiver, "Format.format")?;
    let arguments = argument_array(shared, args.first().copied(), "Format.format")?;
    let text = format_pattern(shared, thread, &pattern, &arguments, "Format.format")?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `MessageFormat.format(Object, StringBuffer, FieldPosition)`.
fn message_format_format_object_buffer(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = receiver_pattern(shared, receiver, "MessageFormat.format")?;
    let arguments = argument_array(shared, args.first().copied(), "MessageFormat.format")?;
    append_to_buffer(shared, thread, &pattern, &arguments, args)
}

/// `MessageFormat.format(Object[], StringBuffer, FieldPosition)`.
fn message_format_format_array_buffer(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = receiver_pattern(shared, receiver, "MessageFormat.format")?;
    let arguments = argument_array(shared, args.first().copied(), "MessageFormat.format")?;
    append_to_buffer(shared, thread, &pattern, &arguments, args)
}

fn append_to_buffer(
    shared: &Shared,
    thread: &ThreadState,
    pattern: &str,
    arguments: &[Value],
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let buffer = buffer_argument(shared, args, 1, "MessageFormat.format")?;
    let text = format_pattern(shared, thread, pattern, arguments, "MessageFormat.format")?;
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
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "MessageFormat.format")?;
    let arguments = argument_array(shared, args.get(1).copied(), "MessageFormat.format")?;
    let text = format_pattern(shared, thread, &pattern, &arguments, "MessageFormat.format")?;
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

// ---------------------------------------------------------------------------
// `java.lang.String.format`
// ---------------------------------------------------------------------------

/// `String.format(String, Object...)`.
fn string_format(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 0, "String.format")?;
    let arguments = argument_array(shared, args.get(1).copied(), "String.format")?;
    let text = format_java(shared, thread, &pattern, &arguments, "String.format")?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// `String.format(Locale, String, Object...)`.
///
/// The locale is ignored: the supported conversions are locale-independent, and a
/// locale-sensitive conversion is a typed error rather than a silently wrong locale.
fn string_format_locale(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let pattern = string_argument(shared, args, 1, "String.format")?;
    let arguments = argument_array(shared, args.get(2).copied(), "String.format")?;
    let text = format_java(shared, thread, &pattern, &arguments, "String.format")?;
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

/// The formatting flags this subset honours.
#[derive(Debug, Clone, Copy, Default)]
struct FormatFlags {
    left_align: bool,
    zero_pad: bool,
    plus: bool,
    space: bool,
}

/// Format `pattern` over `arguments` with the `java.util.Formatter` conversion subset.
fn format_java(
    shared: &Shared,
    thread: &ThreadState,
    pattern: &str,
    arguments: &[Value],
    method: &'static str,
) -> Result<String, VmError> {
    let mut out = String::new();
    let mut next_argument = 0usize;
    let mut characters = pattern.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '%' {
            out.push(character);
            continue;
        }
        let Some(&conversion) = characters.peek() else {
            return Err(VmError::Native(format!(
                "{method}: a '%' at the end of the format string"
            )));
        };
        match conversion {
            '%' => {
                characters.next();
                out.push('%');
                continue;
            }
            'n' => {
                characters.next();
                out.push('\n');
                continue;
            }
            _ => {}
        }
        let mut flags = FormatFlags::default();
        while let Some(flag) = characters.peek().copied() {
            let handled = match flag {
                '-' => Some(FormatFlags {
                    left_align: true,
                    ..flags
                }),
                '0' => Some(FormatFlags {
                    zero_pad: true,
                    ..flags
                }),
                '+' => Some(FormatFlags {
                    plus: true,
                    ..flags
                }),
                ' ' => Some(FormatFlags {
                    space: true,
                    ..flags
                }),
                // `,` (grouping) and `(` (negative parentheses) have no implementation
                // here; refusing them keeps the output honest instead of dropping them.
                ',' | '(' | '#' => {
                    return Err(VmError::Native(format!(
                        "{method}: format flag '{flag}' is not implemented"
                    )));
                }
                _ => None,
            };
            match handled {
                Some(next) => {
                    flags = next;
                    characters.next();
                }
                None => break,
            }
        }
        let mut width = 0usize;
        while let Some(digit) = characters.peek().and_then(|c| c.to_digit(10)) {
            width = width * 10 + digit as usize;
            characters.next();
        }
        let mut precision = None;
        if characters.peek() == Some(&'.') {
            characters.next();
            let mut value = 0usize;
            while let Some(digit) = characters.peek().and_then(|c| c.to_digit(10)) {
                value = value * 10 + digit as usize;
                characters.next();
            }
            precision = Some(value);
        }
        let Some(conversion) = characters.next() else {
            return Err(VmError::Native(format!(
                "{method}: format string ended inside a conversion"
            )));
        };
        let argument = arguments.get(next_argument).copied();
        next_argument += 1;
        let mut text = format_conversion(
            shared,
            thread,
            conversion,
            argument,
            precision,
            flags,
            next_argument - 1,
            method,
        )?;
        if flags.plus
            && !text.starts_with('-')
            && text.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            text.insert(0, '+');
        } else if flags.space && !text.starts_with('-') {
            text.insert(0, ' ');
        }
        if let Some(precision) = precision {
            if matches!(conversion, 's' | 'S' | 'b' | 'B' | 'h' | 'H' | 'c' | 'C') {
                text = text.chars().take(precision).collect();
            }
        }
        if text.chars().count() < width {
            let padding = width - text.chars().count();
            if flags.left_align {
                text.extend(std::iter::repeat_n(' ', padding));
            } else if flags.zero_pad
                && text
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit() || c == '-')
            {
                let sign = text.starts_with('-');
                let digits = text.trim_start_matches('-');
                let mut padded = String::new();
                if sign {
                    padded.push('-');
                }
                padded.extend(std::iter::repeat_n('0', padding));
                padded.push_str(digits);
                text = padded;
            } else {
                text = format!("{}{text}", " ".repeat(padding));
            }
        }
        out.push_str(&text);
    }
    Ok(out)
}

// The specifier, precision, flags and argument index are the call site's state; grouping
// them would only move the fields, as the same allowance in `rava-interp` records.
#[allow(clippy::too_many_arguments)]
fn format_conversion(
    shared: &Shared,
    thread: &ThreadState,
    conversion: char,
    argument: Option<Value>,
    precision: Option<usize>,
    _flags: FormatFlags,
    index: usize,
    method: &'static str,
) -> Result<String, VmError> {
    let value = argument.ok_or_else(|| {
        VmError::Native(format!(
            "{method}: format specifier {} needs argument {}",
            conversion, index
        ))
    })?;
    match conversion {
        's' | 'S' => {
            let text = argument_text(shared, thread, value, method)?;
            Ok(if conversion == 'S' {
                text.to_uppercase()
            } else {
                text
            })
        }
        'b' | 'B' => {
            let text = match value {
                Value::Ref(reference) if !reference.is_null() => {
                    let heap = shared.heap();
                    match heap.get(reference).map_err(heap_error)? {
                        crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Boolean(
                            value,
                        )) => value.to_string(),
                        _ => "true".to_owned(),
                    }
                }
                _ => "false".to_owned(),
            };
            Ok(if conversion == 'B' {
                text.to_uppercase()
            } else {
                text
            })
        }
        'c' | 'C' => {
            let code = integral_value(shared, value, method)?;
            let character = u32::try_from(code)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| {
                    VmError::Native(format!(
                        "{method}: %c argument {code} is not a Unicode code point"
                    ))
                })?;
            let text = character.to_string();
            Ok(if conversion == 'C' {
                text.to_uppercase()
            } else {
                text
            })
        }
        'd' => Ok(integral_value(shared, value, method)?.to_string()),
        'o' => Ok(format!("{:o}", integral_value(shared, value, method)?)),
        'x' => Ok(format!("{:x}", integral_value(shared, value, method)?)),
        'X' => Ok(format!("{:X}", integral_value(shared, value, method)?)),
        'f' => {
            let number = floating_value(shared, value, method)?;
            Ok(match precision {
                Some(precision) => format!("{number:.precision$}"),
                None => format!("{number:.6}"),
            })
        }
        'e' | 'E' => {
            let number = floating_value(shared, value, method)?;
            let precision = precision.unwrap_or(6);
            let text = format!("{number:.precision$e}");
            Ok(if conversion == 'E' {
                text.to_uppercase()
            } else {
                text
            })
        }
        'h' | 'H' => {
            let text = match value {
                Value::Ref(reference) if !reference.is_null() => {
                    format!("{:x}", reference.identity_hash() as u32)
                }
                _ => "null".to_owned(),
            };
            Ok(if conversion == 'H' {
                text.to_uppercase()
            } else {
                text
            })
        }
        other => Err(VmError::Native(format!(
            "{method}: conversion %{other} is not implemented"
        ))),
    }
}

/// An integer argument for `%d`/`%o`/`%x`/`%c`.
fn integral_value(shared: &Shared, value: Value, method: &'static str) -> Result<i64, VmError> {
    let Value::Ref(reference) = value else {
        return Err(VmError::Native(format!(
            "{method}: expected a boxed integral argument"
        )));
    };
    let heap = shared.heap();
    let text = match heap.get(reference).map_err(heap_error)? {
        crate::heap::HeapObject::Boxed(boxed) => match boxed {
            crate::heap::BoxedPrimitive::Byte(value) => *value as i64,
            crate::heap::BoxedPrimitive::Short(value) => *value as i64,
            crate::heap::BoxedPrimitive::Integer(value) => *value as i64,
            crate::heap::BoxedPrimitive::Long(value) => *value,
            _ => {
                return Err(VmError::Native(format!(
                    "{method}: expected a boxed integral argument"
                )));
            }
        },
        _ => {
            return Err(VmError::Native(format!(
                "{method}: expected a boxed integral argument"
            )));
        }
    };
    Ok(text)
}

/// A floating-point argument for `%f`/`%e`.
fn floating_value(shared: &Shared, value: Value, method: &'static str) -> Result<f64, VmError> {
    let Value::Ref(reference) = value else {
        return Err(VmError::Native(format!(
            "{method}: expected a boxed floating-point argument"
        )));
    };
    let heap = shared.heap();
    match heap.get(reference).map_err(heap_error)? {
        crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Float(value)) => {
            Ok(f64::from(*value))
        }
        crate::heap::HeapObject::Boxed(crate::heap::BoxedPrimitive::Double(value)) => Ok(*value),
        _ => Err(VmError::Native(format!(
            "{method}: expected a boxed floating-point argument"
        ))),
    }
}

/// Append text to a `StringBuffer` and return the receiver, as the JDK does.
fn append_buffer(shared: &Shared, receiver: ObjRef, text: &str) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "StringBuffer.append called with a null receiver".to_owned(),
        ));
    }
    shared
        .heap()
        .string_buffer_mut(receiver)
        .map_err(heap_error)?
        .push_str(text);
    Ok(Some(Value::Ref(receiver)))
}

fn string_buffer_append_char(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Int(value)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.append expects a char".to_owned(),
        ));
    };
    let character = char::from_u32(value as u32).unwrap_or('?');
    append_buffer(shared, receiver, &character.to_string())
}

fn string_buffer_append_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Int(value)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.append expects an int".to_owned(),
        ));
    };
    append_buffer(shared, receiver, &value.to_string())
}

fn string_buffer_append_long(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match args.first().copied() {
        Some(Value::Long(value)) => value,
        Some(Value::Int(value)) => i64::from(value),
        _ => {
            return Err(VmError::Native(
                "StringBuffer.append expects a long".to_owned(),
            ));
        }
    };
    append_buffer(shared, receiver, &value.to_string())
}

fn string_buffer_append_float(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Float(value)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.append expects a float".to_owned(),
        ));
    };
    append_buffer(shared, receiver, &crate::string::float_text(value))
}

fn string_buffer_append_double(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Double(value)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.append expects a double".to_owned(),
        ));
    };
    append_buffer(shared, receiver, &crate::string::double_text(value))
}

fn string_buffer_append_boolean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Int(value)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.append expects a boolean".to_owned(),
        ));
    };
    append_buffer(shared, receiver, if value != 0 { "true" } else { "false" })
}

fn string_buffer_append_object(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.append expects a reference".to_owned(),
        ));
    };
    let text = object_text(shared, thread, reference, "StringBuffer.append")?;
    append_buffer(shared, receiver, &text)
}

/// `StringBuffer.setLength(int)`: truncate, or pad with NUL characters.
fn string_buffer_set_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Int(length)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.setLength expects an int".to_owned(),
        ));
    };
    let length = usize::try_from(length)
        .map_err(|_| VmError::Native("StringBuffer.setLength: negative length".to_owned()))?;
    let mut heap = shared.heap();
    let text = heap.string_buffer_mut(receiver).map_err(heap_error)?;
    let current = utf16_units(text).len();
    if length < current {
        let units: Vec<u16> = utf16_units(text).into_iter().take(length).collect();
        *text = crate::string::string_from_utf16(&units, "StringBuffer.setLength")?;
    } else if length > current {
        let padding = "\u{0}".repeat(length - current);
        text.push_str(&padding);
    }
    Ok(None)
}

/// `StringBuffer.charAt(int)`: one UTF-16 code unit.
fn string_buffer_char_at(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Int(index)) = args.first().copied() else {
        return Err(VmError::Native(
            "StringBuffer.charAt expects an int".to_owned(),
        ));
    };
    let text = buffer_text(shared, receiver)?;
    let units = utf16_units(&text);
    let unit = usize::try_from(index)
        .ok()
        .and_then(|index| units.get(index))
        .copied()
        .ok_or_else(|| {
            VmError::Native(format!("StringBuffer.charAt: index {index} out of range"))
        })?;
    Ok(Some(Value::Int(i32::from(unit))))
}
