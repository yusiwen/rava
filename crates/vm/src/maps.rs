//! `java.util.HashMap` natives (M2.13).
//!
//! A map is [`crate::HeapObject::Map`], an insertion-ordered `Vec` of `(key, value)`
//! pairs. There is no second representation: [`crate::Shared::allocate_instance`]
//! recognizes `java.util.HashMap` and allocates that shape, and `Shared::get_class` and
//! `invokevirtual` dispatch report the same class. A map is an ordinary heap object, not a
//! permanent root, so [`crate::Shared::roots`] enumerates the references it holds.
//!
//! Keys and values are [`Value`]s and may be null. Membership uses the equality the JDK's
//! `HashMap` documents for the types this slice models: two strings compare by content,
//! two boxed `Integer`s compare by value, and every other reference compares by identity.
//! `containsValue` uses the same rule, so a stored string content-equals a distinct probe
//! string and every other value is matched by reference. There is no hashing, tree bin or
//! resize: a linear scan over insertion order is the whole lookup, which is the honest
//! cost of this slice.
//!
//! The decoded instruction subset models no Java exception objects yet, so a negative
//! initial capacity, a null receiver and a non-reference key or value are typed
//! [`crate::VmError::Native`] errors rather than synthesized exceptions. Only the methods
//! this slice lists are registry entries; `getOrDefault`, `putIfAbsent`, `putAll`,
//! `keySet`, `values`, `entrySet` and the rest of the `HashMap` stub still reach a
//! declaration with no `Code` and fail loudly instead of guessing.

use crate::VmError;
use crate::heap::{BoxedPrimitive, Heap, HeapError, HeapObject, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// The internal name of the map's runtime class.
pub const HASH_MAP_CLASS: &str = "java/util/HashMap";

/// Registers the M2.13 `java.util.HashMap` natives.
///
/// Every entry is keyed by `java.util.HashMap`, the class the C1 stub declares the method
/// on, so `invokevirtual`, `invokespecial` and `invokestatic` all reach the registry before
/// the stub's missing `Code` is consulted.
pub fn register_natives(shared: &Shared) {
    shared.register_native(HASH_MAP_CLASS, "<init>", "()V", hash_map_init);
    shared.register_native(HASH_MAP_CLASS, "<init>", "(I)V", hash_map_init_capacity);
    shared.register_native(HASH_MAP_CLASS, "size", "()I", hash_map_size);
    shared.register_native(HASH_MAP_CLASS, "isEmpty", "()Z", hash_map_is_empty);
    shared.register_native(
        HASH_MAP_CLASS,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_put,
    );
    shared.register_native(
        HASH_MAP_CLASS,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_get,
    );
    shared.register_native(
        HASH_MAP_CLASS,
        "containsKey",
        "(Ljava/lang/Object;)Z",
        hash_map_contains_key,
    );
    shared.register_native(
        HASH_MAP_CLASS,
        "containsValue",
        "(Ljava/lang/Object;)Z",
        hash_map_contains_value,
    );
    shared.register_native(
        HASH_MAP_CLASS,
        "remove",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_remove,
    );
    shared.register_native(HASH_MAP_CLASS, "clear", "()V", hash_map_clear);
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn null_receiver(method: &'static str) -> VmError {
    VmError::Native(format!("{method} was called on a null receiver"))
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        Some(_) => Err(VmError::Native(format!("{method} expects an int argument"))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

/// A reference argument, including null.
fn reference_argument(
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

/// A map-backed receiver's entries, or a typed error naming the method.
fn entries<'a>(
    heap: &'a Heap,
    receiver: ObjRef,
    method: &'static str,
) -> Result<&'a [(Value, Value)], VmError> {
    if receiver.is_null() {
        return Err(null_receiver(method));
    }
    match heap.map(receiver) {
        Ok(entries) => Ok(entries),
        Err(HeapError::WrongObjectKind { .. }) => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.HashMap"
        ))),
        Err(error) => Err(heap_error(error)),
    }
}

/// The mutable entries behind a non-null map receiver.
///
/// The receiver is validated once through [`entries`], so this second lookup cannot fail on
/// a receiver the caller already accepted.
fn entries_mut(heap: &mut Heap, receiver: ObjRef) -> Result<&mut Vec<(Value, Value)>, VmError> {
    heap.map_mut(receiver).map_err(|error| match error {
        HeapError::WrongObjectKind { .. } => {
            VmError::Native("receiver is not a java.util.HashMap".to_owned())
        }
        other => heap_error(other),
    })
}

/// Equality of two map keys or values.
///
/// A string compares by content and a boxed `Integer` by value; any other pair of
/// references compares by identity, and null equals only null. The non-reference arm is
/// unreachable through the `Object`-typed signatures this slice registers, but the map
/// stores [`Value`]s, so it is defined rather than left to panic.
fn values_equal(heap: &Heap, left: Value, right: Value) -> Result<bool, HeapError> {
    let (Value::Ref(left_ref), Value::Ref(right_ref)) = (left, right) else {
        return Ok(left == right);
    };
    if left_ref.is_null() || right_ref.is_null() {
        return Ok(left_ref == right_ref);
    }
    match (heap.get(left_ref)?, heap.get(right_ref)?) {
        (HeapObject::Str(left), HeapObject::Str(right)) => Ok(left == right),
        (
            HeapObject::Boxed(BoxedPrimitive::Integer(left)),
            HeapObject::Boxed(BoxedPrimitive::Integer(right)),
        ) => Ok(left == right),
        _ => Ok(left_ref == right_ref),
    }
}

