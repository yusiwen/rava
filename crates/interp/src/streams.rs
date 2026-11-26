//! Eager `java.util.stream` pipeline and `java.util.Spliterator`/`Optional` natives (M2.24).
//!
//! Spring's annotation-metadata path reaches
//! `StreamSupport.stream(Spliterator, boolean)` in `TypeMappedAnnotations` and
//! `MergedAnnotationsCollection`, then filters, maps, sorts and collects the merged
//! annotations. This module supplies enough of `java.util.stream` for that path.
//!
//! # Representation
//!
//! A stream is **eager**: [`rava_vm::StreamObject`] holds the whole element list, and
//! every intermediate operation runs while the new stream is built. `filter`, `map`,
//! `sorted`, `limit`, `skip`, `distinct` and `flatMap` each allocate a new stream with a
//! computed element vector, so there is no lazy stage graph, no short-circuiting source,
//! no `Spliterator`-backed stream after construction and no parallel execution. The
//! `boolean` of `StreamSupport.stream` is recorded on the object (`isParallel` reports
//! it) and otherwise ignored, matching the slice boundary.
//!
//! A [`rava_vm::SpliteratorObject`] is a snapshot plus a cursor. `Collection.spliterator`
//! and `Spliterators.*` create one; a Java class that implements `Spliterator` stays an
//! ordinary instance and is drained through its own `tryAdvance`, which is real bytecode.
//!
//! A [`rava_vm::CollectorObject`] carries a kind. `Collectors.toList`/`toSet`/`joining`/
//! `counting`/`toMap`/`toCollection` carry their result shape directly, while
//! `Collector.of` keeps the four callbacks and `Stream.collect` runs them through the
//! interpreter.
//!
//! # Callbacks
//!
//! Every functional interface is invoked the way the rest of the interpreter invokes one:
//! a [`rava_vm::LambdaObject`] runs through [`crate::lambda::invoke_sam`], and any other
//! object resolves its SAM on the receiver's superclass chain and runs through
//! [`crate::run_instance_method`]. A Java `Spliterator` is drained by passing it an
//! internal consumer ([`rava_vm::SinkObject`]) whose `accept` is a registry native that
//! appends to the destination stream, so the Java `tryAdvance` calls back into the VM
//! instead of the VM guessing what the spliterator would yield.
//!
//! `java.util.Optional` is here because `findFirst`/`min`/`max` return one and Spring's
//! `MergedAnnotation.synthesize` builds them with `Optional.of`/`ofNullable`/`empty` and
//! reads them with `orElse`. `Map.forEach(BiConsumer)` is here for the same reason:
//! Spring's `MultiValueMap` collector iterates a map with a callback.
//!
//! No Java exception objects are synthesized, so a callback that is called with the wrong
//! shape is a typed [`rava_vm::VmError::Native`] error. A Java throwable raised by a
//! callback crosses the native ABI as [`rava_vm::VmError::Thrown`] and reaches the
//! caller's exception table unchanged.

use rava_vm::{
    ClassId, ClassRef, CollectorKind, HeapError, HeapObject, LoaderId, ObjRef, Shared, ThreadState,
    Value, VmError,
};

use crate::lambda;
use crate::{InterpError, run_instance_method};

/// The internal name of `java.util.ArrayList`, the `toList` result.
const ARRAY_LIST_CLASS: &str = "java/util/ArrayList";

/// The internal name of `java.util.LinkedHashSet`, the `toSet` result.
const LINKED_HASH_SET_CLASS: &str = "java/util/LinkedHashSet";

/// The internal name of `java.util.HashMap`, the `toMap` result.
const HASH_MAP_CLASS: &str = "java/util/HashMap";

/// The internal name of the stream interface every stream object reports.
const STREAM_CLASS: &str = "java/util/stream/Stream";

/// The internal name of the base stream interface.
const BASE_STREAM_CLASS: &str = "java/util/stream/BaseStream";

/// The internal name of the primitive `int` stream interface.
const INT_STREAM_CLASS: &str = "java/util/stream/IntStream";

/// The internal name of the spliterator interface.
const SPLITERATOR_CLASS: &str = "java/util/Spliterator";

/// The internal name of the collector interface.
const COLLECTOR_CLASS: &str = "java/util/stream/Collector";

/// The internal name of the optional class.
const OPTIONAL_CLASS: &str = "java/util/Optional";

/// The internal name of the consumer interface the internal sink implements.
const CONSUMER_CLASS: &str = "java/util/function/Consumer";

/// The internal name of the iterator interface.
const ITERATOR_CLASS: &str = "java/util/Iterator";

/// `Spliterator.ORDERED`.
const ORDERED: i32 = 0x10;
/// `Spliterator.DISTINCT`.
const DISTINCT: i32 = 0x1;
/// `Spliterator.SIZED`.
const SIZED: i32 = 0x40;
/// `Spliterator.SUBSIZED`.
const SUBSIZED: i32 = 0x4000;

/// `Function.apply(Object)Object`.
const FUNCTION_APPLY: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";
/// `Predicate.test(Object)Z`.
const PREDICATE_TEST: &str = "(Ljava/lang/Object;)Z";
/// `Consumer.accept(Object)V`.
const CONSUMER_ACCEPT: &str = "(Ljava/lang/Object;)V";
/// `BiConsumer.accept(Object, Object)V`.
const BI_CONSUMER_ACCEPT: &str = "(Ljava/lang/Object;Ljava/lang/Object;)V";
/// `Supplier.get()Object`.
const SUPPLIER_GET: &str = "()Ljava/lang/Object;";
/// `Comparator.compare(Object, Object)I`.
const COMPARATOR_COMPARE: &str = "(Ljava/lang/Object;Ljava/lang/Object;)I";

/// One heap error as a native error.
fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// Translate an interpreter failure into the VM boundary's error.
///
/// A Java throwable is preserved (`VmError::Thrown`), so the interpreter can hand it to a
/// caller's exception table instead of losing it.
fn interp_to_vm(error: InterpError) -> VmError {
    match error {
        InterpError::Vm(error) => error,
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(other.to_string()),
    }
}

fn reference_argument(args: &[Value], position: usize, operation: &str) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{operation} expects a reference argument"
        ))),
    }
}

fn int_argument(args: &[Value], position: usize, operation: &str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(format!(
            "{operation} expects an int argument"
        ))),
    }
}

fn long_argument(args: &[Value], position: usize, operation: &str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        Some(Value::Int(value)) => Ok(i64::from(value)),
        _ => Err(VmError::Native(format!(
            "{operation} expects a long argument"
        ))),
    }
}

fn boolean_argument(args: &[Value], position: usize, operation: &str) -> Result<bool, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value != 0),
        _ => Err(VmError::Native(format!(
            "{operation} expects a boolean argument"
        ))),
    }
}

/// The element snapshot of a stream receiver.
fn stream_elements(
    shared: &Shared,
    receiver: ObjRef,
    operation: &str,
) -> Result<Vec<Value>, VmError> {
    let heap = shared.heap();
    Ok(heap
        .stream(receiver)
        .map_err(|error| VmError::Native(format!("{operation} receiver is not a stream: {error}")))?
        .elements
        .clone())
}

/// The parallel flag of a stream receiver.
fn stream_parallel_flag(shared: &Shared, receiver: ObjRef) -> Result<bool, VmError> {
    Ok(shared.heap().stream(receiver).map_err(heap_error)?.parallel)
}

/// Allocate a stream with `elements` unchanged and the receiver's parallel flag.
fn same_shape_stream(
    shared: &Shared,
    receiver: ObjRef,
    elements: Vec<Value>,
) -> Result<ObjRef, VmError> {
    let parallel = stream_parallel_flag(shared, receiver)?;
    let class = {
        let heap = shared.heap();
        match heap.get(receiver).map_err(heap_error)? {
            HeapObject::Stream(stream) => stream.class,
            _ => None,
        }
    };
    Ok(shared.heap().allocate_stream(elements, parallel, class))
}

/// The runtime class of any reference, resolved through the class object.
fn runtime_class(shared: &Shared, reference: ObjRef) -> Result<ClassId, VmError> {
    let class_object = shared.get_class(reference)?;
    shared.heap().class(class_object).map_err(heap_error)
}

/// Resolve an instance method on `start` or a superclass. Mirrors the interpreter's
/// virtual-resolution walk, so a callback object that inherits its SAM still runs.
///
/// The callback natives in [`crate::callbacks`] use the same walk: Spring sorts with an
/// `AnnotationAwareOrderComparator` whose `compare` is declared on its `OrderComparator`
/// superclass, and the callback dispatch must find that declaration rather than stop at
/// the runtime class.
pub(crate) fn resolve_instance_class(
    shared: &Shared,
    start: ClassId,
    name: &str,
    descriptor: &str,
) -> Result<ClassId, VmError> {
    let mut current = Some(start);
    while let Some(class) = current {
        let entry = shared
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        let found = entry
            .file
            .find_method(name, descriptor)
            .map_err(|error| VmError::Native(error.to_string()))?;
        if found.is_some() {
            return Ok(class);
        }
        current = shared.resolution_superclass(class)?;
    }
    Err(VmError::Native(format!(
        "no method {name}{descriptor} on the callback object"
    )))
}

