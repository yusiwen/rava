//! `java.util.concurrent.atomic` natives (M2.23).
//!
//! Spring's bean factory and lifecycle code use `AtomicBoolean`, `AtomicInteger`,
//! `AtomicLong` and `AtomicReference` for initialization flags and cached values. The VM
//! is single-threaded until M5, so a cell in [`HeapObject::Atomic`] is enough: instruction
//! dispatch is the atomicity boundary, and a later lock implementation replaces the cell
//! access, not the Java surface.
//!
//! Every class is allocated through [`crate::Shared::allocate_instance`], which recognizes
//! the four class names and creates the matching [`crate::AtomicCell`]. Constructors only
//! replace the initial value.

use crate::VmError;
use crate::heap::{AtomicCell, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

pub const ATOMIC_BOOLEAN_CLASS: &str = "java/util/concurrent/atomic/AtomicBoolean";
pub const ATOMIC_INTEGER_CLASS: &str = "java/util/concurrent/atomic/AtomicInteger";
pub const ATOMIC_LONG_CLASS: &str = "java/util/concurrent/atomic/AtomicLong";
pub const ATOMIC_REFERENCE_CLASS: &str = "java/util/concurrent/atomic/AtomicReference";

/// The cell a freshly allocated class starts with.
pub(crate) fn initial_cell(class_name: &str) -> AtomicCell {
    match class_name {
        ATOMIC_BOOLEAN_CLASS => AtomicCell::Boolean(false),
        ATOMIC_INTEGER_CLASS => AtomicCell::Integer(0),
        ATOMIC_LONG_CLASS => AtomicCell::Long(0),
        _ => AtomicCell::Reference(Value::Ref(ObjRef::NULL)),
    }
}

pub fn register_natives(shared: &Shared) {
    // AtomicBoolean
    shared.register_native(ATOMIC_BOOLEAN_CLASS, "<init>", "()V", atomic_noop_init);
    shared.register_native(ATOMIC_BOOLEAN_CLASS, "<init>", "(Z)V", boolean_init);
    shared.register_native(ATOMIC_BOOLEAN_CLASS, "get", "()Z", boolean_get);
    shared.register_native(ATOMIC_BOOLEAN_CLASS, "set", "(Z)V", boolean_set);
    shared.register_native(ATOMIC_BOOLEAN_CLASS, "lazySet", "(Z)V", boolean_set);
    shared.register_native(
        ATOMIC_BOOLEAN_CLASS,
        "compareAndSet",
        "(ZZ)Z",
        boolean_compare_and_set,
    );
    shared.register_native(
        ATOMIC_BOOLEAN_CLASS,
        "weakCompareAndSet",
        "(ZZ)Z",
        boolean_compare_and_set,
    );
    shared.register_native(
        ATOMIC_BOOLEAN_CLASS,
        "getAndSet",
        "(Z)Z",
        boolean_get_and_set,
    );

    // AtomicInteger
    shared.register_native(ATOMIC_INTEGER_CLASS, "<init>", "()V", atomic_noop_init);
    shared.register_native(ATOMIC_INTEGER_CLASS, "<init>", "(I)V", integer_init);
    shared.register_native(ATOMIC_INTEGER_CLASS, "get", "()I", integer_get);
    shared.register_native(ATOMIC_INTEGER_CLASS, "set", "(I)V", integer_set);
    shared.register_native(ATOMIC_INTEGER_CLASS, "lazySet", "(I)V", integer_set);
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "compareAndSet",
        "(II)Z",
        integer_compare_and_set,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "weakCompareAndSet",
        "(II)Z",
        integer_compare_and_set,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "getAndSet",
        "(I)I",
        integer_get_and_set,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "getAndIncrement",
        "()I",
        integer_get_and_increment,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "getAndDecrement",
        "()I",
        integer_get_and_decrement,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "incrementAndGet",
        "()I",
        integer_increment_and_get,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "decrementAndGet",
        "()I",
        integer_decrement_and_get,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "addAndGet",
        "(I)I",
        integer_add_and_get,
    );
    shared.register_native(
        ATOMIC_INTEGER_CLASS,
        "getAndAdd",
        "(I)I",
        integer_get_and_add,
    );
    shared.register_native(ATOMIC_INTEGER_CLASS, "intValue", "()I", integer_get);

    // AtomicLong
    shared.register_native(ATOMIC_LONG_CLASS, "<init>", "()V", atomic_noop_init);
    shared.register_native(ATOMIC_LONG_CLASS, "<init>", "(J)V", long_init);
    shared.register_native(ATOMIC_LONG_CLASS, "get", "()J", long_get);
    shared.register_native(ATOMIC_LONG_CLASS, "set", "(J)V", long_set);
    shared.register_native(ATOMIC_LONG_CLASS, "lazySet", "(J)V", long_set);
    shared.register_native(
        ATOMIC_LONG_CLASS,
        "compareAndSet",
        "(JJ)Z",
        long_compare_and_set,
    );
    shared.register_native(ATOMIC_LONG_CLASS, "getAndSet", "(J)J", long_get_and_set);
    shared.register_native(
        ATOMIC_LONG_CLASS,
        "getAndIncrement",
        "()J",
        long_get_and_increment,
    );
    shared.register_native(
        ATOMIC_LONG_CLASS,
        "incrementAndGet",
        "()J",
        long_increment_and_get,
    );
    shared.register_native(ATOMIC_LONG_CLASS, "addAndGet", "(J)J", long_add_and_get);
    shared.register_native(ATOMIC_LONG_CLASS, "getAndAdd", "(J)J", long_get_and_add);
    shared.register_native(ATOMIC_LONG_CLASS, "longValue", "()J", long_get);

    // AtomicReference
    shared.register_native(ATOMIC_REFERENCE_CLASS, "<init>", "()V", atomic_noop_init);
    shared.register_native(
        ATOMIC_REFERENCE_CLASS,
        "<init>",
        "(Ljava/lang/Object;)V",
        reference_init,
    );
    shared.register_native(
        ATOMIC_REFERENCE_CLASS,
        "get",
        "()Ljava/lang/Object;",
        reference_get,
    );
    shared.register_native(
        ATOMIC_REFERENCE_CLASS,
        "set",
        "(Ljava/lang/Object;)V",
        reference_set,
    );
    shared.register_native(
        ATOMIC_REFERENCE_CLASS,
        "lazySet",
        "(Ljava/lang/Object;)V",
        reference_set,
    );
    shared.register_native(
        ATOMIC_REFERENCE_CLASS,
        "compareAndSet",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        reference_compare_and_set,
    );
    shared.register_native(
        ATOMIC_REFERENCE_CLASS,
        "getAndSet",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        reference_get_and_set,
    );
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn bool_argument(args: &[Value], position: usize, method: &'static str) -> Result<bool, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value != 0),
        _ => Err(VmError::Native(format!(
            "{method} expects a boolean argument"
        ))),
    }
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(format!("{method} expects an int argument"))),
    }
}

