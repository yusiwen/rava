//! The `Unsafe` natives: the memory model the host library's own code uses.
//!
//! `Unsafe` is how the class library implements atomics, buffers and reflection data. This VM's
//! objects are a `Vec<Value>` of fields and its arrays a `Vec<Value>` of elements, so the model is
//! a decoding of `(object, offset)` into one of those slots:
//!
//! * on an instance, `offset` is the **field slot** (the index the loader gave the field in the
//!   class's layout) — `objectFieldOffset` returns exactly that number;
//! * on a primitive array, `offset` is a **byte offset** into the element storage, with
//!   `arrayBaseOffset = 0` and `arrayIndexScale = element width`, so the library's
//!   `base + index * scale` arithmetic lands on element boundaries. Reads and writes that cross an
//!   element boundary (the unaligned accessors compose `getByte`/`getShort`) are decoded through a
//!   byte view of the elements;
//! * on a reference array, `offset` is an **element index** (its scale is 1), because the elements
//!   are heap references, not bytes.
//!
//! The volatile variants are the plain ones: this VM has one thread for now (M3 introduces real
//! threads), so there is no other writer to order against. Fences are no-ops for the same reason.
//! Nothing here takes a lock; the interpreter's monitors are the only ordering it has.

use rava_vm::native::expect_args;
use rava_vm::{
    Array, ArrayKind, ClassTarget, HeapObject, NativeCtx, ObjRef, Shared, Value, VmError,
};

use crate::no_op;

/// The accessor shape: how a value is read from, and written to, an `(object, offset)` pair.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
    Reference,
}

impl Kind {
    /// The number of bytes the accessor touches in array storage.
    fn width(self) -> usize {
        match self {
            Self::Boolean | Self::Byte => 1,
            Self::Char | Self::Short => 2,
            Self::Int | Self::Float => 4,
            Self::Long | Self::Double => 8,
            // Reference access is element-addressed; the width is the scale the loader reports.
            Self::Reference => 1,
        }
    }

    /// The value an array element of this kind stores, decoded from its little/native-endian
    /// bytes. Floats are bits, so `getInt` on a `float[]` would still be raw; the getters decode
    /// by their own kind, not by the array's.
    fn decode(self, bytes: &[u8]) -> Value {
        match self {
            Self::Boolean => Value::Int(i32::from(bytes[0] != 0)),
            Self::Byte => Value::Int(i32::from(bytes[0] as i8)),
            Self::Char => Value::Int(i32::from(u16::from_ne_bytes([bytes[0], bytes[1]]))),
            Self::Short => Value::Int(i32::from(i16::from_ne_bytes([bytes[0], bytes[1]]))),
            Self::Int => Value::Int(i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            Self::Float => Value::Float(f32::from_bits(u32::from_ne_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3],
            ]))),
            Self::Long => Value::Long(i64::from_ne_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ])),
            Self::Double => Value::Double(f64::from_bits(u64::from_ne_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]))),
            Self::Reference => unreachable!("a reference is never decoded from bytes"),
        }
    }

    /// The bytes an array element of this kind stores, for the byte view.
    fn to_bytes(self, value: Value) -> Result<Vec<u8>, VmError> {
        let bytes = match self {
            Self::Boolean => vec![u8::from(value.as_int().unwrap_or(0) != 0)],
            Self::Byte => vec![value.as_int().unwrap_or(0) as u8],
            Self::Char => (value.as_int().unwrap_or(0) as u16).to_ne_bytes().to_vec(),
            Self::Short => (value.as_int().unwrap_or(0) as i16).to_ne_bytes().to_vec(),
            Self::Int => value.as_int().unwrap_or(0).to_ne_bytes().to_vec(),
            Self::Float => value
                .as_float()
                .ok_or_else(|| mismatch("a float", value))?
                .to_bits()
                .to_ne_bytes()
                .to_vec(),
            Self::Long => value.as_long().unwrap_or(0).to_ne_bytes().to_vec(),
            Self::Double => value
                .as_double()
                .ok_or_else(|| mismatch("a double", value))?
                .to_bits()
                .to_ne_bytes()
                .to_vec(),
            Self::Reference => return Err(VmError::Native("a reference is not bytes".to_owned())),
        };
        Ok(bytes)
    }

    /// Coerces a stored field value to the accessor's type, the way a raw read would.
    fn coerce(self, value: Value) -> Result<Value, VmError> {
        match self {
            Self::Boolean => Ok(Value::Int(i32::from(value.as_int().unwrap_or(0) != 0))),
            Self::Byte => Ok(Value::Int(i32::from(value.as_int().unwrap_or(0) as i8))),
            Self::Char => Ok(Value::Int(i32::from(value.as_int().unwrap_or(0) as u16))),
            Self::Short => Ok(Value::Int(i32::from(value.as_int().unwrap_or(0) as i16))),
            Self::Int => value
                .as_int()
                .map(Value::Int)
                .ok_or_else(|| mismatch("an int", value)),
            Self::Long => value
                .as_long()
                .map(Value::Long)
                .ok_or_else(|| mismatch("a long", value)),
            Self::Float => value
                .as_float()
                .map(Value::Float)
                .ok_or_else(|| mismatch("a float", value)),
            Self::Double => value
                .as_double()
                .map(Value::Double)
                .ok_or_else(|| mismatch("a double", value)),
            Self::Reference => Ok(value),
        }
    }
}