/// Call a functional-interface method on `object`.
///
/// A lambda runs through [`crate::lambda::invoke_sam`] (its target already knows the SAM
/// name and descriptor); any other object resolves the method on its runtime class chain
/// and runs it through [`crate::run_instance_method`]. A void SAM returns `None`.
fn call_sam(
    shared: &Shared,
    thread: &ThreadState,
    object: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, VmError> {
    if object.is_null() {
        return Err(VmError::Native(format!(
            "{name} was called with a null functional object"
        )));
    }
    let lambda = {
        let heap = shared.heap();
        match heap.get(object).map_err(heap_error)? {
            HeapObject::Lambda(lambda) => Some(lambda.clone()),
            _ => None,
        }
    };
    if let Some(lambda) = lambda {
        return lambda::invoke_sam(shared, thread, &lambda, args).map_err(interp_to_vm);
    }
    let class = resolve_instance_class(shared, runtime_class(shared, object)?, name, descriptor)?;
    run_instance_method(shared, thread, class, name, descriptor, object, args).map_err(interp_to_vm)
}

fn call_predicate(
    shared: &Shared,
    thread: &ThreadState,
    predicate: ObjRef,
    value: Value,
) -> Result<bool, VmError> {
    match call_sam(
        shared,
        thread,
        predicate,
        "test",
        PREDICATE_TEST,
        vec![value],
    )? {
        Some(Value::Int(result)) => Ok(result != 0),
        _ => Err(VmError::Native(
            "Predicate.test did not return a boolean".to_owned(),
        )),
    }
}

fn call_function(
    shared: &Shared,
    thread: &ThreadState,
    function: ObjRef,
    value: Value,
) -> Result<Value, VmError> {
    Ok(call_sam(
        shared,
        thread,
        function,
        "apply",
        FUNCTION_APPLY,
        vec![value],
    )?
    .unwrap_or(Value::Ref(ObjRef::NULL)))
}

fn call_consumer(
    shared: &Shared,
    thread: &ThreadState,
    consumer: ObjRef,
    value: Value,
) -> Result<(), VmError> {
    call_sam(
        shared,
        thread,
        consumer,
        "accept",
        CONSUMER_ACCEPT,
        vec![value],
    )
    .map(|_| ())
}

fn call_bi_consumer(
    shared: &Shared,
    thread: &ThreadState,
    consumer: ObjRef,
    left: Value,
    right: Value,
) -> Result<(), VmError> {
    call_sam(
        shared,
        thread,
        consumer,
        "accept",
        BI_CONSUMER_ACCEPT,
        vec![left, right],
    )
    .map(|_| ())
}

fn call_supplier(
    shared: &Shared,
    thread: &ThreadState,
    supplier: ObjRef,
) -> Result<Value, VmError> {
    Ok(
        call_sam(shared, thread, supplier, "get", SUPPLIER_GET, Vec::new())?
            .unwrap_or(Value::Ref(ObjRef::NULL)),
    )
}

fn call_comparator(
    shared: &Shared,
    thread: &ThreadState,
    comparator: ObjRef,
    left: Value,
    right: Value,
) -> Result<i32, VmError> {
    match call_sam(
        shared,
        thread,
        comparator,
        "compare",
        COMPARATOR_COMPARE,
        vec![left, right],
    )? {
        Some(Value::Int(ordering)) => Ok(ordering),
        _ => Err(VmError::Native(
            "Comparator.compare did not return an int".to_owned(),
        )),
    }
}

/// The text of a value the way `String.valueOf` renders it, for `joining`.
fn text_of(shared: &Shared, value: Value) -> Result<String, VmError> {
    match value {
        Value::Int(value) => Ok(value.to_string()),
        Value::Long(value) => Ok(value.to_string()),
        Value::Float(value) => Ok(rava_vm::string::float_text(value)),
        Value::Double(value) => Ok(rava_vm::string::double_text(value)),
        Value::Ref(reference) if reference.is_null() => Ok("null".to_owned()),
        Value::Ref(reference) => {
            let text = {
                let heap = shared.heap();
                match heap.get(reference).map_err(heap_error)? {
                    HeapObject::Str(text) => Some(text.clone()),
                    HeapObject::StringBuilder(text) => Some(text.clone()),
                    HeapObject::Boxed(boxed) => Some(boxed.text()),
                    _ => None,
                }
            };
            match text {
                Some(text) => Ok(text),
                None => {
                    let class = runtime_class(shared, reference)?;
                    let name = shared
                        .entry(class)
                        .map(|entry| entry.name.replace('/', "."))
                        .unwrap_or_else(|| "java.lang.Object".to_owned());
                    Ok(format!("{name}@{:x}", reference.identity_hash() as u32))
                }
            }
        }
    }
}

/// Equality for `distinct` and `toSet`: strings by content, boxed `Integer`s by value,
/// everything else by reference identity; null equals only null.
fn same_value(shared: &Shared, left: Value, right: Value) -> Result<bool, VmError> {
    let (Value::Ref(left_ref), Value::Ref(right_ref)) = (left, right) else {
        return Ok(left == right);
    };
    if left_ref.is_null() || right_ref.is_null() {
        return Ok(left_ref == right_ref);
    }
    let heap = shared.heap();
    match (
        heap.get(left_ref).map_err(heap_error)?,
        heap.get(right_ref).map_err(heap_error)?,
    ) {
        (HeapObject::Str(left), HeapObject::Str(right)) => Ok(left == right),
        (HeapObject::Boxed(left), HeapObject::Boxed(right)) => Ok(left.java_equals(*right)),
        _ => Ok(left_ref == right_ref),
    }
}

/// Natural ordering for `sorted()` and the natural-order comparators: numbers numerically,
/// strings by UTF-16 code unit.
pub(crate) fn compare_values(shared: &Shared, left: Value, right: Value) -> Result<i32, VmError> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(left.cmp(&right) as i32),
        (Value::Long(left), Value::Long(right)) => Ok(left.cmp(&right) as i32),
        (Value::Float(left), Value::Float(right)) => left
            .partial_cmp(&right)
            .map(|ordering| ordering as i32)
            .ok_or_else(|| VmError::Native("sorted() cannot order a NaN".to_owned())),
        (Value::Double(left), Value::Double(right)) => left
            .partial_cmp(&right)
            .map(|ordering| ordering as i32)
            .ok_or_else(|| VmError::Native("sorted() cannot order a NaN".to_owned())),
        (Value::Ref(left), Value::Ref(right)) if !left.is_null() && !right.is_null() => {
            let heap = shared.heap();
            let left = heap.get(left).map_err(heap_error)?;
            let right = heap.get(right).map_err(heap_error)?;
            match (left, right) {
                (HeapObject::Str(left), HeapObject::Str(right)) => Ok(
                    rava_vm::string::utf16_units(left).cmp(&rava_vm::string::utf16_units(right))
                        as i32,
                ),
                (HeapObject::Boxed(left), HeapObject::Boxed(right)) => left
                    .java_compare(*right)
                    .map(|ordering| ordering as i32)
                    .ok_or_else(|| {
                        VmError::Native(
                            "sorted() cannot compare two different wrapper kinds".to_owned(),
                        )
                    }),
                _ => Err(VmError::Native(
                    "sorted() has no natural ordering for this element type".to_owned(),
                )),
            }
        }
        _ => Err(VmError::Native(
            "sorted() cannot compare a null or mixed element".to_owned(),
        )),
    }
}

/// An in-place insertion sort, stable for equal elements.
fn sort_values(
    shared: &Shared,
    thread: &ThreadState,
    elements: &mut [Value],
    comparator: Option<ObjRef>,
) -> Result<(), VmError> {
    for index in 1..elements.len() {
        let mut position = index;
        while position > 0 {
            let ordering = match comparator {
                Some(comparator) => call_comparator(
                    shared,
                    thread,
                    comparator,
                    elements[position - 1],
                    elements[position],
                )?,
                None => compare_values(shared, elements[position - 1], elements[position])?,
            };
            if ordering <= 0 {
                break;
            }
            elements.swap(position - 1, position);
            position -= 1;
        }
    }
    Ok(())
}

/// Build the `Object[]` a `Stream.toArray` result needs.
fn reference_array(shared: &Shared, elements: Vec<Value>) -> Result<ObjRef, VmError> {
    let object = shared.load_library_class("java/lang/Object")?;
    let reference = shared
        .heap()
        .allocate_array(ClassRef::Named(object), elements.len());
    shared
        .heap()
        .array_mut(reference)
        .map_err(heap_error)?
        .elements = elements;
    Ok(reference)
}

