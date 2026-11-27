//! `java.lang.ref.Reference` and `ReferenceQueue` natives (M2.23).
//!
//! Spring's `ConcurrentReferenceHashMap` wraps keys in `SoftReference`/`WeakReference`
//! objects and tests them through `Reference.get()`; its `ReferenceManager` also polls a
//! `ReferenceQueue`. The VM has no collector until M5 (ADR-0008), so a referent is never
//! cleared or enqueued implicitly. `get` returns the stored referent until `clear` is
//! called, `enqueue` reports `false` and `ReferenceQueue.poll` returns null, which is a
//! truthful no-collector behavior rather than a fabricated collection.
//!
//! The C1 `Reference` hierarchy has no constructor `Code`, so the referent is recorded in
//! [`crate::Shared`] when `Reference.<init>` runs. The subclass (`SoftReference`,
//! `WeakReference`, `PhantomReference`) is an ordinary instance and keeps its own class
//! identity.

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const REFERENCE_CLASS: &str = "java/lang/ref/Reference";
const REFERENCE_QUEUE_CLASS: &str = "java/lang/ref/ReferenceQueue";

pub fn register_natives(shared: &Shared) {
    shared.register_native_if_declared(
        REFERENCE_CLASS,
        "<init>",
        "(Ljava/lang/Object;)V",
        reference_init,
    );
    shared.register_native_if_declared(
        REFERENCE_CLASS,
        "<init>",
        "(Ljava/lang/Object;Ljava/lang/ref/ReferenceQueue;)V",
        reference_init,
    );
    shared.register_native(
        REFERENCE_CLASS,
        "get",
        "()Ljava/lang/Object;",
        reference_get,
    );
    shared.register_native(REFERENCE_CLASS, "clear", "()V", reference_clear);
    shared.register_native(REFERENCE_CLASS, "enqueue", "()Z", reference_enqueue);
    shared.register_native(REFERENCE_CLASS, "isEnqueued", "()Z", reference_is_enqueued);
    shared.register_native(
        REFERENCE_CLASS,
        "refersTo",
        "(Ljava/lang/Object;)Z",
        reference_refers_to,
    );

    for class in [
        "java/lang/ref/SoftReference",
        "java/lang/ref/WeakReference",
        "java/lang/ref/PhantomReference",
    ] {
        shared.register_native(class, "<init>", "(Ljava/lang/Object;)V", reference_init);
        shared.register_native(
            class,
            "<init>",
            "(Ljava/lang/Object;Ljava/lang/ref/ReferenceQueue;)V",
            reference_init,
        );
        shared.register_native(class, "get", "()Ljava/lang/Object;", reference_get);
        shared.register_native(class, "clear", "()V", reference_clear);
        shared.register_native(class, "enqueue", "()Z", reference_enqueue);
        shared.register_native(class, "isEnqueued", "()Z", reference_is_enqueued);
        shared.register_native(
            class,
            "refersTo",
            "(Ljava/lang/Object;)Z",
            reference_refers_to,
        );
    }

    shared.register_native_if_declared(
        REFERENCE_QUEUE_CLASS,
        "<init>",
        "()V",
        reference_queue_init,
    );
    shared.register_native_if_declared(
        REFERENCE_QUEUE_CLASS,
        "<init>",
        "(I)V",
        reference_queue_init,
    );
    shared.register_native(
        REFERENCE_QUEUE_CLASS,
        "poll",
        "()Ljava/lang/ref/Reference;",
        reference_queue_poll,
    );
    shared.register_native(
        REFERENCE_QUEUE_CLASS,
        "remove",
        "()Ljava/lang/ref/Reference;",
        reference_queue_poll,
    );
    shared.register_native(
        REFERENCE_QUEUE_CLASS,
        "remove",
        "(J)Ljava/lang/ref/Reference;",
        reference_queue_poll,
    );
}

fn reference_argument(
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
    }
}

fn reference_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let referent = reference_argument(args, 0, "Reference.<init>")?;
    shared.set_referent(receiver, referent);
    Ok(None)
}

fn reference_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let referent = shared.referent(receiver).unwrap_or(ObjRef::NULL);
    Ok(Some(Value::Ref(referent)))
}

fn reference_clear(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.clear_referent(receiver);
    Ok(None)
}

fn reference_enqueue(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(0)))
}

fn reference_is_enqueued(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(0)))
}

fn reference_refers_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = reference_argument(args, 0, "Reference.refersTo")?;
    let referent = shared.referent(receiver).unwrap_or(ObjRef::NULL);
    Ok(Some(Value::Int(i32::from(referent == other))))
}

fn reference_queue_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn reference_queue_poll(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}
