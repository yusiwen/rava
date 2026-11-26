//! `java.beans.PropertyDescriptor` construction that honours subclass overrides (M4.5).
//!
//! The JDK's `PropertyDescriptor(String, Method, Method)` constructor calls
//! `setReadMethod`/`setWriteMethod` **virtually**, so a subclass such as Spring's
//! `BasicPropertyDescriptor` stores the accessors in its own fields. The `rava-vm` native
//! cannot re-enter the interpreter to make those virtual calls, so this module installs a
//! re-entrant constructor that stores the plain descriptor state first and then calls the
//! two setters on the receiver. With a plain `PropertyDescriptor` receiver the calls land
//! back in the VM natives; with a Spring subclass they run the subclass's own bytecode.
//!
//! This is the difference between a write-only property having a setter and appearing
//! unwritable; Spring's bean-property population depends on it.

use rava_vm::{LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, resolve_virtual, run_instance_method, runtime_class};

const PROPERTY_DESCRIPTOR_CLASS: &str = "java/beans/PropertyDescriptor";
const SET_READ: &str = "(Ljava/lang/reflect/Method;)V";
const SET_WRITE: &str = "(Ljava/lang/reflect/Method;)V";

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        PROPERTY_DESCRIPTOR_CLASS,
        "<init>",
        "(Ljava/lang/String;Ljava/lang/reflect/Method;Ljava/lang/reflect/Method;)V",
        property_descriptor_init,
    );
}

fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &str,
) -> Result<String, VmError> {
    let Some(Value::Ref(reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{method} expects a String argument"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!("{method}: null name")));
    }
    let heap = shared.heap();
    Ok(heap
        .string(reference)
        .map_err(|error| VmError::Native(error.to_string()))?
        .to_owned())
}

fn property_descriptor_init(
    shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "PropertyDescriptor.<init>")?;
    let read = match args.get(1).copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    let write = match args.get(2).copied() {
        Some(Value::Ref(reference)) => reference,
        _ => ObjRef::NULL,
    };
    shared.bean_state().descriptors.insert(
        receiver,
        rava_vm::beans::DescriptorData {
            name,
            read,
            write,
            display_name: None,
            expert: false,
            hidden: false,
            bound: false,
            constrained: false,
        },
    );
    // The JDK constructor dispatches these two calls virtually; a subclass override is what
    // makes a write-only property's setter observable.
    for (method, argument) in [("setReadMethod", read), ("setWriteMethod", write)] {
        let class = match runtime_class(shared, receiver) {
            Ok(class) => class,
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        };
        let (entry, _) = match resolve_virtual(shared, class, method, SET_READ) {
            Ok(found) => found,
            // A plain `PropertyDescriptor` may resolve through an omitted superclass; the
            // VM's own registration handles the call, so a miss is not fatal.
            Err(InterpError::MethodNotFound { .. }) => continue,
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        };
        let descriptor = if method == "setReadMethod" {
            SET_READ
        } else {
            SET_WRITE
        };
        match run_instance_method(
            shared,
            thread,
            entry.id,
            method,
            descriptor,
            receiver,
            vec![Value::Ref(argument)],
        ) {
            Ok(_) => {}
            Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
            Err(error) => return Err(VmError::Native(error.to_string())),
        }
    }
    Ok(None)
}