/// Append `value` to a list or set heap object; a duplicate set element is skipped.
fn append_element(shared: &Shared, target: ObjRef, value: Value) -> Result<(), VmError> {
    let is_list = {
        let heap = shared.heap();
        matches!(heap.get(target).map_err(heap_error)?, HeapObject::List(_))
    };
    if is_list {
        shared
            .heap()
            .list_mut(target)
            .map_err(heap_error)?
            .push(value);
        return Ok(());
    }
    if set_contains(shared, target, value)? {
        return Ok(());
    }
    shared
        .heap()
        .set_mut(target)
        .map_err(heap_error)?
        .push(value);
    Ok(())
}

/// Membership by the `toSet` equality rules.
fn set_contains(shared: &Shared, target: ObjRef, value: Value) -> Result<bool, VmError> {
    let elements = {
        let heap = shared.heap();
        match heap.get(target).map_err(heap_error)? {
            HeapObject::Set(set) => set.elements.clone(),
            HeapObject::List(list) => list.elements.clone(),
            _ => return Err(VmError::Native("collection expected".to_owned())),
        }
    };
    for element in elements {
        if same_value(shared, element, value)? {
            return Ok(true);
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------------------
// Stream sources
// ---------------------------------------------------------------------------------

/// `StreamSupport.stream(Spliterator, boolean)`.
///
/// The spliterator is drained eagerly: a VM spliterator hands over its remaining
/// elements, and a Java spliterator runs its own `tryAdvance` once per element with an
/// internal consumer that appends to the stream being built.
fn stream_support_stream(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let spliterator = reference_argument(args, 0, "StreamSupport.stream")?;
    let parallel = boolean_argument(args, 1, "StreamSupport.stream")?;
    let elements = drain_spliterator(shared, thread, spliterator)?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, parallel, None),
    )))
}

/// The remaining elements of a spliterator, VM or Java.
fn drain_spliterator(
    shared: &Shared,
    thread: &ThreadState,
    spliterator: ObjRef,
) -> Result<Vec<Value>, VmError> {
    if spliterator.is_null() {
        return Err(VmError::Native(
            "StreamSupport.stream received a null Spliterator".to_owned(),
        ));
    }
    {
        let mut heap = shared.heap();
        if let HeapObject::Spliterator(state) = heap.get_mut(spliterator).map_err(heap_error)? {
            let remaining = state.elements.split_off(state.position);
            state.position = state.elements.len();
            return Ok(remaining);
        }
    }
    let class = runtime_class(shared, spliterator)?;
    let destination = shared.heap().allocate_stream(Vec::new(), false, None);
    let consumer = shared.heap().allocate_sink(destination);
    loop {
        let advanced = run_instance_method(
            shared,
            thread,
            class,
            "tryAdvance",
            "(Ljava/util/function/Consumer;)Z",
            spliterator,
            vec![Value::Ref(consumer)],
        )
        .map_err(interp_to_vm)?;
        match advanced {
            Some(Value::Int(value)) if value != 0 => {}
            _ => break,
        }
    }
    Ok(shared
        .heap()
        .stream(destination)
        .map_err(heap_error)?
        .elements
        .clone())
}

/// `Stream.empty()`.
fn stream_empty(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(_shared.heap().allocate_stream(
        Vec::new(),
        false,
        None,
    ))))
}

/// `Stream.of(T)`.
fn stream_of_one(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    Ok(Some(Value::Ref(shared.heap().allocate_stream(
        vec![value],
        false,
        None,
    ))))
}

/// `Stream.of(T...)`.
fn stream_of_many(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = reference_argument(args, 0, "Stream.of")?;
    let elements = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, false, None),
    )))
}

/// `Stream.ofNullable(T)`.
fn stream_of_nullable(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => vec![Value::Ref(reference)],
        Some(value @ Value::Int(_))
        | Some(value @ Value::Long(_))
        | Some(value @ Value::Float(_))
        | Some(value @ Value::Double(_)) => vec![value],
        _ => Vec::new(),
    };
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, false, None),
    )))
}

/// `Stream.concat(Stream, Stream)`.
fn stream_concat(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = reference_argument(args, 0, "Stream.concat")?;
    let right = reference_argument(args, 1, "Stream.concat")?;
    let mut elements = stream_elements(shared, left, "Stream.concat")?;
    elements.extend(stream_elements(shared, right, "Stream.concat")?);
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, false, None),
    )))
}

// ---------------------------------------------------------------------------------
// Intermediate operations
// ---------------------------------------------------------------------------------

/// `Stream.filter(Predicate)`.
fn stream_filter(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "Stream.filter")?;
    let mut kept = Vec::new();
    for element in stream_elements(shared, receiver, "Stream.filter")? {
        if call_predicate(shared, thread, predicate, element)? {
            kept.push(element);
        }
    }
    Ok(Some(Value::Ref(same_shape_stream(shared, receiver, kept)?)))
}

/// `Stream.map(Function)`.
fn stream_map(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let function = reference_argument(args, 0, "Stream.map")?;
    let mut mapped = Vec::new();
    for element in stream_elements(shared, receiver, "Stream.map")? {
        mapped.push(call_function(shared, thread, function, element)?);
    }
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, mapped,
    )?)))
}

/// `Stream.flatMap(Function)`: the function returns a stream whose elements are merged.
fn stream_flat_map(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let function = reference_argument(args, 0, "Stream.flatMap")?;
    let mut merged = Vec::new();
    for element in stream_elements(shared, receiver, "Stream.flatMap")? {
        let Value::Ref(inner) = call_function(shared, thread, function, element)? else {
            return Err(VmError::Native(
                "Stream.flatMap returned a non-reference value".to_owned(),
            ));
        };
        merged.extend(stream_elements(shared, inner, "Stream.flatMap")?);
    }
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, merged,
    )?)))
}

/// `Stream.distinct()`.
fn stream_distinct(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut distinct: Vec<Value> = Vec::new();
    for element in stream_elements(shared, receiver, "Stream.distinct")? {
        let mut duplicate = false;
        for seen in &distinct {
            if same_value(shared, *seen, element)? {
                duplicate = true;
                break;
            }
        }
        if !duplicate {
            distinct.push(element);
        }
    }
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, distinct,
    )?)))
}

/// `Stream.sorted()` and `Stream.sorted(Comparator)`.
fn stream_sorted(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let comparator = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => Some(reference),
        _ => None,
    };
    let mut elements = stream_elements(shared, receiver, "Stream.sorted")?;
    sort_values(shared, thread, &mut elements, comparator)?;
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, elements,
    )?)))
}

/// `Stream.peek(Consumer)`.
fn stream_peek(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let consumer = reference_argument(args, 0, "Stream.peek")?;
    let elements = stream_elements(shared, receiver, "Stream.peek")?;
    for element in &elements {
        call_consumer(shared, thread, consumer, *element)?;
    }
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, elements,
    )?)))
}

/// `Stream.limit(long)`.
fn stream_limit(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let limit = long_argument(args, 0, "Stream.limit")?;
    if limit < 0 {
        return Err(VmError::Native(
            "Stream.limit received a negative size".to_owned(),
        ));
    }
    let elements = stream_elements(shared, receiver, "Stream.limit")?;
    let limited = elements.into_iter().take(limit as usize).collect();
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, limited,
    )?)))
}

/// `Stream.skip(long)`.
fn stream_skip(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let skip = long_argument(args, 0, "Stream.skip")?;
    if skip < 0 {
        return Err(VmError::Native(
            "Stream.skip received a negative count".to_owned(),
        ));
    }
    let elements = stream_elements(shared, receiver, "Stream.skip")?;
    let skipped = elements.into_iter().skip(skip as usize).collect();
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, skipped,
    )?)))
}

/// `BaseStream.sequential()`.
fn stream_sequential(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = stream_elements(shared, receiver, "BaseStream.sequential")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, false, None),
    )))
}

/// `BaseStream.parallel()`.
fn stream_parallel(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = stream_elements(shared, receiver, "BaseStream.parallel")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, true, None),
    )))
}

/// `BaseStream.isParallel()`.
fn stream_is_parallel(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(i32::from(stream_parallel_flag(
        shared, receiver,
    )?))))
}

/// `BaseStream.unordered()`: the eager pipeline is already order-insensitive for the
/// operations this slice supports, so the flag rides along with the elements.
fn stream_unordered(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = stream_elements(shared, receiver, "BaseStream.unordered")?;
    Ok(Some(Value::Ref(same_shape_stream(
        shared, receiver, elements,
    )?)))
}

/// `BaseStream.onClose(Runnable)`: the eager pipeline has no close actions; the stream is
/// returned unchanged.
fn stream_on_close(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(receiver)))
}

/// `BaseStream.close()`: the eager pipeline holds no resource.
fn stream_close(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

// ---------------------------------------------------------------------------------
// Terminal operations
// ---------------------------------------------------------------------------------

/// `Stream.forEach(Consumer)` / `Stream.forEachOrdered(Consumer)`.
fn stream_for_each(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let consumer = reference_argument(args, 0, "Stream.forEach")?;
    for element in stream_elements(shared, receiver, "Stream.forEach")? {
        call_consumer(shared, thread, consumer, element)?;
    }
    Ok(None)
}

/// `Stream.count()`.
fn stream_count(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let length = stream_elements(shared, receiver, "Stream.count")?.len();
    Ok(Some(Value::Long(length as i64)))
}

/// `Stream.anyMatch(Predicate)`.
fn stream_any_match(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "Stream.anyMatch")?;
    for element in stream_elements(shared, receiver, "Stream.anyMatch")? {
        if call_predicate(shared, thread, predicate, element)? {
            return Ok(Some(Value::Int(1)));
        }
    }
    Ok(Some(Value::Int(0)))
}