fn mismatch(expected: &'static str, found: Value) -> VmError {
    VmError::TypeMismatch { expected, found }
}

/// The element width a primitive array of `kind` uses, mirroring `arrayIndexScale0`.
fn scale(kind: ArrayKind) -> usize {
    match kind {
        ArrayKind::Boolean | ArrayKind::Byte => 1,
        ArrayKind::Char | ArrayKind::Short => 2,
        ArrayKind::Int | ArrayKind::Float => 4,
        ArrayKind::Long | ArrayKind::Double => 8,
        ArrayKind::Reference => 1,
    }
}

/// The `Kind` an array element is stored as, for the byte view.
fn element_kind(kind: ArrayKind) -> Kind {
    match kind {
        ArrayKind::Boolean => Kind::Boolean,
        ArrayKind::Byte => Kind::Byte,
        ArrayKind::Char => Kind::Char,
        ArrayKind::Short => Kind::Short,
        ArrayKind::Int => Kind::Int,
        ArrayKind::Long => Kind::Long,
        ArrayKind::Float => Kind::Float,
        ArrayKind::Double => Kind::Double,
        ArrayKind::Reference => Kind::Reference,
    }
}

/// Decodes an offset for an instance: the field slot.
fn instance_slot(instance: &rava_vm::Instance, offset: usize) -> Result<usize, VmError> {
    if offset >= instance.fields.len() {
        return Err(VmError::Native(format!(
            "field offset {offset} is outside the {}-field layout",
            instance.fields.len()
        )));
    }
    Ok(offset)
}

/// One byte of an array's element storage, as a byte view would see it.
fn element_byte(shared: &Shared, array: &Array, position: usize) -> Result<u8, VmError> {
    let width = scale(array.kind);
    let index = position / width;
    let within = position % width;
    if array.kind == ArrayKind::Reference {
        return Err(VmError::Native(
            "a reference array has no byte view".to_owned(),
        ));
    }
    let value = *array.elements.get(index).ok_or(VmError::IndexOutOfBounds {
        index: index as i32,
        length: array.elements.len(),
    })?;
    let bytes = element_kind(array.kind).to_bytes(value)?;
    let _ = shared;
    Ok(bytes[within])
}

/// Reads `width` bytes at a byte offset of a primitive array.
fn read_bytes(
    shared: &Shared,
    array: &Array,
    offset: usize,
    width: usize,
) -> Result<Vec<u8>, VmError> {
    let mut bytes = Vec::with_capacity(width);
    let total = array.elements.len() * scale(array.kind);
    if array.kind == ArrayKind::Reference {
        return Err(VmError::Native(
            "a reference array is addressed by element, not byte".to_owned(),
        ));
    }
    for position in offset..offset + width {
        if position >= total {
            return Err(VmError::IndexOutOfBounds {
                index: position as i32,
                length: total,
            });
        }
        bytes.push(element_byte(shared, array, position)?);
    }
    Ok(bytes)
}

