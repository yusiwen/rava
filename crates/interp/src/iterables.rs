//! `Iterable.spliterator()` (M4.5).
//!
//! The JDK's default `spliterator()` wraps `iterator()` in a size-unknown spliterator.
//! Spring Boot's configuration-property sources are `Iterable`, and `StreamSupport` asks
//! them for a spliterator to build a stream, so the default must run.
//!
//! The VM's spliterator shape is a snapshot, so the native drains the receiver's own
//! `iterator()` here and records the values. Draining calls Java `hasNext`/`next`, which
//! re-enters the interpreter, hence this module lives in `rava-interp`.

use rava_vm::{LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, resolve_virtual, run_instance_method, runtime_class};

const ITERABLE_CLASS: &str = "java/lang/Iterable";
const ITERATOR_DESCRIPTOR: &str = "()Ljava/util/Iterator;";
const HAS_NEXT_DESCRIPTOR: &str = "()Z";
const NEXT_DESCRIPTOR: &str = "()Ljava/lang/Object;";

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        ITERABLE_CLASS,
        "spliterator",
        "()Ljava/util/Spliterator;",
        iterable_spliterator,
    );
}

fn invoke(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
) -> Result<Option<Value>, VmError> {
    let class = match runtime_class(shared, receiver) {
        Ok(class) => class,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let (entry, _) = match resolve_virtual(shared, class, name, descriptor) {
        Ok(found) => found,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    match run_instance_method(
        shared,
        thread,
        entry.id,
        name,
        descriptor,
        receiver,
        Vec::new(),
    ) {
        Ok(value) => Ok(value),
        Err(InterpError::Thrown(throwable)) => Err(VmError::Thrown(throwable)),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

fn iterable_spliterator(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let iterator = match invoke(shared, thread, receiver, "iterator", ITERATOR_DESCRIPTOR)? {
        Some(Value::Ref(iterator)) if !iterator.is_null() => iterator,
        _ => {
            return Err(VmError::Native(
                "Iterable.spliterator: iterator() did not return an Iterator".to_owned(),
            ));
        }
    };
    let mut elements = Vec::new();
    loop {
        let has_next = invoke(shared, thread, iterator, "hasNext", HAS_NEXT_DESCRIPTOR)?;
        if !matches!(has_next, Some(Value::Int(value)) if value != 0) {
            break;
        }
        let next = invoke(shared, thread, iterator, "next", NEXT_DESCRIPTOR)?;
        elements.push(next.unwrap_or(Value::Ref(ObjRef::NULL)));
    }
    Ok(Some(Value::Ref(
        shared.heap().allocate_spliterator(elements, 0),
    )))
}
