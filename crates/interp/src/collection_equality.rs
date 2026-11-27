//! Java `equals` for collection membership (M4.5).
//!
//! The VM's collection natives in `rava-vm` compare members with reference identity for
//! every value that is not a `String` or a boxed number. That is not the JDK contract:
//! `List.contains`, `indexOf`, `remove(Object)` and the `Set` membership methods call
//! `Object.equals`. Spring's `MutablePropertySources.replace` is the startup path that
//! exposes it: it looks up a `PropertySource` by a name-comparison stub, and identity
//! equality makes a present source look absent.
//!
//! Dispatching a Java `equals` re-enters the interpreter, which `rava-vm` may not do, so
//! these overrides live here and are installed by the runner after
//! [`rava_vm::library::register_all`], exactly as the M4.2 `ServiceLoader` iterator
//! overrides are. Each function snapshots the container's elements first and drops the
//! heap lock before calling Java, so a Java `equals` implementation may touch the heap.
//!
//! A `null` probe is the JDK's `Objects.equals(null, element)` case: null matches null and
//! nothing else. A non-null probe calls the probe's own `equals(element)`, the receiver
//! order the JDK collections use.
//!
//! Map key and value operations follow the same rule for the listed operations; `put`
//! keeps the stored key object when an equal key arrives, and `IdentityHashMap` is the
//! exception (reference identity, including a `Map`-typed call, which checks the receiver's
//! runtime class). The set copy constructors and the `Collection` bulk operations use the
//! same Java-`equals` rule; map copy constructors still copy with the `rava-vm` rule.

use rava_vm::collections::{
    ARRAY_DEQUE_CLASS, ARRAY_LIST_CLASS, CONCURRENT_LINKED_DEQUE_CLASS,
    CONCURRENT_LINKED_QUEUE_CLASS, COPY_ON_WRITE_ARRAY_LIST_CLASS, COPY_ON_WRITE_ARRAY_SET_CLASS,
    HASH_SET_CLASS, LINKED_HASH_SET_CLASS, LINKED_LIST_CLASS, TREE_SET_CLASS,
};
use rava_vm::maps::{
    CONCURRENT_HASH_MAP_CLASS, ENUM_MAP_CLASS, HASH_MAP_CLASS, HASHTABLE_CLASS,
    IDENTITY_HASH_MAP_CLASS, LINKED_HASH_MAP_CLASS, PROPERTIES_CLASS, TREE_MAP_CLASS,
    WEAK_HASH_MAP_CLASS,
};
use rava_vm::{HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, resolve_virtual, run_instance_method, runtime_class};

/// The list classes and interfaces whose element search calls Java `equals`.
const LIST_TYPES: [&str; 10] = [
    ARRAY_LIST_CLASS,
    LINKED_LIST_CLASS,
    COPY_ON_WRITE_ARRAY_LIST_CLASS,
    ARRAY_DEQUE_CLASS,
    CONCURRENT_LINKED_DEQUE_CLASS,
    CONCURRENT_LINKED_QUEUE_CLASS,
    "java/util/List",
    "java/util/AbstractList",
    "java/util/AbstractCollection",
    "java/util/Collection",
];

/// The set classes and interfaces whose element search calls Java `equals`.
const SET_TYPES: [&str; 8] = [
    HASH_SET_CLASS,
    LINKED_HASH_SET_CLASS,
    TREE_SET_CLASS,
    COPY_ON_WRITE_ARRAY_SET_CLASS,
    "java/util/Set",
    "java/util/AbstractSet",
    "java/util/AbstractCollection",
    "java/util/Collection",
];

/// The map classes whose key and value comparisons call Java `equals`. `IdentityHashMap`
/// is excluded: its JDK contract is reference identity, which is the VM's default; a call
/// through the `Map` interface still checks the receiver's runtime class below.
const MAP_TYPES: [&str; 10] = [
    HASH_MAP_CLASS,
    LINKED_HASH_MAP_CLASS,
    TREE_MAP_CLASS,
    WEAK_HASH_MAP_CLASS,
    CONCURRENT_HASH_MAP_CLASS,
    HASHTABLE_CLASS,
    PROPERTIES_CLASS,
    ENUM_MAP_CLASS,
    "java/util/Map",
    "java/util/AbstractMap",
];

