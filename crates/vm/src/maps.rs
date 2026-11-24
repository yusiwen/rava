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

/// The internal name of the insertion-ordered map's runtime class.
pub const LINKED_HASH_MAP_CLASS: &str = "java/util/LinkedHashMap";

/// The internal name of the sorted map's runtime class.
pub const TREE_MAP_CLASS: &str = "java/util/TreeMap";

/// The internal name of the identity map's runtime class.
pub const IDENTITY_HASH_MAP_CLASS: &str = "java/util/IdentityHashMap";

/// The internal name of the concurrent map's runtime class.
pub const CONCURRENT_HASH_MAP_CLASS: &str = "java/util/concurrent/ConcurrentHashMap";

/// The internal name of the legacy hash table.
pub const HASHTABLE_CLASS: &str = "java/util/Hashtable";

/// The internal name of the properties table.
pub const PROPERTIES_CLASS: &str = "java/util/Properties";

/// The internal name of the map entry interface.
pub const MAP_ENTRY_CLASS: &str = "java/util/Map$Entry";

/// Registers the M2.13 `java.util.HashMap` natives.
///
/// Every entry is keyed by `java.util.HashMap`, the class the C1 stub declares the method
/// on, so `invokevirtual`, `invokespecial` and `invokestatic` all reach the registry before
/// the stub's missing `Code` is consulted.
pub fn register_natives(shared: &Shared) {
    for class in [
        HASH_MAP_CLASS,
        TREE_MAP_CLASS,
        IDENTITY_HASH_MAP_CLASS,
        CONCURRENT_HASH_MAP_CLASS,
        HASHTABLE_CLASS,
    ] {
        register_map_surface(shared, class);
    }
    // `Properties` extends `Hashtable`, so the `Hashtable` entries serve a `Properties`
    // receiver through the superclass walk; only its own constructors and property access
    // are distinct.
    for descriptor in ["()V", "(Ljava/util/Properties;)V", "(I)V"] {
        shared.register_native(PROPERTIES_CLASS, "<init>", descriptor, map_init);
    }
    shared.register_native(
        PROPERTIES_CLASS,
        "getProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
        properties_get,
    );
    shared.register_native(
        PROPERTIES_CLASS,
        "getProperty",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        properties_get_default,
    );
    shared.register_native(
        PROPERTIES_CLASS,
        "setProperty",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;",
        properties_set,
    );
    // `LinkedHashMap` inherits the `HashMap` surface, so the `HashMap` entries serve it
    // through the ordinary superclass walk; only its constructors are distinct.
    shared.register_native(LINKED_HASH_MAP_CLASS, "<init>", "()V", map_init);
    shared.register_native(LINKED_HASH_MAP_CLASS, "<init>", "(I)V", map_init_capacity);
    shared.register_native(
        LINKED_HASH_MAP_CLASS,
        "<init>",
        "(ILF)V",
        map_init_threshold,
    );
    shared.register_native(
        LINKED_HASH_MAP_CLASS,
        "<init>",
        "(Ljava/util/Map;)V",
        map_init_from,
    );
    shared.register_native(
        CONCURRENT_HASH_MAP_CLASS,
        "newKeySet",
        "()Ljava/util/concurrent/ConcurrentHashMap$KeySetView;",
        concurrent_hash_map_new_key_set,
    );
    shared.register_native(
        CONCURRENT_HASH_MAP_CLASS,
        "newKeySet",
        "(I)Ljava/util/concurrent/ConcurrentHashMap$KeySetView;",
        concurrent_hash_map_new_key_set,
    );
    register_map_entry_natives(shared);
}

/// Register the map surface for one concrete map class.
fn register_map_surface(shared: &Shared, class: &'static str) {
    shared.register_native(class, "<init>", "()V", map_init);
    shared.register_native(class, "<init>", "(I)V", map_init_capacity);
    shared.register_native(class, "<init>", "(ILF)V", map_init_threshold);
    shared.register_native(class, "<init>", "(Ljava/util/Map;)V", map_init_from);
    shared.register_native(class, "size", "()I", hash_map_size);
    shared.register_native(class, "isEmpty", "()Z", hash_map_is_empty);
    shared.register_native(
        class,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_put,
    );
    shared.register_native(
        class,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_get,
    );
    shared.register_native(
        class,
        "getOrDefault",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_get_or_default,
    );
    shared.register_native(
        class,
        "containsKey",
        "(Ljava/lang/Object;)Z",
        hash_map_contains_key,
    );
    shared.register_native(
        class,
        "containsValue",
        "(Ljava/lang/Object;)Z",
        hash_map_contains_value,
    );
    shared.register_native(
        class,
        "remove",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_remove,
    );
    shared.register_native(
        class,
        "remove",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        hash_map_remove_pair,
    );
    shared.register_native(class, "clear", "()V", hash_map_clear);
    shared.register_native(class, "putAll", "(Ljava/util/Map;)V", hash_map_put_all);
    shared.register_native(
        class,
        "putIfAbsent",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        hash_map_put_if_absent,
    );
    shared.register_native(class, "keySet", "()Ljava/util/Set;", hash_map_key_set);
    shared.register_native(class, "entrySet", "()Ljava/util/Set;", hash_map_entry_set);
    shared.register_native(class, "values", "()Ljava/util/Collection;", hash_map_values);
}

