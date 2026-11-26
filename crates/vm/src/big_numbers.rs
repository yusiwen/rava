//! `java.math.BigInteger` natives over a 128-bit host value (M4.5).
//!
//! Spring's `NumberUtils` static initializer stores `Long.MIN_VALUE` and `Long.MAX_VALUE`
//! as `BigInteger`s and its conversion path compares against them, so the pinned framework
//! needs a real integer object, not a placeholder. The C1 stub declares no fields, so the
//! value lives in a side table keyed by [`ObjRef`], exactly as the M4.1 resource and
//! `java.time` objects do; the objects stay ordinary `BigInteger` instances.
//!
//! The host value is an `i128`. That covers the `long` boundary checks and any ordinary
//! configuration number, but a value outside the `i128` range is a typed error rather
//! than a silent truncation. Arbitrary-precision arithmetic (a limb representation), the
//! prime/modular surface and `BigDecimal` are later slices.

use std::collections::HashMap;

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `java.math.BigInteger`'s binary-stable internal name.
pub const BIG_INTEGER_CLASS: &str = "java/math/BigInteger";

/// The value of every live `BigInteger`.
#[derive(Debug, Default)]
pub struct BigNumberState {
    integers: HashMap<ObjRef, i128>,
}

impl BigNumberState {
    /// The objects whose value the side table holds, for root enumeration.
    pub fn references(&self) -> Vec<ObjRef> {
        self.integers.keys().copied().collect()
    }

    /// Record the value of a freshly allocated `BigInteger`.
    pub fn insert_integer(&mut self, object: ObjRef, value: i128) {
        self.integers.insert(object, value);
    }

    /// The value of a `BigInteger`, or `None` for any other object.
    pub fn integer(&self, object: ObjRef) -> Option<i128> {
        self.integers.get(&object).copied()
    }
}

pub fn register_natives(shared: &Shared) {
    shared.register_native(
        BIG_INTEGER_CLASS,
        "valueOf",
        "(J)Ljava/math/BigInteger;",
        big_integer_value_of,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "<init>",
        "(Ljava/lang/String;)V",
        big_integer_init_string,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "<init>",
        "(Ljava/lang/String;I)V",
        big_integer_init_string_radix,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "compareTo",
        "(Ljava/math/BigInteger;)I",
        big_integer_compare_to,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "negate",
        "()Ljava/math/BigInteger;",
        big_integer_negate,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "abs",
        "()Ljava/math/BigInteger;",
        big_integer_abs,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "add",
        "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        big_integer_add,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "subtract",
        "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        big_integer_subtract,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "multiply",
        "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        big_integer_multiply,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "divide",
        "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        big_integer_divide,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "remainder",
        "(Ljava/math/BigInteger;)Ljava/math/BigInteger;",
        big_integer_remainder,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "toString",
        "()Ljava/lang/String;",
        big_integer_to_string,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "toString",
        "(I)Ljava/lang/String;",
        big_integer_to_string_radix,
    );
    shared.register_native(BIG_INTEGER_CLASS, "intValue", "()I", big_integer_int_value);
    shared.register_native(
        BIG_INTEGER_CLASS,
        "longValue",
        "()J",
        big_integer_long_value,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "byteValue",
        "()B",
        big_integer_byte_value,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "shortValue",
        "()S",
        big_integer_short_value,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "floatValue",
        "()F",
        big_integer_float_value,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "doubleValue",
        "()D",
        big_integer_double_value,
    );
    shared.register_native(
        BIG_INTEGER_CLASS,
        "equals",
        "(Ljava/lang/Object;)Z",
        big_integer_equals,
    );
    shared.register_native(BIG_INTEGER_CLASS, "hashCode", "()I", big_integer_hash_code);
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
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
        return Err(VmError::Native(format!("{method}: null String argument")));
    }
    let text = shared
        .heap()
        .string(reference)
        .map_err(heap_error)?
        .to_owned();
    Ok(text)
}

fn new_big_integer(shared: &Shared, value: i128) -> Result<Value, VmError> {
    let class = shared.load_library_class(BIG_INTEGER_CLASS)?;
    let object = shared.allocate_instance(class)?;
    shared.big_number_state().insert_integer(object, value);
    Ok(Value::Ref(object))
}

fn integer_value(shared: &Shared, receiver: ObjRef, method: &str) -> Result<i128, VmError> {
    shared
        .big_number_state()
        .integer(receiver)
        .ok_or_else(|| VmError::Native(format!("{method}: BigInteger has no value")))
}

fn integer_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &str,
) -> Result<i128, VmError> {
    let Some(Value::Ref(reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{method} expects a BigInteger argument"
        )));
    };
    integer_value(shared, reference, method)
}

fn radix_argument(args: &[Value], position: usize, method: &str) -> Result<u32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) if (2..=36).contains(&value) => Ok(value as u32),
        _ => Err(VmError::Native(format!(
            "{method}: radix must be between 2 and 36"
        ))),
    }
}

fn big_integer_value_of(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = match args.first().copied() {
        Some(Value::Long(value)) => i128::from(value),
        Some(Value::Int(value)) => i128::from(value),
        _ => {
            return Err(VmError::Native(
                "BigInteger.valueOf expects a long".to_owned(),
            ));
        }
    };
    Ok(Some(new_big_integer(shared, value)?))
}