/// Install the `equals`-based membership overrides.
pub fn register_natives(shared: &Shared) {
    for class in LIST_TYPES {
        shared.register_native(
            class,
            "contains",
            "(Ljava/lang/Object;)Z",
            collection_contains,
        );
        shared.register_native(class, "remove", "(Ljava/lang/Object;)Z", collection_remove);
        shared.register_native(class, "indexOf", "(Ljava/lang/Object;)I", list_index_of);
        shared.register_native(
            class,
            "lastIndexOf",
            "(Ljava/lang/Object;)I",
            list_last_index_of,
        );
    }
    for class in SET_TYPES {
        shared.register_native(
            class,
            "contains",
            "(Ljava/lang/Object;)Z",
            collection_contains,
        );
        shared.register_native(class, "remove", "(Ljava/lang/Object;)Z", collection_remove);
    }
    shared.register_native(
        COPY_ON_WRITE_ARRAY_LIST_CLASS,
        "addIfAbsent",
        "(Ljava/lang/Object;)Z",
        list_add_if_absent,
    );
    // A set copy constructor dedupes with Java `equals`; the VM's `set_init` uses its own
    // rule. Register on every set class that declares the `Collection` constructor.
    for class in [
        HASH_SET_CLASS,
        LINKED_HASH_SET_CLASS,
        TREE_SET_CLASS,
        COPY_ON_WRITE_ARRAY_SET_CLASS,
    ] {
        shared.register_native(
            class,
            "<init>",
            "(Ljava/util/Collection;)V",
            set_copy_constructor,
        );
    }
    // The VM implements the bulk operations with `values_equal`; the JDK uses the
    // collection's element `equals`. Register at every declaration site a virtual call can
    // resolve to, for both container shapes.
    for class in LIST_TYPES.into_iter().chain(SET_TYPES) {
        shared.register_native(
            class,
            "addAll",
            "(Ljava/util/Collection;)Z",
            collection_add_all,
        );
        shared.register_native(
            class,
            "containsAll",
            "(Ljava/util/Collection;)Z",
            collection_contains_all,
        );
        shared.register_native(
            class,
            "removeAll",
            "(Ljava/util/Collection;)Z",
            collection_remove_all,
        );
        shared.register_native(
            class,
            "retainAll",
            "(Ljava/util/Collection;)Z",
            collection_retain_all,
        );
    }
    // `AbstractSet.equals` is the JDK's size + contained-elements check; the C1 stub has no
    // `Code`, and the operation needs Java `equals` on the elements.
    for class in ["java/util/AbstractSet", "java/util/Set"] {
        shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", set_equals);
    }
    for class in [
        "java/util/AbstractList",
        "java/util/List",
        ARRAY_LIST_CLASS,
        LINKED_LIST_CLASS,
        COPY_ON_WRITE_ARRAY_LIST_CLASS,
        ARRAY_DEQUE_CLASS,
    ] {
        shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", list_equals);
    }
    for class in [
        "java/util/AbstractMap",
        "java/util/Map",
        "java/util/HashMap",
        "java/util/LinkedHashMap",
        "java/util/TreeMap",
        "java/util/Hashtable",
        "java/util/Properties",
        "java/util/IdentityHashMap",
        "java/util/WeakHashMap",
        "java/util/EnumMap",
        "java/util/concurrent/ConcurrentHashMap",
    ] {
        shared.register_native(class, "equals", "(Ljava/lang/Object;)Z", map_equals);
    }
    for class in MAP_TYPES {
        register_map_membership(shared, class);
    }
}