/// Writes bytes into a primitive array at a byte offset, read-modify-writing any element they
/// straddle.
fn write_bytes(
    shared: &mut Shared,
    reference: ObjRef,
    offset: usize,
    bytes: &[u8],
) -> Result<(), VmError> {
    let kind = shared.array(reference)?.kind;
    if kind == ArrayKind::Reference {
        return Err(VmError::Native(
            "a reference array is addressed by element, not byte".to_owned(),
        ));
    }
    let width = scale(kind);
    let total = shared.array(reference)?.elements.len() * width;
    if offset + bytes.len() > total {
        return Err(VmError::IndexOutOfBounds {
            index: (offset + bytes.len()) as i32,
            length: total,
        });
    }
    for (step, byte) in bytes.iter().enumerate() {
        let position = offset + step;
        let index = position / width;
        let within = position % width;
        let array = shared.array_mut(reference)?;
        let current = *array.elements.get(index).ok_or(VmError::IndexOutOfBounds {
            index: index as i32,
            length: array.elements.len(),
        })?;
        let mut element = element_kind(kind).to_bytes(current)?;
        element[within] = *byte;
        array.elements[index] = element_kind(kind).decode(&element);
    }
    Ok(())
}

/// Reads a cell: an instance field value, a reference element, or `width` bytes of a primitive
/// array starting at a byte offset.
fn read_cell(shared: &Shared, object: Value, offset: Value, kind: Kind) -> Result<Value, VmError> {
    let reference = object
        .as_ref()
        .ok_or_else(|| mismatch("a non-null object", object))?;
    let offset = offset
        .as_long()
        .ok_or_else(|| mismatch("a long offset", offset))?;
    let offset = usize::try_from(offset)
        .map_err(|_| VmError::Native("a negative Unsafe offset".to_owned()))?;
    match shared.heap.get(reference.index()) {
        Some(HeapObject::Instance(instance)) => {
            let slot = instance_slot(instance, offset)?;
            kind.coerce(instance.fields[slot])
        }
        Some(HeapObject::Array(array)) if array.kind == ArrayKind::Reference => {
            if kind != Kind::Reference {
                return Err(VmError::Native(
                    "a reference array is addressed by reference accessors only".to_owned(),
                ));
            }
            array
                .elements
                .get(offset)
                .copied()
                .ok_or(VmError::IndexOutOfBounds {
                    index: offset as i32,
                    length: array.elements.len(),
                })
        }
        Some(HeapObject::Array(array)) => {
            let bytes = read_bytes(shared, array, offset, kind.width())?;
            Ok(kind.decode(&bytes))
        }
        Some(_) => Err(mismatch("an object or array", object)),
        None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
    }
}

/// Writes a cell, mirroring [`read_cell`].
fn write_cell(
    shared: &mut Shared,
    object: Value,
    offset: Value,
    kind: Kind,
    value: Value,
) -> Result<(), VmError> {
    let reference = object
        .as_ref()
        .ok_or_else(|| mismatch("a non-null object", object))?;
    let offset = offset
        .as_long()
        .ok_or_else(|| mismatch("a long offset", offset))?;
    let offset = usize::try_from(offset)
        .map_err(|_| VmError::Native("a negative Unsafe offset".to_owned()))?;
    match shared.heap.get(reference.index()) {
        Some(HeapObject::Instance(instance)) => {
            let slot = instance_slot(instance, offset)?;
            let stored = kind.coerce(value)?;
            shared.set_instance_field(reference, slot, stored)
        }
        Some(HeapObject::Array(array)) if array.kind == ArrayKind::Reference => {
            if kind != Kind::Reference {
                return Err(VmError::Native(
                    "a reference array is addressed by reference accessors only".to_owned(),
                ));
            }
            let cells = &mut shared.array_mut(reference)?.elements;
            let length = cells.len();
            let cell = cells.get_mut(offset).ok_or(VmError::IndexOutOfBounds {
                index: offset as i32,
                length,
            })?;
            *cell = value;
            Ok(())
        }
        Some(HeapObject::Array(_)) => {
            let bytes = kind.to_bytes(value)?;
            write_bytes(shared, reference, offset, &bytes)
        }
        Some(_) => Err(mismatch("an object or array", object)),
        None => Err(VmError::Native(format!("dangling reference {reference:?}"))),
    }
}