fn long_argument(args: &[Value], position: usize, method: &'static str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        _ => Err(VmError::Native(format!("{method} expects a long argument"))),
    }
}

fn reference_argument(args: &[Value], position: usize, _method: &'static str) -> ObjRef {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    }
}

fn atomic_noop_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn boolean_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = bool_argument(args, 0, "AtomicBoolean.<init>")?;
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? = AtomicCell::Boolean(value);
    Ok(None)
}

fn boolean_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match shared.heap().atomic(receiver).map_err(heap_error)? {
        AtomicCell::Boolean(value) => *value,
        _ => {
            return Err(VmError::Native(
                "AtomicBoolean.get on a non-boolean cell".to_owned(),
            ));
        }
    };
    Ok(Some(Value::Int(i32::from(value))))
}

fn boolean_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = bool_argument(args, 0, "AtomicBoolean.set")?;
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? = AtomicCell::Boolean(value);
    Ok(None)
}

fn boolean_compare_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let expected = bool_argument(args, 0, "AtomicBoolean.compareAndSet")?;
    let new = bool_argument(args, 1, "AtomicBoolean.compareAndSet")?;
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Boolean(current) = cell else {
        return Err(VmError::Native(
            "AtomicBoolean.compareAndSet on a non-boolean cell".to_owned(),
        ));
    };
    if *current == expected {
        *current = new;
        Ok(Some(Value::Int(1)))
    } else {
        Ok(Some(Value::Int(0)))
    }
}

