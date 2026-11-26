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
    shared.register_native(BIT_SET_CLASS, "<init>", "(I)V", bit_set_init);
    shared.register_native(BIT_SET_CLASS, "set", "(I)V", bit_set_set);
    shared.register_native(BIT_SET_CLASS, "set", "(II)V", bit_set_set_range);
    shared.register_native(BIT_SET_CLASS, "set", "(IZ)V", bit_set_set_flag);
    shared.register_native(BIT_SET_CLASS, "get", "(I)Z", bit_set_get);
    shared.register_native(BIT_SET_CLASS, "clear", "()V", bit_set_clear);
    shared.register_native(BIT_SET_CLASS, "clear", "(I)V", bit_set_clear_index);
    shared.register_native(BIT_SET_CLASS, "clear", "(II)V", bit_set_clear_range);
    shared.register_native(BIT_SET_CLASS, "length", "()I", bit_set_length);
    shared.register_native(BIT_SET_CLASS, "size", "()I", bit_set_length);
    shared.register_native(BIT_SET_CLASS, "isEmpty", "()Z", bit_set_is_empty);
    shared.register_native(BIT_SET_CLASS, "cardinality", "()I", bit_set_cardinality);
    shared.register_native(BIT_SET_CLASS, "nextSetBit", "(I)I", bit_set_next_set_bit);
    shared.register_native(
        BIT_SET_CLASS,
        "nextClearBit",
        "(I)I",
        bit_set_next_clear_bit,
    );
    shared.register_native(
        BIT_SET_CLASS,
        "toString",
        "()Ljava/lang/String;",
        bit_set_to_string,
    );
    for (name, function) in [
        ("and", bit_set_and as crate::NativeFn),
        ("or", bit_set_or),
        ("xor", bit_set_xor),
        ("andNot", bit_set_and_not),
    ] {
        shared.register_native(BIT_SET_CLASS, name, "(Ljava/util/BitSet;)V", function);
    }
    shared.register_native(
        BIT_SET_CLASS,
        "clone",
        "()Ljava/lang/Object;",
        bit_set_clone,
    );
    shared.register_native(
        BIT_SET_CLASS,
        "equals",
        "(Ljava/lang/Object;)Z",
        bit_set_equals,
    );
    shared.register_native(BIT_SET_CLASS, "hashCode", "()I", bit_set_hash_code);
}

fn other_bits(
    shared: &Shared,
    _receiver: ObjRef,
    args: &[Value],
    method: &str,
) -> Result<std::collections::BTreeSet<u32>, VmError> {
    let other = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => return Err(VmError::Native(format!("{method} expects a BitSet"))),
    };
    Ok(shared
        .heap()
        .bit_set(other)
        .map_err(heap_error)?
        .bits
        .clone())
}

fn bit_set_and(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = other_bits(shared, receiver, args, "BitSet.and")?;
    let mut heap = shared.heap();
    let set = heap.bit_set_mut(receiver).map_err(heap_error)?;
    set.bits = set.bits.intersection(&other).copied().collect();
    Ok(None)
}

fn bit_set_or(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = other_bits(shared, receiver, args, "BitSet.or")?;
    let mut heap = shared.heap();
    let set = heap.bit_set_mut(receiver).map_err(heap_error)?;
    set.bits.extend(other);
    Ok(None)
}

fn bit_set_xor(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = other_bits(shared, receiver, args, "BitSet.xor")?;
    let mut heap = shared.heap();
    let set = heap.bit_set_mut(receiver).map_err(heap_error)?;
    set.bits = set.bits.symmetric_difference(&other).copied().collect();
    Ok(None)
}

fn bit_set_and_not(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let other = other_bits(shared, receiver, args, "BitSet.andNot")?;
    let mut heap = shared.heap();
    let set = heap.bit_set_mut(receiver).map_err(heap_error)?;
    set.bits = set.bits.difference(&other).copied().collect();
    Ok(None)
}

fn bit_set_clone(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bits = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .clone();
    let object = shared.heap().allocate_bit_set();
    shared.heap().bit_set_mut(object).map_err(heap_error)?.bits = bits;
    Ok(Some(Value::Ref(object)))
}

