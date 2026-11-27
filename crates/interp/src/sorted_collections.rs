//! Sorted `TreeSet`/`TreeMap` behaviour and Map copy constructors (M4.5 residual, #104).
//!
//! The VM stores every set as an insertion-ordered `SetObject` and every map as an
//! insertion-ordered `MapObject`. `TreeSet`/`TreeMap` need a comparator (or natural
//! `Comparable` ordering), comparator-equality for membership and a sorted insertion point.
//! Comparing calls Java `compare`/`compareTo`, which re-enters the interpreter, so these
//! overrides live here and are installed after [`crate::collection_equality`].
//!
//! The comparator is side-table state on [`Shared`] keyed by the collection object, like the
//! other interpreter-side attachments; the roots walk keeps both the collection and the
//! comparator alive. A missing entry means natural ordering.
//!
//! Map copy constructors (`HashMap`, `LinkedHashMap`, `Hashtable`) copy through Java
//! `equals`; `TreeMap` copy constructors use the sorted insertion above and inherit the
//! comparator for the `SortedMap` form.

use std::cmp::Ordering;

use rava_vm::collections::TREE_SET_CLASS;
use rava_vm::maps::{HASH_MAP_CLASS, HASHTABLE_CLASS, LINKED_HASH_MAP_CLASS, TREE_MAP_CLASS};
use rava_vm::{HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::collection_equality::{interp_to_vm, objects_equal};
use crate::{InterpError, resolve_virtual, run_instance_method, runtime_class};

/// Install the sorted-collection and map-copy overrides.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        TREE_SET_CLASS,
        "<init>",
        "(Ljava/util/Comparator;)V",
        tree_set_init_comparator,
    );
    shared.register_native(
        TREE_SET_CLASS,
        "<init>",
        "(Ljava/util/Collection;)V",
        tree_set_init_collection,
    );
    shared.register_native(
        TREE_SET_CLASS,
        "<init>",
        "(Ljava/util/SortedSet;)V",
        tree_set_init_sorted,
    );
    shared.register_native(TREE_SET_CLASS, "add", "(Ljava/lang/Object;)Z", tree_set_add);
    shared.register_native(
        TREE_SET_CLASS,
        "first",
        "()Ljava/lang/Object;",
        tree_set_first,
    );
    shared.register_native(
        TREE_SET_CLASS,
        "last",
        "()Ljava/lang/Object;",
        tree_set_last,
    );
    shared.register_native(
        TREE_SET_CLASS,
        "comparator",
        "()Ljava/util/Comparator;",
        tree_set_comparator,
    );

    shared.register_native(
        TREE_MAP_CLASS,
        "<init>",
        "(Ljava/util/Comparator;)V",
        tree_map_init_comparator,
    );
    shared.register_native(
        TREE_MAP_CLASS,
        "<init>",
        "(Ljava/util/Map;)V",
        tree_map_init_map,
    );
    shared.register_native(
        TREE_MAP_CLASS,
        "<init>",
        "(Ljava/util/SortedMap;)V",
        tree_map_init_sorted,
    );
    shared.register_native(
        TREE_MAP_CLASS,
        "firstKey",
        "()Ljava/lang/Object;",
        tree_map_first_key,
    );
    shared.register_native(
        TREE_MAP_CLASS,
        "lastKey",
        "()Ljava/lang/Object;",
        tree_map_last_key,
    );
    shared.register_native(
        TREE_MAP_CLASS,
        "comparator",
        "()Ljava/util/Comparator;",
        tree_map_comparator,
    );
    shared.register_native(
        TREE_MAP_CLASS,
        "putAll",
        "(Ljava/util/Map;)V",
        tree_map_put_all,
    );

    // Map copy constructors: a Java `equals` copy that keeps the first key object and the
    // last value, as `put` does. `TreeMap(Map/SortedMap)` is handled above with the
    // comparator path.
    for class in [HASH_MAP_CLASS, LINKED_HASH_MAP_CLASS, HASHTABLE_CLASS] {
        shared.register_native(class, "<init>", "(Ljava/util/Map;)V", map_copy_constructor);
    }
}

/// Whether `receiver`'s runtime class is `TreeSet`.
pub(crate) fn is_tree_set(shared: &Shared, receiver: ObjRef) -> bool {
    runtime_name(shared, receiver).as_deref() == Some(TREE_SET_CLASS)
}

/// Whether `receiver`'s runtime class is `TreeMap`.
pub(crate) fn is_tree_map(shared: &Shared, receiver: ObjRef) -> bool {
    runtime_name(shared, receiver).as_deref() == Some(TREE_MAP_CLASS)
}

