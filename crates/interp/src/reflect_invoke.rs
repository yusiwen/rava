//! Reflective invocation natives (`Method.invoke`, `Constructor.newInstance`) — M2.10.
//!
//! These natives live in `rava-interp`, not `rava-vm`, because they re-enter the
//! interpreter's existing call machinery: a reflected method runs through
//! [`crate::run_static_method`] or [`crate::run_instance_method`], and a reflected
//! constructor allocates its declaring class through [`rava_vm::Shared::allocate_instance`]
//! and then runs `<init>` through [`crate::run_instance_method`]. There is no second
//! execution path and no second frame model.
//!
//! The public surface is intentionally narrow: one `int`/reference argument conversion in
//! each direction ([`rava_vm::boxed::unbox_argument`] and
//! [`rava_vm::boxed::box_result`]), no overload resolution, no varargs, no access checks
//! and no `setAccessible`. A `Method` object is invoked exactly by the name and descriptor
//! it was reflected with.
//!
//! `java.lang.reflect.InvocationTargetException` is deferred. A Java throwable raised by
//! the invoked method is not swallowed: it crosses the native ABI as
//! [`rava_vm::VmError::Thrown`], the interpreter turns it back into
//! [`crate::InterpError::Thrown`], and a caller's exception table can catch the original
//! object. The only deviation from the JDK is that the original throwable propagates
//! instead of being wrapped in an `InvocationTargetException`.

use rava_classfile::model::ACC_STATIC;
use rava_vm::{
    ArrayKind, ClassId, HeapError, LoaderId, MemberKind, ObjRef, ReflectMember, Shared,
    ThreadState, Value, VmError, boxed, reflection,
};

use crate::{InterpError, run_instance_method, run_static_method};

/// The `Method.invoke(Object, Object[])` descriptor.
const METHOD_INVOKE: &str = "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;";

/// The `Constructor.newInstance(Object[])` descriptor.
const CONSTRUCTOR_NEW_INSTANCE: &str = "([Ljava/lang/Object;)Ljava/lang/Object;";

/// Registers the M2.10 reflective-invocation natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        "java/lang/reflect/Method",
        "invoke",
        METHOD_INVOKE,
        method_invoke,
    );
    shared.register_native(
        "java/lang/reflect/Constructor",
        "newInstance",
        CONSTRUCTOR_NEW_INSTANCE,
        constructor_new_instance,
    );
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// The member identity behind a reflected-member receiver.
fn member_of(shared: &Shared, receiver: ObjRef) -> Result<ReflectMember, VmError> {
    let member = shared.heap().reflect(receiver).map_err(heap_error)?.clone();
    Ok(member)
}

fn class_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("class {}", class.0))
}

/// Whether the reflected method is static, read from the same typed metadata the member
/// object came from.
fn method_is_static(shared: &Shared, member: &ReflectMember) -> Result<bool, VmError> {
    let entry = shared
        .entry(member.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", member.class.0)))?;
    let metadata = entry.metadata()?;
    let method = metadata
        .method(&member.name, &member.descriptor)
        .ok_or_else(|| {
            VmError::Native(format!(
                "class {} has no method {}:{}",
                entry.name, member.name, member.descriptor
            ))
        })?;
    Ok(method.access_flags & ACC_STATIC != 0)
}

/// The `Object[]` argument list of an invocation, validated against the parameter count.
///
/// A null array is accepted only for a zero-parameter method, as the JDK documents for
/// `Method.invoke` and `Constructor.newInstance`. The elements stay `Value`s until the
/// caller applies [`boxed::unbox_argument`] with each parameter descriptor.
fn argument_array(
    shared: &Shared,
    args: &[Value],
    position: usize,
    parameter_count: usize,
    operation: &str,
) -> Result<Vec<Value>, VmError> {
    let Some(Value::Ref(array)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects an Object[] argument"
        )));
    };
    if array.is_null() {
        return if parameter_count == 0 {
            Ok(Vec::new())
        } else {
            Err(VmError::Native(format!(
                "{operation} received a null Object[] for {parameter_count} parameter(s)"
            )))
        };
    }
    let elements = {
        let heap = shared.heap();
        let array = heap.array(array).map_err(heap_error)?;
        if array.kind != ArrayKind::Reference {
            return Err(VmError::Native(format!(
                "{operation} expects a reference array, found a {} array",
                array.kind
            )));
        }
        array.elements.clone()
    };
    if elements.len() != parameter_count {
        return Err(VmError::Native(format!(
            "{operation} expects {parameter_count} argument(s), found {}",
            elements.len()
        )));
    }
    Ok(elements)
}

/// Convert the boxed arguments according to the member's parameter descriptors.
fn unbox_arguments(
    shared: &Shared,
    descriptors: &[&str],
    values: Vec<Value>,
    operation: &str,
) -> Result<Vec<Value>, VmError> {
    let mut arguments = Vec::with_capacity(values.len());
    for (index, (descriptor, value)) in descriptors.iter().zip(values).enumerate() {
        let argument = boxed::unbox_argument(shared, descriptor, value)
            .map_err(|error| VmError::Native(format!("{operation} argument {index}: {error}")))?;
        arguments.push(argument);
    }
    Ok(arguments)
}

