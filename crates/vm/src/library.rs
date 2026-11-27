//! One entry point that installs the runtime's native class library.
//!
//! The natives live with the feature that owns them (`object`, `string`, `collections`,
//! `maps`, `boxed`, `annotation`, `reflection`, `instance`, `method_handles`). A runner
//! calls [`register_all`] instead of repeating that list, so a new native is installed
//! once here and every caller gets it.
//!
//! `Method.invoke` and `Constructor.newInstance` are deliberately absent: they re-enter
//! the interpreter, so they live in `rava-interp::reflect_invoke`, and a runner that
//! needs them registers that module too. String concatenation is not a registry entry
//! either; the interpreter dispatches an `invokedynamic` call site to
//! [`crate::string_concat`] directly.
//!
//! The minimal standard streams are installed here as well: the C1 stubs carry no
//! initializer `Code`, so `System.out`/`System.err` must be written into the statics table
//! before Java code reads them. The error stream is registered with
//! [`Shared::register_error_stream`], so `System.err` writes reach the error capture and
//! never the program's stdout. A runner can also enable [`Shared::set_output_streaming`],
//! which writes both streams through to the host as bytes are produced.

use crate::VmError;
use crate::heap::{HeapObject, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;
use crate::{
    annotation, arrays, atomic, beans, big_numbers, bitset, boxed, charset, class_loader,
    collections, direct_buffer, exceptions, format, generic, instance, io_file, java_time, jul,
    locks, manifest, maps, math, method_handles, modules, object, proxy, random, references,
    reflection, regex, resources, string, system, thread_natives, uri,
};

/// `java.lang.System`'s binary-stable internal name.
const SYSTEM_CLASS: &str = "java/lang/System";

/// `java.io.PrintStream`'s internal name.
const PRINT_STREAM_CLASS: &str = "java/io/PrintStream";

/// The descriptor of the `System.out`/`System.err` fields.
const PRINT_STREAM_DESCRIPTOR: &str = "Ljava/io/PrintStream;";

/// Install every class-library native and the standard output streams.
///
/// The registrations are idempotent (the registry is keyed by owner, name and
/// descriptor), but calling this twice installs two fresh `PrintStream` objects over the
/// old ones. It must run after the bootstrap class source is available and before any
/// application code executes.
pub fn register_all(shared: &Shared) -> Result<(), VmError> {
    object::register_natives(shared);
    instance::register_natives(shared);
    string::register_natives(shared);
    collections::register_natives(shared);
    collections::install_empty_constants(shared)?;
    maps::register_natives(shared);
    manifest::register_natives(shared);
    boxed::register_natives(shared);
    reflection::register_natives(shared);
    generic::register_natives(shared);
    annotation::register_natives(shared);
    jul::register_natives(shared);
    system::register_natives(shared);
    atomic::register_natives(shared);
    math::register_natives(shared);
    arrays::register_natives(shared);
    locks::register_natives(shared);
    references::register_natives(shared);
    exceptions::register_natives(shared);
    regex::register_natives(shared);
    format::register_natives(shared);
    thread_natives::register_natives(shared);
    // `java.time`/`Locale` are M4.5: the logging stack's date formatter needs an instant,
    // a zone and a pattern, and the state lives in the VM side table.
    java_time::register_natives(shared);
    big_numbers::register_natives(shared);
    big_numbers::install_constants(shared)?;
    random::register_natives(shared);
    uri::register_natives(shared);
    io_file::register_natives(shared);
    beans::register_natives(shared);
    charset::register_natives(shared);
    charset::install_constants(shared)?;
    java_time::install_constants(shared)?;
    // `java.lang.ClassLoader` owns the loader hierarchy and `defineClass` (M3.3):
    // `getSystemClassLoader`, `loadClass` and the three `defineClass` overloads are
    // installed here. Its resource methods call the M4.1 lookup in `Shared`.
    class_loader::register_natives(shared);
    // `java.net.URL`, `java.net.URLConnection` and `java.io.InputStream` are M4.1. The
    // URL's text and a stream's bytes live in the VM side table, not in heap objects.
    resources::register_natives(shared);
    // `Proxy`/`InvocationHandler` are M3.2: the `Proxy` natives plus the runtime-private
    // `rava/proxy/ProxySupport` helper class the generated proxy classes call.
    proxy::register_natives(shared);
    // `ByteBuffer.allocateDirect` and `DirectByteBuffer.isDirect` are M3.5. The install
    // marks the `ByteBuffer` stub initialized because its C1 `<clinit>` reads U1 members.
    direct_buffer::install(shared)?;
    // `java.lang.invoke` is M3.4: `MethodHandles`/`Lookup`, `MethodType` and the
    // non-re-entrant `MethodHandle` queries. `MethodHandle.invoke*` lives in the
    // interpreter, like `Method.invoke`, because it re-enters the frame machinery.
    method_handles::register_natives(shared);
    // `java.lang.module.ModuleFinder` reports this VM's empty system module set (M3.6).
    modules::register_natives(shared)?;
    // `java.util.BitSet` for CGLIB's `equals` helper and Spring's `MimeType` (M3.6).
    bitset::register_natives(shared);
    register_print_stream(shared);
    install_system_streams(shared)?;
    proxy::install_support(shared)?;
    jul::install_levels(shared)?;
    boxed::install_booleans(shared)?;
    thread_natives::install_thread_states(shared)
}

/// Register the `java.io.PrintStream` `print`/`println`/`write` surface.
fn register_print_stream(shared: &Shared) {
    shared.register_native(
        PRINT_STREAM_CLASS,
        "println",
        "(Ljava/lang/String;)V",
        print_stream_println_string,
    );
    shared.register_native(PRINT_STREAM_CLASS, "println", "()V", print_stream_println);
    shared.register_native(
        PRINT_STREAM_CLASS,
        "print",
        "(Ljava/lang/String;)V",
        print_stream_print_string,
    );
    // `PrintStream`'s primitive, `char[]` and `Object` overloads; the JDK's text is the
    // `String.valueOf` text, and the `Object` form goes through the interpreter's
    // `toString` resolver (installed by the runner, #73).
    for (name, descriptor, function) in [
        (
            "print",
            "(Z)V",
            print_stream_print_boolean as crate::NativeFn,
        ),
        ("print", "(C)V", print_stream_print_char),
        ("print", "(I)V", print_stream_print_int),
        ("print", "(J)V", print_stream_print_long),
        ("print", "(F)V", print_stream_print_float),
        ("print", "(D)V", print_stream_print_double),
        ("print", "([C)V", print_stream_print_chars),
        ("print", "(Ljava/lang/Object;)V", print_stream_print_object),
        ("println", "(Z)V", print_stream_println_boolean),
        ("println", "(C)V", print_stream_println_char),
        ("println", "(I)V", print_stream_println_int),
        ("println", "(J)V", print_stream_println_long),
        ("println", "(F)V", print_stream_println_float),
        ("println", "(D)V", print_stream_println_double),
        ("println", "([C)V", print_stream_println_chars),
        (
            "println",
            "(Ljava/lang/Object;)V",
            print_stream_println_object,
        ),
    ] {
        shared.register_native(PRINT_STREAM_CLASS, name, descriptor, function);
    }
    // `printf`/`format` reuse `String.format`'s conversion subset; `append` writes text and
    // returns the receiver. The `Appendable` descriptors are the bridge methods javac emits
    // for an interface call site.
    for (name, descriptor, function) in [
        (
            "printf",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
            print_stream_printf as crate::NativeFn,
        ),
        (
            "printf",
            "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
            print_stream_printf_locale,
        ),
        (
            "format",
            "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
            print_stream_format,
        ),
        (
            "format",
            "(Ljava/util/Locale;Ljava/lang/String;[Ljava/lang/Object;)Ljava/io/PrintStream;",
            print_stream_format_locale,
        ),
        (
            "append",
            "(C)Ljava/io/PrintStream;",
            print_stream_append_char,
        ),
        (
            "append",
            "(C)Ljava/lang/Appendable;",
            print_stream_append_char,
        ),
        (
            "append",
            "(Ljava/lang/CharSequence;)Ljava/io/PrintStream;",
            print_stream_append_chars,
        ),
        (
            "append",
            "(Ljava/lang/CharSequence;)Ljava/lang/Appendable;",
            print_stream_append_chars,
        ),
        (
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/io/PrintStream;",
            print_stream_append_range,
        ),
        (
            "append",
            "(Ljava/lang/CharSequence;II)Ljava/lang/Appendable;",
            print_stream_append_range,
        ),
    ] {
        shared.register_native(PRINT_STREAM_CLASS, name, descriptor, function);
    }
    // A `PrintStream` over the standard output is a byte sink for logback's console
    // target, which writes its encoded bytes through `write`.
    shared.register_native(PRINT_STREAM_CLASS, "write", "(I)V", print_stream_write);
    shared.register_native(
        PRINT_STREAM_CLASS,
        "write",
        "([B)V",
        print_stream_write_bytes,
    );
    shared.register_native(
        PRINT_STREAM_CLASS,
        "write",
        "([BII)V",
        print_stream_write_range,
    );
    shared.register_native(PRINT_STREAM_CLASS, "flush", "()V", print_stream_flush);
    shared.register_native(PRINT_STREAM_CLASS, "close", "()V", print_stream_flush);
}

/// `PrintStream.write(int)`: the low eight bits of the argument.
fn print_stream_write(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, "PrintStream.write")?;
    let byte = match args.first().copied() {
        Some(Value::Int(value)) => value as u8,
        Some(_) => {
            return Err(VmError::Native(
                "PrintStream.write expects an int argument".to_owned(),
            ));
        }
        None => {
            return Err(VmError::Native(
                "PrintStream.write is missing its argument".to_owned(),
            ));
        }
    };
    shared.write_output(receiver, &[byte]);
    Ok(None)
}

