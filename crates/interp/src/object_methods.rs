//! `java.lang.runtime.ObjectMethods` record call sites (M3.6).
//!
//! Java compiles a record's `hashCode`, `equals` and `toString` to one `invokedynamic`
//! each, bootstrapped by `java.lang.runtime.ObjectMethods.bootstrap`. CGLIB's cache key
//! (`Enhancer$EnhancerKey`) is a record, and `DefaultNamingPolicy` calls `key.hashCode()`
//! to build the generated class's name, so the record `hashCode` must execute.
//!
//! The bootstrap's static arguments are the record class, the component names (`name1;…`)
//! and one `REF_getField` handle per component. This module implements the `hashCode` call
//! site in the interpreter's one frame model: it reads every component through its getter
//! handle, folds it with the JDK's `31 * hash + componentHash` formula, and pushes the
//! `int`. Primitive components use the `Wrapper.hashCode(primitive)` formulas; a reference
//! component is hashed by dispatching its own `hashCode()` (the same virtual dispatch any
//! other call uses), with `null` hashing to 0.
//!
//! `equals` and `toString` are not synthesized: the call sites this slice does not reach
//! fail with a typed error naming the call site, instead of guessing a comparison or a
//! rendering.
//!
//! `java.util.AbstractList.hashCode` is registered here because it needs the same
//! per-element `hashCode()` dispatch that cannot run from a `rava-vm` native. The JDK
//! starts at 1 and folds `31 * hash + (element == null ? 0 : element.hashCode())`.

use rava_classfile::{
    BootstrapArgument, BootstrapMethodEntry, BootstrapMethodHandle, ReferenceKind,
};
use rava_vm::{ClassId, HeapError, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, dispatch_class, resolve_virtual, run_instance_method};

/// The record bootstrap's owner.
pub const OBJECT_METHODS: &str = "java/lang/runtime/ObjectMethods";

/// The record `hashCode` call site and the `Object.hashCode` descriptor.
const HASH_CODE: &str = "()I";

/// Registers the re-entrant natives this module owns.
pub fn register_natives(shared: &Shared) {
    // `ArrayList` declares its own `hashCode` in the C1 projection, so both the declaring
    // class and the abstract base the other list classes inherit from need the entry.
    for class in ["java/util/ArrayList", "java/util/AbstractList"] {
        shared.register_native(class, "hashCode", HASH_CODE, abstract_list_hash_code);
    }
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn interp_to_vm(error: InterpError) -> VmError {
    match error {
        InterpError::Vm(error) => error,
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(other.to_string()),
    }
}

/// Execute one `ObjectMethods` `invokedynamic` call site.
pub(crate) fn invoke_call_site(
    shared: &Shared,
    thread: &ThreadState,
    caller: ClassId,
    bootstrap: &BootstrapMethodEntry,
    name: &str,
    descriptor: &str,
    args: &[Value],
) -> Result<Value, InterpError> {
    if name != "hashCode" {
        return Err(InterpError::Decode(format!(
            "ObjectMethods.bootstrap call site {name}{descriptor} is not implemented; only \
             a record's hashCode is"
        )));
    }
    let receiver = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => return Err(InterpError::WrongValue),
    };
    let mut getters = Vec::new();
    for argument in &bootstrap.arguments {
        if let BootstrapArgument::MethodHandle(handle) = argument {
            getters.push(handle);
        }
    }
    let mut hash = 0i32;
    for getter in getters {
        let value = read_component(shared, caller, receiver, getter)?;
        let component = component_hash(shared, thread, caller, getter, value)?;
        hash = hash.wrapping_mul(31).wrapping_add(component);
    }
    Ok(Value::Int(hash))
}

/// The value of one record component, read through its `REF_getField` getter.
fn read_component(
    shared: &Shared,
    caller: ClassId,
    receiver: ObjRef,
    getter: &BootstrapMethodHandle,
) -> Result<Value, InterpError> {
    if getter.reference_kind != ReferenceKind::GetField {
        return Err(InterpError::Decode(format!(
            "ObjectMethods component getter {}::{} is {:?}, not REF_getField",
            getter.owner, getter.name, getter.reference_kind
        )));
    }
    let actual = shared.heap().object_class(receiver)?;
    let class = resolve_class(shared, caller, &getter.owner)?;
    if actual != class {
        let found = shared
            .entry(actual)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| format!("class {}", actual.0));
        return Err(InterpError::NotAssignable {
            expected: getter.owner.clone(),
            found,
        });
    }
    let slot = shared
        .field_layout(actual)?
        .slot(&getter.name, &getter.descriptor)
        .ok_or_else(|| {
            InterpError::Decode(format!(
                "record {} has no component field {}:{}",
                getter.owner, getter.name, getter.descriptor
            ))
        })?;
    Ok(shared.heap().object_fields(receiver)?[slot])
}