fn runtime_name(shared: &Shared, receiver: ObjRef) -> Option<String> {
    let class = shared.heap().object_class(receiver).ok()?;
    shared.entry(class).map(|entry| entry.name.clone())
}

/// Compare two values by the receiver's stored comparator, or natural ordering.
pub(crate) fn compare_for(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    left: Value,
    right: Value,
) -> Result<Ordering, VmError> {
    match shared.sorted_comparator(receiver) {
        Some(comparator) => {
            let result = invoke_comparator(shared, thread, comparator, left, right)?;
            Ok(result.cmp(&0))
        }
        None => natural_compare(shared, thread, left, right),
    }
}

fn invoke_comparator(
    shared: &Shared,
    thread: &ThreadState,
    comparator: ObjRef,
    left: Value,
    right: Value,
) -> Result<i32, VmError> {
    let args = vec![left, right];
    let lambda = match shared.heap().get(comparator) {
        Ok(HeapObject::Lambda(lambda)) => Some(lambda.clone()),
        _ => None,
    };
    if let Some(lambda) = lambda {
        let value =
            crate::lambda::invoke_sam(shared, thread, &lambda, args).map_err(interp_to_vm)?;
        return int_result(value, "Comparator.compare");
    }
    let class =
        runtime_class(shared, comparator).map_err(|error| VmError::Native(error.to_string()))?;
    let (entry, _) = resolve_virtual(
        shared,
        class,
        "compare",
        "(Ljava/lang/Object;Ljava/lang/Object;)I",
    )
    .map_err(interp_to_vm)?;
    let value = run_instance_method(
        shared,
        thread,
        entry.id,
        "compare",
        "(Ljava/lang/Object;Ljava/lang/Object;)I",
        comparator,
        args,
    )
    .map_err(interp_to_vm)?;
    int_result(value, "Comparator.compare")
}

fn natural_compare(
    shared: &Shared,
    thread: &ThreadState,
    left: Value,
    right: Value,
) -> Result<Ordering, VmError> {
    let Value::Ref(left_ref) = left else {
        return Err(np_exception(
            shared,
            "TreeSet/TreeMap compares a non-reference value",
        )?);
    };
    if left_ref.is_null() {
        return Err(np_exception(
            shared,
            "TreeSet/TreeMap does not accept a null key",
        )?);
    }
    let class =
        runtime_class(shared, left_ref).map_err(|error| VmError::Native(error.to_string()))?;
    let name = shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_default();
    let concrete = format!("(L{name};)I");
    // The concrete `compareTo` is the implemented one; the erased `Object` bridge in the
    // C1 stub has no `Code` and no native, so try the concrete descriptor first.
    for descriptor in [concrete.as_str(), "(Ljava/lang/Object;)I"] {
        match invoke_descriptor(
            shared,
            thread,
            left_ref,
            "compareTo",
            descriptor,
            vec![right],
        ) {
            Ok(Some(Value::Int(value))) => return Ok(value.cmp(&0)),
            Ok(other) => {
                return Err(VmError::Native(format!(
                    "compareTo returned {other:?}, expected an int"
                )));
            }
            Err(InterpError::MethodNotFound { .. }) => continue,
            Err(error) => return Err(interp_to_vm(error)),
        }
    }
    Err(VmError::Thrown(rava_vm::exceptions::new_throwable(
        shared,
        "java/lang/ClassCastException",
        format!("{name} is not Comparable"),
    )?))
}

fn invoke_descriptor(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    let class = runtime_class(shared, receiver)?;
    let (entry, _) = resolve_virtual(shared, class, name, descriptor)?;
    run_instance_method(shared, thread, entry.id, name, descriptor, receiver, args)
}

fn int_result(value: Option<Value>, method: &str) -> Result<i32, VmError> {
    match value {
        Some(Value::Int(value)) => Ok(value),
        other => Err(VmError::Native(format!(
            "{method} returned {other:?}, expected an int"
        ))),
    }
}

fn np_exception(shared: &Shared, message: &str) -> Result<VmError, VmError> {
    Ok(VmError::Thrown(rava_vm::exceptions::new_throwable(
        shared,
        "java/lang/NullPointerException",
        message.to_owned(),
    )?))
}

fn no_such_element(shared: &Shared, message: &str) -> Result<VmError, VmError> {
    Ok(VmError::Thrown(rava_vm::exceptions::new_throwable(
        shared,
        "java/util/NoSuchElementException",
        message.to_owned(),
    )?))
}

fn reference_argument(args: &[Value], position: usize, method: &str) -> Result<Value, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(Value::Ref(reference)),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its argument"))),
    }
}

fn set_values(shared: &Shared, receiver: ObjRef, method: &str) -> Result<Vec<Value>, VmError> {
    match shared.heap().get(receiver) {
        Ok(HeapObject::Set(set)) => Ok(set.elements.clone()),
        Ok(_) => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Set"
        ))),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

