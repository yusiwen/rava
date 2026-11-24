//! Callback-taking collection natives (M2.23).
//!
//! `Map.computeIfAbsent` is the first Java method the Spring startup path calls that must
//! run a Java callback from inside a native: the second argument is a
//! `java.util.function.Function` implemented by a lambda object. A `rava-vm` native cannot
//! re-enter the interpreter, so this module lives in `rava-interp` next to
//! [`crate::reflect_invoke`] and uses the same call machinery: a lambda runs through
//! [`crate::lambda::invoke_sam`], and any other object runs its own `apply` method through
//! [`crate::run_instance_method`].
//!
//! The registered surface is what the Spring startup path reached: `Map.computeIfAbsent`,
//! `Iterable.forEach`, `Arrays.sort(Object[], Comparator)`, `List.sort(Comparator)` and the
//! `Comparator.comparingInt`/`reversed` factories with their native lambda targets.
//! `compute`, `computeIfPresent`, `merge`, the remaining `Comparator` factories and the
//! stream operations still fail loudly as unimplemented methods rather than guessing.
//!
//! A key equality here takes the already-locked [`rava_vm::Heap`] rather than locking it,
//! because `computeIfAbsent` scans the entries while it holds the guard; locking again on
//! the same thread would deadlock.

use rava_vm::{
    ClassId, HeapObject, LambdaObject, LambdaTarget, LambdaTargetKind, LoaderId, ObjRef, Shared,
    ThreadState, Value, VmError,
};

use crate::{InterpError, lambda, run_instance_method};

/// The `Function.apply(Object)Object` descriptor.
const FUNCTION_APPLY: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";

/// The `Comparator.compare(Object, Object)int` descriptor.
const COMPARATOR_COMPARE: &str = "(Ljava/lang/Object;Ljava/lang/Object;)I";

/// The `computeIfAbsent` descriptor.
const COMPUTE_IF_ABSENT: &str =
    "(Ljava/lang/Object;Ljava/util/function/Function;)Ljava/lang/Object;";

/// `Comparator.compare(Object, Object)int`.
const COMPARATOR_INT_COMPARE: &str = "(Ljava/lang/Object;Ljava/lang/Object;)I";

/// The `Comparator.comparingInt` descriptor.
const COMPARING_INT: &str = "(Ljava/util/function/ToIntFunction;)Ljava/util/Comparator;";

/// The registry name of the native lambda target a `comparingInt` comparator calls.
const COMPARING_INT_TARGET_NAME: &str = "comparingInt$";

/// The native lambda target's descriptor: the captured extractor plus the two compared
/// values.
const COMPARING_INT_TARGET: &str =
    "(Ljava/util/function/ToIntFunction;Ljava/lang/Object;Ljava/lang/Object;)I";

/// `ToIntFunction.applyAsInt(Object)int`.
const TO_INT_APPLY_AS_INT: &str = "(Ljava/lang/Object;)I";

/// `Comparator.reversed()Comparator`.
const COMPARATOR_REVERSED: &str = "()Ljava/util/Comparator;";

/// The registry name of the native lambda target a reversed comparator calls.
const REVERSED_TARGET_NAME: &str = "reversed$";

/// The reversed target's descriptor: the wrapped comparator plus the two compared values.
const REVERSED_TARGET: &str = "(Ljava/util/Comparator;Ljava/lang/Object;Ljava/lang/Object;)I";

pub fn register_natives(shared: &Shared) {
    for class in [
        "java/util/concurrent/ConcurrentHashMap",
        "java/util/HashMap",
        "java/util/Map",
        "java/util/concurrent/ConcurrentMap",
        "java/util/concurrent/ConcurrentNavigableMap",
    ] {
        shared.register_native(
            class,
            "computeIfAbsent",
            COMPUTE_IF_ABSENT,
            compute_if_absent,
        );
    }
    shared.register_native(
        "java/util/Comparator",
        "comparingInt",
        COMPARING_INT,
        comparator_comparing_int,
    );
    shared.register_native(
        "java/util/Comparator",
        COMPARING_INT_TARGET_NAME,
        COMPARING_INT_TARGET,
        comparator_comparing_int_compare,
    );
    shared.register_native(
        "java/util/Comparator",
        "reversed",
        COMPARATOR_REVERSED,
        comparator_reversed,
    );
    shared.register_native(
        "java/util/Comparator",
        REVERSED_TARGET_NAME,
        REVERSED_TARGET,
        comparator_reversed_compare,
    );
    shared.register_native(
        "java/util/Arrays",
        "sort",
        "([Ljava/lang/Object;Ljava/util/Comparator;)V",
        arrays_sort_comparator,
    );
    for class in ["java/util/List", "java/util/ArrayList"] {
        shared.register_native(
            class,
            "sort",
            "(Ljava/util/Comparator;)V",
            list_sort_comparator,
        );
    }
    for class in [
        "java/util/ArrayList",
        "java/util/LinkedList",
        "java/util/List",
        "java/util/Collection",
        "java/lang/Iterable",
        "java/util/Set",
        "java/util/HashSet",
        "java/util/LinkedHashSet",
        "java/util/TreeSet",
        "java/util/ArrayDeque",
    ] {
        shared.register_native(
            class,
            "forEach",
            "(Ljava/util/function/Consumer;)V",
            collection_for_each,
        );
    }
}