/// `PrintStream.write(byte[])`: the whole array.
fn print_stream_write_bytes(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_write_range(shared, _thread, _loader, receiver, args)
}

/// `PrintStream.write(byte[], int, int)`: `len` bytes from `offset`.
///
/// The `byte[]` element values are `Int`s in `-128..=127`; the low eight bits are the
/// output byte.
fn print_stream_write_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, "PrintStream.write")?;
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "PrintStream.write expects a non-null byte[] argument".to_owned(),
            ));
        }
    };
    let (offset, length) = if args.len() >= 3 {
        let int = |position: usize, what: &str| match args.get(position).copied() {
            Some(Value::Int(value)) => Ok(value),
            _ => Err(VmError::Native(format!(
                "PrintStream.write expects an int {what}"
            ))),
        };
        (int(1, "offset")?, int(2, "length")?)
    } else {
        (0, i32::MAX)
    };
    if offset < 0 || length < 0 {
        return Err(VmError::Native(
            "PrintStream.write: negative offset or length".to_owned(),
        ));
    }
    let bytes = {
        let heap = shared.heap();
        let array = heap.array(reference).map_err(|error| {
            VmError::Native(format!("PrintStream.write expects a byte[]: {error}"))
        })?;
        array.elements.clone()
    };
    let start = offset as usize;
    let end = start.saturating_add(length as usize).min(bytes.len());
    let start = start.min(bytes.len());
    let bytes: Vec<u8> = bytes[start..end]
        .iter()
        .map(|value| match value {
            Value::Int(byte) => *byte as u8,
            _ => 0,
        })
        .collect();
    shared.write_output(receiver, &bytes);
    Ok(None)
}