fn map_entries(
    shared: &Shared,
    receiver: ObjRef,
    method: &str,
) -> Result<Vec<(Value, Value)>, VmError> {
    match shared.heap().get(receiver) {
        Ok(HeapObject::Map(map)) => Ok(map.entries.clone()),
        Ok(_) => Err(VmError::Native(format!(
            "{method} receiver is not a java.util.Map"
        ))),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

fn collection_values(
    shared: &Shared,
    reference: ObjRef,
    method: &'static str,
) -> Result<Vec<Value>, VmError> {
    match shared.heap().get(reference) {
        Ok(HeapObject::List(list)) => Ok(list.elements.clone()),
        Ok(HeapObject::Set(set)) => Ok(set.elements.clone()),
        Ok(_) => Err(VmError::Native(format!(
            "{method} source is not a java.util.Collection"
        ))),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

fn replace_set(
    shared: &Shared,
    receiver: ObjRef,
    values: Vec<Value>,
    method: &str,
) -> Result<(), VmError> {
    let mut heap = shared.heap();
    let set = heap
        .set_mut(receiver)
        .map_err(|error| VmError::Native(format!("{method}: {error}")))?;
    set.clear();
    set.extend(values);
    Ok(())
}

/// Insert `value` into a sorted set, returning whether it was added.
pub(crate) fn set_insert(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    elements: &mut Vec<Value>,
    value: Value,
) -> Result<bool, VmError> {
    if elements.is_empty() {
        // The JDK's `TreeMap.put` type-checks the first key with `compare(key, key)`.
        compare_for(shared, thread, receiver, value, value)?;
        elements.push(value);
        return Ok(true);
    }
    for (index, existing) in elements.iter().enumerate() {
        match compare_for(shared, thread, receiver, value, *existing)? {
            Ordering::Less => {
                elements.insert(index, value);
                return Ok(true);
            }
            Ordering::Equal => return Ok(false),
            Ordering::Greater => {}
        }
    }
    elements.push(value);
    Ok(true)
}

/// Insert a `(key, value)` entry into a sorted map, replacing an equal key's value.
fn map_insert(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    entries: &mut Vec<(Value, Value)>,
    key: Value,
    value: Value,
) -> Result<Option<Value>, VmError> {
    if entries.is_empty() {
        compare_for(shared, thread, receiver, key, key)?;
        entries.push((key, value));
        return Ok(None);
    }
    for (index, (existing, _)) in entries.iter().enumerate() {
        match compare_for(shared, thread, receiver, key, *existing)? {
            Ordering::Less => {
                entries.insert(index, (key, value));
                return Ok(None);
            }
            Ordering::Equal => {
                return Ok(Some(std::mem::replace(&mut entries[index].1, value)));
            }
            Ordering::Greater => {}
        }
    }
    entries.push((key, value));
    Ok(None)
}

fn tree_set_init_comparator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    replace_set(shared, receiver, Vec::new(), "TreeSet.<init>")?;
    let comparator = match args.first().copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    shared.set_sorted_comparator(receiver, comparator);
    Ok(None)
}

fn tree_set_init_collection(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.clear_sorted_comparator(receiver);
    let source = reference_argument(args, 0, "TreeSet.<init>")?;
    let values = collection_values(shared, source_of(source), "TreeSet.<init>")?;
    let mut elements = Vec::new();
    for value in values {
        set_insert(shared, thread, receiver, &mut elements, value)?;
    }
    replace_set(shared, receiver, elements, "TreeSet.<init>")?;
    Ok(None)
}

fn tree_set_init_sorted(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "TreeSet.<init>")?;
    let source = source_of(source);
    shared.set_sorted_comparator(
        receiver,
        shared.sorted_comparator(source).unwrap_or(ObjRef::NULL),
    );
    let values = collection_values(shared, source, "TreeSet.<init>")?;
    let mut elements = Vec::new();
    for value in values {
        set_insert(shared, thread, receiver, &mut elements, value)?;
    }
    replace_set(shared, receiver, elements, "TreeSet.<init>")?;
    Ok(None)
}

fn source_of(value: Value) -> ObjRef {
    match value {
        Value::Ref(reference) => reference,
        _ => ObjRef::NULL,
    }
}

fn tree_set_add(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = reference_argument(args, 0, "TreeSet.add")?;
    let mut elements = set_values(shared, receiver, "TreeSet.add")?;
    let changed = set_insert(shared, thread, receiver, &mut elements, value)?;
    if changed {
        replace_set(shared, receiver, elements, "TreeSet.add")?;
    }
    Ok(Some(Value::Int(i32::from(changed))))
}

fn tree_set_first(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match set_values(shared, receiver, "TreeSet.first")?
        .first()
        .copied()
    {
        Some(value) => Ok(Some(value)),
        None => Err(no_such_element(shared, "TreeSet.first")?),
    }
}

fn tree_set_last(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match set_values(shared, receiver, "TreeSet.last")?
        .last()
        .copied()
    {
        Some(value) => Ok(Some(value)),
        None => Err(no_such_element(shared, "TreeSet.last")?),
    }
}

fn tree_set_comparator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.sorted_comparator(receiver).unwrap_or(ObjRef::NULL),
    )))
}