/// `Method.invoke(Object, Object[])`: run a static or instance method by its reflected
/// name and descriptor.
///
/// For a static method the `Object` receiver is ignored, as on the JDK. For an instance
/// method it must be a non-null instance assignable to the declaring class. The result is
/// boxed by [`boxed::box_result`], so an `int` return becomes a `java.lang.Integer` and a
/// reference return keeps its handle.
fn method_invoke(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    if member.kind != MemberKind::Method {
        return Err(VmError::Native(format!(
            "Method.invoke requires a Method receiver, found a {}",
            member.kind.binary_name()
        )));
    }
    let parameters = reflection::parameter_descriptors(&member.descriptor)?;
    let values = argument_array(shared, args, 1, parameters.len(), "Method.invoke")?;
    let values = unbox_arguments(shared, &parameters, values, "Method.invoke")?;
    let is_static = method_is_static(shared, &member)?;
    let result = if is_static {
        // JVMS §6.5.invokestatic: no receiver. The `Object` argument is ignored, so a
        // null or unrelated handle is not an error.
        run_static_method(
            shared,
            thread,
            member.class,
            &member.name,
            &member.descriptor,
            values,
        )
    } else {
        let Some(Value::Ref(target)) = args.first().copied() else {
            return Err(VmError::Native(
                "Method.invoke expects an Object receiver".to_owned(),
            ));
        };
        if target.is_null() {
            return Err(VmError::Native(
                "Method.invoke received a null instance receiver".to_owned(),
            ));
        }
        require_instance(shared, target, member.class, "Method.invoke")?;
        run_instance_method(
            shared,
            thread,
            member.class,
            &member.name,
            &member.descriptor,
            target,
            values,
        )
    };
    let result = result.map_err(invocation_error)?;
    let return_type = reflection::return_descriptor(&member.descriptor)?;
    Ok(Some(boxed::box_result(shared, return_type, result)?))
}

/// `Constructor.newInstance(Object[])`: allocate the declaring class's instance, unbox
/// the arguments and run `<init>` through the existing instance machinery.
fn constructor_new_instance(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let member = member_of(shared, receiver)?;
    if member.kind != MemberKind::Constructor {
        return Err(VmError::Native(format!(
            "Constructor.newInstance requires a Constructor receiver, found a {}",
            member.kind.binary_name()
        )));
    }
    let parameters = reflection::parameter_descriptors(&member.descriptor)?;
    let values = argument_array(shared, args, 0, parameters.len(), "Constructor.newInstance")?;
    let values = unbox_arguments(shared, &parameters, values, "Constructor.newInstance")?;
    let instance = shared.allocate_instance(member.class)?;
    let result = run_instance_method(
        shared,
        thread,
        member.class,
        &member.name,
        &member.descriptor,
        instance,
        values,
    );
    match result {
        Ok(_) => Ok(Some(Value::Ref(instance))),
        Err(error) => Err(invocation_error(error)),
    }
}

/// Require an instance receiver assignable to the declaring class.
fn require_instance(
    shared: &Shared,
    target: ObjRef,
    expected: ClassId,
    operation: &str,
) -> Result<(), VmError> {
    let actual = {
        let heap = shared.heap();
        match heap.instance(target) {
            Ok(instance) => instance.class,
            Err(error) => {
                return Err(VmError::Native(format!(
                    "{operation} receiver is not an instance: {error}"
                )));
            }
        }
    };
    if is_assignable(shared, actual, expected)? {
        return Ok(());
    }
    Err(VmError::Native(format!(
        "{operation} receiver of class {} is not assignable to {}",
        class_name(shared, actual),
        class_name(shared, expected)
    )))
}

/// Is `from` the same class as `to`, or a subclass of it?
fn is_assignable(shared: &Shared, mut from: ClassId, to: ClassId) -> Result<bool, VmError> {
    loop {
        if from == to {
            return Ok(true);
        }
        let entry = shared
            .entry(from)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", from.0)))?;
        let Some(super_name) = entry.file.super_class_name()? else {
            return Ok(false);
        };
        let super_name = super_name.to_owned();
        from = shared.load_class(entry.loader, &super_name)?;
    }
}

/// Convert an invocation failure to the native ABI's error type.
///
/// A Java throwable keeps its identity as [`VmError::Thrown`] so the interpreter can turn
/// it back into `InterpError::Thrown`; every other failure is a typed native error.
fn invocation_error(error: InterpError) -> VmError {
    match error {
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(format!("reflective invocation failed: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_helpers_split_parameters_and_return() {
        let parameters = reflection::parameter_descriptors("(I[Ljava/lang/String;J)V").unwrap();
        assert_eq!(parameters, vec!["I", "[Ljava/lang/String;", "J"]);
        assert_eq!(reflection::return_descriptor("(I)V").unwrap(), "V");
    }
}
