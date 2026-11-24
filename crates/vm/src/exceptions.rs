//! `java.lang.Throwable` and its common subclasses (M2.23).
//!
//! The C1 stubs carry no constructor `Code` and their `detailMessage`/`cause` fields are
//! private, so a throwable's message lives in [`crate::Shared`]'s side table when one of
//! these constructors runs. The object itself is an ordinary instance, so `athrow`,
//! handler matching and `getClass()` behave exactly as for a compiled throwable.
//!
//! Only construction, `getMessage`, `getLocalizedMessage`, `getCause`, `initCause`,
//! `toString` and a no-op `printStackTrace` are modelled. Stack traces, suppressed
//! exceptions and `fillInStackTrace` are M5/M6 work; `getCause` returns null unless
//! `initCause` set it, which is the JDK's behavior for a constructor without a cause.

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

pub fn register_natives(shared: &Shared) {
    for &class in THROWABLE_CLASSES {
        for descriptor in [
            "()V",
            "(Ljava/lang/String;)V",
            "(Ljava/lang/String;Ljava/lang/Throwable;)V",
            "(Ljava/lang/Throwable;)V",
        ] {
            shared.register_native(class, "<init>", descriptor, throwable_init);
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
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

fn throwable_init_cause(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
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
    let class = {
        let heap = shared.heap();
        heap.object_class(receiver).map_err(heap_error)?
    };
    let name = shared
        .entry(class)
        .map(|entry| entry.name.replace('/', "."))
        .unwrap_or_else(|| "java.lang.Throwable".to_owned());
    let message = shared.throwable_message(receiver).unwrap_or_default();
    shared.write_stderr(format!("{name}: {message}\n").as_bytes());
    Ok(None)
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
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}
