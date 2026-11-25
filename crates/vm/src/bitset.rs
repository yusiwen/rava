//! `java.util.BitSet` natives (M3.6).
//!
//! CGLIB emits the `equals` helper for a generated class with a `BitSet` that remembers
//! which parameter positions already have a getter, and Spring's `MimeType` static
//! initializer builds its token tables from two more. The heap shape is
//! [`rava_vm::HeapObject::BitSet`], a sorted set of set-bit indices.
//!
//! Only the surface this slice reaches is registered: the no-argument constructor,
//! `set(int)` and `get(int)`. The capacity constructor, `clear`, `length`, `cardinality`
//! and the range forms fail with the ordinary "no native implementation" error instead of
//! guessing.

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `java.util.BitSet`'s binary-stable internal name.
pub const BIT_SET_CLASS: &str = "java/util/BitSet";

/// Registers the `BitSet` surface CGLIB and `MimeType` reach.
pub fn register_natives(shared: &Shared) {
    shared.register_native(BIT_SET_CLASS, "<init>", "()V", bit_set_init);
    shared.register_native(BIT_SET_CLASS, "set", "(I)V", bit_set_set);
    shared.register_native(BIT_SET_CLASS, "get", "(I)Z", bit_set_get);
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// A bit index argument. A negative index is out of range on the JDK.
fn bit_index(args: &[Value], method: &str) -> Result<u32, VmError> {
    match args.first().copied() {
        Some(Value::Int(index)) => u32::try_from(index).map_err(|_| {
            VmError::Native(format!("{method} received the negative bit index {index}"))
        }),
        Some(_) => Err(VmError::Native(format!(
            "{method} expects an int bit index"
        ))),
        None => Err(VmError::Native(format!("{method} is missing its index"))),
    }
}

/// `BitSet()`: an empty set.
fn bit_set_init(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

/// `BitSet.set(int)`: add the index.
fn bit_set_set(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = bit_index(args, "BitSet.set")?;
    shared
        .heap()
        .bit_set_mut(receiver)
        .map_err(heap_error)?
        .bits
        .insert(index);
    Ok(None)
}

/// `BitSet.get(int)`: whether the index is set.
fn bit_set_get(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = bit_index(args, "BitSet.get")?;
    let set = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .contains(&index);
    Ok(Some(Value::Int(i32::from(set))))
}