/// Register the `java.util.Map$Entry` surface.
fn register_map_entry_natives(shared: &Shared) {
    shared.register_native(
        MAP_ENTRY_CLASS,
        "getKey",
        "()Ljava/lang/Object;",
        entry_get_key,
    );
    shared.register_native(
        MAP_ENTRY_CLASS,
        "getValue",
        "()Ljava/lang/Object;",
        entry_get_value,
    );
    shared.register_native(
        MAP_ENTRY_CLASS,
        "setValue",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        entry_set_value,
    );
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
pub(crate) fn values_equal(heap: &Heap, left: Value, right: Value) -> Result<bool, HeapError> {
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

/// `Map.<init>()V`: the empty map `new` already allocated.
fn map_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let heap = shared.heap();
    entries(&heap, receiver, "Map.<init>")?;
    Ok(None)
}

/// `Map.<init>(I)V`: a capacity hint, not a stored size.
fn map_init_capacity(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "Map.<init>")?;
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "Map.<init>: negative capacity {capacity}"
        )));
    }
    let heap = shared.heap();
    entries(&heap, receiver, "Map.<init>")?;
    Ok(None)
}

/// `Map.<init>(IF)V`: a capacity and load-factor hint.
fn map_init_threshold(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let capacity = int_argument(args, 0, "Map.<init>")?;
    if capacity < 0 {
        return Err(VmError::Native(format!(
            "Map.<init>: negative capacity {capacity}"
        )));
    }
    let heap = shared.heap();
    entries(&heap, receiver, "Map.<init>")?;
    Ok(None)
}

/// `Map.<init>(Map)V`: copy every entry of the source map.
fn map_init_from(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    put_all_entries(shared, receiver, args, "Map.<init>")
}

/// `Map.getOrDefault(Object, Object)`.
fn hash_map_get_or_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "Map.getOrDefault")?;
    let fallback = reference_argument(args, 1, "Map.getOrDefault")?;
    let found = {
        let heap = shared.heap();
        let entries = entries(&heap, receiver, "Map.getOrDefault")?;
        find_key(&heap, entries, Value::Ref(key))
            .map_err(heap_error)?
            .map(|position| entries[position].1)
    };
    Ok(Some(found.unwrap_or(Value::Ref(fallback))))
}

/// `Map.remove(Object, Object)`: remove only when the stored value matches.
fn hash_map_remove_pair(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "Map.remove")?;
    let value = reference_argument(args, 1, "Map.remove")?;
    let position = {
        let heap = shared.heap();
        let entries = entries(&heap, receiver, "Map.remove")?;
        let position = find_key(&heap, entries, Value::Ref(key)).map_err(heap_error)?;
        match position {
            Some(position) => {
                let (_, stored) = entries[position];
                if values_equal(&heap, stored, Value::Ref(value)).map_err(heap_error)? {
                    Some(position)
                } else {
                    None
                }
            }
            None => None,
        }
    };
    let removed = match position {
        Some(position) => {
            shared
                .heap()
                .map_mut(receiver)
                .map_err(heap_error)?
                .remove(position);
            true
        }
        None => false,
    };
    Ok(Some(Value::Int(i32::from(removed))))
}

/// `Map.putAll(Map)`: copy every entry of the argument map.
fn hash_map_put_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    put_all_entries(shared, receiver, args, "Map.putAll")
}

fn put_all_entries(
    shared: &Shared,
    receiver: ObjRef,
    args: &[Value],
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, method)?;
    let source_entries = {
        let heap = shared.heap();
        entries(&heap, source, method)?.to_vec()
    };
    for (key, value) in source_entries {
        let position = {
            let heap = shared.heap();
            let entries = entries(&heap, receiver, method)?;
            find_key(&heap, entries, key).map_err(heap_error)?
        };
        let mut heap = shared.heap();
        match position {
            Some(position) => {
                heap.map_mut(receiver).map_err(heap_error)?[position].1 = value;
            }
            None => heap
                .map_mut(receiver)
                .map_err(heap_error)?
                .push((key, value)),
        }
    }
    Ok(None)
}

/// `Map.putIfAbsent(Object, Object)`: insert unless the key is present.
fn hash_map_put_if_absent(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "Map.putIfAbsent")?;
    let value = reference_argument(args, 1, "Map.putIfAbsent")?;
    let mut heap = shared.heap();
    let position = {
        let entries = entries(&heap, receiver, "Map.putIfAbsent")?;
        find_key(&heap, entries, Value::Ref(key)).map_err(heap_error)?
    };
    match position {
        Some(position) => {
            let previous = entries(&heap, receiver, "Map.putIfAbsent")?[position].1;
            Ok(Some(previous))
        }
        None => {
            heap.map_mut(receiver)
                .map_err(heap_error)?
                .push((Value::Ref(key), Value::Ref(value)));
            Ok(Some(Value::Ref(ObjRef::NULL)))
        }
    }
}