fn tree_map_init_comparator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared
        .heap()
        .map_mut(receiver)
        .map_err(|error| VmError::Native(format!("TreeMap.<init>: {error}")))?
        .clear();
    let comparator = match args.first().copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    shared.set_sorted_comparator(receiver, comparator);
    Ok(None)
}

fn tree_map_init_map(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared.clear_sorted_comparator(receiver);
    let source = reference_argument(args, 0, "TreeMap.<init>")?;
    copy_map_sorted(shared, thread, receiver, source_of(source))
}

fn tree_map_init_sorted(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "TreeMap.<init>")?;
    let source = source_of(source);
    shared.set_sorted_comparator(
        receiver,
        shared.sorted_comparator(source).unwrap_or(ObjRef::NULL),
    );
    copy_map_sorted(shared, thread, receiver, source)
}

fn copy_map_sorted(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    source: ObjRef,
) -> Result<Option<Value>, VmError> {
    let source_entries = map_entries(shared, source, "TreeMap.<init>")?;
    let mut entries = Vec::new();
    for (key, value) in source_entries {
        map_insert(shared, thread, receiver, &mut entries, key, value)?;
    }
    let mut heap = shared.heap();
    let target = heap
        .map_mut(receiver)
        .map_err(|error| VmError::Native(format!("TreeMap.<init>: {error}")))?;
    target.clear();
    target.extend(entries);
    Ok(None)
}

fn tree_map_put_all(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "TreeMap.putAll")?;
    let source_entries = map_entries(shared, source_of(source), "TreeMap.putAll")?;
    let mut entries = map_entries(shared, receiver, "TreeMap.putAll")?;
    for (key, value) in source_entries {
        map_insert(shared, thread, receiver, &mut entries, key, value)?;
    }
    let mut heap = shared.heap();
    let target = heap
        .map_mut(receiver)
        .map_err(|error| VmError::Native(format!("TreeMap.putAll: {error}")))?;
    target.clear();
    target.extend(entries);
    Ok(None)
}

fn tree_map_first_key(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match map_entries(shared, receiver, "TreeMap.firstKey")?
        .first()
        .copied()
    {
        Some((key, _)) => Ok(Some(key)),
        None => Err(no_such_element(shared, "TreeMap.firstKey")?),
    }
}

fn tree_map_last_key(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match map_entries(shared, receiver, "TreeMap.lastKey")?
        .last()
        .copied()
    {
        Some((key, _)) => Ok(Some(key)),
        None => Err(no_such_element(shared, "TreeMap.lastKey")?),
    }
}

fn tree_map_comparator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.sorted_comparator(receiver).unwrap_or(ObjRef::NULL),
    )))
}

fn map_copy_constructor(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "Map.<init>")?;
    let source_entries = map_entries(shared, source_of(source), "Map.<init>")?;
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for (key, value) in source_entries {
        let mut found = None;
        for (index, (existing, _)) in entries.iter().enumerate() {
            if objects_equal(shared, thread, key, *existing).map_err(interp_to_vm)? {
                found = Some(index);
                break;
            }
        }
        match found {
            Some(index) => entries[index].1 = value,
            None => entries.push((key, value)),
        }
    }
    let mut heap = shared.heap();
    let target = heap
        .map_mut(receiver)
        .map_err(|error| VmError::Native(format!("Map.<init>: {error}")))?;
    target.clear();
    target.extend(entries);
    Ok(None)
}

/// The sorted insertion point for a key absent from a `TreeMap`.
pub(crate) fn map_insert_position(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    entries: &[(Value, Value)],
    key: Value,
) -> Result<usize, VmError> {
    if entries.is_empty() {
        // The empty-map branch still type-checks a natural key (null/non-Comparable).
        compare_for(shared, thread, receiver, key, key)?;
        return Ok(0);
    }
    for (index, (existing, _)) in entries.iter().enumerate() {
        if compare_for(shared, thread, receiver, key, *existing)? == Ordering::Less {
            return Ok(index);
        }
    }
    Ok(entries.len())
}