/// `Stream.allMatch(Predicate)`.
fn stream_all_match(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "Stream.allMatch")?;
    for element in stream_elements(shared, receiver, "Stream.allMatch")? {
        if !call_predicate(shared, thread, predicate, element)? {
            return Ok(Some(Value::Int(0)));
        }
    }
    Ok(Some(Value::Int(1)))
}

/// `Stream.noneMatch(Predicate)`.
fn stream_none_match(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "Stream.noneMatch")?;
    for element in stream_elements(shared, receiver, "Stream.noneMatch")? {
        if call_predicate(shared, thread, predicate, element)? {
            return Ok(Some(Value::Int(0)));
        }
    }
    Ok(Some(Value::Int(1)))
}

/// `Stream.findFirst()` / `Stream.findAny()`: the eager pipeline is ordered, so both
/// return the first element.
fn stream_find_first(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let first = stream_elements(shared, receiver, "Stream.findFirst")?
        .first()
        .copied();
    Ok(Some(Value::Ref(shared.heap().allocate_optional(first))))
}

/// `Stream.min(Comparator)` / `Stream.max(Comparator)`.
fn stream_min_max(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
    maximum: bool,
) -> Result<Option<Value>, VmError> {
    let comparator = reference_argument(args, 0, "Stream.min")?;
    let elements = stream_elements(shared, receiver, "Stream.min")?;
    let mut best: Option<Value> = None;
    for element in elements {
        match best {
            None => best = Some(element),
            Some(current) => {
                let ordering = call_comparator(shared, thread, comparator, element, current)?;
                let better = if maximum { ordering > 0 } else { ordering < 0 };
                if better {
                    best = Some(element);
                }
            }
        }
    }
    Ok(Some(Value::Ref(shared.heap().allocate_optional(best))))
}

fn stream_min(
    shared: &Shared,
    thread: &ThreadState,
    loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    stream_min_max(shared, thread, loader, receiver, args, false)
}

fn stream_max(
    shared: &Shared,
    thread: &ThreadState,
    loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    stream_min_max(shared, thread, loader, receiver, args, true)
}

/// `Stream.reduce(T, BinaryOperator)`.
fn stream_reduce_identity(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mut accumulator = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let function = reference_argument(args, 1, "Stream.reduce")?;
    for element in stream_elements(shared, receiver, "Stream.reduce")? {
        accumulator = call_sam(
            shared,
            thread,
            function,
            "apply",
            "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
            vec![accumulator, element],
        )?
        .unwrap_or(Value::Ref(ObjRef::NULL));
    }
    Ok(Some(accumulator))
}

/// `Stream.toArray()`.
fn stream_to_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = stream_elements(shared, receiver, "Stream.toArray")?;
    Ok(Some(Value::Ref(reference_array(shared, elements)?)))
}

/// `Stream.toArray(IntFunction)`: the function allocates the array and the elements fill
/// it, exactly as the JDK's generator overload does.
fn stream_to_array_generator(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let generator = reference_argument(args, 0, "Stream.toArray")?;
    let elements = stream_elements(shared, receiver, "Stream.toArray")?;
    let generated = call_sam(
        shared,
        thread,
        generator,
        "apply",
        "(I)Ljava/lang/Object;",
        vec![Value::Int(elements.len() as i32)],
    )?
    .unwrap_or(Value::Ref(ObjRef::NULL));
    let Value::Ref(array) = generated else {
        return Err(VmError::Native(
            "Stream.toArray generator returned a non-reference value".to_owned(),
        ));
    };
    if array.is_null() {
        return Err(VmError::Native(
            "Stream.toArray generator returned null".to_owned(),
        ));
    }
    shared.heap().array_mut(array).map_err(heap_error)?.elements = elements;
    Ok(Some(Value::Ref(array)))
}

/// `Stream.collect(Collector)`.
fn stream_collect(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let collector = reference_argument(args, 0, "Stream.collect")?;
    let kind = shared
        .heap()
        .collector(collector)
        .map_err(|error| {
            VmError::Native(format!(
                "Stream.collect received an object that is not a Collector: {error}"
            ))
        })?
        .kind
        .clone();
    let elements = stream_elements(shared, receiver, "Stream.collect")?;
    Ok(Some(Value::Ref(collect(shared, thread, &kind, elements)?)))
}

/// `Stream.collect(Supplier, BiConsumer, BiConsumer)`.
fn stream_collect_supplier(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let supplier = reference_argument(args, 0, "Stream.collect")?;
    let accumulator = reference_argument(args, 1, "Stream.collect")?;
    let result = call_supplier(shared, thread, supplier)?;
    for element in stream_elements(shared, receiver, "Stream.collect")? {
        call_bi_consumer(shared, thread, accumulator, result, element)?;
    }
    Ok(Some(result))
}

/// `Stream.toList()` (JDK 16): the eager elements as an `ArrayList`.
///
/// The JDK's list is unmodifiable; this slice has no unmodifiable wrapper, so the result
/// is the ordinary heap list. A caller that mutates it observes a normal list.
fn stream_to_list(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = stream_elements(shared, receiver, "Stream.toList")?;
    let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
    let list = shared.heap().allocate_list(class);
    shared
        .heap()
        .list_mut(list)
        .map_err(heap_error)?
        .extend(elements);
    Ok(Some(Value::Ref(list)))
}

/// `Stream.iterator()`: an iterator over the eager elements.
fn stream_iterator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let elements = stream_elements(shared, receiver, "Stream.iterator")?;
    let class = shared.load_library_class(ITERATOR_CLASS)?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_iterator(class, elements),
    )))
}

/// Execute one collector kind over `elements`.
fn collect(
    shared: &Shared,
    thread: &ThreadState,
    kind: &CollectorKind,
    elements: Vec<Value>,
) -> Result<ObjRef, VmError> {
    match kind {
        CollectorKind::ToList => {
            let class = shared.load_library_class(ARRAY_LIST_CLASS)?;
            let list = shared.heap().allocate_list(class);
            shared
                .heap()
                .list_mut(list)
                .map_err(heap_error)?
                .extend(elements);
            Ok(list)
        }
        CollectorKind::ToSet => {
            let class = shared.load_library_class(LINKED_HASH_SET_CLASS)?;
            let set = shared.heap().allocate_set(class);
            for element in elements {
                append_element(shared, set, element)?;
            }
            Ok(set)
        }
        CollectorKind::ToCollection { supplier } => {
            let target = call_supplier(shared, thread, *supplier)?;
            let Value::Ref(target) = target else {
                return Err(VmError::Native(
                    "Collectors.toCollection supplier returned a non-reference".to_owned(),
                ));
            };
            if target.is_null() {
                return Err(VmError::Native(
                    "Collectors.toCollection supplier returned null".to_owned(),
                ));
            }
            for element in elements {
                append_element(shared, target, element)?;
            }
            Ok(target)
        }
        CollectorKind::Joining {
            delimiter,
            prefix,
            suffix,
        } => {
            let mut joined = String::new();
            joined.push_str(prefix);
            for (position, element) in elements.iter().enumerate() {
                if position > 0 {
                    joined.push_str(delimiter);
                }
                joined.push_str(&text_of(shared, *element)?);
            }
            joined.push_str(suffix);
            Ok(shared.heap().allocate_string(joined))
        }
        // `Collectors.counting()` returns a `Long`, as the JDK does.
        CollectorKind::Counting => Ok(shared.box_long(elements.len() as i64)),
        CollectorKind::ToMap { key, value } => {
            let class = shared.load_library_class(HASH_MAP_CLASS)?;
            let map = shared.heap().allocate_map(class);
            for element in elements {
                let entry_key = call_function(shared, thread, *key, element)?;
                let entry_value = call_function(shared, thread, *value, element)?;
                shared
                    .heap()
                    .map_mut(map)
                    .map_err(heap_error)?
                    .push((entry_key, entry_value));
            }
            Ok(map)
        }
        CollectorKind::Custom {
            supplier,
            accumulator,
            combiner: _,
            finisher,
        } => {
            let accumulator_value = call_supplier(shared, thread, *supplier)?;
            for element in elements {
                call_bi_consumer(shared, thread, *accumulator, accumulator_value, element)?;
            }
            if finisher.is_null() {
                Ok(accumulator_value.as_ref().ok_or_else(|| {
                    VmError::Native("the collector accumulator is not a reference".to_owned())
                })?)
            } else {
                let Value::Ref(finished) =
                    call_function(shared, thread, *finisher, accumulator_value)?
                else {
                    return Err(VmError::Native(
                        "the collector finisher returned a non-reference".to_owned(),
                    ));
                };
                Ok(finished)
            }
        }
    }
}