fn boolean_get_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let new = bool_argument(args, 0, "AtomicBoolean.getAndSet")?;
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Boolean(current) = cell else {
        return Err(VmError::Native(
            "AtomicBoolean.getAndSet on a non-boolean cell".to_owned(),
        ));
    };
    let previous = *current;
    *current = new;
    Ok(Some(Value::Int(i32::from(previous))))
}

fn integer_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "AtomicInteger.<init>")?;
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? = AtomicCell::Integer(value);
    Ok(None)
}

fn integer_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match shared.heap().atomic(receiver).map_err(heap_error)? {
        AtomicCell::Integer(value) => *value,
        _ => {
            return Err(VmError::Native(
                "AtomicInteger.get on a non-integer cell".to_owned(),
            ));
        }
    };
    Ok(Some(Value::Int(value)))
}

fn integer_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = int_argument(args, 0, "AtomicInteger.set")?;
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? = AtomicCell::Integer(value);
    Ok(None)
}

fn integer_compare_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let expected = int_argument(args, 0, "AtomicInteger.compareAndSet")?;
    let new = int_argument(args, 1, "AtomicInteger.compareAndSet")?;
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Integer(current) = cell else {
        return Err(VmError::Native(
            "AtomicInteger.compareAndSet on a non-integer cell".to_owned(),
        ));
    };
    if *current == expected {
        *current = new;
        Ok(Some(Value::Int(1)))
    } else {
        Ok(Some(Value::Int(0)))
    }
}

fn integer_update(
    shared: &Shared,
    receiver: ObjRef,
    update: impl FnOnce(i32) -> i32,
    method: &'static str,
) -> Result<i32, VmError> {
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Integer(current) = cell else {
        return Err(VmError::Native(format!("{method} on a non-integer cell")));
    };
    let previous = *current;
    *current = update(previous);
    Ok(previous)
}

fn integer_get_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let new = int_argument(args, 0, "AtomicInteger.getAndSet")?;
    let previous = integer_update(shared, receiver, |_| new, "AtomicInteger.getAndSet")?;
    Ok(Some(Value::Int(previous)))
}

fn integer_get_and_increment(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let previous = integer_update(
        shared,
        receiver,
        |value| value.wrapping_add(1),
        "AtomicInteger.getAndIncrement",
    )?;
    Ok(Some(Value::Int(previous)))
}

fn integer_get_and_decrement(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let previous = integer_update(
        shared,
        receiver,
        |value| value.wrapping_sub(1),
        "AtomicInteger.getAndDecrement",
    )?;
    Ok(Some(Value::Int(previous)))
}

fn integer_increment_and_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let previous = integer_update(
        shared,
        receiver,
        |value| value.wrapping_add(1),
        "AtomicInteger.incrementAndGet",
    )?;
    Ok(Some(Value::Int(previous.wrapping_add(1))))
}

fn integer_decrement_and_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let previous = integer_update(
        shared,
        receiver,
        |value| value.wrapping_sub(1),
        "AtomicInteger.decrementAndGet",
    )?;
    Ok(Some(Value::Int(previous.wrapping_sub(1))))
}

fn integer_add_and_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let delta = int_argument(args, 0, "AtomicInteger.addAndGet")?;
    let previous = integer_update(
        shared,
        receiver,
        |value| value.wrapping_add(delta),
        "AtomicInteger.addAndGet",
    )?;
    Ok(Some(Value::Int(previous.wrapping_add(delta))))
}

fn integer_get_and_add(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let delta = int_argument(args, 0, "AtomicInteger.getAndAdd")?;
    let previous = integer_update(
        shared,
        receiver,
        |value| value.wrapping_add(delta),
        "AtomicInteger.getAndAdd",
    )?;
    Ok(Some(Value::Int(previous)))
}

fn long_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = long_argument(args, 0, "AtomicLong.<init>")?;
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? = AtomicCell::Long(value);
    Ok(None)
}

