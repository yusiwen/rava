//! `java.lang.StackWalker` (M4.5): a walk over an empty stack.
//!
//! `SpringApplication`'s constructor calls `StackWalker.getInstance(Option).walk(...)` to
//! deduce the main application class from the frame whose method is `main`. The
//! interpreter keeps no Java-visible frame metadata (a [`crate::Frame`] records the class
//! and bytecode, not the method name a `StackFrame` would report), so this native cannot
//! answer real caller frames yet. It calls the supplied `Function` with an empty
//! `java.util.stream.Stream<StackWalker$StackFrame>` instead.
//!
//! That is exactly the JDK result for a main class that cannot be deduced: Spring's
//! `findMainClass` returns `Optional.empty()` and `deduceMainApplicationClass` yields
//! `null`. Spring Boot tolerates a null main application class — it only shortens the
//! startup log line and answers `getMainApplicationClass` — so startup proceeds. A real
//! stack walk (and `StackWalker$StackFrame`) is a later slice; the empty stream is loud
//! in code review because this file is the only implementation.
//!
//! The native lives in `rava-interp` because `walk` must call the Java `Function`, which
//! re-enters the interpreter.

use rava_vm::{HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, lambda};

/// The runtime `java.lang.StackWalker` class.
const STACK_WALKER_CLASS: &str = "java/lang/StackWalker";

/// The runtime `java.util.stream.Stream` class.
const STREAM_CLASS: &str = "java/util/stream/Stream";

/// Registers the `StackWalker` natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        STACK_WALKER_CLASS,
        "getInstance",
        "()Ljava/lang/StackWalker;",
        stack_walker_get_instance,
    );
    shared.register_native(
        STACK_WALKER_CLASS,
        "getInstance",
        "(Ljava/lang/StackWalker$Option;)Ljava/lang/StackWalker;",
        stack_walker_get_instance,
    );
    shared.register_native(
        STACK_WALKER_CLASS,
        "walk",
        "(Ljava/util/function/Function;)Ljava/lang/Object;",
        stack_walker_walk,
    );
}

fn stack_walker_get_instance(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(STACK_WALKER_CLASS)?;
    Ok(Some(Value::Ref(shared.allocate_instance(class)?)))
}

/// `StackWalker.walk(Function)`: call the function with an empty stack stream.
fn stack_walker_walk(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let function = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "StackWalker.walk expects a non-null Function".to_owned(),
            ));
        }
    };
    let lambda = match shared.heap().get(function) {
        Ok(HeapObject::Lambda(lambda)) => lambda.clone(),
        Ok(_) => {
            return Err(VmError::Native(
                "StackWalker.walk: the Function is not a lambda object".to_owned(),
            ));
        }
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    // The stream type is loaded for its identity even though the heap shape carries no
    // class of its own; the empty pipeline is the whole result.
    shared.load_library_class(STREAM_CLASS)?;
    let stream = shared.heap().allocate_stream(Vec::new(), false, None);
    match lambda::invoke_sam(shared, thread, &lambda, vec![Value::Ref(stream)]) {
        Ok(value) => Ok(value),
        Err(InterpError::Thrown(throwable)) => Err(VmError::Thrown(throwable)),
        Err(error) => Err(VmError::Native(format!(
            "StackWalker.walk: Function.apply failed: {error}"
        ))),
    }
}