/// The position of `probe` in `entries`, or `None` when the key is absent.
fn find_key(
    heap: &Heap,
    entries: &[(Value, Value)],
    probe: Value,
) -> Result<Option<usize>, HeapError> {
    for (position, (key, _)) in entries.iter().enumerate() {
        if values_equal(heap, *key, probe)? {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

/// The position of a value in `entries`, or `None` when the value is absent.
fn find_value(
    heap: &Heap,
    entries: &[(Value, Value)],
    probe: Value,
) -> Result<Option<usize>, HeapError> {
    for (position, (_, value)) in entries.iter().enumerate() {
        if values_equal(heap, *value, probe)? {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

/// `HashMap.<init>()V`: the empty map `new` already allocated.
fn hash_map_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let heap = shared.heap();
    entries(&heap, receiver, "HashMap.<init>")?;
    Ok(None)
}

/// `HashMap.<init>(I)V`: a capacity hint, not a stored size.
///
/// The JDK raises `IllegalArgumentException` for a negative capacity; this slice has no
/// exception-object synthesis, so it is a typed error. The capacity itself is ignored, the
/// same choice the M2.12 `ArrayList` and `StringBuilder` constructors make.
fn hash_map_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "HashMap.<init>")?;
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "HashMap.<init>: negative capacity {capacity}"
        )));
    }
    let heap = shared.heap();
    entries(&heap, receiver, "HashMap.<init>")?;
    Ok(None)
}

/// `HashMap.size()`: the number of entries.
fn hash_map_size(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let length = {
        let heap = shared.heap();
        entries(&heap, receiver, "HashMap.size")?.len()
    };
    let size = i32::try_from(length)
        .map_err(|_| VmError::Native("HashMap is longer than an int can hold".to_owned()))?;
    Ok(Some(Value::Int(size)))
}

/// `HashMap.isEmpty()`.
fn hash_map_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let empty = {
        let heap = shared.heap();
        entries(&heap, receiver, "HashMap.isEmpty")?.is_empty()
    };
    Ok(Some(Value::Int(i32::from(empty))))
}

/// `HashMap.put(Object, Object)`: store the pair and return the previous value, or null.
///
/// A null key and a null value are both valid and are stored as [`ObjRef::NULL`]. An equal
/// key replaces its value in place, keeping the key object already stored and the insertion
/// position, exactly as the JDK's map keeps the existing entry.
fn hash_map_put(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "HashMap.put")?;
    let value = reference_argument(args, 1, "HashMap.put")?;
    let mut heap = shared.heap();
    let position = {
        let entries = entries(&heap, receiver, "HashMap.put")?;
        find_key(&heap, entries, Value::Ref(key)).map_err(heap_error)?
    };
    let entries = entries_mut(&mut heap, receiver)?;
    match position {
        Some(position) => {
            let previous = std::mem::replace(&mut entries[position].1, Value::Ref(value));
            Ok(Some(previous))
        }
        None => {
            entries.push((Value::Ref(key), Value::Ref(value)));
            Ok(Some(Value::Ref(ObjRef::NULL)))
        }
    }
}

/// `HashMap.get(Object)`: the stored value, or null when the key is absent.
///
/// A stored null value and an absent key both return null, as on the JDK; `containsKey`
/// tells them apart.
fn hash_map_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "HashMap.get")?;
    let heap = shared.heap();
    let entries = entries(&heap, receiver, "HashMap.get")?;
    let position = find_key(&heap, entries, Value::Ref(key)).map_err(heap_error)?;
    Ok(Some(match position {
        Some(position) => entries[position].1,
        None => Value::Ref(ObjRef::NULL),
    }))
}

/// `HashMap.containsKey(Object)`.
fn hash_map_contains_key(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "HashMap.containsKey")?;
    let heap = shared.heap();
    let entries = entries(&heap, receiver, "HashMap.containsKey")?;
    let present = find_key(&heap, entries, Value::Ref(key))
        .map_err(heap_error)?
        .is_some();
    Ok(Some(Value::Int(i32::from(present))))
}

/// `HashMap.containsValue(Object)`.
fn hash_map_contains_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "HashMap.containsValue")?;
    let heap = shared.heap();
    let entries = entries(&heap, receiver, "HashMap.containsValue")?;
    let present = find_value(&heap, entries, Value::Ref(value))
        .map_err(heap_error)?
        .is_some();
    Ok(Some(Value::Int(i32::from(present))))
}

/// `HashMap.remove(Object)`: remove the entry and return its value, or null.
fn hash_map_remove(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "HashMap.remove")?;
    let mut heap = shared.heap();
    let position = {
        let entries = entries(&heap, receiver, "HashMap.remove")?;
        find_key(&heap, entries, Value::Ref(key)).map_err(heap_error)?
    };
    let Some(position) = position else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let entries = entries_mut(&mut heap, receiver)?;
    Ok(Some(entries.remove(position).1))
}

/// `HashMap.clear()`: drop every entry, including a null key or value.
fn hash_map_clear(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    {
        let heap = shared.heap();
        entries(&heap, receiver, "HashMap.clear")?;
    }
    let mut heap = shared.heap();
    entries_mut(&mut heap, receiver)?.clear();
    Ok(None)
}