// One accessor family per type. The volatile variants share the plain implementation: with a
// single thread there is nothing to order against, and registering them keeps the library from
// seeing a missing native where the plain one would do.
macro_rules! accessors {
    ($get:ident, $put:ident, $get_volatile:ident, $put_volatile:ident, $kind:expr) => {
        fn $get(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
            let [_this, object, offset] = expect_args::<3>(ctx)?;
            read_cell(ctx.shared, *object, *offset, $kind)
        }

        fn $put(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
            let [_this, object, offset, value] = expect_args::<4>(ctx)?;
            write_cell(ctx.shared, *object, *offset, $kind, *value)?;
            Ok(Value::NULL)
        }

        fn $get_volatile(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
            $get(ctx)
        }

        fn $put_volatile(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
            $put(ctx)
        }
    };
}

accessors!(
    get_boolean,
    put_boolean,
    get_boolean_volatile,
    put_boolean_volatile,
    Kind::Boolean
);
accessors!(
    get_byte,
    put_byte,
    get_byte_volatile,
    put_byte_volatile,
    Kind::Byte
);
accessors!(
    get_char,
    put_char,
    get_char_volatile,
    put_char_volatile,
    Kind::Char
);
accessors!(
    get_short,
    put_short,
    get_short_volatile,
    put_short_volatile,
    Kind::Short
);
accessors!(
    get_int,
    put_int,
    get_int_volatile,
    put_int_volatile,
    Kind::Int
);
accessors!(
    get_long,
    put_long,
    get_long_volatile,
    put_long_volatile,
    Kind::Long
);
accessors!(
    get_float,
    put_float,
    get_float_volatile,
    put_float_volatile,
    Kind::Float
);
accessors!(
    get_double,
    put_double,
    get_double_volatile,
    put_double_volatile,
    Kind::Double
);
accessors!(
    get_reference,
    put_reference,
    get_reference_volatile,
    put_reference_volatile,
    Kind::Reference
);

fn compare_and_set_reference(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, object, offset, expected, new] = expect_args::<5>(ctx)?;
    let current = read_cell(ctx.shared, *object, *offset, Kind::Reference)?;
    if current.as_ref() == expected.as_ref() {
        write_cell(ctx.shared, *object, *offset, Kind::Reference, *new)?;
        Ok(Value::Int(1))
    } else {
        Ok(Value::Int(0))
    }
}

fn compare_and_exchange_reference(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, object, offset, expected, new] = expect_args::<5>(ctx)?;
    let current = read_cell(ctx.shared, *object, *offset, Kind::Reference)?;
    if current.as_ref() == expected.as_ref() {
        write_cell(ctx.shared, *object, *offset, Kind::Reference, *new)?;
    }
    Ok(current)
}

fn as_i32(value: Value) -> Result<i32, VmError> {
    value.as_int().ok_or_else(|| mismatch("an int", value))
}

fn as_i64(value: Value) -> Result<i64, VmError> {
    value.as_long().ok_or_else(|| mismatch("a long", value))
}

fn compare_and_set_int(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, object, offset, expected, new] = expect_args::<5>(ctx)?;
    let current = read_cell(ctx.shared, *object, *offset, Kind::Int)?;
    if as_i32(current)? == as_i32(*expected)? {
        write_cell(ctx.shared, *object, *offset, Kind::Int, *new)?;
        Ok(Value::Int(1))
    } else {
        Ok(Value::Int(0))
    }
}

fn compare_and_exchange_int(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, object, offset, expected, new] = expect_args::<5>(ctx)?;
    let current = read_cell(ctx.shared, *object, *offset, Kind::Int)?;
    if as_i32(current)? == as_i32(*expected)? {
        write_cell(ctx.shared, *object, *offset, Kind::Int, *new)?;
    }
    Ok(current)
}