/// `Map.keySet()`: a set of the current keys.
fn hash_map_key_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let keys = {
        let heap = shared.heap();
        entries(&heap, receiver, "Map.keySet")?
            .iter()
            .map(|(key, _)| *key)
            .collect::<Vec<_>>()
    };
    let set = new_set(shared, keys)?;
    Ok(Some(Value::Ref(set)))
}

/// `Map.values()`: a list of the current values.
fn hash_map_values(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let values = {
        let heap = shared.heap();
        entries(&heap, receiver, "Map.values")?
            .iter()
            .map(|(_, value)| *value)
            .collect::<Vec<_>>()
    };
    let list = new_list(shared, values)?;
    Ok(Some(Value::Ref(list)))
}

/// `Map.entrySet()`: a set of `Map.Entry` objects over a snapshot of the entries.
fn hash_map_entry_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let entries = {
        let heap = shared.heap();
        entries(&heap, receiver, "Map.entrySet")?.to_vec()
    };
    let mut values = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        let entry = shared.heap().allocate_map_entry(key, value);
        values.push(Value::Ref(entry));
    }
    let set = new_set(shared, values)?;
    Ok(Some(Value::Ref(set)))
}

/// `ConcurrentHashMap.newKeySet()`: an empty concurrent set.
fn concurrent_hash_map_new_key_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let set = new_set(shared, Vec::new())?;
    Ok(Some(Value::Ref(set)))
}

fn new_set(shared: &Shared, values: Vec<Value>) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(crate::collections::LINKED_HASH_SET_CLASS)?;
    let set = shared.heap().allocate_set(class);
    {
        let mut heap = shared.heap();
        *heap.set_mut(set).expect("a freshly allocated set is a set") = values;
    }
    Ok(set)
}

fn new_list(shared: &Shared, values: Vec<Value>) -> Result<ObjRef, VmError> {
    let class = shared.load_library_class(crate::collections::ARRAY_LIST_CLASS)?;
    let list = shared.heap().allocate_list(class);
    {
        let mut heap = shared.heap();
        *heap
            .list_mut(list)
            .expect("a freshly allocated list is a list") = values;
    }
    Ok(list)
}

/// `Map.Entry.getKey()`.
fn entry_get_key(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = {
        let heap = shared.heap();
        heap.map_entry(receiver).map_err(heap_error)?.key
    };
    Ok(Some(key))
}

/// `Map.Entry.getValue()`.
fn entry_get_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = {
        let heap = shared.heap();
        heap.map_entry(receiver).map_err(heap_error)?.value
    };
    Ok(Some(value))
}

/// `Map.Entry.setValue(Object)`: replace this entry's value and return the previous one.
///
/// The entry is a snapshot: updating it does not write back to the source map, which is
/// the documented M2.23 boundary.
fn entry_set_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "Map.Entry.setValue")?;
    let mut heap = shared.heap();
    let entry = heap
        .get_mut(receiver)
        .map_err(heap_error)
        .and_then(|object| match object {
            HeapObject::MapEntry(entry) => Ok(entry),
            _ => Err(VmError::Native(
                "Map.Entry.setValue receiver is not a Map.Entry".to_owned(),
            )),
        })?;
    let previous = entry.value;
    entry.value = Value::Ref(value);
    Ok(Some(previous))
}

fn properties_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    properties_get_default(
        shared,
        _thread,
        _loader,
        receiver,
        &[args[0], Value::Ref(ObjRef::NULL)],
    )
}

fn properties_get_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "Properties.getProperty")?;
    let fallback = args.get(1).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let found = {
        let heap = shared.heap();
        let entries = entries(&heap, receiver, "Properties.getProperty")?;
        find_key(&heap, entries, Value::Ref(key))
            .map_err(heap_error)?
            .map(|position| entries[position].1)
    };
    Ok(Some(found.unwrap_or(fallback)))
}

fn properties_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "Properties.setProperty")?;
    let value = reference_argument(args, 1, "Properties.setProperty")?;
    let mut heap = shared.heap();
    let position = {
        let entries = entries(&heap, receiver, "Properties.setProperty")?;
        find_key(&heap, entries, Value::Ref(key)).map_err(heap_error)?
    };
    match position {
        Some(position) => {
            let entries = heap.map_mut(receiver).map_err(heap_error)?;
            let previous = entries[position].1;
            entries[position].1 = Value::Ref(value);
            Ok(Some(previous))
        }
        None => {
            heap.map_mut(receiver)
                .map_err(heap_error)?
                .push((Value::Ref(key), Value::Ref(value)));
            Ok(Some(Value::Ref(ObjRef::NULL)))
        }
    }
}