/// One `PrintStream` scalar conversion. The bytecode's descriptor fixes the kind, so the
/// helper does not have to guess from the value.
#[derive(Clone, Copy)]
enum PrintScalar {
    Boolean,
    Char,
    Int,
    Long,
    Float,
    Double,
    Chars,
    Object,
}

/// Write one `print`/`println` value through [`Shared::write_output`], so `System.err`
/// keeps its routing and the runner's streaming mode is unchanged.
fn print_stream_scalar(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    args: &[Value],
    newline: bool,
    scalar: PrintScalar,
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, method)?;
    let text = match scalar {
        PrintScalar::Boolean => match args.first().copied() {
            Some(Value::Int(value)) => (if value != 0 { "true" } else { "false" }).to_owned(),
            _ => return Err(VmError::Native(format!("{method} expects a boolean"))),
        },
        PrintScalar::Char => match args.first().copied() {
            Some(Value::Int(value)) => char::from_u32(u32::from(value as u16))
                .unwrap_or('\u{FFFD}')
                .to_string(),
            _ => return Err(VmError::Native(format!("{method} expects a char"))),
        },
        PrintScalar::Int => match args.first().copied() {
            Some(Value::Int(value)) => value.to_string(),
            _ => return Err(VmError::Native(format!("{method} expects an int"))),
        },
        PrintScalar::Long => match args.first().copied() {
            Some(Value::Long(value)) => value.to_string(),
            _ => return Err(VmError::Native(format!("{method} expects a long"))),
        },
        PrintScalar::Float => match args.first().copied() {
            Some(Value::Float(value)) => crate::heap::BoxedPrimitive::Float(value).text(),
            _ => return Err(VmError::Native(format!("{method} expects a float"))),
        },
        PrintScalar::Double => match args.first().copied() {
            Some(Value::Double(value)) => crate::heap::BoxedPrimitive::Double(value).text(),
            _ => return Err(VmError::Native(format!("{method} expects a double"))),
        },
        PrintScalar::Chars => char_array_text(shared, args, method)?,
        PrintScalar::Object => match args.first().copied() {
            Some(Value::Ref(reference)) => string::object_text(shared, thread, reference, method)?,
            _ => {
                return Err(VmError::Native(format!(
                    "{method} expects an Object argument"
                )));
            }
        },
    };
    let mut text = text;
    if newline {
        text.push('\n');
    }
    shared.write_output(receiver, text.as_bytes());
    Ok(None)
}

