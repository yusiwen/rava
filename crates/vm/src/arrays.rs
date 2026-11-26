//! `java.util.Arrays` natives (M2.23).
//!
//! Spring sorts and copies arrays while ordering bean post-processors and annotation
//! metadata: `Arrays.sort(Object[])`, `Arrays.asList(Object[])`, `Arrays.copyOf` and
//! `Arrays.toString`. The implementations operate on the heap's existing array shape and
//! keep the component type, so an `Adapt[]` stays an `Adapt[]` across a copy.
//!
//! Natural ordering covers the element types this slice models: strings compare by
//! content, boxed `Integer`s numerically, and every other reference keeps its input order
//! (there is no `Comparable` dispatch yet). A comparator-taking `sort` is not registered
//! because it needs the interpreter's callback bridge.

use crate::VmError;
use crate::heap::{HeapObject, ObjRef};
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const ARRAYS_CLASS: &str = "java/util/Arrays";

pub fn register_natives(shared: &Shared) {
    shared.register_native(ARRAYS_CLASS, "sort", "([Ljava/lang/Object;)V", arrays_sort);
    // The primitive overloads share the one implementation: a primitive array's elements
    // are already `Value::Int`/`Long`/`Float`/`Double`, which `natural_compare` orders.
    for descriptor in [
        "([I)V", "([J)V", "([B)V", "([S)V", "([C)V", "([F)V", "([D)V",
    ] {
        shared.register_native(ARRAYS_CLASS, "sort", descriptor, arrays_sort);
    }
    shared.register_native(
        ARRAYS_CLASS,
        "asList",
        "([Ljava/lang/Object;)Ljava/util/List;",
        arrays_as_list,
    );
    shared.register_native(
        ARRAYS_CLASS,
        "copyOf",
        "([Ljava/lang/Object;I)[Ljava/lang/Object;",
        arrays_copy_of,
    );
    shared.register_native(
        ARRAYS_CLASS,
        "copyOfRange",
        "([Ljava/lang/Object;II)[Ljava/lang/Object;",
        arrays_copy_of_range,
    );
    for descriptor in [
        "([II)V", "([JJ)V", "([BB)V", "([SS)V", "([CC)V", "([ZZ)V", "([FF)V", "([DD)V",
    ] {
        shared.register_native(ARRAYS_CLASS, "fill", descriptor, arrays_fill);
    }
    shared.register_native(
        ARRAYS_CLASS,
        "fill",
        "([Ljava/lang/Object;Ljava/lang/Object;)V",
        arrays_fill,
    );
    shared.register_native(
        ARRAYS_CLASS,
        "hashCode",
        "([Ljava/lang/Object;)I",
        arrays_hash_code,
    );
    shared.register_native(
        ARRAYS_CLASS,
        "equals",
        "([Ljava/lang/Object;[Ljava/lang/Object;)Z",
        arrays_equals,
    );
    shared.register_native(
        ARRAYS_CLASS,
        "toString",
        "([Ljava/lang/Object;)Ljava/lang/String;",
        arrays_to_string,
    );
    // `SpringFactoriesLoader` now reads `META-INF/spring.factories` through the M4.1
    // resources and turns the factory names into a stream. The eager stream shape already
    // exists in the heap, so this is only the array entry point.
    shared.register_native(
        ARRAYS_CLASS,
        "stream",
        "([Ljava/lang/Object;)Ljava/util/stream/Stream;",
        arrays_stream,
    );
    shared.register_native(
        ARRAYS_CLASS,
        "stream",
        "([Ljava/lang/Object;II)Ljava/util/stream/Stream;",
        arrays_stream_range,
    );
}

/// `Arrays.stream(Object[])`: an eager stream over the array's current elements.
fn arrays_stream(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.stream")?;
    let elements = {
        let heap = shared.heap();
        let array = heap.array(array).map_err(heap_error)?;
        array.elements.clone()
    };
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, false, None),
    )))
}

