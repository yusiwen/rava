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

/// Register the minimal `java.io.PrintStream` output surface.
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
