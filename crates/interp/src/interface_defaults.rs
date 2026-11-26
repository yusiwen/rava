//! Interface default methods that must call Java (M4.5).
//!
//! The JDK defines `CharSequence.isEmpty()` as `length() == 0` in the interface, so the C1
//! stub declares it without `Code`. Spring's configuration-property name builder calls it
//! on the elements' text. The native dispatches `length()` on the receiver's runtime class
//! so a `String`, a `StringBuilder` or any other implementation keeps its own behavior.
//!
//! This module is the home for such re-entrant interface defaults; a default that needs no
//! Java call belongs in its feature's `rava-vm` module instead.

use rava_vm::{LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, resolve_virtual, run_instance_method, runtime_class};

const CHAR_SEQUENCE_CLASS: &str = "java/lang/CharSequence";

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        CHAR_SEQUENCE_CLASS,
        "isEmpty",
        "()Z",
        char_sequence_is_empty,
    );
    // `Map.getOrDefault` is an interface default whose JDK body calls `get` and
    // `containsKey`; a custom Map implementation (Spring has several) has no `Code` in
    // the C1 stub for the default, so this re-entrant version calls the receiver's own
    // methods.
    shared.register_native(
        "java/util/Map",
        "getOrDefault",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        map_get_or_default,
    );
}

fn invoke_map_method(
    shared: &Shared,
    thread: &ThreadState,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, VmError> {
    let class = match runtime_class(shared, receiver) {
        Ok(class) => class,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let (entry, _) = match resolve_virtual(shared, class, name, descriptor) {
        Ok(found) => found,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    match run_instance_method(shared, thread, entry.id, name, descriptor, receiver, args) {
        Ok(value) => Ok(value),
        Err(InterpError::Thrown(throwable)) => Err(VmError::Thrown(throwable)),
        Err(error) => Err(VmError::Native(error.to_string())),
    }
}

fn map_get_or_default(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let key = args.first().copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let fallback = args.get(1).copied().unwrap_or(Value::Ref(ObjRef::NULL));
    let value = invoke_map_method(
        shared,
        thread,
        receiver,
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        vec![key],
    )?;
    if !matches!(value, Some(Value::Ref(reference)) if reference.is_null()) && value.is_some() {
        return Ok(value);
    }
    let present = invoke_map_method(
        shared,
        thread,
        receiver,
        "containsKey",
        "(Ljava/lang/Object;)Z",
        vec![key],
    )?;
    if matches!(present, Some(Value::Int(flag)) if flag != 0) {
        return Ok(value);
    }
    Ok(Some(fallback))
}

fn char_sequence_is_empty(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "CharSequence.isEmpty called on a null receiver".to_owned(),
        ));
    }
    let class = match runtime_class(shared, receiver) {
        Ok(class) => class,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let (entry, _) = match resolve_virtual(shared, class, "length", "()I") {
        Ok(found) => found,
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    let length = match run_instance_method(
        shared,
        thread,
        entry.id,
        "length",
        "()I",
        receiver,
        Vec::new(),
    ) {
        Ok(Some(Value::Int(length))) => length,
        Ok(_) => {
            return Err(VmError::Native(
                "CharSequence.length did not return an int".to_owned(),
            ));
        }
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => return Err(VmError::Native(error.to_string())),
    };
    Ok(Some(Value::Int(i32::from(length == 0))))
}
