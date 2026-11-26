//! `java.lang.Thread` natives for the single-threaded VM (M2.23).
//!
//! M5 introduces real threads. Until then `currentThread` returns one cached `Thread`
//! object, `sleep`/`yield`/`join` return immediately, and `getState` reports `RUNNABLE`.
//! Spring's lifecycle code reads `Thread.currentThread()` and compares its state with
//! `Thread.State.WAITING`, so the six enum constants are installed into `Thread$State`'s
//! statics exactly as `java.util.logging.Level` is.
//!
//! `ClassLoader` natives live in [`crate::class_loader`] (M3.3); this module only answers
//! `Thread.getContextClassLoader`/`setContextClassLoader` through the application loader
//! object.

use crate::VmError;
use crate::heap::{EnumConstant, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const THREAD_CLASS: &str = "java/lang/Thread";
const THREAD_STATE_CLASS: &str = "java/lang/Thread$State";
const THREAD_LOCAL_CLASS: &str = "java/lang/ThreadLocal";
const INHERITABLE_THREAD_LOCAL_CLASS: &str = "java/lang/InheritableThreadLocal";

const THREAD_STATES: [&str; 6] = [
    "NEW",
    "RUNNABLE",
    "BLOCKED",
    "WAITING",
    "TIMED_WAITING",
    "TERMINATED",
];

pub fn register_natives(shared: &Shared) {
    // A `Thread` object exists before M5; its constructors only record nothing, and
    // `start` stays a no-op. Boot's logging stack allocates a thread for its background
    // initializer, which is the production path that must not fail.
    for descriptor in [
        "()V",
        "(Ljava/lang/Runnable;)V",
        "(Ljava/lang/Runnable;Ljava/lang/String;)V",
        "(Ljava/lang/String;)V",
        "(Ljava/lang/ThreadGroup;Ljava/lang/Runnable;)V",
    ] {
        shared.register_native(THREAD_CLASS, "<init>", descriptor, thread_noop);
    }
    shared.register_native(
        THREAD_CLASS,
        "getThreadGroup",
        "()Ljava/lang/ThreadGroup;",
        thread_get_group,
    );
    shared.register_native(
        "java/lang/ThreadGroup",
        "<init>",
        "(Ljava/lang/String;)V",
        thread_group_init,
    );
    shared.register_native(
        "java/lang/ThreadGroup",
        "getName",
        "()Ljava/lang/String;",
        thread_group_name,
    );
    shared.register_native(
        "java/lang/ThreadGroup",
        "toString",
        "()Ljava/lang/String;",
        thread_group_name,
    );
    shared.register_native(
        "java/lang/ThreadGroup",
        "getParent",
        "()Ljava/lang/ThreadGroup;",
        thread_group_parent,
    );
    shared.register_native(
        THREAD_CLASS,
        "currentThread",
        "()Ljava/lang/Thread;",
        thread_current,
    );
    shared.register_native(
        THREAD_CLASS,
        "getName",
        "()Ljava/lang/String;",
        thread_get_name,
    );
    shared.register_native(
        THREAD_CLASS,
        "setName",
        "(Ljava/lang/String;)V",
        thread_noop_reference,
    );
    shared.register_native(
        THREAD_CLASS,
        "getState",
        "()Ljava/lang/Thread$State;",
        thread_get_state,
    );
    shared.register_native(THREAD_CLASS, "getId", "()J", thread_get_id);
    shared.register_native(THREAD_CLASS, "getPriority", "()I", thread_get_priority);
    shared.register_native(THREAD_CLASS, "setPriority", "(I)V", thread_noop);
    shared.register_native(THREAD_CLASS, "sleep", "(J)V", thread_noop);
    shared.register_native(THREAD_CLASS, "sleep", "(JI)V", thread_noop);
    shared.register_native(THREAD_CLASS, "yield", "()V", thread_noop);
    shared.register_native(THREAD_CLASS, "interrupt", "()V", thread_noop);
    shared.register_native(THREAD_CLASS, "isInterrupted", "()Z", thread_false);
    shared.register_native(THREAD_CLASS, "interrupted", "()Z", thread_false);
    shared.register_native(THREAD_CLASS, "isAlive", "()Z", thread_true);
    shared.register_native(THREAD_CLASS, "join", "()V", thread_noop);
    shared.register_native(THREAD_CLASS, "join", "(J)V", thread_noop);
    shared.register_native(THREAD_CLASS, "join", "(JI)V", thread_noop);
    shared.register_native(THREAD_CLASS, "start", "()V", thread_noop);
    shared.register_native(
        THREAD_CLASS,
        "getContextClassLoader",
        "()Ljava/lang/ClassLoader;",
        current_class_loader,
    );
    shared.register_native(
        THREAD_CLASS,
        "setContextClassLoader",
        "(Ljava/lang/ClassLoader;)V",
        thread_noop_reference,
    );

    shared.register_native(THREAD_LOCAL_CLASS, "<init>", "()V", thread_noop);
    shared.register_native(
        THREAD_LOCAL_CLASS,
        "get",
        "()Ljava/lang/Object;",
        thread_local_get,
    );
    shared.register_native(
        THREAD_LOCAL_CLASS,
        "set",
        "(Ljava/lang/Object;)V",
        thread_local_set,
    );
    shared.register_native(THREAD_LOCAL_CLASS, "remove", "()V", thread_local_remove);
    shared.register_native(
        THREAD_LOCAL_CLASS,
        "initialValue",
        "()Ljava/lang/Object;",
        thread_null,
    );

    // `InheritableThreadLocal` declares its own constructor and `childValue` in the C1
    // stub, so those two declarations need their own entries. Its inherited `get`/`set`/
    // `remove` still resolve to the `ThreadLocal` declarations above. Until M5 creates a
    // child thread, an inheritable thread-local stores its value exactly like a plain one.
    shared.register_native(INHERITABLE_THREAD_LOCAL_CLASS, "<init>", "()V", thread_noop);
    shared.register_native(
        INHERITABLE_THREAD_LOCAL_CLASS,
        "childValue",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        thread_identity,
    );
}

/// Install the six `Thread.State` enum constants into the stub's statics.
pub fn install_thread_states(shared: &Shared) -> Result<(), VmError> {
    let state_class = shared.load_library_class(THREAD_STATE_CLASS)?;
    let mut values = Vec::with_capacity(THREAD_STATES.len());
    for name in THREAD_STATES {
        let object = shared.heap().allocate_enum_constant(
            state_class,
            EnumConstant {
                class: state_class,
                name: name.to_owned(),
            }
            .name,
        );
        values.push((
            name.to_owned(),
            format!("L{THREAD_STATE_CLASS};"),
            Value::Ref(object),
        ));
    }
    let entry = shared
        .entry(state_class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", state_class.0)))?;
    let mut statics = entry.statics.lock().expect("static field lock poisoned");
    for (name, descriptor, value) in values {
        statics.insert((name, descriptor), value);
    }
    Ok(())
}

fn current_class_loader(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let loader = shared.class_loader_object()?;
    Ok(Some(Value::Ref(loader)))
}

fn thread_local_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(
        shared
            .thread_local(receiver)
            .unwrap_or(Value::Ref(ObjRef::NULL)),
    ))
}

