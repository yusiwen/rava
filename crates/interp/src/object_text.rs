//! Java `toString` for the VM's text conversions (#73).
//!
//! `String.valueOf(Object)`, `StringBuilder`/`StringBuffer.append(Object)`, the `%s`
//! conversion of `String.format`/`Formatter`/`MessageFormat`, string concatenation,
//! `Objects.toString` and `Arrays.toString` build text inside `rava-vm` natives, and a VM
//! native cannot re-enter the interpreter. The runner installs this resolver through
//! [`rava_vm::Hooks::register_object_text`]; the VM's `object_text` calls it for an object
//! that is not already a known wrapper/string/builder shape.
//!
//! The resolver dispatches `toString` for an ordinary `Instance` and returns `None` when
//! the class does not override it (the resolved declaration is `java.lang.Object`, whose
//! text is the VM's `ClassName@hex` shape), and for shapes without an instance class
//! (arrays, class objects, lambdas, lists/maps): those keep their structural text. The
//! heap lock is never held across the Java call, the same rule as
//! [`crate::collection_equality`].

use std::sync::Arc;

use rava_vm::{HeapObject, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, resolve_virtual, run_instance_method};

/// Install the resolver; the runner calls this once after the VM library is registered.
pub fn register(shared: &Shared) {
    shared.hooks().register_object_text(Arc::new(resolve_text));
}

fn resolve_text(
    shared: &Shared,
    thread: &ThreadState,
    reference: ObjRef,
) -> Result<Option<String>, VmError> {
    let class = {
        let heap = shared.heap();
        match heap.get(reference) {
            Ok(HeapObject::Instance(instance)) => instance.class,
            _ => return Ok(None),
        }
    };
    let (entry, _) =
        resolve_virtual(shared, class, "toString", "()Ljava/lang/String;").map_err(interp_to_vm)?;
    // A class that does not override `toString` resolves to `Object.toString`, whose text
    // is exactly the VM's structural shape; asking the VM keeps the fast path.
    if shared
        .entry(entry.id)
        .map(|entry| entry.name.as_str() == "java/lang/Object")
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let result = run_instance_method(
        shared,
        thread,
        entry.id,
        "toString",
        "()Ljava/lang/String;",
        reference,
        Vec::new(),
    )
    .map_err(interp_to_vm)?;
    match result {
        Some(Value::Ref(text)) => {
            let text = {
                let heap = shared.heap();
                heap.string(text)
                    .map_err(|error| VmError::Native(error.to_string()))?
                    .to_owned()
            };
            Ok(Some(text))
        }
        _ => Ok(None),
    }
}

fn interp_to_vm(error: InterpError) -> VmError {
    match error {
        InterpError::Vm(error) => error,
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(other.to_string()),
    }
}