/// A `char[]` argument as text. A null array is the JDK's `NullPointerException`.
fn char_array_text(
    shared: &Shared,
    args: &[Value],
    method: &'static str,
) -> Result<String, VmError> {
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        Some(Value::Ref(_)) => {
            let throwable = crate::exceptions::new_throwable(
                shared,
                "java/lang/NullPointerException",
                format!("{method}: null char[]"),
            )?;
            return Err(VmError::Thrown(throwable));
        }
        _ => {
            return Err(VmError::Native(format!(
                "{method} expects a char[] argument"
            )));
        }
    };
    let elements = {
        let heap = shared.heap();
        heap.array(reference)
            .map_err(|error| VmError::Native(format!("{method}: {error}")))?
            .elements
            .clone()
    };
    Ok(elements
        .iter()
        .filter_map(|element| match element {
            Value::Int(code) => char::from_u32(u32::from(*code as u16)),
            _ => None,
        })
        .collect())
}

fn print_stream_print_boolean(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Boolean,
        "PrintStream.print",
    )
}
fn print_stream_print_char(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Char,
        "PrintStream.print",
    )
}
fn print_stream_print_int(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Int,
        "PrintStream.print",
    )
}
fn print_stream_print_long(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Long,
        "PrintStream.print",
    )
}
fn print_stream_print_float(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Float,
        "PrintStream.print",
    )
}
fn print_stream_print_double(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Double,
        "PrintStream.print",
    )
}
fn print_stream_print_chars(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Chars,
        "PrintStream.print",
    )
}
fn print_stream_print_object(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        false,
        PrintScalar::Object,
        "PrintStream.print",
    )
}
fn print_stream_println_boolean(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Boolean,
        "PrintStream.println",
    )
}
fn print_stream_println_char(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Char,
        "PrintStream.println",
    )
}
fn print_stream_println_int(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Int,
        "PrintStream.println",
    )
}
fn print_stream_println_long(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Long,
        "PrintStream.println",
    )
}
fn print_stream_println_float(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Float,
        "PrintStream.println",
    )
}
fn print_stream_println_double(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Double,
        "PrintStream.println",
    )
}
fn print_stream_println_chars(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Chars,
        "PrintStream.println",
    )
}
fn print_stream_println_object(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_scalar(
        shared,
        thread,
        receiver,
        args,
        true,
        PrintScalar::Object,
        "PrintStream.println",
    )
}