fn thread_local_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    shared.set_thread_local(receiver, value);
    Ok(None)
}

fn thread_local_remove(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.remove_thread_local(receiver);
    Ok(None)
}

fn thread_current(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if let Some(reference) = shared.main_thread() {
        return Ok(Some(Value::Ref(reference)));
    }
    let class = shared.load_library_class(THREAD_CLASS)?;
    let reference = shared.allocate_instance(class)?;
    shared.set_main_thread(reference);
    Ok(Some(Value::Ref(reference)))
}

fn thread_get_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = shared.intern_string("main");
    Ok(Some(Value::Ref(text)))
}

fn thread_get_state(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(THREAD_STATE_CLASS)?;
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    let statics = entry.statics.lock().expect("static field lock poisoned");
    let value = statics
        .get(&("RUNNABLE".to_owned(), format!("L{THREAD_STATE_CLASS};")))
        .copied()
        .ok_or_else(|| VmError::Native("Thread.State.RUNNABLE is not installed".to_owned()))?;
    Ok(Some(value))
}

fn thread_get_id(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(1)))
}

fn thread_get_priority(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(5)))
}

fn thread_noop(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn thread_noop_reference(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

/// Return the only argument unchanged (`InheritableThreadLocal.childValue`).
fn thread_identity(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    match args.first().copied() {
        Some(value) => Ok(Some(value)),
        None => Err(VmError::Native(
            "InheritableThreadLocal.childValue is missing its argument".to_owned(),
        )),
    }
}

fn thread_false(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(0)))
}

fn thread_true(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(1)))
}

fn thread_null(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

fn thread_get_group(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class("java/lang/ThreadGroup")?;
    Ok(Some(Value::Ref(shared.allocate_instance(class)?)))
}

fn thread_group_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn thread_group_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.heap().allocate_string("main".to_owned()),
    )))
}

fn thread_group_parent(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}