/// `Arrays.stream(Object[], int, int)`: the `[start, end)` slice as a stream.
fn arrays_stream_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.stream")?;
    let start = int_argument(args, 1, "Arrays.stream")?;
    let end = int_argument(args, 2, "Arrays.stream")?;
    let elements = {
        let heap = shared.heap();
        let array = heap.array(array).map_err(heap_error)?;
        let start = usize::try_from(start)
            .map_err(|_| VmError::Native(format!("Arrays.stream: negative start {start}")))?;
        let end = usize::try_from(end)
            .map_err(|_| VmError::Native(format!("Arrays.stream: negative end {end}")))?;
        if start > end || end > array.elements.len() {
            return Err(VmError::Native(format!(
                "Arrays.stream: range {start}..{end} is outside an array of length {}",
                array.elements.len()
            )));
        }
        array.elements[start..end].to_vec()
    };
    Ok(Some(Value::Ref(
        shared.heap().allocate_stream(elements, false, None),
    )))
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn array_argument(
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{method} expects an array argument"
        ))),
    }
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(format!("{method} expects an int argument"))),
    }
}

/// Compare two values for `Arrays.sort`: natural order for primitives, and content for a
/// string or a box among object elements (every other reference keeps its input order).
fn natural_compare(heap: &crate::Heap, left: Value, right: Value) -> std::cmp::Ordering {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => left.cmp(&right),
        (Value::Long(left), Value::Long(right)) => left.cmp(&right),
        (Value::Float(left), Value::Float(right)) => compare_float(left, right),
        (Value::Double(left), Value::Double(right)) => compare_double(left, right),
        (Value::Ref(left_ref), Value::Ref(right_ref))
            if !left_ref.is_null() && !right_ref.is_null() =>
        {
            match (heap.get(left_ref), heap.get(right_ref)) {
                (Ok(HeapObject::Str(left)), Ok(HeapObject::Str(right))) => left.cmp(right),
                (Ok(HeapObject::Boxed(left)), Ok(HeapObject::Boxed(right))) => left
                    .java_compare(*right)
                    .unwrap_or(std::cmp::Ordering::Equal),
                _ => std::cmp::Ordering::Equal,
            }
        }
        _ => std::cmp::Ordering::Equal,
    }
}

/// `Float.compare(float, float)`: `NaN` sorts above every value and `-0.0` below `0.0`.
fn compare_float(left: f32, right: f32) -> std::cmp::Ordering {
    if left < right {
        return std::cmp::Ordering::Less;
    }
    if left > right {
        return std::cmp::Ordering::Greater;
    }
    left.to_bits().cmp(&right.to_bits())
}

/// `Double.compare(double, double)`: the `Float.compare` order widened to `double`.
fn compare_double(left: f64, right: f64) -> std::cmp::Ordering {
    if left < right {
        return std::cmp::Ordering::Less;
    }
    if left > right {
        return std::cmp::Ordering::Greater;
    }
    left.to_bits().cmp(&right.to_bits())
}

fn arrays_sort(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.sort")?;
    let mut elements = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    elements.sort_by(|left, right| natural_compare(&shared.heap(), *left, *right));
    shared.heap().array_mut(array).map_err(heap_error)?.elements = elements;
    Ok(None)
}

fn arrays_as_list(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.asList")?;
    let elements = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    let class = shared.load_library_class(crate::collections::ARRAY_LIST_CLASS)?;
    let list = shared.heap().allocate_list(class);
    {
        let mut heap = shared.heap();
        *heap.list_mut(list).expect("a freshly allocated list") = elements;
    }
    Ok(Some(Value::Ref(list)))
}

fn arrays_copy_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.copyOf")?;
    let length = int_argument(args, 1, "Arrays.copyOf")?;
    if length < 0 {
        return Err(VmError::Native(
            "Arrays.copyOf expects a non-negative length".to_owned(),
        ));
    }
    copy_array(shared, array, 0, length as usize)
}

fn arrays_copy_of_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.copyOfRange")?;
    let from = int_argument(args, 1, "Arrays.copyOfRange")?;
    let to = int_argument(args, 2, "Arrays.copyOfRange")?;
    if from < 0 || to < from {
        return Err(VmError::Native(
            "Arrays.copyOfRange received a reversed range".to_owned(),
        ));
    }
    copy_array(shared, array, from as usize, (to - from) as usize)
}

