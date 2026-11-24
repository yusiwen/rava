//! One entry point that installs the runtime's native class library.
//!
//! The natives live with the feature that owns them (`object`, `string`, `collections`,
//! `maps`, `boxed`, `annotation`, `reflection`, `instance`). A runner calls
//! [`register_all`] instead of repeating that list, so a new native is installed once
//! here and every caller gets it.
//!
//! `Method.invoke` and `Constructor.newInstance` are deliberately absent: they re-enter
//! the interpreter, so they live in `rava-interp::reflect_invoke`, and a runner that
//! needs them registers that module too. String concatenation is not a registry entry
//! either; the interpreter dispatches an `invokedynamic` call site to
//! [`crate::string_concat`] directly.
//!
//! The minimal standard streams are installed here as well: the C1 stubs carry no
//! initializer `Code`, so `System.out`/`System.err` must be written into the statics table
//! before Java code reads them. Both streams write to [`Shared::stdout`]; this slice has
//! one captured output stream and no separate error stream.

use crate::VmError;
use crate::heap::{HeapObject, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;
use crate::{
    annotation, arrays, atomic, boxed, collections, exceptions, format, generic, instance, jul,
    locks, maps, math, object, references, reflection, regex, string, system, thread_natives,
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
    maps::register_natives(shared);
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
    register_print_stream(shared);
    install_system_streams(shared)?;
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
    shared.write_stdout(format!("{text}\n").as_bytes());
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
    shared.write_stdout(b"\n");
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
    shared.write_stdout(text.as_bytes());
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
