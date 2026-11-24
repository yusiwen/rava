//! Instance natives (M2.5).
//!
//! The only instance-level native this slice needs is `java.lang.Object.<init>()V`: the
//! C1 stub declares the constructor but carries no `Code` attribute, and a constructor
//! whose body does nothing is exactly its correct behaviour. Allocation, field layout and
//! instance-method execution live in `rava-interp` and the heap; this module only owns
//! the constructor's native entry.

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// Registers the M2.5 instance natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native("java/lang/Object", "<init>", "()V", object_init);
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
