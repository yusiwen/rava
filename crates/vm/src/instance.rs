//! Instance natives (M2.5): the base-class constructors and the `EventObject` source.
//!
//! `java.lang.Object.<init>()V`, the root every constructor calls, and
//! `java.lang.Record.<init>()V`, which a record's canonical constructor calls before
//! initialising its components, have no body to run: their C1 stubs declare the
//! constructor but carry no `Code`, and a no-op is exactly the right behaviour.
//!
//! `java.util.EventObject` is the base class of Java's event objects. Its constructor
//! stores the event source in the `source` field the C1 stub declares, and `getSource()`
//! reads it back; the JDK's own bodies are absent from the stub tree, and Spring's
//! `ApplicationEvent` calls `super(source)` from its compiled constructor. A null source
//! raises the JDK's `IllegalArgumentException` through the `exceptions` module.
//!
//! Allocation, field layout and instance-method execution live in `rava-interp` and the
//! heap; this module only owns these native entries.

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// The `EventObject.source` field descriptor.
const EVENT_SOURCE_DESCRIPTOR: &str = "Ljava/lang/Object;";

/// Registers the instance natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native("java/lang/Object", "<init>", "()V", object_init);
    shared.register_native("java/lang/Record", "<init>", "()V", record_init);
    shared.register_native(
        "java/util/EventObject",
        "<init>",
        "(Ljava/lang/Object;)V",
        event_object_init,
    );
    shared.register_native(
        "java/util/EventObject",
        "getSource",
        "()Ljava/lang/Object;",
        event_object_get_source,
    );
}

/// `java.lang.Object.<init>()V`: a no-op on a non-null receiver.
///
/// `Object` has no instance state, so there is nothing to initialise. The receiver is
/// still checked: a constructor call on a null reference is an error, not a silent
/// success.
fn object_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "Object.<init> called with a null receiver".to_owned(),
        ));
    }
    Ok(None)
}

/// `java.lang.Record.<init>()V`: a no-op on a non-null receiver.
///
/// A record's canonical constructor invokes this superclass constructor first. `Record`
/// holds no instance state of its own (a record's components live in its own layout), so
/// there is nothing to initialise; the receiver is still checked.
fn record_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "Record.<init> called with a null receiver".to_owned(),
        ));
    }
    Ok(None)
}

/// `java.util.EventObject.<init>(Object)`: store the non-null event source.
///
/// The receiver is the runtime event object (a subclass such as Spring's
/// `ApplicationEvent`), so the layout is resolved from its runtime class and the
/// superclass slot is written. The JDK throws `IllegalArgumentException` for a null source;
/// that is a real synthesized Java throwable, so a compiled catch clause can observe it.
fn event_object_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    if receiver.is_null() {
        return Err(VmError::Native(
            "EventObject.<init> called with a null receiver".to_owned(),
        ));
    }
    let source = match args.first().copied() {
        Some(Value::Ref(source)) => source,
        _ => {
            return Err(VmError::Native(
                "EventObject.<init> expects an Object source".to_owned(),
            ));
        }
    };
    if source.is_null() {
        let throwable = crate::exceptions::new_throwable(
            shared,
            "java/lang/IllegalArgumentException",
            "null source".to_owned(),
        )?;
        return Err(VmError::Thrown(throwable));
    }
    let class = {
        let heap = shared.heap();
        heap.object_class(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
    };
    let slot = shared
        .field_layout(class)?
        .slot("source", EVENT_SOURCE_DESCRIPTOR)
        .ok_or_else(|| {
            VmError::Native(format!(
                "EventObject.<init>: class {} has no source field",
                class.0
            ))
        })?;
    let mut heap = shared.heap();
    let instance = heap
        .instance_mut(receiver)
        .map_err(|error| VmError::Native(error.to_string()))?;
    instance.fields[slot] = Value::Ref(source);
    Ok(None)
}

/// `java.util.EventObject.getSource()`: the source stored by the constructor.
fn event_object_get_source(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = {
        let heap = shared.heap();
        heap.object_class(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
    };
    let slot = shared
        .field_layout(class)?
        .slot("source", EVENT_SOURCE_DESCRIPTOR)
        .ok_or_else(|| {
            VmError::Native(format!(
                "EventObject.getSource: class {} has no source field",
                class.0
            ))
        })?;
    let value = {
        let heap = shared.heap();
        heap.instance(receiver)
            .map_err(|error| VmError::Native(error.to_string()))?
            .fields[slot]
    };
    Ok(Some(value))
}