/// `Collection.removeIf(Predicate)`: remove the matching elements and report whether
/// anything was removed.
///
/// A VM list or set is rebuilt in place. Any other receiver that implements `Collection`
/// (a Java view object such as Spring's `ConcurrentReferenceHashMap$KeySet`) is handled by
/// the JDK default method's semantics: walk the receiver's own `iterator()`, and call
/// `Iterator.remove()` for every element the predicate matches. That keeps a view's
/// removal visible through the receiver instead of snapshotting it into a detached set.
fn collection_remove_if(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "Collection.removeIf")?;
    let Some((elements, _is_set)) = collection_elements(shared, receiver)? else {
        return remove_if_through_iterator(shared, thread, receiver, predicate);
    };
    let mut kept = Vec::with_capacity(elements.len());
    let mut removed = false;
    for element in elements {
        if call_predicate(shared, thread, predicate, element)? {
            removed = true;
        } else {
            kept.push(element);
        }
    }
    if removed {
        match shared.heap().get_mut(receiver).map_err(heap_error)? {
            HeapObject::List(list) => list.elements = kept,
            HeapObject::Set(set) => set.elements = kept,
            _ => {
                return Err(VmError::Native(
                    "Collection.removeIf receiver is not a collection".to_owned(),
                ));
            }
        }
    }
    Ok(Some(Value::Int(i32::from(removed))))
}

/// The JDK default `Collection.removeIf` over a receiver's own `Iterator`.
fn remove_if_through_iterator(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    predicate: ObjRef,
) -> Result<Option<Value>, VmError> {
    let iterator = call_instance(
        shared,
        thread,
        receiver,
        "iterator",
        "()Ljava/util/Iterator;",
        Vec::new(),
    )?
    .and_then(as_reference)
    .ok_or_else(|| {
        VmError::Native(format!(
            "Collection.removeIf iterator() returned no iterator for {}",
            reference_class_name(shared, receiver)
        ))
    })?;
    let mut removed = false;
    loop {
        let has_next = call_instance(shared, thread, iterator, "hasNext", "()Z", Vec::new())?;
        if !matches!(has_next, Some(Value::Int(value)) if value != 0) {
            break;
        }
        let element = call_instance(
            shared,
            thread,
            iterator,
            "next",
            "()Ljava/lang/Object;",
            Vec::new(),
        )?
        .unwrap_or(Value::Ref(ObjRef::NULL));
        if call_predicate(shared, thread, predicate, element)? {
            call_instance(shared, thread, iterator, "remove", "()V", Vec::new())?;
            removed = true;
        }
    }
    Ok(Some(Value::Int(i32::from(removed))))
}

/// Run an instance method on a receiver, resolving it down the receiver's class chain.
fn call_instance(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, VmError> {
    let class = {
        let heap = shared.heap();
        heap.object_class(receiver).map_err(heap_error)?
    };
    let class = resolve_instance_class(shared, class, name, descriptor)?;
    run_instance_method(shared, thread, class, name, descriptor, receiver, args)
        .map_err(interp_to_vm)
}

fn as_reference(value: Value) -> Option<ObjRef> {
    match value {
        Value::Ref(reference) if !reference.is_null() => Some(reference),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------
// Spliterators
// ---------------------------------------------------------------------------------

/// Split a `characteristics` bit set for a list or set snapshot.
fn collection_characteristics(is_set: bool) -> i32 {
    let base = ORDERED | SIZED | SUBSIZED;
    if is_set { base | DISTINCT } else { base }
}

/// The elements and characteristics of a collection receiver.
fn collection_snapshot(
    shared: &Shared,
    receiver: ObjRef,
    operation: &str,
) -> Result<(Vec<Value>, i32), VmError> {
    match collection_elements(shared, receiver)? {
        Some((elements, is_set)) => Ok((elements, collection_characteristics(is_set))),
        None => Err(VmError::Native(format!(
            "{operation} receiver is not a collection: {}",
            reference_class_name(shared, receiver)
        ))),
    }
}

/// The elements of a VM list or set receiver, or `None` for any other object.
///
/// A Java class that implements `Collection` but is not a VM container (for example a
/// view object over another map) is `None`; the caller decides whether to run the Java
/// implementation or report a typed error.
fn collection_elements(
    shared: &Shared,
    receiver: ObjRef,
) -> Result<Option<(Vec<Value>, bool)>, VmError> {
    let heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        HeapObject::List(list) => Ok(Some((list.elements.clone(), false))),
        HeapObject::Set(set) => Ok(Some((set.elements.clone(), true))),
        _ => Ok(None),
    }
}

/// The runtime class name of a reference, for a typed error message.
///
/// The heap guard is taken and released before the class table is read, so this does not
/// hold one shared lock while acquiring another.
fn reference_class_name(shared: &Shared, reference: ObjRef) -> String {
    let class = {
        let heap = shared.heap();
        heap.object_class(reference).ok()
    };
    class
        .and_then(|class| shared.entry(class))
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| reference.to_string())
}

/// `Collection.stream()`.
fn collection_stream(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (elements, _) = collection_snapshot(shared, receiver, "Collection.stream")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, false, None),
    )))
}

/// `Collection.parallelStream()`.
fn collection_parallel_stream(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (elements, _) = collection_snapshot(shared, receiver, "Collection.parallelStream")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, true, None),
    )))
}

/// `Collection.spliterator()`.
fn collection_spliterator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (elements, characteristics) =
        collection_snapshot(shared, receiver, "Collection.spliterator")?;
    Ok(Some(Value::Ref(
        shared
            .heap()
            .allocate_spliterator(elements, characteristics),
    )))
}

/// `Spliterator.tryAdvance(Consumer)`.
fn spliterator_try_advance(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let consumer = reference_argument(args, 0, "Spliterator.tryAdvance")?;
    let next = {
        let mut heap = shared.heap();
        let state = match heap.get_mut(receiver).map_err(heap_error)? {
            HeapObject::Spliterator(state) => state,
            _ => {
                return Err(VmError::Native(
                    "Spliterator.tryAdvance receiver is not a VM spliterator".to_owned(),
                ));
            }
        };
        if state.position >= state.elements.len() {
            None
        } else {
            let value = state.elements[state.position];
            state.position += 1;
            Some(value)
        }
    };
    match next {
        Some(value) => {
            call_consumer(shared, thread, consumer, value)?;
            Ok(Some(Value::Int(1)))
        }
        None => Ok(Some(Value::Int(0))),
    }
}

/// `Spliterator.forEachRemaining(Consumer)`.
fn spliterator_for_each_remaining(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let consumer = reference_argument(args, 0, "Spliterator.forEachRemaining")?;
    let remaining = {
        let mut heap = shared.heap();
        match heap.get_mut(receiver).map_err(heap_error)? {
            HeapObject::Spliterator(state) => {
                let remaining = state.elements.split_off(state.position);
                state.position = state.elements.len();
                remaining
            }
            _ => {
                return Err(VmError::Native(
                    "Spliterator.forEachRemaining receiver is not a VM spliterator".to_owned(),
                ));
            }
        }
    };
    for value in remaining {
        call_consumer(shared, thread, consumer, value)?;
    }
    Ok(None)
}

/// `Spliterator.trySplit()`: the eager VM spliterator does not split.
fn spliterator_try_split(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

/// `Spliterator.estimateSize()` / `getExactSizeIfKnown()`.
fn spliterator_estimate_size(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let heap = shared.heap();
    let state = heap.spliterator(receiver).map_err(heap_error)?;
    Ok(Some(Value::Long(
        (state.elements.len() - state.position) as i64,
    )))
}

/// `Spliterator.characteristics()`.
fn spliterator_characteristics(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let heap = shared.heap();
    let state = heap.spliterator(receiver).map_err(heap_error)?;
    Ok(Some(Value::Int(state.characteristics)))
}

/// `Spliterator.hasCharacteristics(int)`.
fn spliterator_has_characteristics(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let mask = int_argument(args, 0, "Spliterator.hasCharacteristics")?;
    let heap = shared.heap();
    let state = heap.spliterator(receiver).map_err(heap_error)?;
    Ok(Some(Value::Int(i32::from(
        state.characteristics & mask == mask,
    ))))
}

/// `Spliterator.getComparator()`: a VM spliterator has no comparator.
fn spliterator_get_comparator(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

/// `Spliterators.emptySpliterator()`.
fn spliterators_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared
            .heap()
            .allocate_spliterator(Vec::new(), SIZED | SUBSIZED),
    )))
}

/// `Spliterators.spliterator(Object[], int)` and its `from`/`to` form.
fn spliterators_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = reference_argument(args, 0, "Spliterators.spliterator")?;
    let elements = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    let (start, end, characteristics) = match args.len() {
        2 => (
            0,
            elements.len(),
            int_argument(args, 1, "Spliterators.spliterator")?,
        ),
        _ => (
            int_argument(args, 1, "Spliterators.spliterator")? as usize,
            int_argument(args, 2, "Spliterators.spliterator")? as usize,
            int_argument(args, 3, "Spliterators.spliterator")?,
        ),
    };
    if start > end || end > elements.len() {
        return Err(VmError::Native(format!(
            "Spliterators.spliterator range {start}..{end} is outside {} element(s)",
            elements.len()
        )));
    }
    Ok(Some(Value::Ref(shared.heap().allocate_spliterator(
        elements[start..end].to_vec(),
        characteristics,
    ))))
}