/// Install the `equals`-based key and value operations for one map class or interface.
fn register_map_membership(shared: &Shared, class: &'static str) {
    for (name, descriptor, function) in [
        (
            "get",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            map_get as rava_vm::NativeFn,
        ),
        (
            "getOrDefault",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            map_get_or_default,
        ),
        ("containsKey", "(Ljava/lang/Object;)Z", map_contains_key),
        ("containsValue", "(Ljava/lang/Object;)Z", map_contains_value),
        (
            "put",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            map_put,
        ),
        (
            "putIfAbsent",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            map_put_if_absent,
        ),
        (
            "computeIfAbsent",
            "(Ljava/lang/Object;Ljava/util/function/Function;)Ljava/lang/Object;",
            map_compute_if_absent,
        ),
        (
            "remove",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            map_remove,
        ),
        (
            "remove",
            "(Ljava/lang/Object;Ljava/lang/Object;)Z",
            map_remove_pair,
        ),
    ] {
        shared.register_native(class, name, descriptor, function);
    }
}

/// A map receiver's entries `(key, value)`, cloned so no heap lock is held across Java.
fn map_entries(
    shared: &Shared,
    receiver: ObjRef,
    method: &'static str,
) -> Result<Vec<(Value, Value)>, VmError> {
    let heap = shared.heap();
    match heap.get(receiver) {
        Ok(HeapObject::Map(map)) => Ok(map.entries.clone()),
        Ok(_) => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Map"
        ))),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