fn parse_integer(text: &str, radix: u32, method: &str) -> Result<i128, VmError> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let magnitude = i128::from_str_radix(digits, radix).map_err(|_| {
        VmError::Native(format!(
            "{method}: {text:?} is not a base-{radix} integer in the i128 range"
        ))
    })?;
    Ok(if negative { -magnitude } else { magnitude })
}

fn big_integer_init_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "BigInteger.<init>")?;
    let value = parse_integer(&text, 10, "BigInteger.<init>")?;
    shared.big_number_state().insert_integer(receiver, value);
    Ok(None)
}

fn big_integer_init_string_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = string_argument(shared, args, 0, "BigInteger.<init>")?;
    let radix = radix_argument(args, 1, "BigInteger.<init>")?;
    let value = parse_integer(&text, radix, "BigInteger.<init>")?;
    shared.big_number_state().insert_integer(receiver, value);
    Ok(None)
}

fn big_integer_compare_to(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let left = integer_value(shared, receiver, "BigInteger.compareTo")?;
    let right = integer_argument(shared, args, 0, "BigInteger.compareTo")?;
    Ok(Some(Value::Int(left.cmp(&right) as i32)))
}

fn unary(
    shared: &Shared,
    receiver: ObjRef,
    method: &str,
    operation: impl FnOnce(i128) -> i128,
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, method)?;
    Ok(Some(new_big_integer(shared, operation(value))?))
}

fn big_integer_negate(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    unary(shared, receiver, "BigInteger.negate", |value| -value)
}

fn big_integer_abs(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    unary(shared, receiver, "BigInteger.abs", i128::abs)
}

fn binary(
    shared: &Shared,
    receiver: ObjRef,
    args: &[Value],
    method: &str,
    operation: impl FnOnce(i128, i128) -> Result<i128, VmError>,
) -> Result<Option<Value>, VmError> {
    let left = integer_value(shared, receiver, method)?;
    let right = integer_argument(shared, args, 0, method)?;
    Ok(Some(new_big_integer(shared, operation(left, right)?)?))
}

fn big_integer_add(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    binary(shared, receiver, args, "BigInteger.add", |a, b| {
        a.checked_add(b)
            .ok_or_else(|| VmError::Native("BigInteger.add overflowed i128".to_owned()))
    })
}

fn big_integer_subtract(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    binary(shared, receiver, args, "BigInteger.subtract", |a, b| {
        a.checked_sub(b)
            .ok_or_else(|| VmError::Native("BigInteger.subtract overflowed i128".to_owned()))
    })
}

fn big_integer_multiply(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    binary(shared, receiver, args, "BigInteger.multiply", |a, b| {
        a.checked_mul(b)
            .ok_or_else(|| VmError::Native("BigInteger.multiply overflowed i128".to_owned()))
    })
}

fn big_integer_divide(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    binary(shared, receiver, args, "BigInteger.divide", |a, b| {
        if b == 0 {
            return Err(VmError::Native("BigInteger.divide by zero".to_owned()));
        }
        Ok(a / b)
    })
}

fn big_integer_remainder(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    binary(shared, receiver, args, "BigInteger.remainder", |a, b| {
        if b == 0 {
            return Err(VmError::Native("BigInteger.remainder by zero".to_owned()));
        }
        Ok(a % b)
    })
}

fn integer_text(value: i128, radix: u32) -> String {
    if radix == 10 {
        return value.to_string();
    }
    if value == 0 {
        return "0".to_owned();
    }
    let mut magnitude = value.unsigned_abs();
    let mut digits = Vec::new();
    while magnitude > 0 {
        let digit = (magnitude % u128::from(radix)) as u32;
        digits.push(std::char::from_digit(digit, radix).unwrap_or('0'));
        magnitude /= u128::from(radix);
    }
    if value < 0 {
        digits.push('-');
    }
    digits.iter().rev().collect()
}

fn big_integer_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.toString")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(value.to_string()),
    )))
}

fn big_integer_to_string_radix(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.toString")?;
    let radix = radix_argument(args, 0, "BigInteger.toString")?;
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(integer_text(value, radix)),
    )))
}

fn big_integer_int_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.intValue")?;
    Ok(Some(Value::Int(value as i32)))
}

fn big_integer_long_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.longValue")?;
    Ok(Some(Value::Long(value as i64)))
}

fn big_integer_byte_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.byteValue")?;
    Ok(Some(Value::Int(value as i8 as i32)))
}

fn big_integer_short_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.shortValue")?;
    Ok(Some(Value::Int(value as i16 as i32)))
}

fn big_integer_float_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.floatValue")?;
    Ok(Some(Value::Float(value as f32)))
}

fn big_integer_double_value(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.doubleValue")?;
    Ok(Some(Value::Double(value as f64)))
}

fn big_integer_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(other)) = args.first().copied() else {
        return Ok(Some(Value::Int(0)));
    };
    let left = integer_value(shared, receiver, "BigInteger.equals")?;
    let equal = match shared.big_number_state().integer(other) {
        Some(right) => left == right,
        None => false,
    };
    Ok(Some(Value::Int(i32::from(equal))))
}

fn big_integer_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = integer_value(shared, receiver, "BigInteger.hashCode")?;
    Ok(Some(Value::Int(value as i64 as i32)))
}
