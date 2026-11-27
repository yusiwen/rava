//! `java.lang.Throwable` and its common subclasses (M2.23).
//!
//! The C1 stubs carry no constructor `Code` and their `detailMessage`/`cause` fields are
//! private, so a throwable's message lives in [`crate::Shared`]'s side table when one of
//! these constructors runs. The object itself is an ordinary instance, so `athrow`,
//! handler matching and `getClass()` behave exactly as for a compiled throwable.
//!
//! Construction, `getMessage`, `getLocalizedMessage`, `getCause`, `initCause`, `toString`,
//! `printStackTrace`, `addSuppressed`/`getSuppressed` and an empty `getStackTrace` are
//! modelled. Stack frames and `fillInStackTrace` are M5/M6 work, so the trace has no
//! `at …` lines; it does print the class/header, the `Caused by:` chain and the
//! `Suppressed:` entries. `getCause` returns null unless the constructor or `initCause`
//! recorded a cause, which is the JDK's behavior for a throwable without one.

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// The throwable classes whose constructors Spring's startup path invokes.
const THROWABLE_CLASSES: &[&str] = &[
    "java/lang/Throwable",
    "java/lang/Exception",
    "java/lang/RuntimeException",
    "java/lang/IllegalArgumentException",
    "java/lang/IllegalStateException",
    "java/lang/NullPointerException",
    "java/lang/ClassCastException",
    "java/lang/ArrayIndexOutOfBoundsException",
    "java/lang/IndexOutOfBoundsException",
    "java/lang/UnsupportedOperationException",
    "java/lang/ClassNotFoundException",
    "java/io/FileNotFoundException",
    "java/lang/NoSuchElementException",
    "java/lang/NoSuchMethodException",
    "java/lang/NoSuchFieldException",
    "java/lang/IllegalAccessException",
    "java/lang/InvocationTargetException",
    "java/lang/SecurityException",
    "java/lang/Error",
    "java/lang/AssertionError",
    "java/lang/LinkageError",
    "java/lang/ExceptionInInitializerError",
    "java/lang/OutOfMemoryError",
    "java/lang/StackOverflowError",
    "java/lang/reflect/InvocationTargetException",
    "java/lang/reflect/UndeclaredThrowableException",
    "java/util/NoSuchElementException",
    "java/util/ConcurrentModificationException",
];

/// Allocate a throwable of `class_name` and record `message` as its detail message.
///
/// This is how a native raises a Java exception it has to synthesize itself: the class is
/// loaded from the class library (so it has the same `ClassId` a compiled `catch` resolves
/// to), an ordinary instance is allocated, and the message goes into the same side table
/// [`throwable_init`] writes. No Java constructor runs — the C1 stubs have
/// no `Code` — but the object then answers `getMessage`/`toString` and `athrow` matches it
/// exactly like a constructed throwable.
pub fn new_throwable(
    shared: &Shared,
    class_name: &str,
    message: String,
) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(class_name)?;
    let object = shared.allocate_instance(class)?;
    shared.set_throwable_message(object, message);
    Ok(object)
}

pub fn register_natives(shared: &Shared) {
    for &class in THROWABLE_CLASSES {
        for descriptor in [
            "()V",
            "(Ljava/lang/String;)V",
            "(Ljava/lang/String;Ljava/lang/Throwable;)V",
            "(Ljava/lang/Throwable;)V",
        ] {
            shared.register_native_if_declared(class, "<init>", descriptor, throwable_init);
        }
    }
    for (name, descriptor, function) in [
        (
            "getMessage",
            "()Ljava/lang/String;",
            throwable_get_message as crate::NativeFn,
        ),
        (
            "getLocalizedMessage",
            "()Ljava/lang/String;",
            throwable_get_message,
        ),
        ("getCause", "()Ljava/lang/Throwable;", throwable_get_cause),
        (
            "initCause",
            "(Ljava/lang/Throwable;)Ljava/lang/Throwable;",
            throwable_init_cause,
        ),
        ("toString", "()Ljava/lang/String;", throwable_to_string),
        ("printStackTrace", "()V", throwable_print_stack_trace),
        (
            "fillInStackTrace",
            "()Ljava/lang/Throwable;",
            throwable_fill_in_stack_trace,
        ),
        (
            "addSuppressed",
            "(Ljava/lang/Throwable;)V",
            throwable_add_suppressed,
        ),
        (
            "getSuppressed",
            "()[Ljava/lang/Throwable;",
            throwable_get_suppressed,
        ),
        (
            "getStackTrace",
            "()[Ljava/lang/StackTraceElement;",
            throwable_get_stack_trace,
        ),
    ] {
        shared.register_native("java/lang/Throwable", name, descriptor, function);
    }
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// The detail message argument of a throwable constructor, if present.
fn message_argument(shared: &Shared, args: &[Value]) -> Option<String> {
    let Some(Value::Ref(reference)) = args.first().copied() else {
        return None;
    };
    if reference.is_null() {
        return None;
    }
    shared.heap().string(reference).ok().map(str::to_owned)
}

fn throwable_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    if let Some(message) = message_argument(shared, args) {
        shared.set_throwable_message(receiver, message);
    }
    // `(Throwable)`, `(String, Throwable)` and `(String, Throwable, boolean, boolean)` all
    // carry a cause; it is the first reference argument after a possible message.
    for argument in args {
        if let Value::Ref(cause) = argument {
            if cause.is_null() || *cause == receiver {
                continue;
            }
            // A `String` argument is the detail message, not a cause.
            let is_string = matches!(shared.heap().get(*cause), Ok(crate::HeapObject::Str(_)));
            if is_string {
                continue;
            }
            shared.set_throwable_cause(receiver, *cause);
            break;
        }
    }
    Ok(None)
}