/// `PrintStream.printf`/`format` without a locale.
fn print_stream_printf(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_format_call(
        shared,
        thread,
        receiver,
        args,
        0,
        1,
        None,
        "PrintStream.printf",
    )
}

/// `PrintStream.printf(Locale, String, Object...)`.
fn print_stream_printf_locale(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_format_call(
        shared,
        thread,
        receiver,
        args,
        1,
        2,
        Some(0),
        "PrintStream.printf",
    )
}

/// `PrintStream.format(String, Object...)`.
fn print_stream_format(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_format_call(
        shared,
        thread,
        receiver,
        args,
        0,
        1,
        None,
        "PrintStream.format",
    )
}

/// `PrintStream.format(Locale, String, Object...)`.
fn print_stream_format_locale(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    print_stream_format_call(
        shared,
        thread,
        receiver,
        args,
        1,
        2,
        Some(0),
        "PrintStream.format",
    )
}

/// Format one `PrintStream.printf`/`format` call and write the text.
// The argument positions and the method name are the two call-site shapes; grouping them
// would only move the fields, as the same allowance in `rava-interp` records.
#[allow(clippy::too_many_arguments)]
fn print_stream_format_call(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    args: &[Value],
    pattern_index: usize,
    arguments_index: usize,
    locale_index: Option<usize>,
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, method)?;
    let pattern = match args.get(pattern_index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            let heap = shared.heap();
            heap.string(reference)
                .map_err(|error| VmError::Native(format!("{method}: {error}")))?
                .to_owned()
        }
        _ => {
            return Err(VmError::Native(format!(
                "{method} expects a non-null format string"
            )));
        }
    };
    let arguments = format::argument_array(shared, args.get(arguments_index).copied(), method)?;
    let locale = locale_index.and_then(|index| match args.get(index).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => Some(reference),
        _ => None,
    });
    let text = format::format_stream(shared, thread, &pattern, &arguments, locale, method)?;
    shared.write_output(receiver, text.as_bytes());
    Ok(Some(Value::Ref(receiver)))
}

/// `PrintStream.append(char)` and its `Appendable` bridge.
fn print_stream_append_char(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, "PrintStream.append")?;
    let character = match args.first().copied() {
        Some(Value::Int(code)) => char::from_u32(u32::from(code as u16)).unwrap_or('\u{FFFD}'),
        _ => {
            return Err(VmError::Native(
                "PrintStream.append expects a char".to_owned(),
            ));
        }
    };
    shared.write_output(receiver, character.to_string().as_bytes());
    Ok(Some(Value::Ref(receiver)))
}

/// `PrintStream.append(CharSequence)` and its `Appendable` bridge.
fn print_stream_append_chars(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, "PrintStream.append")?;
    let text = char_sequence_text(shared, thread, args.first().copied(), "PrintStream.append")?;
    shared.write_output(receiver, text.as_bytes());
    Ok(Some(Value::Ref(receiver)))
}

