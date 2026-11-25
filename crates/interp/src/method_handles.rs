//! `MethodHandle.invoke*` natives (M3.4).
//!
//! These natives live in `rava-interp`, not `rava-vm`, because invoking a handle re-enters
//! the interpreter's call machinery exactly as `Method.invoke` does: a `Static` handle runs
//! through [`crate::run_static_method`], a `Virtual`/`Special` handle runs through
//! [`crate::run_instance_method`], and a `Constructor` handle allocates the owner through
//! [`rava_vm::Shared::allocate_instance`] and then runs `<init>`. There is no second
//! execution path and no second frame model.
//!
//! The tested surface is the real `invokeExact(Object[])` / `invoke(Object[])` /
//! `invokeWithArguments(Object[])` declarations. Signature-polymorphic call sites
//! (`handle.invoke(2, 3)` in compiled bytecode) are not supported by this interpreter: a
//! call site is resolved by the descriptor in the class file, and that descriptor is not
//! the handle's own type. `invokeWithArguments` accepts a null array when the handle takes
//! no parameters, as the JDK documents.
//!
//! Arguments are unboxed through [`rava_vm::boxed::unbox_argument`] against the handle's
//! parameter descriptors (including the receiver for a `Virtual`/`Special` handle), and the
//! result is boxed through [`rava_vm::boxed::box_result`] against the handle's return
//! descriptor. No `asType` conversion is applied: a value of the wrong shape is a typed
//! error. The `invokeWithArguments(List)` overload has no native either.

use rava_vm::{
    HandleKind, HeapError, LoaderId, MethodHandleObject, ObjRef, Shared, ThreadState, Value,
    VmError, boxed,
};

use crate::reflect_invoke::argument_array;
use crate::{InterpError, dispatch_class, resolve_virtual, run_instance_method, run_static_method};

/// The `Object[]` descriptor shared by `invokeExact`, `invoke` and `invokeWithArguments`.
const INVOKE_ARRAY: &str = "([Ljava/lang/Object;)Ljava/lang/Object;";

/// Registers the M3.4 method-handle invocation natives.
///
/// The three names share one implementation: without signature polymorphism, `invoke` and
/// `invokeExact` cannot differ in the conversions they apply.
pub fn register_natives(shared: &Shared) {
    for name in ["invokeExact", "invoke", "invokeWithArguments"] {
        shared.register_native(
            "java/lang/invoke/MethodHandle",
            name,
            INVOKE_ARRAY,
            invoke_with_arguments,
        );
    }
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// The handle behind a `MethodHandle` receiver.
fn method_handle_of(
    shared: &Shared,
    receiver: ObjRef,
    operation: &str,
) -> Result<MethodHandleObject, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(format!(
            "{operation} called with a null receiver"
        )));
    }
    Ok(shared
        .heap()
        .method_handle(receiver)
        .map_err(heap_error)?
        .clone())
}

/// A method-handle invocation failure, preserving a Java throwable's identity.
fn invocation_error(error: InterpError) -> VmError {
    match error {
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(format!("method-handle invocation failed: {other}")),
    }
}

/// `MethodHandle.invokeExact(Object[])` / `invoke(Object[])` /
/// `invokeWithArguments(Object[])`.
fn invoke_with_arguments(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let handle = method_handle_of(shared, receiver, "MethodHandle.invoke")?;
    let parameters = &handle.method_type.parameters;
    let values = argument_array(
        shared,
        args,
        0,
        parameters.len(),
        "MethodHandle.invokeWithArguments",
    )?;
    let mut descriptors = Vec::with_capacity(parameters.len());
    for parameter in parameters {
        descriptors.push(shared.class_ref_descriptor(parameter)?);
    }
    let mut values = values;
    for (index, descriptor) in descriptors.iter().enumerate() {
        values[index] =
            boxed::unbox_argument(shared, descriptor, values[index]).map_err(|error| {
                VmError::Native(format!(
                    "MethodHandle.invokeWithArguments argument {index}: {error}"
                ))
            })?;
    }

    let result = match handle.kind {
        HandleKind::Static => run_static_method(
            shared,
            thread,
            handle.owner,
            &handle.name,
            &handle.descriptor,
            values,
        )
        .map_err(invocation_error)?,
        HandleKind::Virtual => {
            let (receiver, arguments) = split_receiver(values, "virtual")?;
            // A null receiver has no runtime class to start from; the owner is the only
            // truthful starting point, and running on it reports the null receiver.
            let start = if receiver.is_null() {
                handle.owner
            } else {
                dispatch_class(shared, handle.owner, receiver).map_err(invocation_error)?
            };
            let (entry, _) = resolve_virtual(shared, start, &handle.name, &handle.descriptor)
                .map_err(invocation_error)?;
            run_instance_method(
                shared,
                thread,
                entry.id,
                &handle.name,
                &handle.descriptor,
                receiver,
                arguments,
            )
            .map_err(invocation_error)?
        }
        HandleKind::Special => {
            let (receiver, arguments) = split_receiver(values, "special")?;
            run_instance_method(
                shared,
                thread,
                handle.owner,
                &handle.name,
                &handle.descriptor,
                receiver,
                arguments,
            )
            .map_err(invocation_error)?
        }
        HandleKind::Constructor => {
            let instance = shared.allocate_instance(handle.owner)?;
            run_instance_method(
                shared,
                thread,
                handle.owner,
                "<init>",
                &handle.descriptor,
                instance,
                values,
            )
            .map_err(invocation_error)?;
            Some(Value::Ref(instance))
        }
    };

    let return_descriptor = shared.class_ref_descriptor(&handle.method_type.return_type)?;
    Ok(Some(boxed::box_result(shared, &return_descriptor, result)?))
}

/// Split an unboxed argument list into its receiver and the method arguments.
fn split_receiver(values: Vec<Value>, kind: &str) -> Result<(ObjRef, Vec<Value>), VmError> {
    let mut values = values;
    if values.is_empty() {
        return Err(VmError::Native(format!(
            "a {kind} MethodHandle takes a receiver as its first argument, found no arguments"
        )));
    }
    let receiver = match values.remove(0) {
        Value::Ref(reference) => reference,
        other => {
            return Err(VmError::Native(format!(
                "a {kind} MethodHandle requires a reference receiver, found {other:?}"
            )));
        }
    };
    Ok((receiver, values))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_arguments_reject_a_missing_receiver() {
        assert!(split_receiver(Vec::new(), "virtual").is_err());
        let (receiver, rest) =
            split_receiver(vec![Value::Ref(ObjRef::NULL), Value::Int(1)], "virtual")
                .expect("a reference receiver splits");
        assert!(receiver.is_null());
        assert_eq!(rest, vec![Value::Int(1)]);
    }
}