fn compare_and_set_long(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, object, offset, expected, new] = expect_args::<5>(ctx)?;
    let current = read_cell(ctx.shared, *object, *offset, Kind::Long)?;
    if as_i64(current)? == as_i64(*expected)? {
        write_cell(ctx.shared, *object, *offset, Kind::Long, *new)?;
        Ok(Value::Int(1))
    } else {
        Ok(Value::Int(0))
    }
}

fn compare_and_exchange_long(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, object, offset, expected, new] = expect_args::<5>(ctx)?;
    let current = read_cell(ctx.shared, *object, *offset, Kind::Long)?;
    if as_i64(current)? == as_i64(*expected)? {
        write_cell(ctx.shared, *object, *offset, Kind::Long, *new)?;
    }
    Ok(current)
}

/// `Unsafe.objectFieldOffset(Class, String)`: the field's slot in the class layout.
fn object_field_offset_name(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, class, name] = expect_args::<3>(ctx)?;
    let reference = class
        .as_ref()
        .ok_or_else(|| mismatch("a class object", *class))?;
    let target = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Loaded(class)) => *class,
        _ => {
            return Err(VmError::Native(
                "objectFieldOffset needs a loaded class".to_owned(),
            ));
        }
    };
    let name_reference = name.as_ref().ok_or_else(|| mismatch("a String", *name))?;
    let name = ctx.shared.string_text(name_reference)?;
    let slot = ctx
        .shared
        .class(target)
        .field_slot_by_name(&name)
        .ok_or_else(|| VmError::FieldNotFound {
            class: ctx.shared.class(target).name.clone(),
            name,
        })?;
    Ok(Value::Long(slot as i64))
}

/// `Unsafe.objectFieldOffset(Field)`: the slot of the field the `Field` object names.
fn object_field_offset_field(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, field] = expect_args::<2>(ctx)?;
    let reference = field
        .as_ref()
        .ok_or_else(|| mismatch("a Field object", *field))?;
    let (class, index) = ctx
        .shared
        .field_ref(reference)
        .ok_or_else(|| VmError::Native(format!("{reference:?} is not a Field object")))?;
    let info = ctx.shared.class(class).fields.get(index).ok_or_else(|| {
        VmError::Native(format!(
            "field index {index} is outside {}",
            ctx.shared.class(class).name
        ))
    })?;
    let (name, descriptor) = (info.name.clone(), info.descriptor.clone());
    let slot = ctx
        .shared
        .class(class)
        .field_slot(&name, &descriptor)
        .ok_or(VmError::FieldNotFound {
            class: ctx.shared.class(class).name.clone(),
            name,
        })?;
    Ok(Value::Long(slot as i64))
}

/// `Unsafe.allocateInstance(Class)`: an instance with the class's default field values, without
/// running a constructor.
fn allocate_instance(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, class] = expect_args::<2>(ctx)?;
    let reference = class
        .as_ref()
        .ok_or_else(|| mismatch("a class object", *class))?;
    let target = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Loaded(class)) => *class,
        _ => {
            return Err(VmError::Native(
                "allocateInstance needs a loaded class".to_owned(),
            ));
        }
    };
    Ok(Value::Ref(ctx.shared.new_instance(target)))
}

/// `Unsafe.throwException(Throwable)`: hand the throwable to the interpreter's unwinding.
fn throw_exception(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, throwable] = expect_args::<2>(ctx)?;
    let object = throwable
        .as_ref()
        .ok_or_else(|| mismatch("a Throwable", *throwable))?;
    Err(VmError::Thrown { object })
}

/// The array geometry answers (kept from S10; `ArraysSupport` reads the scales in its `<clinit>`).
fn array_base_offset(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, _array_class] = expect_args::<2>(ctx)?;
    Ok(Value::Int(0))
}