/// `Spliterators.spliterator(Collection, int)`.
fn spliterators_collection(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let collection = reference_argument(args, 0, "Spliterators.spliterator")?;
    let characteristics = int_argument(args, 1, "Spliterators.spliterator")?;
    let (elements, base) = collection_snapshot(shared, collection, "Spliterators.spliterator")?;
    Ok(Some(Value::Ref(
        shared
            .heap()
            .allocate_spliterator(elements, characteristics | base),
    )))
}

/// `Spliterators.iterator(Spliterator)`: materialize the remaining elements and wrap them
/// in the VM iterator.
fn spliterators_iterator(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let spliterator = reference_argument(args, 0, "Spliterators.iterator")?;
    let elements = drain_spliterator(shared, thread, spliterator)?;
    let class = shared.load_library_class(ITERATOR_CLASS)?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_iterator(class, elements),
    )))
}

// ---------------------------------------------------------------------------------
// Collectors
// ---------------------------------------------------------------------------------

/// One collector over the given kind.
fn make_collector(shared: &Shared, kind: CollectorKind) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(shared.heap().allocate_collector(kind))))
}

/// `Collectors.toList()` / `toUnmodifiableList()`.
fn collectors_to_list(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    make_collector(shared, CollectorKind::ToList)
}

/// `Collectors.toSet()` / `toUnmodifiableSet()`.
fn collectors_to_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    make_collector(shared, CollectorKind::ToSet)
}

/// `Collectors.toCollection(Supplier)`.
fn collectors_to_collection(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let supplier = reference_argument(args, 0, "Collectors.toCollection")?;
    make_collector(shared, CollectorKind::ToCollection { supplier })
}

/// `Collectors.joining(...)`.
fn collectors_joining(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = |position: usize| -> Result<String, VmError> {
        match args.get(position).copied() {
            Some(Value::Ref(reference)) if reference.is_null() => Ok(String::new()),
            Some(value) => text_of(shared, value),
            None => Ok(String::new()),
        }
    };
    let (delimiter, prefix, suffix) = match args.len() {
        0 => (String::new(), String::new(), String::new()),
        1 => (text(0)?, String::new(), String::new()),
        _ => (text(0)?, text(1)?, text(2)?),
    };
    make_collector(
        shared,
        CollectorKind::Joining {
            delimiter,
            prefix,
            suffix,
        },
    )
}

/// `Collectors.counting()`.
fn collectors_counting(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    make_collector(shared, CollectorKind::Counting)
}

/// `Collectors.toMap(keyMapper, valueMapper)`.
fn collectors_to_map(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = reference_argument(args, 0, "Collectors.toMap")?;
    let value = reference_argument(args, 1, "Collectors.toMap")?;
    make_collector(shared, CollectorKind::ToMap { key, value })
}

/// `Collector.of(...)`, both the four-argument and the five-argument form.
fn collector_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let supplier = reference_argument(args, 0, "Collector.of")?;
    let accumulator = reference_argument(args, 1, "Collector.of")?;
    let combiner = reference_argument(args, 2, "Collector.of")?;
    // The four-argument form's fourth value is the characteristics array, so only the
    // five-argument form stores a finisher.
    let finisher = match args.get(3).copied() {
        Some(Value::Ref(reference)) if args.len() >= 5 => reference,
        _ => ObjRef::NULL,
    };
    make_collector(
        shared,
        CollectorKind::Custom {
            supplier,
            accumulator,
            combiner,
            finisher,
        },
    )
}

// ---------------------------------------------------------------------------------
// Optional
// ---------------------------------------------------------------------------------

/// `Optional.empty()`.
fn optional_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(shared.heap().allocate_optional(None))))
}

/// `Optional.of(Object)`.
fn optional_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    if matches!(value, Value::Ref(reference) if reference.is_null()) {
        return Err(VmError::Native(
            "Optional.of received a null reference".to_owned(),
        ));
    }
    Ok(Some(Value::Ref(
        shared.heap().allocate_optional(Some(value)),
    )))
}

/// `Optional.ofNullable(Object)`.
fn optional_of_nullable(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match args.first().copied() {
        Some(Value::Ref(reference)) if reference.is_null() => None,
        Some(value) => Some(value),
        None => None,
    };
    Ok(Some(Value::Ref(shared.heap().allocate_optional(value))))
}

/// `Optional.isPresent()` / `isEmpty()`.
fn optional_is_present(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    Ok(Some(Value::Int(i32::from(value.is_some()))))
}

fn optional_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    Ok(Some(Value::Int(i32::from(value.is_none()))))
}

/// `Optional.get()`.
fn optional_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    value
        .ok_or_else(|| VmError::Native("Optional.get on an empty Optional".to_owned()))
        .map(Some)
}

/// `Optional.orElse(Object)`.
fn optional_or_else(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    Ok(Some(value.unwrap_or_else(|| {
        args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL))
    })))
}

/// `Optional.orElseGet(Supplier)`.
fn optional_or_else_get(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    match value {
        Some(value) => Ok(Some(value)),
        None => {
            let supplier = reference_argument(args, 0, "Optional.orElseGet")?;
            Ok(Some(call_supplier(shared, thread, supplier)?))
        }
    }
}

/// `Optional.orElseThrow(Supplier)`: the supplier builds the throwable, which is then
/// thrown as the JDK's `orElseThrow` does.
fn optional_or_else_throw(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    match value {
        Some(value) => Ok(Some(value)),
        None => {
            let supplier = reference_argument(args, 0, "Optional.orElseThrow")?;
            let throwable = call_supplier(shared, thread, supplier)?;
            match throwable {
                Value::Ref(reference) if !reference.is_null() => Err(VmError::Thrown(reference)),
                _ => Err(VmError::Native(
                    "Optional.orElseThrow supplier did not produce a throwable".to_owned(),
                )),
            }
        }
    }
}

/// `Optional.orElseThrow()`: the value, or a typed empty-Optional error.
fn optional_or_else_throw_no_supplier(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    match shared.heap().optional(receiver).map_err(heap_error)? {
        Some(value) => Ok(Some(value)),
        None => Err(VmError::Native(
            "Optional.orElseThrow on an empty Optional (NoSuchElementException is not synthesized)"
                .to_owned(),
        )),
    }
}

/// `Optional.ifPresent(Consumer)`.
fn optional_if_present(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    if let Some(value) = value {
        let consumer = reference_argument(args, 0, "Optional.ifPresent")?;
        call_consumer(shared, thread, consumer, value)?;
    }
    Ok(None)
}

/// `Optional.map(Function)`.
fn optional_map(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    let mapped = match value {
        Some(value) => {
            let function = reference_argument(args, 0, "Optional.map")?;
            match call_function(shared, thread, function, value)? {
                Value::Ref(reference) if reference.is_null() => None,
                other => Some(other),
            }
        }
        None => None,
    };
    Ok(Some(Value::Ref(shared.heap().allocate_optional(mapped))))
}

/// `Optional.filter(Predicate)`.
fn optional_filter(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = shared.heap().optional(receiver).map_err(heap_error)?;
    let kept = match value {
        Some(value) => {
            let predicate = reference_argument(args, 0, "Optional.filter")?;
            if call_predicate(shared, thread, predicate, value)? {
                Some(value)
            } else {
                None
            }
        }
        None => None,
    };
    Ok(Some(Value::Ref(shared.heap().allocate_optional(kept))))
}

/// `Optional.toString()`.
fn optional_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = match shared.heap().optional(receiver).map_err(heap_error)? {
        Some(value) => format!("Optional[{}]", text_of(shared, value)?),
        None => "Optional.empty".to_owned(),
    };
    Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
}

// ---------------------------------------------------------------------------------
// Map.forEach
// ---------------------------------------------------------------------------------

/// `Map.forEach(BiConsumer)`.
fn map_for_each(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let consumer = reference_argument(args, 0, "Map.forEach")?;
    let entries = {
        let heap = shared.heap();
        heap.map(receiver)
            .map_err(|error| {
                VmError::Native(format!("Map.forEach receiver is not a map: {error}"))
            })?
            .to_vec()
    };
    for (key, value) in entries {
        call_bi_consumer(shared, thread, consumer, key, value)?;
    }
    Ok(None)
}

// ---------------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------------