fn heap_error(error: rava_vm::HeapError) -> VmError {
    VmError::Native(error.to_string())
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

/// Equality of two map keys: strings by content, boxed primitives by per-kind Java value
/// equality, everything else by reference identity; null equals only null.
///
/// The heap is passed in rather than locked here: `computeIfAbsent` already holds the guard
/// while it scans the entries, and locking it a second time on the same thread deadlocks.
fn keys_equal(heap: &rava_vm::Heap, left: Value, right: Value) -> Result<bool, VmError> {
    let (Value::Ref(left_ref), Value::Ref(right_ref)) = (left, right) else {
        return Ok(left == right);
    };
    if left_ref.is_null() || right_ref.is_null() {
        return Ok(left_ref == right_ref);
    }
    match (
        heap.get(left_ref).map_err(heap_error)?,
        heap.get(right_ref).map_err(heap_error)?,
    ) {
        (HeapObject::Str(left), HeapObject::Str(right)) => Ok(left == right),
        (HeapObject::Boxed(left), HeapObject::Boxed(right)) => Ok(left.java_equals(*right)),
        _ => Ok(left_ref == right_ref),
    }
}

fn find_key(
    heap: &rava_vm::Heap,
    entries: &[(Value, Value)],
    key: Value,
) -> Result<Option<usize>, VmError> {
    for (position, (candidate, _)) in entries.iter().enumerate() {
        if keys_equal(heap, *candidate, key)? {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

fn compute_if_absent(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let function = reference_argument(args, 1, "Map.computeIfAbsent")?;
    if receiver.is_null() {
        return Err(VmError::Native(
            "Map.computeIfAbsent was called on a null receiver".to_owned(),
        ));
    }
    let existing = {
        let heap = shared.heap();
        let entries = heap.map(receiver).map_err(|error| {
            VmError::Native(format!(
                "Map.computeIfAbsent receiver is not a map: {error}"
            ))
        })?;
        let position = find_key(&heap, entries, key)?;
        position.map(|position| entries[position].1)
    };
    if let Some(value) = existing {
        return Ok(Some(value));
    }

    let computed = invoke_apply(shared, thread, function, key)?;
    {
        let mut heap = shared.heap();
        let entries = heap.map_mut(receiver).map_err(heap_error)?;
        entries.push((key, computed));
    }
    Ok(Some(computed))
}

/// `Iterable.forEach(Consumer)`: visit a snapshot of a list or set.
fn collection_for_each(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let consumer = reference_argument(args, 0, "Iterable.forEach")?;
    let elements = {
        let heap = shared.heap();
        match heap.get(receiver).map_err(heap_error)? {
            HeapObject::List(list) => list.elements.clone(),
            HeapObject::Set(set) => set.elements.clone(),
            _ => {
                return Err(VmError::Native(
                    "Iterable.forEach receiver is not a collection".to_owned(),
                ));
            }
        }
    };
    for element in elements {
        invoke_accept(shared, thread, consumer, element)?;
    }
    Ok(None)
}

fn invoke_accept(
    shared: &Shared,
    thread: &ThreadState,
    consumer: ObjRef,
    argument: Value,
) -> Result<(), VmError> {
    let lambda = {
        let heap = shared.heap();
        match heap.get(consumer).map_err(heap_error)? {
            HeapObject::Lambda(lambda) => Some(lambda.clone()),
            _ => None,
        }
    };
    if let Some(lambda) = lambda {
        lambda::invoke_sam(shared, thread, &lambda, vec![argument]).map_err(interp_to_vm)?;
        return Ok(());
    }
    let class: ClassId = {
        let heap = shared.heap();
        heap.object_class(consumer).map_err(heap_error)?
    };
    run_instance_method(
        shared,
        thread,
        class,
        "accept",
        "(Ljava/lang/Object;)V",
        consumer,
        vec![argument],
    )
    .map_err(interp_to_vm)?;
    Ok(())
}

/// `Arrays.sort(Object[], Comparator)`: insertion sort over the array's elements.
fn arrays_sort_comparator(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = reference_argument(args, 0, "Arrays.sort")?;
    let comparator = reference_argument(args, 1, "Arrays.sort")?;
    let mut elements = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    insertion_sort(shared, thread, comparator, &mut elements)?;
    shared.heap().array_mut(array).map_err(heap_error)?.elements = elements;
    Ok(None)
}

/// `List.sort(Comparator)`: the same ordering over a list's elements.
fn list_sort_comparator(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let comparator = reference_argument(args, 0, "List.sort")?;
    let mut elements = {
        let heap = shared.heap();
        heap.list(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
            .to_vec()
    };
    insertion_sort(shared, thread, comparator, &mut elements)?;
    *shared.heap().list_mut(receiver).map_err(heap_error)? = elements;
    Ok(None)
}

fn insertion_sort(
    shared: &Shared,
    thread: &ThreadState,
    comparator: ObjRef,
    elements: &mut [Value],
) -> Result<(), VmError> {
    for index in 1..elements.len() {
        let mut position = index;
        while position > 0 {
            let ordering = invoke_compare(
                shared,
                thread,
                comparator,
                elements[position - 1],
                elements[position],
            )?;
            if ordering <= 0 {
                break;
            }
            elements.swap(position - 1, position);
            position -= 1;
        }
    }
    Ok(())
}

fn invoke_compare(
    shared: &Shared,
    thread: &ThreadState,
    comparator: ObjRef,
    left: Value,
    right: Value,
) -> Result<i32, VmError> {
    let lambda = {
        let heap = shared.heap();
        match heap.get(comparator).map_err(heap_error)? {
            HeapObject::Lambda(lambda) => Some(lambda.clone()),
            _ => None,
        }
    };
    let result = if let Some(lambda) = lambda {
        lambda::invoke_sam(shared, thread, &lambda, vec![left, right]).map_err(interp_to_vm)?
    } else {
        let class: ClassId = {
            let heap = shared.heap();
            heap.object_class(comparator).map_err(heap_error)?
        };
        run_instance_method(
            shared,
            thread,
            class,
            "compare",
            COMPARATOR_COMPARE,
            comparator,
            vec![left, right],
        )
        .map_err(interp_to_vm)?
    };
    match result {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(
            "Comparator.compare returned a non-int value".to_owned(),
        )),
    }
}

fn invoke_apply(
    shared: &Shared,
    thread: &ThreadState,
    function: ObjRef,
    argument: Value,
) -> Result<Value, VmError> {
    if function.is_null() {
        return Err(VmError::Native(
            "Map.computeIfAbsent received a null Function".to_owned(),
        ));
    }
    let lambda = {
        let heap = shared.heap();
        match heap.get(function).map_err(heap_error)? {
            HeapObject::Lambda(lambda) => Some(lambda.clone()),
            _ => None,
        }
    };
    if let Some(lambda) = lambda {
        return lambda::invoke_sam(shared, thread, &lambda, vec![argument])
            .map_err(interp_to_vm)
            .map(|value| value.unwrap_or(Value::Ref(ObjRef::NULL)));
    }
    let class: ClassId = {
        let heap = shared.heap();
        heap.object_class(function).map_err(heap_error)?
    };
    run_instance_method(
        shared,
        thread,
        class,
        "apply",
        FUNCTION_APPLY,
        function,
        vec![argument],
    )
    .map_err(interp_to_vm)
    .map(|value| value.unwrap_or(Value::Ref(ObjRef::NULL)))
}

fn interp_to_vm(error: InterpError) -> VmError {
    match error {
        InterpError::Vm(error) => error,
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(other.to_string()),
    }
}

/// `Comparator.comparingInt(ToIntFunction)`: a comparator over the extractor's `int` key.
///
/// The JDK returns a synthetic lambda whose `compare` calls `applyAsInt` on both arguments
/// and compares the results. There is no class file for that body here either, so the
/// lambda carries a [`LambdaTargetKind::Native`] target: the registry entry
/// `Comparator.comparingInt$` implements the comparison.
fn comparator_comparing_int(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let function = reference_argument(args, 0, "Comparator.comparingInt")?;
    if function.is_null() {
        return Err(VmError::Native(
            "Comparator.comparingInt received a null extractor".to_owned(),
        ));
    }
    let comparator = shared.load_library_class("java/util/Comparator")?;
    let lambda = shared.allocate_lambda(LambdaObject {
        interface: comparator,
        sam_name: "compare".to_owned(),
        sam_descriptor: COMPARATOR_INT_COMPARE.to_owned(),
        instantiated_descriptor: COMPARATOR_INT_COMPARE.to_owned(),
        target: LambdaTarget {
            class: comparator,
            name: COMPARING_INT_TARGET_NAME.to_owned(),
            descriptor: COMPARING_INT_TARGET.to_owned(),
            kind: LambdaTargetKind::Native,
        },
        captured: vec![Value::Ref(function)],
    });
    Ok(Some(Value::Ref(lambda)))
}

/// The body of a `comparingInt` comparator: compare the two extracted `int` keys.
fn comparator_comparing_int_compare(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let function = reference_argument(args, 0, "Comparator.comparingInt")?;
    let left = reference_argument(args, 1, "Comparator.comparingInt")?;
    let right = reference_argument(args, 2, "Comparator.comparingInt")?;
    let left_key = invoke_to_int(shared, thread, function, Value::Ref(left))?;
    let right_key = invoke_to_int(shared, thread, function, Value::Ref(right))?;
    Ok(Some(Value::Int(match left_key.cmp(&right_key) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    })))
}

/// Call a `ToIntFunction`, through a lambda object or the object's own `applyAsInt`.
fn invoke_to_int(
    shared: &Shared,
    thread: &ThreadState,
    function: ObjRef,
    argument: Value,
) -> Result<i32, VmError> {
    if function.is_null() {
        return Err(VmError::Native(
            "Comparator.comparingInt received a null extractor".to_owned(),
        ));
    }
    let lambda = {
        let heap = shared.heap();
        match heap.get(function).map_err(heap_error)? {
            HeapObject::Lambda(lambda) => Some(lambda.clone()),
            _ => None,
        }
    };
    let result = if let Some(lambda) = lambda {
        lambda::invoke_sam(shared, thread, &lambda, vec![argument]).map_err(interp_to_vm)?
    } else {
        let class: ClassId = {
            let heap = shared.heap();
            heap.object_class(function).map_err(heap_error)?
        };
        run_instance_method(
            shared,
            thread,
            class,
            "applyAsInt",
            TO_INT_APPLY_AS_INT,
            function,
            vec![argument],
        )
        .map_err(interp_to_vm)?
    };
    match result {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(
            "ToIntFunction.applyAsInt returned a non-int value".to_owned(),
        )),
    }
}

/// `Comparator.reversed()`: a comparator over the same keys in the opposite order.
///
/// The JDK's default method returns `Collections.reverseOrder(this)`, a comparator whose
/// `compare` negates the wrapped comparator's result. It is a native here because the C1
/// stub carries no `Code`, and its body is another [`LambdaTargetKind::Native`] lambda.
fn comparator_reversed(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "Comparator.reversed was called on a null receiver".to_owned(),
        ));
    }
    let comparator = shared.load_library_class("java/util/Comparator")?;
    let lambda = shared.allocate_lambda(LambdaObject {
        interface: comparator,
        sam_name: "compare".to_owned(),
        sam_descriptor: COMPARATOR_INT_COMPARE.to_owned(),
        instantiated_descriptor: COMPARATOR_INT_COMPARE.to_owned(),
        target: LambdaTarget {
            class: comparator,
            name: REVERSED_TARGET_NAME.to_owned(),
            descriptor: REVERSED_TARGET.to_owned(),
            kind: LambdaTargetKind::Native,
        },
        captured: vec![Value::Ref(receiver)],
    });
    Ok(Some(Value::Ref(lambda)))
}

/// The body of a reversed comparator: negate the wrapped comparator's result.
fn comparator_reversed_compare(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let inner = reference_argument(args, 0, "Comparator.reversed")?;
    let left = reference_argument(args, 1, "Comparator.reversed")?;
    let right = reference_argument(args, 2, "Comparator.reversed")?;
    let ordering = invoke_compare(shared, thread, inner, Value::Ref(left), Value::Ref(right))?;
    // `Collections.reverseOrder` negates the result, which wraps like Java's `-`.
    Ok(Some(Value::Int(ordering.wrapping_neg())))
}