fn throwable_get_message(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match shared.throwable_message(receiver) {
        Some(message) => {
            let text = shared.intern_string(&message);
            Ok(Some(Value::Ref(text)))
        }
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

fn throwable_get_cause(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.throwable_cause(receiver).unwrap_or(ObjRef::NULL),
    )))
}

fn throwable_init_cause(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    if let Some(Value::Ref(cause)) = args.first().copied() {
        if !cause.is_null() {
            shared.set_throwable_cause(receiver, cause);
        }
    }
    Ok(Some(Value::Ref(receiver)))
}

fn throwable_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = {
        let heap = shared.heap();
        heap.object_class(receiver).map_err(heap_error)?
    };
    let name = shared
        .entry(class)
        .map(|entry| entry.name.replace('/', "."))
        .unwrap_or_else(|| "java.lang.Throwable".to_owned());
    let text = match shared.throwable_message(receiver) {
        Some(message) => format!("{name}: {message}"),
        None => name,
    };
    let text = shared.intern_string(&text);
    Ok(Some(Value::Ref(text)))
}

fn throwable_print_stack_trace(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // The frame lines are absent (no Java frame metadata), but the diagnostic value of a
    // trace is mostly the class/message and the cause chain, which this VM does have.
    let mut text = format!("{}\n", throwable_header(shared, receiver));
    let mut seen = vec![receiver];
    let mut cause = shared.throwable_cause(receiver);
    while let Some(current) = cause {
        if seen.contains(&current) {
            break;
        }
        seen.push(current);
        text.push_str(&format!(
            "Caused by: {}\n",
            throwable_header(shared, current)
        ));
        for suppressed in shared.throwable_suppressed(current) {
            text.push_str(&format!(
                "Suppressed: {}\n",
                throwable_header(shared, suppressed)
            ));
        }
        cause = shared.throwable_cause(current);
    }
    for suppressed in shared.throwable_suppressed(receiver) {
        text.push_str(&format!(
            "Suppressed: {}\n",
            throwable_header(shared, suppressed)
        ));
    }
    shared.write_stderr(text.as_bytes());
    Ok(None)
}

/// `Throwable.toString()`'s text: the class name, then the message when one was recorded.
fn throwable_header(shared: &Shared, reference: ObjRef) -> String {
    let class = {
        let heap = shared.heap();
        heap.object_class(reference).map_err(heap_error)
    };
    let name = class
        .ok()
        .and_then(|class| {
            shared
                .entry(class)
                .map(|entry| entry.name.replace('/', "."))
        })
        .unwrap_or_else(|| "java.lang.Throwable".to_owned());
    match shared.throwable_message(reference) {
        Some(message) if !message.is_empty() => format!("{name}: {message}"),
        _ => name,
    }
}

fn throwable_fill_in_stack_trace(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(receiver)))
}

fn throwable_add_suppressed(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(suppressed)) = args.first().copied() else {
        return Err(VmError::Native(
            "Throwable.addSuppressed expects a Throwable argument".to_owned(),
        ));
    };
    if suppressed.is_null() {
        return Err(VmError::Native(
            "Throwable.addSuppressed received null".to_owned(),
        ));
    }
    if suppressed == receiver {
        return Err(VmError::Native(
            "Throwable.addSuppressed: self-suppression is not permitted".to_owned(),
        ));
    }
    shared.add_throwable_suppressed(receiver, suppressed);
    Ok(None)
}

/// `Throwable.getSuppressed()`: the recorded suppressed exceptions, in declaration order.
fn throwable_get_suppressed(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let suppressed = shared.throwable_suppressed(receiver);
    let class = shared.load_library_class("java/lang/Throwable")?;
    let array = shared
        .heap()
        .allocate_array(crate::heap::ClassRef::Named(class), suppressed.len());
    {
        let mut heap = shared.heap();
        let slots = heap.array_mut(array).map_err(heap_error)?;
        for (slot, reference) in slots.elements.iter_mut().zip(suppressed) {
            *slot = Value::Ref(reference);
        }
    }
    Ok(Some(Value::Ref(array)))
}

/// `Throwable.getStackTrace()`: an empty array until the VM records Java frames.
///
/// The interpreter keeps no Java-visible frame metadata yet, so a stack trace cannot be
/// reported. An empty array is the honest value; there is no debug hook in the shipped
/// API.
fn throwable_get_stack_trace(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class("java/lang/StackTraceElement")?;
    let array = shared
        .heap()
        .allocate_array(crate::heap::ClassRef::Named(class), 0);
    Ok(Some(Value::Ref(array)))
}