/// Register every M2.24 native.
pub fn register_natives(shared: &Shared) {
    register_int_stream_natives(shared);
    // Stream sources.
    shared.register_native(
        "java/util/stream/StreamSupport",
        "stream",
        "(Ljava/util/Spliterator;Z)Ljava/util/stream/Stream;",
        stream_support_stream,
    );
    for (name, descriptor, function) in [
        (
            "empty",
            "()Ljava/util/stream/Stream;",
            stream_empty as rava_vm::NativeFn,
        ),
        (
            "of",
            "(Ljava/lang/Object;)Ljava/util/stream/Stream;",
            stream_of_one,
        ),
        (
            "of",
            "([Ljava/lang/Object;)Ljava/util/stream/Stream;",
            stream_of_many,
        ),
        (
            "ofNullable",
            "(Ljava/lang/Object;)Ljava/util/stream/Stream;",
            stream_of_nullable,
        ),
        (
            "concat",
            "(Ljava/util/stream/Stream;Ljava/util/stream/Stream;)Ljava/util/stream/Stream;",
            stream_concat,
        ),
    ] {
        shared.register_native(STREAM_CLASS, name, descriptor, function);
    }

    // Intermediate operations.
    for (name, descriptor, function) in [
        (
            "filter",
            "(Ljava/util/function/Predicate;)Ljava/util/stream/Stream;",
            stream_filter as rava_vm::NativeFn,
        ),
        (
            "map",
            "(Ljava/util/function/Function;)Ljava/util/stream/Stream;",
            stream_map,
        ),
        (
            "flatMap",
            "(Ljava/util/function/Function;)Ljava/util/stream/Stream;",
            stream_flat_map,
        ),
        ("distinct", "()Ljava/util/stream/Stream;", stream_distinct),
        ("sorted", "()Ljava/util/stream/Stream;", stream_sorted),
        (
            "sorted",
            "(Ljava/util/Comparator;)Ljava/util/stream/Stream;",
            stream_sorted,
        ),
        (
            "peek",
            "(Ljava/util/function/Consumer;)Ljava/util/stream/Stream;",
            stream_peek,
        ),
        ("limit", "(J)Ljava/util/stream/Stream;", stream_limit),
        ("skip", "(J)Ljava/util/stream/Stream;", stream_skip),
    ] {
        shared.register_native(STREAM_CLASS, name, descriptor, function);
    }

    // Terminal operations.
    for (name, descriptor, function) in [
        (
            "forEach",
            "(Ljava/util/function/Consumer;)V",
            stream_for_each as rava_vm::NativeFn,
        ),
        (
            "forEachOrdered",
            "(Ljava/util/function/Consumer;)V",
            stream_for_each,
        ),
        ("count", "()J", stream_count),
        (
            "anyMatch",
            "(Ljava/util/function/Predicate;)Z",
            stream_any_match,
        ),
        (
            "allMatch",
            "(Ljava/util/function/Predicate;)Z",
            stream_all_match,
        ),
        (
            "noneMatch",
            "(Ljava/util/function/Predicate;)Z",
            stream_none_match,
        ),
        ("findFirst", "()Ljava/util/Optional;", stream_find_first),
        ("findAny", "()Ljava/util/Optional;", stream_find_first),
        (
            "min",
            "(Ljava/util/Comparator;)Ljava/util/Optional;",
            stream_min,
        ),
        (
            "max",
            "(Ljava/util/Comparator;)Ljava/util/Optional;",
            stream_max,
        ),
        (
            "reduce",
            "(Ljava/lang/Object;Ljava/util/function/BinaryOperator;)Ljava/lang/Object;",
            stream_reduce_identity,
        ),
        ("toArray", "()[Ljava/lang/Object;", stream_to_array),
        (
            "toArray",
            "(Ljava/util/function/IntFunction;)[Ljava/lang/Object;",
            stream_to_array_generator,
        ),
        (
            "collect",
            "(Ljava/util/stream/Collector;)Ljava/lang/Object;",
            stream_collect,
        ),
        ("toList", "()Ljava/util/List;", stream_to_list),
        (
            "collect",
            "(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BiConsumer;)Ljava/lang/Object;",
            stream_collect_supplier,
        ),
    ] {
        shared.register_native(STREAM_CLASS, name, descriptor, function);
    }

    // BaseStream. The return type erases to `BaseStream`, not `Stream`: javac emits
    // `invokeinterface BaseStream.parallel:()Ljava/util/stream/BaseStream;` and casts the
    // result, so a `Stream`-typed descriptor here is unreachable.
    for (name, descriptor, function) in [
        ("isParallel", "()Z", stream_is_parallel as rava_vm::NativeFn),
        (
            "sequential",
            "()Ljava/util/stream/BaseStream;",
            stream_sequential,
        ),
        (
            "parallel",
            "()Ljava/util/stream/BaseStream;",
            stream_parallel,
        ),
        (
            "unordered",
            "()Ljava/util/stream/BaseStream;",
            stream_unordered,
        ),
        (
            "onClose",
            "(Ljava/lang/Runnable;)Ljava/util/stream/BaseStream;",
            stream_on_close,
        ),
        ("close", "()V", stream_close),
        ("iterator", "()Ljava/util/Iterator;", stream_iterator),
    ] {
        shared.register_native(BASE_STREAM_CLASS, name, descriptor, function);
    }

    // Collection sources. A class inherits `stream`/`spliterator` from `Collection`,
    // while `spliterator` is also declared on the concrete list and set classes.
    for class in [
        "java/util/Collection",
        "java/util/List",
        "java/util/Set",
        "java/util/AbstractCollection",
        "java/util/AbstractList",
        "java/util/AbstractSet",
        ARRAY_LIST_CLASS,
        "java/util/LinkedList",
        "java/util/concurrent/CopyOnWriteArrayList",
        "java/util/ArrayDeque",
        "java/util/HashSet",
        LINKED_HASH_SET_CLASS,
        "java/util/TreeSet",
        "java/util/concurrent/CopyOnWriteArraySet",
        "java/util/EnumSet",
    ] {
        shared.register_native(
            class,
            "stream",
            "()Ljava/util/stream/Stream;",
            collection_stream,
        );
        shared.register_native(
            class,
            "parallelStream",
            "()Ljava/util/stream/Stream;",
            collection_parallel_stream,
        );
        shared.register_native(
            class,
            "spliterator",
            "()Ljava/util/Spliterator;",
            collection_spliterator,
        );
    }

    // Collection.removeIf and the aborting reduce forms follow the same predicate and
    // accumulator helpers.
    for class in [
        "java/util/Collection",
        "java/util/List",
        "java/util/Set",
        "java/util/AbstractCollection",
        ARRAY_LIST_CLASS,
        "java/util/LinkedList",
        "java/util/concurrent/CopyOnWriteArrayList",
        "java/util/ArrayDeque",
        "java/util/HashSet",
        LINKED_HASH_SET_CLASS,
        "java/util/TreeSet",
        "java/util/concurrent/CopyOnWriteArraySet",
        "java/util/EnumSet",
    ] {
        shared.register_native(
            class,
            "removeIf",
            "(Ljava/util/function/Predicate;)Z",
            collection_remove_if,
        );
    }

    // Spliterator.
    for (name, descriptor, function) in [
        (
            "tryAdvance",
            "(Ljava/util/function/Consumer;)Z",
            spliterator_try_advance as rava_vm::NativeFn,
        ),
        (
            "forEachRemaining",
            "(Ljava/util/function/Consumer;)V",
            spliterator_for_each_remaining,
        ),
        (
            "trySplit",
            "()Ljava/util/Spliterator;",
            spliterator_try_split,
        ),
        ("estimateSize", "()J", spliterator_estimate_size),
        ("getExactSizeIfKnown", "()J", spliterator_estimate_size),
        ("characteristics", "()I", spliterator_characteristics),
        (
            "hasCharacteristics",
            "(I)Z",
            spliterator_has_characteristics,
        ),
        (
            "getComparator",
            "()Ljava/util/Comparator;",
            spliterator_get_comparator,
        ),
    ] {
        shared.register_native(SPLITERATOR_CLASS, name, descriptor, function);
    }

    // Spliterators.
    shared.register_native(
        "java/util/Spliterators",
        "emptySpliterator",
        "()Ljava/util/Spliterator;",
        spliterators_empty,
    );
    shared.register_native(
        "java/util/Spliterators",
        "spliterator",
        "([Ljava/lang/Object;I)Ljava/util/Spliterator;",
        spliterators_array,
    );
    shared.register_native(
        "java/util/Spliterators",
        "spliterator",
        "([Ljava/lang/Object;III)Ljava/util/Spliterator;",
        spliterators_array,
    );
    shared.register_native(
        "java/util/Spliterators",
        "spliterator",
        "(Ljava/util/Collection;I)Ljava/util/Spliterator;",
        spliterators_collection,
    );
    shared.register_native(
        "java/util/Spliterators",
        "iterator",
        "(Ljava/util/Spliterator;)Ljava/util/Iterator;",
        spliterators_iterator,
    );

    // Collectors.
    for (name, descriptor, function) in [
        (
            "toList",
            "()Ljava/util/stream/Collector;",
            collectors_to_list as rava_vm::NativeFn,
        ),
        (
            "toUnmodifiableList",
            "()Ljava/util/stream/Collector;",
            collectors_to_list,
        ),
        ("toSet", "()Ljava/util/stream/Collector;", collectors_to_set),
        (
            "toUnmodifiableSet",
            "()Ljava/util/stream/Collector;",
            collectors_to_set,
        ),
        (
            "toCollection",
            "(Ljava/util/function/Supplier;)Ljava/util/stream/Collector;",
            collectors_to_collection,
        ),
        (
            "joining",
            "()Ljava/util/stream/Collector;",
            collectors_joining,
        ),
        (
            "joining",
            "(Ljava/lang/CharSequence;)Ljava/util/stream/Collector;",
            collectors_joining,
        ),
        (
            "joining",
            "(Ljava/lang/CharSequence;Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Ljava/util/stream/Collector;",
            collectors_joining,
        ),
        (
            "counting",
            "()Ljava/util/stream/Collector;",
            collectors_counting,
        ),
        (
            "toMap",
            "(Ljava/util/function/Function;Ljava/util/function/Function;)Ljava/util/stream/Collector;",
            collectors_to_map,
        ),
    ] {
        shared.register_native("java/util/stream/Collectors", name, descriptor, function);
    }

    // Collector.of.
    shared.register_native(
        COLLECTOR_CLASS,
        "of",
        "(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BinaryOperator;[Ljava/util/stream/Collector$Characteristics;)Ljava/util/stream/Collector;",
        collector_of,
    );
    shared.register_native(
        COLLECTOR_CLASS,
        "of",
        "(Ljava/util/function/Supplier;Ljava/util/function/BiConsumer;Ljava/util/function/BinaryOperator;Ljava/util/function/Function;[Ljava/util/stream/Collector$Characteristics;)Ljava/util/stream/Collector;",
        collector_of,
    );

    // Optional.
    for (name, descriptor, function) in [
        (
            "empty",
            "()Ljava/util/Optional;",
            optional_empty as rava_vm::NativeFn,
        ),
        (
            "of",
            "(Ljava/lang/Object;)Ljava/util/Optional;",
            optional_of,
        ),
        (
            "ofNullable",
            "(Ljava/lang/Object;)Ljava/util/Optional;",
            optional_of_nullable,
        ),
        ("get", "()Ljava/lang/Object;", optional_get),
        ("isPresent", "()Z", optional_is_present),
        ("isEmpty", "()Z", optional_is_empty),
        (
            "orElse",
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            optional_or_else,
        ),
        (
            "orElseGet",
            "(Ljava/util/function/Supplier;)Ljava/lang/Object;",
            optional_or_else_get,
        ),
        (
            "orElseThrow",
            "(Ljava/util/function/Supplier;)Ljava/lang/Object;",
            optional_or_else_throw,
        ),
        (
            "orElseThrow",
            "()Ljava/lang/Object;",
            optional_or_else_throw_no_supplier,
        ),
        (
            "ifPresent",
            "(Ljava/util/function/Consumer;)V",
            optional_if_present,
        ),
        (
            "map",
            "(Ljava/util/function/Function;)Ljava/util/Optional;",
            optional_map,
        ),
        (
            "filter",
            "(Ljava/util/function/Predicate;)Ljava/util/Optional;",
            optional_filter,
        ),
        ("toString", "()Ljava/lang/String;", optional_to_string),
    ] {
        shared.register_native(OPTIONAL_CLASS, name, descriptor, function);
    }

    // The internal consumer a Java spliterator calls back into.
    shared.register_native(CONSUMER_CLASS, "accept", CONSUMER_ACCEPT, sink_accept);

    // Map.forEach.
    for class in [
        "java/util/Map",
        HASH_MAP_CLASS,
        "java/util/LinkedHashMap",
        "java/util/TreeMap",
        "java/util/Hashtable",
        "java/util/Properties",
        "java/util/concurrent/ConcurrentHashMap",
        "java/util/IdentityHashMap",
    ] {
        shared.register_native(
            class,
            "forEach",
            "(Ljava/util/function/BiConsumer;)V",
            map_for_each,
        );
    }
}

