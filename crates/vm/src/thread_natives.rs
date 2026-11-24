//! `java.lang.Thread` and `ClassLoader` natives for the single-threaded VM (M2.23).
//!
//! M5 introduces real threads. Until then `currentThread` returns one cached `Thread`
//! object, `sleep`/`yield`/`join` return immediately, and `getState` reports `RUNNABLE`.
//! Spring's lifecycle code reads `Thread.currentThread()` and compares its state with
//! `Thread.State.WAITING`, so the six enum constants are installed into `Thread$State`'s
//! statics exactly as `java.util.logging.Level` is.
//!
//! `ClassLoader` has no public object model yet: `getSystemClassLoader` and
//! `getContextClassLoader` return null, which makes Spring fall back to
//! `Class.forName(String)` and the application loader.

use crate::VmError;
use crate::heap::{EnumConstant, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const THREAD_CLASS: &str = "java/lang/Thread";
const THREAD_STATE_CLASS: &str = "java/lang/Thread$State";
const CLASS_LOADER_CLASS: &str = "java/lang/ClassLoader";
const THREAD_LOCAL_CLASS: &str = "java/lang/ThreadLocal";

const THREAD_STATES: [&str; 6] = [
    "NEW",
    "RUNNABLE",
    "BLOCKED",
    "WAITING",
    "TIMED_WAITING",
    "TERMINATED",
];

pub fn register_natives(shared: &Shared) {
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
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getResource",
        "(Ljava/lang/String;)Ljava/net/URL;",
        thread_null,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getResources",
        "(Ljava/lang/String;)Ljava/util/Enumeration;",
        class_loader_get_resources,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getSystemResources",
        "(Ljava/lang/String;)Ljava/util/Enumeration;",
        class_loader_get_resources,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getSystemResource",
        "(Ljava/lang/String;)Ljava/net/URL;",
        thread_null,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getResourceAsStream",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        thread_null,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getSystemResourceAsStream",
        "(Ljava/lang/String;)Ljava/io/InputStream;",
        thread_null,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getSystemClassLoader",
        "()Ljava/lang/ClassLoader;",
        current_class_loader,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getPlatformClassLoader",
        "()Ljava/lang/ClassLoader;",
        current_class_loader,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "getContextClassLoader",
        "()Ljava/lang/ClassLoader;",
        current_class_loader,
    );
    shared.register_native(
        CLASS_LOADER_CLASS,
        "setContextClassLoader",
        "(Ljava/lang/ClassLoader;)V",
        thread_noop_reference,
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

fn class_loader_get_resources(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let enumeration = shared.heap().allocate_enumeration(Vec::new());
    Ok(Some(Value::Ref(enumeration)))
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
