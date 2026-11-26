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

use rava_vm::collections::{
    ARRAY_DEQUE_CLASS, ARRAY_LIST_CLASS, CONCURRENT_LINKED_DEQUE_CLASS,
    CONCURRENT_LINKED_QUEUE_CLASS, COPY_ON_WRITE_ARRAY_LIST_CLASS, COPY_ON_WRITE_ARRAY_SET_CLASS,
    HASH_SET_CLASS, LINKED_HASH_SET_CLASS, LINKED_LIST_CLASS, TREE_SET_CLASS,
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
    for (key, value) in &left {
        let mut matched = false;
        for (other_key, other_value) in &right {
            let keys_equal = match objects_equal(shared, thread, *key, *other_key) {
                Ok(equal) => equal,
                Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
                Err(error) => return Err(VmError::Native(error.to_string())),
            };
            if !keys_equal {
                continue;
            }
            let values_equal = match objects_equal(shared, thread, *value, *other_value) {
                Ok(equal) => equal,
                Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
                Err(error) => return Err(VmError::Native(error.to_string())),
            };
            if values_equal {
                matched = true;
                break;
            }
        }
        if !matched {
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