fn array_index_scale(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_this, array_class] = expect_args::<2>(ctx)?;
    let reference = array_class
        .as_ref()
        .ok_or_else(|| mismatch("an array class", *array_class))?;
    let descriptor = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Type(descriptor)) => descriptor.as_str(),
        _ => {
            return Err(VmError::Native(
                "arrayIndexScale0 needs a primitive array class".to_owned(),
            ));
        }
    };
    let scale = match descriptor {
        "[Z" | "[B" => 1,
        "[C" | "[S" => 2,
        "[I" | "[F" => 4,
        "[J" | "[D" => 8,
        other if other.starts_with("[L") => 1,
        other => {
            return Err(VmError::Native(format!(
                "arrayIndexScale0: {other} is not an array type"
            )));
        }
    };
    Ok(Value::Int(scale))
}

/// A no-op for a native declared on an instance: the receiver is still on the argument list.
fn no_op_with_receiver(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
}

/// Installs the `Unsafe` natives.
pub fn register(shared: &mut Shared) {
    const U: &str = "jdk/internal/misc/Unsafe";
    shared.register_native(U, "registerNatives", "()V", no_op);
    shared.register_native(
        U,
        "objectFieldOffset0",
        "(Ljava/lang/reflect/Field;)J",
        object_field_offset_field,
    );
    shared.register_native(
        U,
        "objectFieldOffset1",
        "(Ljava/lang/Class;Ljava/lang/String;)J",
        object_field_offset_name,
    );
    shared.register_native(
        U,
        "arrayBaseOffset0",
        "(Ljava/lang/Class;)I",
        array_base_offset,
    );
    shared.register_native(
        U,
        "arrayIndexScale0",
        "(Ljava/lang/Class;)I",
        array_index_scale,
    );
    shared.register_native(
        U,
        "allocateInstance",
        "(Ljava/lang/Class;)Ljava/lang/Object;",
        allocate_instance,
    );
    shared.register_native(
        U,
        "throwException",
        "(Ljava/lang/Throwable;)V",
        throw_exception,
    );
    shared.register_native(
        U,
        "compareAndSetReference",
        "(Ljava/lang/Object;JLjava/lang/Object;Ljava/lang/Object;)Z",
        compare_and_set_reference,
    );
    shared.register_native(
        U,
        "compareAndExchangeReference",
        "(Ljava/lang/Object;JLjava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        compare_and_exchange_reference,
    );
    shared.register_native(
        U,
        "compareAndSetInt",
        "(Ljava/lang/Object;JII)Z",
        compare_and_set_int,
    );
    shared.register_native(
        U,
        "compareAndExchangeInt",
        "(Ljava/lang/Object;JII)I",
        compare_and_exchange_int,
    );
    shared.register_native(
        U,
        "compareAndSetLong",
        "(Ljava/lang/Object;JJJ)Z",
        compare_and_set_long,
    );
    shared.register_native(
        U,
        "compareAndExchangeLong",
        "(Ljava/lang/Object;JJJ)J",
        compare_and_exchange_long,
    );
    // Heap writeback is a no-op: this VM owns the heap and the library never sees a copy.
    shared.register_native(U, "writeback0", "(J)V", no_op_with_receiver);
    shared.register_native(U, "writebackPreSync0", "()V", no_op_with_receiver);
    shared.register_native(U, "writebackPostSync0", "()V", no_op_with_receiver);
    shared.register_native(U, "fullFence", "()V", no_op_with_receiver);
    register_accessors(shared);
}