/// One component's `hashCode`, by descriptor.
fn component_hash(
    shared: &Shared,
    thread: &ThreadState,
    caller: ClassId,
    getter: &BootstrapMethodHandle,
    value: Value,
) -> Result<i32, InterpError> {
    let descriptor = getter.descriptor.as_str();
    match descriptor {
        "Z" => Ok(match value {
            Value::Int(0) => 1237,
            Value::Int(_) => 1231,
            other => return Err(component_mismatch(getter, other)),
        }),
        "B" | "S" | "I" | "C" => match value {
            Value::Int(value) => Ok(value),
            other => Err(component_mismatch(getter, other)),
        },
        "J" => match value {
            Value::Long(value) => Ok(long_hash(value)),
            other => Err(component_mismatch(getter, other)),
        },
        "F" => match value {
            Value::Float(value) => Ok(value.to_bits() as i32),
            other => Err(component_mismatch(getter, other)),
        },
        "D" => match value {
            Value::Double(value) => Ok(long_hash(value.to_bits() as i64)),
            other => Err(component_mismatch(getter, other)),
        },
        _ => match value {
            Value::Ref(reference) => hash_code_of(shared, thread, caller, reference),
            other => Err(component_mismatch(getter, other)),
        },
    }
}

fn component_mismatch(getter: &BootstrapMethodHandle, value: Value) -> InterpError {
    InterpError::Decode(format!(
        "record component {}:{} holds {value:?}",
        getter.name, getter.descriptor
    ))
}

/// `Long.hashCode(long)`: the high and low halves folded together.
fn long_hash(value: i64) -> i32 {
    ((value as u64) ^ ((value as u64) >> 32)) as i32
}

/// `Objects.hashCode(Object)`: 0 for null, otherwise the value's own `hashCode()`.
fn hash_code_of(
    shared: &Shared,
    thread: &ThreadState,
    caller: ClassId,
    reference: ObjRef,
) -> Result<i32, InterpError> {
    if reference.is_null() {
        return Ok(0);
    }
    let start = dispatch_class(shared, caller, reference)?;
    let (entry, _) = resolve_virtual(shared, start, "hashCode", HASH_CODE)?;
    let declaring = entry.id;
    match run_instance_method(
        shared,
        thread,
        declaring,
        "hashCode",
        HASH_CODE,
        reference,
        Vec::new(),
    )? {
        Some(Value::Int(hash)) => Ok(hash),
        other => Err(InterpError::Decode(format!(
            "{}::hashCode() returned {other:?}, not an int",
            entry.name
        ))),
    }
}

/// `AbstractList.hashCode()`: 1 folded with every element's `hashCode()`.
fn abstract_list_hash_code(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // The list's runtime class is the loader context the element dispatch starts from;
    // every library class it may need (String, a box) lives in the bootstrap loader.
    let (class, elements) = {
        let heap = shared.heap();
        (
            heap.object_class(receiver).map_err(heap_error)?,
            heap.list(receiver).map_err(heap_error)?.to_vec(),
        )
    };
    let mut hash = 1i32;
    for element in elements {
        let Value::Ref(reference) = element else {
            return Err(VmError::Native(
                "AbstractList.hashCode found a non-reference element".to_owned(),
            ));
        };
        let element_hash = hash_code_of(shared, thread, class, reference).map_err(interp_to_vm)?;
        hash = hash.wrapping_mul(31).wrapping_add(element_hash);
    }
    Ok(Some(Value::Int(hash)))
}

/// Resolve `name` through `caller`'s loader.
fn resolve_class(shared: &Shared, caller: ClassId, name: &str) -> Result<ClassId, InterpError> {
    let loader = shared
        .entry(caller)
        .ok_or_else(|| InterpError::Decode("caller class vanished".to_owned()))?
        .loader;
    Ok(shared.load_class(loader, name)?)
}
