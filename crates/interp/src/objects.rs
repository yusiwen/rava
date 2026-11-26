//! `java.util.Objects` helpers that call Java `hashCode` (M4.5).
//!
//! `Objects.hash(Object...)` / `Objects.hashCode(Object)` must run the element's own
//! `hashCode`, which re-enters the interpreter. The C1 stub declares them without `Code`,
//! so they live here like the other re-entrant natives.

use rava_vm::{LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, resolve_virtual, run_instance_method, runtime_class};

const OBJECTS_CLASS: &str = "java/util/Objects";

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        OBJECTS_CLASS,
        "hash",
        "([Ljava/lang/Object;)I",
        objects_hash,
    );
    shared.register_native(
        OBJECTS_CLASS,
        "hashCode",
        "(Ljava/lang/Object;)I",
        objects_hash_code,
    );
}

fn invoke_hash_code(shared: &Shared, thread: &ThreadState, value: Value) -> Result<i32, VmError> {
    let Value::Ref(reference) = value else {
        return Ok(0);
    };
    if reference.is_null() {
        return Ok(0);
    }
    let class = match runtime_class(shared, reference) {
        Ok(class) => class,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let (entry, _) = match resolve_virtual(shared, class, "hashCode", "()I") {
        Ok(found) => found,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    match run_instance_method(
        shared,
        thread,
        entry.id,
        "hashCode",
        "()I",
        reference,
        Vec::new(),
    ) {
        Ok(Some(Value::Int(hash))) => Ok(hash),
        Ok(_) => Err(VmError::Native(
            "Objects.hashCode: hashCode did not return an int".to_owned(),
        )),
        Err(InterpError::Thrown(throwable)) => Err(VmError::Thrown(throwable)),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

fn objects_hash(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let values = match args.first().copied() {
        Some(Value::Ref(array)) if !array.is_null() => {
            let heap = shared.heap();
            heap.array(array)
                .map_err(|error| VmError::Native(error.to_string()))?
                .elements
                .clone()
        }
        _ => Vec::new(),
    };
    let mut result = 1i32;
    for value in values {
        result = result
            .wrapping_mul(31)
            .wrapping_add(invoke_hash_code(shared, thread, value)?);
    }
    Ok(Some(Value::Int(result)))
}

fn objects_hash_code(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(invoke_hash_code(
        shared,
        thread,
        args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL)),
    )?)))
}
