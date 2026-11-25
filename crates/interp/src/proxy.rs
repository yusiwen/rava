//! `java.lang.reflect.InvocationHandler.invokeDefault` — M3.2.
//!
//! A proxy's handler decides whether an interface method call should reach the handler or
//! the interface's default implementation. `InvocationHandler.invokeDefault(proxy, method,
//! args)` runs that default implementation through the interpreter's ordinary instance
//! machinery: it unboxes the argument array with [`rava_vm::boxed::unbox_argument`] against
//! the reflected method's parameter descriptors, runs the method on its declaring interface
//! with the proxy as the receiver through [`crate::run_instance_method`], and boxes the
//! result with [`rava_vm::boxed::box_result`]. There is no second execution path and no
//! second frame model; a Java throwable raised by the default body keeps its identity as
//! [`rava_vm::VmError::Thrown`], exactly as `Method.invoke` does.
//!
//! The native lives in `rava-interp`, not `rava-vm`, because it re-enters the interpreter.
//! A runner that wants it registers [`register_natives`] after
//! [`rava_vm::library::register_all`], next to `reflect_invoke`.
//!
//! Deliberate limits: the reflected method is run exactly as it was reflected (no access
//! check, no `IllegalArgumentException` synthesis for a non-interface method), the argument
//! array follows the `reflect_invoke` convention (null is accepted only for a
//! zero-parameter method), and the JDK's `NullPointerException`/`IllegalAccessException`
//! shapes are typed native errors instead of synthesized exceptions.

use rava_vm::{
    ClassId, HeapError, LoaderId, MemberKind, ObjRef, ReflectMember, Shared, ThreadState, Value,
    VmError, boxed, reflection,
};

use crate::{InterpError, reflect_invoke, run_instance_method};

/// `InvocationHandler.invokeDefault(Object, Method, Object[])`.
const INVOKE_DEFAULT_DESCRIPTOR: &str =
    "(Ljava/lang/Object;Ljava/lang/reflect/Method;[Ljava/lang/Object;)Ljava/lang/Object;";

/// Registers the `InvocationHandler.invokeDefault` native.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        "java/lang/reflect/InvocationHandler",
        "invokeDefault",
        INVOKE_DEFAULT_DESCRIPTOR,
        invoke_default,
    );
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn class_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("class {}", class.0))
}

/// `InvocationHandler.invokeDefault(Object, Method, Object[])`: run an interface default
/// method with the proxy as its receiver and return the boxed result.
///
/// The operation names the failure that surfaced, so a long reflective chain names the
/// proxy call it failed in. A Java throwable keeps its identity as
/// [`VmError::Thrown`]; every other interpreter failure becomes a typed native error.
fn invoke_default(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let proxy = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "InvocationHandler.invokeDefault expects a non-null proxy object".to_owned(),
            ));
        }
    };
    let member = match args.get(1).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => shared
            .heap()
            .reflect(reference)
            .map_err(heap_error)?
            .clone(),
        _ => {
            return Err(VmError::Native(
                "InvocationHandler.invokeDefault expects a Method argument".to_owned(),
            ));
        }
    };
    if member.kind != MemberKind::Method {
        return Err(VmError::Native(format!(
            "InvocationHandler.invokeDefault requires a Method receiver, found a {}",
            member.kind.binary_name()
        )));
    }
    let parameters = reflection::parameter_descriptors(&member.descriptor)?;
    let values = reflect_invoke::argument_array(
        shared,
        args,
        2,
        parameters.len(),
        "InvocationHandler.invokeDefault",
    )?;
    let values = reflect_invoke::unbox_arguments(
        shared,
        &parameters,
        values,
        "InvocationHandler.invokeDefault",
    )?;
    let result = run_instance_method(
        shared,
        thread,
        member.class,
        &member.name,
        &member.descriptor,
        proxy,
        values,
    )
    .map_err(|error| invocation_error(shared, &member, error))?;
    let return_type = reflection::return_descriptor(&member.descriptor)?;
    Ok(Some(boxed::box_result(shared, return_type, result)?))
}

/// Convert an interpreter failure to the native ABI's error type.
///
/// A Java throwable keeps its identity as [`VmError::Thrown`] so the caller's exception
/// table can catch it; every other failure gains the reflected method's identity.
fn invocation_error(shared: &Shared, member: &ReflectMember, error: InterpError) -> VmError {
    match error {
        InterpError::Thrown(throwable) => VmError::Thrown(throwable),
        other => VmError::Native(format!(
            "InvocationHandler.invokeDefault for {}.{}{} failed: {other}",
            class_name(shared, member.class),
            member.name,
            member.descriptor
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_native_uses_the_jdk_descriptor() {
        let (parameters, return_type) = INVOKE_DEFAULT_DESCRIPTOR
            .strip_prefix('(')
            .and_then(|rest| rest.split_once(')'))
            .expect("a well-formed descriptor");
        assert_eq!(
            parameters,
            "Ljava/lang/Object;Ljava/lang/reflect/Method;[Ljava/lang/Object;"
        );
        assert_eq!(return_type, "Ljava/lang/Object;");
    }
}