fn long_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match shared.heap().atomic(receiver).map_err(heap_error)? {
        AtomicCell::Long(value) => *value,
        _ => {
            return Err(VmError::Native(
                "AtomicLong.get on a non-long cell".to_owned(),
            ));
        }
    };
    Ok(Some(Value::Long(value)))
}

fn long_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = long_argument(args, 0, "AtomicLong.set")?;
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? = AtomicCell::Long(value);
    Ok(None)
}

fn long_compare_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let expected = long_argument(args, 0, "AtomicLong.compareAndSet")?;
    let new = long_argument(args, 1, "AtomicLong.compareAndSet")?;
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Long(current) = cell else {
        return Err(VmError::Native(
            "AtomicLong.compareAndSet on a non-long cell".to_owned(),
        ));
    };
    if *current == expected {
        *current = new;
        Ok(Some(Value::Int(1)))
    } else {
        Ok(Some(Value::Int(0)))
    }
}

fn long_update(
    shared: &Shared,
    receiver: ObjRef,
    update: impl FnOnce(i64) -> i64,
    method: &'static str,
) -> Result<i64, VmError> {
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Long(current) = cell else {
        return Err(VmError::Native(format!("{method} on a non-long cell")));
    };
    let previous = *current;
    *current = update(previous);
    Ok(previous)
}

fn long_get_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let new = long_argument(args, 0, "AtomicLong.getAndSet")?;
    let previous = long_update(shared, receiver, |_| new, "AtomicLong.getAndSet")?;
    Ok(Some(Value::Long(previous)))
}

fn long_get_and_increment(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let previous = long_update(
        shared,
        receiver,
        |value| value.wrapping_add(1),
        "AtomicLong.getAndIncrement",
    )?;
    Ok(Some(Value::Long(previous)))
}

fn long_increment_and_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let previous = long_update(
        shared,
        receiver,
        |value| value.wrapping_add(1),
        "AtomicLong.incrementAndGet",
    )?;
    Ok(Some(Value::Long(previous.wrapping_add(1))))
}

fn long_add_and_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let delta = long_argument(args, 0, "AtomicLong.addAndGet")?;
    let previous = long_update(
        shared,
        receiver,
        |value| value.wrapping_add(delta),
        "AtomicLong.addAndGet",
    )?;
    Ok(Some(Value::Long(previous.wrapping_add(delta))))
}

fn long_get_and_add(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let delta = long_argument(args, 0, "AtomicLong.getAndAdd")?;
    let previous = long_update(
        shared,
        receiver,
        |value| value.wrapping_add(delta),
        "AtomicLong.getAndAdd",
    )?;
    Ok(Some(Value::Long(previous)))
}

fn reference_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "AtomicReference.<init>");
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? =
        AtomicCell::Reference(Value::Ref(value));
    Ok(None)
}

fn reference_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match shared.heap().atomic(receiver).map_err(heap_error)? {
        AtomicCell::Reference(value) => *value,
        _ => {
            return Err(VmError::Native(
                "AtomicReference.get on a non-reference cell".to_owned(),
            ));
        }
    };
    Ok(Some(value))
}

fn reference_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "AtomicReference.set");
    *shared.heap().atomic_mut(receiver).map_err(heap_error)? =
        AtomicCell::Reference(Value::Ref(value));
    Ok(None)
}

fn reference_compare_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let expected = reference_argument(args, 0, "AtomicReference.compareAndSet");
    let new = reference_argument(args, 1, "AtomicReference.compareAndSet");
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Reference(current) = cell else {
        return Err(VmError::Native(
            "AtomicReference.compareAndSet on a non-reference cell".to_owned(),
        ));
    };
    if *current == Value::Ref(expected) {
        *current = Value::Ref(new);
        Ok(Some(Value::Int(1)))
    } else {
        Ok(Some(Value::Int(0)))
    }
}

fn reference_get_and_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let new = reference_argument(args, 0, "AtomicReference.getAndSet");
    let mut heap = shared.heap();
    let cell = heap.atomic_mut(receiver).map_err(heap_error)?;
    let AtomicCell::Reference(current) = cell else {
        return Err(VmError::Native(
            "AtomicReference.getAndSet on a non-reference cell".to_owned(),
        ));
    };
    let previous = *current;
    *current = Value::Ref(new);
    Ok(Some(previous))
}