/// `Consumer.accept(Object)` on the internal sink: append to its destination stream.
fn sink_accept(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let destination = {
        let heap = shared.heap();
        heap.sink(receiver)
            .map_err(|error| {
                VmError::Native(format!(
                    "Consumer.accept receiver is not a VM sink: {error}"
                ))
            })?
            .destination
    };
    shared
        .heap()
        .stream_mut(destination)
        .map_err(heap_error)?
        .elements
        .push(value);
    Ok(None)
}

/// Register the `java.util.stream.IntStream` surface.
///
/// `String.chars()` is the source; `allMatch`/`anyMatch`/`noneMatch` and `filter` take an
/// `IntPredicate` (`(I)Z`), whose element is an `int` value. The eager pipeline keeps the
/// `IntStream` class tag so chained primitive-stream calls resolve here.
fn register_int_stream_natives(shared: &Shared) {
    shared.register_native(
        INT_STREAM_CLASS,
        "allMatch",
        "(Ljava/util/function/IntPredicate;)Z",
        int_stream_all_match,
    );
    shared.register_native(
        INT_STREAM_CLASS,
        "anyMatch",
        "(Ljava/util/function/IntPredicate;)Z",
        int_stream_any_match,
    );
    shared.register_native(
        INT_STREAM_CLASS,
        "noneMatch",
        "(Ljava/util/function/IntPredicate;)Z",
        int_stream_none_match,
    );
    shared.register_native(
        INT_STREAM_CLASS,
        "filter",
        "(Ljava/util/function/IntPredicate;)Ljava/util/stream/IntStream;",
        int_stream_filter,
    );
    shared.register_native(INT_STREAM_CLASS, "count", "()J", int_stream_count);
    shared.register_native(INT_STREAM_CLASS, "sum", "()I", int_stream_sum);
    shared.register_native(INT_STREAM_CLASS, "toArray", "()[I", int_stream_to_array);
}

/// A stream's elements as `i32`s, or a typed error when an element is not an int.
fn int_stream_values(shared: &Shared, receiver: ObjRef, method: &str) -> Result<Vec<i32>, VmError> {
    let mut values = Vec::new();
    for element in stream_elements(shared, receiver, method)? {
        match element {
            Value::Int(value) => values.push(value),
            _ => {
                return Err(VmError::Native(format!(
                    "{method} found a non-int element in an IntStream"
                )));
            }
        }
    }
    Ok(values)
}

fn int_predicate(
    shared: &Shared,
    thread: &ThreadState,
    predicate: ObjRef,
    value: i32,
) -> Result<bool, VmError> {
    match call_sam(
        shared,
        thread,
        predicate,
        "test",
        "(I)Z",
        vec![Value::Int(value)],
    )? {
        Some(Value::Int(result)) => Ok(result != 0),
        _ => Err(VmError::Native(
            "IntPredicate.test did not return a boolean".to_owned(),
        )),
    }
}

fn int_stream_all_match(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "IntStream.allMatch")?;
    for value in int_stream_values(shared, receiver, "IntStream.allMatch")? {
        if !int_predicate(shared, thread, predicate, value)? {
            return Ok(Some(Value::Int(0)));
        }
    }
    Ok(Some(Value::Int(1)))
}

fn int_stream_any_match(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "IntStream.anyMatch")?;
    for value in int_stream_values(shared, receiver, "IntStream.anyMatch")? {
        if int_predicate(shared, thread, predicate, value)? {
            return Ok(Some(Value::Int(1)));
        }
    }
    Ok(Some(Value::Int(0)))
}

fn int_stream_none_match(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "IntStream.noneMatch")?;
    for value in int_stream_values(shared, receiver, "IntStream.noneMatch")? {
        if int_predicate(shared, thread, predicate, value)? {
            return Ok(Some(Value::Int(0)));
        }
    }
    Ok(Some(Value::Int(1)))
}

fn int_stream_filter(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let predicate = reference_argument(args, 0, "IntStream.filter")?;
    let mut kept = Vec::new();
    for value in int_stream_values(shared, receiver, "IntStream.filter")? {
        if int_predicate(shared, thread, predicate, value)? {
            kept.push(Value::Int(value));
        }
    }
    Ok(Some(Value::Ref(same_shape_stream(shared, receiver, kept)?)))
}

fn int_stream_count(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = stream_elements(shared, receiver, "IntStream.count")?.len();
    Ok(Some(Value::Long(count as i64)))
}

fn int_stream_sum(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let sum = int_stream_values(shared, receiver, "IntStream.sum")?
        .into_iter()
        .fold(0i32, i32::wrapping_add);
    Ok(Some(Value::Int(sum)))
}

fn int_stream_to_array(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let values = int_stream_values(shared, receiver, "IntStream.toArray")?;
    let array = shared.heap().allocate_array(
        ClassRef::Primitive(rava_vm::PrimitiveKind::Int),
        values.len(),
    );
    {
        let mut heap = shared.heap();
        let slots = heap.array_mut(array).map_err(heap_error)?;
        for (slot, value) in slots.elements.iter_mut().zip(values) {
            *slot = Value::Int(value);
        }
    }
    Ok(Some(Value::Ref(array)))
}