fn copy_array(
    shared: &Shared,
    source: ObjRef,
    from: usize,
    length: usize,
) -> Result<Option<Value>, VmError> {
    let (component, elements) = {
        let heap = shared.heap();
        let array = heap.array(source).map_err(heap_error)?;
        let mut elements = Vec::with_capacity(length);
        for position in 0..length {
            elements.push(
                array
                    .elements
                    .get(from + position)
                    .copied()
                    .unwrap_or(Value::Ref(ObjRef::NULL)),
            );
        }
        (array.component.clone(), elements)
    };
    let copy = shared.heap().allocate_array(component, length);
    {
        let mut heap = shared.heap();
        heap.array_mut(copy)
            .map_err(heap_error)?
            .elements
            .copy_from_slice(&elements);
    }
    Ok(Some(Value::Ref(copy)))
}

fn arrays_to_string(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.toString")?;
    if array.is_null() {
        let text = shared.intern_string("null");
        return Ok(Some(Value::Ref(text)));
    }
    let elements = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    let mut parts = Vec::with_capacity(elements.len());
    for element in elements {
        parts.push(match element {
            Value::Ref(reference) if reference.is_null() => "null".to_owned(),
            Value::Ref(reference) => {
                crate::string::object_text(shared, thread, reference, "Arrays.toString")?
            }
            Value::Int(value) => value.to_string(),
            Value::Long(value) => value.to_string(),
            Value::Float(value) => value.to_string(),
            Value::Double(value) => value.to_string(),
        });
    }
    let text = shared.intern_string(&format!("[{}]", parts.join(", ")));
    Ok(Some(Value::Ref(text)))
}

fn element_hash(shared: &Shared, value: Value) -> Result<i32, VmError> {
    match value {
        Value::Ref(reference) if reference.is_null() => Ok(0),
        Value::Ref(reference) => {
            let heap = shared.heap();
            Ok(match heap.get(reference).map_err(heap_error)? {
                HeapObject::Str(text) => {
                    let mut hash = 0i32;
                    for unit in crate::string::utf16_units(text) {
                        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
                    }
                    hash
                }
                HeapObject::Boxed(boxed) => boxed.java_hash_code(),
                _ => reference.identity_hash(),
            })
        }
        Value::Int(value) => Ok(value),
        Value::Long(value) => Ok((value as i32) ^ ((value >> 32) as i32)),
        Value::Float(value) => Ok(crate::heap::canonical_float_bits(value) as i32),
        Value::Double(value) => {
            let bits = crate::heap::canonical_double_bits(value);
            Ok((bits as i32) ^ ((bits >> 32) as i32))
        }
    }
}

fn arrays_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.hashCode")?;
    if array.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    let elements = {
        let heap = shared.heap();
        heap.array(array).map_err(heap_error)?.elements.to_vec()
    };
    let mut hash = 1i32;
    for element in elements {
        hash = hash
            .wrapping_mul(31)
            .wrapping_add(element_hash(shared, element)?);
    }
    Ok(Some(Value::Int(hash)))
}

fn arrays_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = array_argument(args, 0, "Arrays.equals")?;
    let right = array_argument(args, 1, "Arrays.equals")?;
    if left == right {
        return Ok(Some(Value::Int(1)));
    }
    if left.is_null() || right.is_null() {
        return Ok(Some(Value::Int(0)));
    }
    let (left_elements, right_elements) = {
        let heap = shared.heap();
        (
            heap.array(left).map_err(heap_error)?.elements.to_vec(),
            heap.array(right).map_err(heap_error)?.elements.to_vec(),
        )
    };
    let equal = left_elements.len() == right_elements.len()
        && left_elements
            .iter()
            .zip(right_elements.iter())
            .all(|(left, right)| left == right);
    Ok(Some(Value::Int(i32::from(equal))))
}

/// `Arrays.fill(array, value)`: fill every element with the second argument.
fn arrays_fill(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let array = array_argument(args, 0, "Arrays.fill")?;
    let value = args.get(1).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let mut heap = shared.heap();
    let elements = &mut heap.array_mut(array).map_err(heap_error)?.elements;
    for element in elements.iter_mut() {
        *element = value;
    }
    Ok(None)
}