fn bit_set_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let equal = match args.first().copied() {
        Some(Value::Ref(other)) if !other.is_null() => {
            let left = shared
                .heap()
                .bit_set(receiver)
                .map_err(heap_error)?
                .bits
                .clone();
            shared
                .heap()
                .bit_set(other)
                .map(|right| right.bits == left)
                .unwrap_or(false)
        }
        _ => false,
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

fn bit_set_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bits = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .clone();
    let mut hash = 1234i32;
    for index in bits {
        hash = hash.wrapping_add((index as i32).wrapping_mul(31));
    }
    Ok(Some(Value::Int(hash)))
}

fn bit_range(args: &[Value], method: &str) -> Result<(u32, u32), VmError> {
    let from = match args.first().copied() {
        Some(Value::Int(value)) => u32::try_from(value)
            .map_err(|_| VmError::Native(format!("{method}: negative bit index {value}")))?,
        _ => return Err(VmError::Native(format!("{method} expects an int range"))),
    };
    let to = match args.get(1).copied() {
        Some(Value::Int(value)) => u32::try_from(value)
            .map_err(|_| VmError::Native(format!("{method}: negative bit index {value}")))?,
        _ => return Err(VmError::Native(format!("{method} expects an int range"))),
    };
    Ok((from, to))
}

fn bit_set_set_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (from, to) = bit_range(args, "BitSet.set")?;
    let mut heap = shared.heap();
    let set = heap.bit_set_mut(receiver).map_err(heap_error)?;
    for index in from..to {
        set.bits.insert(index);
    }
    Ok(None)
}

fn bit_set_set_flag(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = bit_index(args, "BitSet.set")?;
    let value = matches!(args.get(1).copied(), Some(Value::Int(value)) if value != 0);
    let mut heap = shared.heap();
    let set = heap.bit_set_mut(receiver).map_err(heap_error)?;
    if value {
        set.bits.insert(index);
    } else {
        set.bits.remove(&index);
    }
    Ok(None)
}

fn bit_set_clear(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    shared
        .heap()
        .bit_set_mut(receiver)
        .map_err(heap_error)?
        .bits
        .clear();
    Ok(None)
}

fn bit_set_clear_index(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let index = bit_index(args, "BitSet.clear")?;
    shared
        .heap()
        .bit_set_mut(receiver)
        .map_err(heap_error)?
        .bits
        .remove(&index);
    Ok(None)
}

fn bit_set_clear_range(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let (from, to) = bit_range(args, "BitSet.clear")?;
    let mut heap = shared.heap();
    let set = heap.bit_set_mut(receiver).map_err(heap_error)?;
    for index in from..to {
        set.bits.remove(&index);
    }
    Ok(None)
}

fn bit_set_length(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let length = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .iter()
        .next_back()
        .map(|highest| highest.saturating_add(1))
        .unwrap_or(0);
    Ok(Some(Value::Int(length as i32)))
}

fn bit_set_is_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let empty = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .is_empty();
    Ok(Some(Value::Int(i32::from(empty))))
}

fn bit_set_cardinality(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let count = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .len();
    Ok(Some(Value::Int(count as i32)))
}

fn bit_set_next_set_bit(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let from = bit_index(args, "BitSet.nextSetBit")?;
    let next = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .range(from..)
        .next()
        .copied()
        .map(|index| index as i32)
        .unwrap_or(-1);
    Ok(Some(Value::Int(next)))
}

fn bit_set_next_clear_bit(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let from = bit_index(args, "BitSet.nextClearBit")?;
    let bits = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .clone();
    let mut index = from;
    while bits.contains(&index) {
        index = index.saturating_add(1);
    }
    Ok(Some(Value::Int(index as i32)))
}

fn bit_set_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let bits = shared
        .heap()
        .bit_set(receiver)
        .map_err(heap_error)?
        .bits
        .iter()
        .map(|index| index.to_string())
        .collect::<Vec<_>>();
    Ok(Some(Value::Ref(
        shared
            .heap()
            .allocate_string(format!("{{{}}}", bits.join(", "))),
    )))
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