/// The 32 plain/volatile accessor registrations, kept together so a missing one shows up as a
/// diff of this function rather than of the library.
fn register_accessors(shared: &mut Shared) {
    const U: &str = "jdk/internal/misc/Unsafe";
    shared.register_native(U, "getBoolean", "(Ljava/lang/Object;J)Z", get_boolean);
    shared.register_native(
        U,
        "getBooleanVolatile",
        "(Ljava/lang/Object;J)Z",
        get_boolean_volatile,
    );
    shared.register_native(U, "putBoolean", "(Ljava/lang/Object;JZ)V", put_boolean);
    shared.register_native(
        U,
        "putBooleanVolatile",
        "(Ljava/lang/Object;JZ)V",
        put_boolean_volatile,
    );
    shared.register_native(U, "getByte", "(Ljava/lang/Object;J)B", get_byte);
    shared.register_native(
        U,
        "getByteVolatile",
        "(Ljava/lang/Object;J)B",
        get_byte_volatile,
    );
    shared.register_native(U, "putByte", "(Ljava/lang/Object;JB)V", put_byte);
    shared.register_native(
        U,
        "putByteVolatile",
        "(Ljava/lang/Object;JB)V",
        put_byte_volatile,
    );
    shared.register_native(U, "getChar", "(Ljava/lang/Object;J)C", get_char);
    shared.register_native(
        U,
        "getCharVolatile",
        "(Ljava/lang/Object;J)C",
        get_char_volatile,
    );
    shared.register_native(U, "putChar", "(Ljava/lang/Object;JC)V", put_char);
    shared.register_native(
        U,
        "putCharVolatile",
        "(Ljava/lang/Object;JC)V",
        put_char_volatile,
    );
    shared.register_native(U, "getShort", "(Ljava/lang/Object;J)S", get_short);
    shared.register_native(
        U,
        "getShortVolatile",
        "(Ljava/lang/Object;J)S",
        get_short_volatile,
    );
    shared.register_native(U, "putShort", "(Ljava/lang/Object;JS)V", put_short);
    shared.register_native(
        U,
        "putShortVolatile",
        "(Ljava/lang/Object;JS)V",
        put_short_volatile,
    );
    shared.register_native(U, "getInt", "(Ljava/lang/Object;J)I", get_int);
    shared.register_native(
        U,
        "getIntVolatile",
        "(Ljava/lang/Object;J)I",
        get_int_volatile,
    );
    shared.register_native(U, "putInt", "(Ljava/lang/Object;JI)V", put_int);
    shared.register_native(
        U,
        "putIntVolatile",
        "(Ljava/lang/Object;JI)V",
        put_int_volatile,
    );
    shared.register_native(U, "getLong", "(Ljava/lang/Object;J)J", get_long);
    shared.register_native(
        U,
        "getLongVolatile",
        "(Ljava/lang/Object;J)J",
        get_long_volatile,
    );
    shared.register_native(U, "putLong", "(Ljava/lang/Object;JJ)V", put_long);
    shared.register_native(
        U,
        "putLongVolatile",
        "(Ljava/lang/Object;JJ)V",
        put_long_volatile,
    );
    shared.register_native(U, "getFloat", "(Ljava/lang/Object;J)F", get_float);
    shared.register_native(
        U,
        "getFloatVolatile",
        "(Ljava/lang/Object;J)F",
        get_float_volatile,
    );
    shared.register_native(U, "putFloat", "(Ljava/lang/Object;JF)V", put_float);
    shared.register_native(
        U,
        "putFloatVolatile",
        "(Ljava/lang/Object;JF)V",
        put_float_volatile,
    );
    shared.register_native(U, "getDouble", "(Ljava/lang/Object;J)D", get_double);
    shared.register_native(
        U,
        "getDoubleVolatile",
        "(Ljava/lang/Object;J)D",
        get_double_volatile,
    );
    shared.register_native(U, "putDouble", "(Ljava/lang/Object;JD)V", put_double);
    shared.register_native(
        U,
        "putDoubleVolatile",
        "(Ljava/lang/Object;JD)V",
        put_double_volatile,
    );
    shared.register_native(
        U,
        "getReference",
        "(Ljava/lang/Object;J)Ljava/lang/Object;",
        get_reference,
    );
    shared.register_native(
        U,
        "getReferenceVolatile",
        "(Ljava/lang/Object;J)Ljava/lang/Object;",
        get_reference_volatile,
    );
    shared.register_native(
        U,
        "putReference",
        "(Ljava/lang/Object;JLjava/lang/Object;)V",
        put_reference,
    );
    shared.register_native(
        U,
        "putReferenceVolatile",
        "(Ljava/lang/Object;JLjava/lang/Object;)V",
        put_reference_volatile,
    );
}