/// `PrintStream.append(CharSequence, int, int)` and its `Appendable` bridge.
fn print_stream_append_range(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, "PrintStream.append")?;
    let start = match args.get(1).copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "PrintStream.append expects an int start".to_owned(),
            ));
        }
    };
    let end = match args.get(2).copied() {
        Some(Value::Int(value)) => value,
        _ => {
            return Err(VmError::Native(
                "PrintStream.append expects an int end".to_owned(),
            ));
        }
    };
    let text = char_sequence_text(shared, thread, args.first().copied(), "PrintStream.append")?;
    let units: Vec<u16> = text.encode_utf16().collect();
    if start < 0 || end < 0 || start > end || end as usize > units.len() {
        return Err(VmError::Native(format!(
            "PrintStream.append: range {start}..{end} is outside a {}-unit CharSequence",
            units.len()
        )));
    }
    let text = String::from_utf16_lossy(&units[start as usize..end as usize]);
    shared.write_output(receiver, text.as_bytes());
    Ok(Some(Value::Ref(receiver)))
}

/// The text of a `CharSequence` argument; null is the JDK's `"null"`.
fn char_sequence_text(
    shared: &Shared,
    thread: &ThreadState,
    value: Option<Value>,
    method: &'static str,
) -> Result<String, VmError> {
    match value {
        Some(Value::Ref(reference)) => string::object_text(shared, thread, reference, method),
        _ => Err(VmError::Native(format!(
            "{method} expects a CharSequence argument"
        ))),
    }
}

fn print_stream_flush(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

/// Write the standard streams into `java.lang.System`'s statics.
///
/// The class library has no `System.<clinit>`, so the normal initializer path would leave
/// both fields absent and the first `System.out` read would fail. One `PrintStream`
/// instance is allocated for each field through the ordinary instance allocator, which
/// also builds the `FilterOutputStream` field layout.
fn install_system_streams(shared: &Shared) -> Result<(), VmError> {
    let system = shared.load_library_class(SYSTEM_CLASS)?;
    let print_stream = shared.load_library_class(PRINT_STREAM_CLASS)?;
    let out = shared.allocate_instance(print_stream)?;
    let err = shared.allocate_instance(print_stream)?;
    let entry = shared
        .entry(system)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", system.0)))?;
    let mut statics = entry.statics.lock().expect("static field lock poisoned");
    statics.insert(
        ("out".to_owned(), PRINT_STREAM_DESCRIPTOR.to_owned()),
        Value::Ref(out),
    );
    statics.insert(
        ("err".to_owned(), PRINT_STREAM_DESCRIPTOR.to_owned()),
        Value::Ref(err),
    );
    drop(statics);
    // `System.err` writes reach the error capture, not `System.out`.
    shared.register_error_stream(err);
    Ok(())
}

/// `PrintStream.println(String)`: the text followed by a newline.
fn print_stream_println_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, receiver, args, "println")?;
    shared.write_output(receiver, format!("{text}\n").as_bytes());
    Ok(None)
}

/// `PrintStream.println()`: one newline.
fn print_stream_println(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    require_receiver(receiver, "PrintStream.println")?;
    shared.write_output(receiver, b"\n");
    Ok(None)
}

/// `PrintStream.print(String)`: the text without a newline.
fn print_stream_print_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, receiver, args, "print")?;
    shared.write_output(receiver, text.as_bytes());
    Ok(None)
}

fn require_receiver(receiver: ObjRef, method: &str) -> Result<(), VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(format!(
            "{method} called with a null receiver"
        )));
    }
    Ok(())
}

/// The `String` argument of `print`/`println`; a null reference prints `"null"`.
fn string_argument(
    shared: &Shared,
    receiver: ObjRef,
    args: &[Value],
    method: &str,
) -> Result<String, VmError> {
    require_receiver(receiver, &format!("PrintStream.{method}"))?;
    match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            let heap = shared.heap();
            match heap
                .get(reference)
                .map_err(|error| VmError::Native(error.to_string()))?
            {
                HeapObject::Str(text) => Ok(text.clone()),
                _ => Err(VmError::Native(format!(
                    "PrintStream.{method}(String) expects a String"
                ))),
            }
        }
        Some(Value::Ref(_)) => Ok("null".to_owned()),
        _ => Err(VmError::Native(format!(
            "PrintStream.{method}(String) expects a String"
        ))),
    }
}