/// Whether the receiver's runtime class is `IdentityHashMap` (identity comparisons).
fn map_uses_identity(shared: &Shared, receiver: ObjRef) -> Result<bool, VmError> {
    let class = shared
        .heap()
        .object_class(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?;
    let name = shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_default();
    Ok(name == IDENTITY_HASH_MAP_CLASS)
}

/// The JDK's key/value comparison for this receiver: Java `equals`, or identity.
fn map_values_equal(
    shared: &Shared,
    thread: &ThreadState,
    identity: bool,
    probe: Value,
    stored: Value,
) -> Result<bool, VmError> {
    if identity {
        return Ok(probe == stored);
    }
    objects_equal(shared, thread, probe, stored).map_err(interp_to_vm)
}

fn interp_to_vm(error: InterpError) -> VmError {
    match error {
        InterpError::Vm(error) => error,
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(other.to_string()),
    }
}

/// The position of `probe` in `entries`, by the receiver's comparison rule.
fn map_position(
    shared: &Shared,
    thread: &ThreadState,
    identity: bool,
    entries: &[(Value, Value)],
    probe: Value,
) -> Result<Option<usize>, VmError> {
    for (position, (key, _)) in entries.iter().enumerate() {
        if map_values_equal(shared, thread, identity, probe, *key)? {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

/// Record a `LinkedHashMap` access-order touch after a successful read or update.
fn touch_map_entry(shared: &Shared, receiver: ObjRef, position: usize) -> Result<(), VmError> {
    let mut heap = shared.heap();
    if !heap
        .map_access_order(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?
    {
        return Ok(());
    }
    let entries = heap
        .map_mut(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?;
    if position < entries.len() {
        let entry = entries.remove(position);
        entries.push(entry);
    }
    Ok(())
}

fn map_reference_argument(
    args: &[Value],
    position: usize,
    method: &str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn map_get(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.get")?;
    let entries = map_entries(shared, receiver, "Map.get")?;
    let identity = map_uses_identity(shared, receiver)?;
    match map_position(shared, thread, identity, &entries, Value::Ref(key))? {
        Some(position) => {
            let value = entries[position].1;
            touch_map_entry(shared, receiver, position)?;
            Ok(Some(value))
        }
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

fn map_contains_key(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.containsKey")?;
    let entries = map_entries(shared, receiver, "Map.containsKey")?;
    let identity = map_uses_identity(shared, receiver)?;
    let found = map_position(shared, thread, identity, &entries, Value::Ref(key))?.is_some();
    Ok(Some(Value::Int(i32::from(found))))
}

fn map_contains_value(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = map_reference_argument(args, 0, "Map.containsValue")?;
    let entries = map_entries(shared, receiver, "Map.containsValue")?;
    let identity = map_uses_identity(shared, receiver)?;
    for (_, stored) in &entries {
        if map_values_equal(shared, thread, identity, Value::Ref(value), *stored)? {
            return Ok(Some(Value::Int(1)));
        }
    }
    Ok(Some(Value::Int(0)))
}

fn map_put(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.put")?;
    let value = map_reference_argument(args, 1, "Map.put")?;
    let entries = map_entries(shared, receiver, "Map.put")?;
    let identity = map_uses_identity(shared, receiver)?;
    let position = map_position(shared, thread, identity, &entries, Value::Ref(key))?;
    let previous = {
        let mut heap = shared.heap();
        let entries = heap
            .map_mut(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?;
        match position {
            Some(position) => std::mem::replace(&mut entries[position].1, Value::Ref(value)),
            None => {
                entries.push((Value::Ref(key), Value::Ref(value)));
                Value::Ref(ObjRef::NULL)
            }
        }
    };
    if let Some(position) = position {
        // An update of an existing key is an access for an access-ordered map.
        touch_map_entry(shared, receiver, position)?;
    }
    Ok(Some(previous))
}

fn map_put_if_absent(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.putIfAbsent")?;
    let value = map_reference_argument(args, 1, "Map.putIfAbsent")?;
    let entries = map_entries(shared, receiver, "Map.putIfAbsent")?;
    let identity = map_uses_identity(shared, receiver)?;
    let position = map_position(shared, thread, identity, &entries, Value::Ref(key))?;
    match position {
        Some(position) => {
            let existing = entries[position].1;
            if existing != Value::Ref(ObjRef::NULL) {
                touch_map_entry(shared, receiver, position)?;
                return Ok(Some(existing));
            }
            // A mapping to null is replaced, as `putIfAbsent` does on the JDK.
            {
                let mut heap = shared.heap();
                let entries = heap
                    .map_mut(receiver)
                    .map_err(|error| VmError::Native(error.to_string()))?;
                entries[position].1 = Value::Ref(value);
            }
            touch_map_entry(shared, receiver, position)?;
            Ok(Some(Value::Ref(ObjRef::NULL)))
        }
        None => {
            shared
                .heap()
                .map_mut(receiver)
                .map_err(|error| VmError::Native(error.to_string()))?
                .push((Value::Ref(key), Value::Ref(value)));
            Ok(Some(Value::Ref(ObjRef::NULL)))
        }
    }
}

fn map_get_or_default(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.getOrDefault")?;
    let fallback = map_reference_argument(args, 1, "Map.getOrDefault")?;
    let entries = map_entries(shared, receiver, "Map.getOrDefault")?;
    let identity = map_uses_identity(shared, receiver)?;
    match map_position(shared, thread, identity, &entries, Value::Ref(key))? {
        Some(position) => {
            let value = entries[position].1;
            touch_map_entry(shared, receiver, position)?;
            Ok(Some(value))
        }
        None => Ok(Some(Value::Ref(fallback))),
    }
}

fn map_compute_if_absent(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.computeIfAbsent")?;
    let function = map_reference_argument(args, 1, "Map.computeIfAbsent")?;
    let entries = map_entries(shared, receiver, "Map.computeIfAbsent")?;
    let identity = map_uses_identity(shared, receiver)?;
    let position = map_position(shared, thread, identity, &entries, Value::Ref(key))?;
    if let Some(position) = position {
        let existing = entries[position].1;
        if existing != Value::Ref(ObjRef::NULL) {
            touch_map_entry(shared, receiver, position)?;
            return Ok(Some(existing));
        }
    }
    // The JDK runs the function before storing, and a null result stores nothing.
    let computed = call_function(shared, thread, function, Value::Ref(key))?;
    let value = match computed {
        Some(value) => value,
        None => return Ok(Some(Value::Ref(ObjRef::NULL))),
    };
    if value == Value::Ref(ObjRef::NULL) {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    }
    let mut heap = shared.heap();
    let entries = heap
        .map_mut(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?;
    match position {
        Some(position) => entries[position].1 = value,
        None => entries.push((Value::Ref(key), value)),
    }
    Ok(Some(value))
}

fn call_function(
    shared: &Shared,
    thread: &ThreadState,
    function: ObjRef,
    argument: Value,
) -> Result<Option<Value>, VmError> {
    if function.is_null() {
        return Err(VmError::Native(
            "Map.computeIfAbsent received a null Function".to_owned(),
        ));
    }
    // A lambda call site is a `LambdaObject`, not a class with a `Code` body; the
    // interpreter's SAM invocation is the only way to run it.
    let lambda = {
        let heap = shared.heap();
        match heap.get(function) {
            Ok(HeapObject::Lambda(lambda)) => Some(lambda.clone()),
            _ => None,
        }
    };
    if let Some(lambda) = lambda {
        return crate::lambda::invoke_sam(shared, thread, &lambda, vec![argument])
            .map_err(interp_to_vm);
    }
    let class =
        runtime_class(shared, function).map_err(|error| VmError::Native(error.to_string()))?;
    let (entry, _) = resolve_virtual(
        shared,
        class,
        "apply",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
    )
    .map_err(|error| VmError::Native(error.to_string()))?;
    run_instance_method(
        shared,
        thread,
        entry.id,
        "apply",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        function,
        vec![argument],
    )
    .map_err(interp_to_vm)
}

fn map_remove(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.remove")?;
    let entries = map_entries(shared, receiver, "Map.remove")?;
    let identity = map_uses_identity(shared, receiver)?;
    let Some(position) = map_position(shared, thread, identity, &entries, Value::Ref(key))? else {
        return Ok(Some(Value::Ref(ObjRef::NULL)));
    };
    let removed = shared
        .heap()
        .map_mut(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?
        .remove(position);
    Ok(Some(removed.1))
}

fn map_remove_pair(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = map_reference_argument(args, 0, "Map.remove")?;
    let value = map_reference_argument(args, 1, "Map.remove")?;
    let entries = map_entries(shared, receiver, "Map.remove")?;
    let identity = map_uses_identity(shared, receiver)?;
    let Some(position) = map_position(shared, thread, identity, &entries, Value::Ref(key))? else {
        return Ok(Some(Value::Int(0)));
    };
    if !map_values_equal(
        shared,
        thread,
        identity,
        Value::Ref(value),
        entries[position].1,
    )? {
        return Ok(Some(Value::Int(0)));
    }
    shared
        .heap()
        .map_mut(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?
        .remove(position);
    Ok(Some(Value::Int(1)))
}

/// Snapshot a list's elements, releasing the heap lock before any Java call.
fn list_elements(shared: &Shared, receiver: ObjRef, method: &str) -> Result<Vec<Value>, VmError> {
    let heap = shared.heap();
    match heap.get(receiver) {
        Ok(HeapObject::List(list)) => Ok(list.elements.clone()),
        Ok(_) => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Collection"
        ))),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

/// Whether two values are equal by the JDK's `Objects.equals` rule.
fn objects_equal(
    shared: &Shared,
    thread: &ThreadState,
    probe: Value,
    element: Value,
) -> Result<bool, InterpError> {
    if let (Value::Ref(left), Value::Ref(right)) = (probe, element) {
        if left.is_null() || right.is_null() {
            return Ok(left.is_null() && right.is_null());
        }
        let class = runtime_class(shared, left)?;
        let (entry, _) = resolve_virtual(shared, class, "equals", "(Ljava/lang/Object;)Z")?;
        let result = run_instance_method(
            shared,
            thread,
            entry.id,
            "equals",
            "(Ljava/lang/Object;)Z",
            left,
            vec![Value::Ref(right)],
        )?;
        return Ok(matches!(result, Some(Value::Int(value)) if value != 0));
    }
    Ok(probe == element)
}

fn contains(
    shared: &Shared,
    thread: &ThreadState,
    elements: &[Value],
    probe: Value,
) -> Result<bool, InterpError> {
    for element in elements {
        if objects_equal(shared, thread, probe, *element)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn collection_contains(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let probe = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let elements = match shared.heap().get(receiver) {
        Ok(HeapObject::List(list)) => list.elements.clone(),
        Ok(HeapObject::Set(set)) => set.elements.clone(),
        Ok(_) => {
            return Err(VmError::Native(
                "contains receiver is not a java.util.Collection".to_owned(),
            ));
        }
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let found = match contains(shared, thread, &elements, probe) {
        Ok(found) => found,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    Ok(Some(Value::Int(i32::from(found))))
}

fn collection_remove(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let probe = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let (elements, is_list) = match shared.heap().get(receiver) {
        Ok(HeapObject::List(list)) => (list.elements.clone(), true),
        Ok(HeapObject::Set(set)) => (set.elements.clone(), false),
        Ok(_) => {
            return Err(VmError::Native(
                "remove receiver is not a java.util.Collection".to_owned(),
            ));
        }
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let mut position = None;
    for (index, element) in elements.iter().enumerate() {
        match objects_equal(shared, thread, probe, *element) {
            Ok(true) => {
                position = Some(index);
                break;
            }
            Ok(false) => {}
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        }
    }
    let Some(position) = position else {
        return Ok(Some(Value::Int(0)));
    };
    if is_list {
        shared
            .heap()
            .list_mut(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
            .remove(position);
    } else {
        shared
            .heap()
            .set_mut(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
            .remove(position);
    }
    Ok(Some(Value::Int(1)))
}

fn index_of(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    args: &[Value],
    from_end: bool,
    method: &'static str,
) -> Result<Option<Value>, VmError> {
    let probe = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let elements = list_elements(shared, receiver, method)?;
    let mut found = -1i32;
    let indices: Vec<usize> = if from_end {
        (0..elements.len()).rev().collect()
    } else {
        (0..elements.len()).collect()
    };
    for index in indices {
        match objects_equal(shared, thread, probe, elements[index]) {
            Ok(true) => {
                found = i32::try_from(index).unwrap_or(i32::MAX);
                break;
            }
            Ok(false) => {}
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        }
    }
    Ok(Some(Value::Int(found)))
}

fn list_index_of(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    index_of(shared, thread, receiver, args, false, "List.indexOf")
}

fn list_last_index_of(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    index_of(shared, thread, receiver, args, true, "List.lastIndexOf")
}

fn list_add_if_absent(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let elements = list_elements(shared, receiver, "CopyOnWriteArrayList.addIfAbsent")?;
    match contains(shared, thread, &elements, value) {
        Ok(true) => return Ok(Some(Value::Int(0))),
        Ok(false) => {}
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    }
    shared
        .heap()
        .list_mut(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?
        .push(value);
    Ok(Some(Value::Int(1)))
}

/// The elements of a list or set receiver, cloned so no heap lock is held while Java
/// `equals` runs.
fn collection_values(
    shared: &Shared,
    reference: ObjRef,
    method: &'static str,
) -> Result<Vec<Value>, VmError> {
    if reference.is_null() {
        return Err(VmError::Native(format!("{method}: null collection")));
    }
    match shared.heap().get(reference) {
        Ok(HeapObject::List(list)) => Ok(list.elements.clone()),
        Ok(HeapObject::Set(set)) => Ok(set.elements.clone()),
        Ok(_) => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Collection"
        ))),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

/// Replace a list's or set's elements, keeping the receiver's identity.
fn replace_collection(
    shared: &Shared,
    receiver: ObjRef,
    values: Vec<Value>,
    method: &'static str,
) -> Result<(), VmError> {
    let mut heap = shared.heap();
    match heap.get(receiver) {
        Ok(HeapObject::List(_)) => {
            let list = heap
                .list_mut(receiver)
                .map_err(|error| VmError::Native(error.to_string()))?;
            list.clear();
            list.extend(values);
            Ok(())
        }
        Ok(HeapObject::Set(_)) => {
            let set = heap
                .set_mut(receiver)
                .map_err(|error| VmError::Native(error.to_string()))?;
            set.clear();
            set.extend(values);
            Ok(())
        }
        Ok(_) => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Collection"
        ))),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

/// `Set(Collection)`: clear the receiver and add each source element only when no element
/// already added is Java-`equals` to it. First-occurrence order is kept; `TreeSet` stays
/// insertion-ordered (its documented boundary).
fn set_copy_constructor(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "Set.<init> expects a non-null Collection".to_owned(),
            ));
        }
    };
    let values = collection_values(shared, source, "Set.<init>")?;
    let mut unique = Vec::with_capacity(values.len());
    for value in values {
        let present = contains(shared, thread, &unique, value).map_err(interp_to_vm)?;
        if !present {
            unique.push(value);
        }
    }
    replace_collection(shared, receiver, unique, "Set.<init>")?;
    Ok(None)
}

/// `Collection.addAll(Collection)`: append for a list; add only absent elements for a set,
/// both with Java `equals`. Returns whether the receiver changed.
fn collection_add_all(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (source, is_set) = collection_argument(shared, receiver, args, "Collection.addAll")?;
    let incoming = collection_values(shared, source, "Collection.addAll")?;
    let existing = collection_values(shared, receiver, "Collection.addAll")?;
    if is_set {
        let mut merged = existing.clone();
        for value in incoming {
            let present = contains(shared, thread, &merged, value).map_err(interp_to_vm)?;
            if !present {
                merged.push(value);
            }
        }
        let changed = merged.len() != existing.len();
        if changed {
            replace_collection(shared, receiver, merged, "Collection.addAll")?;
        }
        Ok(Some(Value::Int(i32::from(changed))))
    } else {
        // `List.addAll` appends every element, including duplicates of the receiver.
        let changed = !incoming.is_empty();
        if changed {
            let mut merged = existing;
            merged.extend(incoming);
            replace_collection(shared, receiver, merged, "Collection.addAll")?;
        }
        Ok(Some(Value::Int(i32::from(changed))))
    }
}

/// `Collection.containsAll(Collection)`: every source element is present, using the source
/// element as the `equals` probe (the JDK's `c.contains` direction).
fn collection_contains_all(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (source, _) = collection_argument(shared, receiver, args, "Collection.containsAll")?;
    let wanted = collection_values(shared, source, "Collection.containsAll")?;
    let elements = collection_values(shared, receiver, "Collection.containsAll")?;
    for probe in wanted {
        if !contains(shared, thread, &elements, probe).map_err(interp_to_vm)? {
            return Ok(Some(Value::Int(0)));
        }
    }
    Ok(Some(Value::Int(1)))
}

/// `Collection.removeAll(Collection)`: drop every receiver element the argument contains.
/// The JDK calls `c.contains(element)`, so the receiver element is the probe.
fn collection_remove_all(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (source, _) = collection_argument(shared, receiver, args, "Collection.removeAll")?;
    let source = collection_values(shared, source, "Collection.removeAll")?;
    let elements = collection_values(shared, receiver, "Collection.removeAll")?;
    let original = elements.len();
    let mut kept = Vec::with_capacity(original);
    for element in elements {
        if !contains(shared, thread, &source, element).map_err(interp_to_vm)? {
            kept.push(element);
        }
    }
    let changed = kept.len() != original;
    if changed {
        replace_collection(shared, receiver, kept, "Collection.removeAll")?;
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

/// `Collection.retainAll(Collection)`: keep only the receiver elements the argument
/// contains; the receiver element is the probe, as in `removeAll`.
fn collection_retain_all(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (source, _) = collection_argument(shared, receiver, args, "Collection.retainAll")?;
    let source = collection_values(shared, source, "Collection.retainAll")?;
    let elements = collection_values(shared, receiver, "Collection.retainAll")?;
    let original = elements.len();
    let mut kept = Vec::with_capacity(original);
    for element in elements {
        if contains(shared, thread, &source, element).map_err(interp_to_vm)? {
            kept.push(element);
        }
    }
    let changed = kept.len() != original;
    if changed {
        replace_collection(shared, receiver, kept, "Collection.retainAll")?;
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

/// The argument collection and whether the receiver is a set, for the bulk operations.
fn collection_argument(
    shared: &Shared,
    receiver: ObjRef,
    args: &[Value],
    method: &'static str,
) -> Result<(ObjRef, bool), VmError> {
    let source = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(format!(
                "{method} expects a non-null Collection"
            )));
        }
    };
    let is_set = matches!(shared.heap().get(receiver), Ok(HeapObject::Set(_)));
    Ok((source, is_set))
}

/// `Set.equals(Object)`: same size and the same elements.
fn set_equals(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(other)) = args.first().copied() else {
        return Ok(Some(Value::Int(0)));
    };
    if other == receiver {
        return Ok(Some(Value::Int(1)));
    }
    let (left, right) = {
        let heap = shared.heap();
        match (heap.get(receiver), heap.get(other)) {
            (Ok(HeapObject::Set(left)), Ok(HeapObject::Set(right))) => {
                (left.elements.clone(), right.elements.clone())
            }
            _ => return Ok(Some(Value::Int(0))),
        }
    };
    if left.len() != right.len() {
        return Ok(Some(Value::Int(0)));
    }
    for element in &left {
        let mut found = false;
        for candidate in &right {
            match objects_equal(shared, thread, *element, *candidate) {
                Ok(true) => {
                    found = true;
                    break;
                }
                Ok(false) => {}
                Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
                Err(error) => return Err(VmError::Native(error.to_string())),
            }
        }
        if !found {
            return Ok(Some(Value::Int(0)));
        }
    }
    Ok(Some(Value::Int(1)))
}

/// `Map.equals(Object)`: same size and the same key/value pairs, by Java `equals`.
fn map_equals(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(other)) = args.first().copied() else {
        return Ok(Some(Value::Int(0)));
    };
    if other == receiver {
        return Ok(Some(Value::Int(1)));
    }
    let (left, right) = {
        let heap = shared.heap();
        match (heap.get(receiver), heap.get(other)) {
            (Ok(HeapObject::Map(left)), Ok(HeapObject::Map(right))) => {
                (left.entries.clone(), right.entries.clone())
            }
            _ => return Ok(Some(Value::Int(0))),
        }
    };
    if left.len() != right.len() {
        return Ok(Some(Value::Int(0)));
    }
    // `IdentityHashMap.equals` is its own contract: the argument must be an
    // `IdentityHashMap`, and every key and value must be the same reference. It does not
    // fall back to `AbstractMap.equals`.
    if map_uses_identity(shared, receiver)? {
        if !map_uses_identity(shared, other)? {
            return Ok(Some(Value::Int(0)));
        }
        for (key, value) in &left {
            let matched = right
                .iter()
                .any(|(other_key, other_value)| other_key == key && other_value == value);
            if !matched {
                return Ok(Some(Value::Int(0)));
            }
        }
        return Ok(Some(Value::Int(1)));
    }
    // `AbstractMap.equals`: for each entry in this map, the argument's own `get` (with the
    // argument's equality rule) must find the same key, and the value's `equals` must
    // accept the stored value.
    let right_identity = map_uses_identity(shared, other)?;
    for (key, value) in &left {
        let Some(position) = map_position(shared, thread, right_identity, &right, *key)? else {
            return Ok(Some(Value::Int(0)));
        };
        let other_value = right[position].1;
        let values_equal = match value {
            Value::Ref(reference) if reference.is_null() => other_value == Value::Ref(ObjRef::NULL),
            _ => match objects_equal(shared, thread, *value, other_value) {
                Ok(equal) => equal,
                Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
                Err(error) => return Err(VmError::Native(error.to_string())),
            },
        };
        if !values_equal {
            return Ok(Some(Value::Int(0)));
        }
    }
    Ok(Some(Value::Int(1)))
}

/// `List.equals(Object)`: same size and the same elements in order.
fn list_equals(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(other)) = args.first().copied() else {
        return Ok(Some(Value::Int(0)));
    };
    if other == receiver {
        return Ok(Some(Value::Int(1)));
    }
    let (left, right) = {
        let heap = shared.heap();
        match (heap.get(receiver), heap.get(other)) {
            (Ok(HeapObject::List(left)), Ok(HeapObject::List(right))) => {
                (left.elements.clone(), right.elements.clone())
            }
            _ => return Ok(Some(Value::Int(0))),
        }
    };
    if left.len() != right.len() {
        return Ok(Some(Value::Int(0)));
    }
    for (left, right) in left.iter().zip(right.iter()) {
        match objects_equal(shared, thread, *left, *right) {
            Ok(true) => {}
            Ok(false) => return Ok(Some(Value::Int(0))),
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        }
    }
    Ok(Some(Value::Int(1)))
}
