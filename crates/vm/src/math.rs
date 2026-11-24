//! `java.lang.Math` natives (M2.23).
//!
//! Spring's collection sizing (`ConcurrentReferenceHashMap`), `ObjectUtils` and the
//! annotation metadata use `ceil`, `max`, `min`, `abs` and `round`. The implementations
//! map to Rust's `f32`/`f64` operations, including the saturating casts the JDK uses for
//! `round`.
//!
//! There is no `StrictMath` entry point and no `Math.multiplyExact`/`addExact`; those
//! raise Java exceptions on overflow, which this slice does not synthesize.

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

const MATH_CLASS: &str = "java/lang/Math";

pub fn register_natives(shared: &Shared) {
    for (name, descriptor, function) in [
        ("ceil", "(D)D", math_ceil as crate::NativeFn),
        ("floor", "(D)D", math_floor),
        ("rint", "(D)D", math_rint),
        ("sqrt", "(D)D", math_sqrt),
        ("cbrt", "(D)D", math_cbrt),
        ("pow", "(DD)D", math_pow),
        ("exp", "(D)D", math_exp),
        ("log", "(D)D", math_log),
        ("log10", "(D)D", math_log10),
        ("toRadians", "(D)D", math_to_radians),
        ("toDegrees", "(D)D", math_to_degrees),
        ("sin", "(D)D", math_sin),
        ("cos", "(D)D", math_cos),
        ("tan", "(D)D", math_tan),
        ("round", "(D)J", math_round_double),
        ("round", "(F)I", math_round_float),
        ("random", "()D", math_random),
        ("abs", "(I)I", math_abs_int),
        ("abs", "(J)J", math_abs_long),
        ("abs", "(F)F", math_abs_float),
        ("abs", "(D)D", math_abs_double),
        ("max", "(II)I", math_max_int),
        ("max", "(JJ)J", math_max_long),
        ("max", "(FF)F", math_max_float),
        ("max", "(DD)D", math_max_double),
        ("min", "(II)I", math_min_int),
        ("min", "(JJ)J", math_min_long),
        ("min", "(FF)F", math_min_float),
        ("min", "(DD)D", math_min_double),
    ] {
        shared.register_native(MATH_CLASS, name, descriptor, function);
    }
}

fn double_argument(args: &[Value], position: usize, method: &'static str) -> Result<f64, VmError> {
    match args.get(position).copied() {
        Some(Value::Double(value)) => Ok(value),
        Some(Value::Float(value)) => Ok(f64::from(value)),
        Some(Value::Int(value)) => Ok(f64::from(value)),
        Some(Value::Long(value)) => Ok(value as f64),
        _ => Err(VmError::Native(format!(
            "{method} expects a double argument"
        ))),
    }
}

fn float_argument(args: &[Value], position: usize, method: &'static str) -> Result<f32, VmError> {
    match args.get(position).copied() {
        Some(Value::Float(value)) => Ok(value),
        Some(Value::Double(value)) => Ok(value as f32),
        Some(Value::Int(value)) => Ok(value as f32),
        _ => Err(VmError::Native(format!(
            "{method} expects a float argument"
        ))),
    }
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(format!("{method} expects an int argument"))),
    }
}

fn long_argument(args: &[Value], position: usize, method: &'static str) -> Result<i64, VmError> {
    match args.get(position).copied() {
        Some(Value::Long(value)) => Ok(value),
        _ => Err(VmError::Native(format!("{method} expects a long argument"))),
    }
}

macro_rules! unary_double {
    ($name:ident, $method:literal, $operation:expr) => {
        fn $name(
            _shared: &Shared,
            _thread: &ThreadState,
            _loader: LoaderId,
            _receiver: ObjRef,
            args: &[Value],
        ) -> Result<Option<Value>, VmError> {
            let value = double_argument(args, 0, $method)?;
            Ok(Some(Value::Double($operation(value))))
        }
    };
}

unary_double!(math_ceil, "Math.ceil", f64::ceil);
unary_double!(math_floor, "Math.floor", f64::floor);
unary_double!(math_rint, "Math.rint", f64::round_ties_even);
unary_double!(math_sqrt, "Math.sqrt", f64::sqrt);
unary_double!(math_cbrt, "Math.cbrt", f64::cbrt);
unary_double!(math_exp, "Math.exp", f64::exp);
unary_double!(math_log, "Math.log", f64::ln);
unary_double!(math_log10, "Math.log10", f64::log10);
unary_double!(math_to_radians, "Math.toRadians", f64::to_radians);
unary_double!(math_to_degrees, "Math.toDegrees", f64::to_degrees);
unary_double!(math_sin, "Math.sin", f64::sin);
unary_double!(math_cos, "Math.cos", f64::cos);
unary_double!(math_tan, "Math.tan", f64::tan);

fn math_pow(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let base = double_argument(args, 0, "Math.pow")?;
    let exponent = double_argument(args, 1, "Math.pow")?;
    Ok(Some(Value::Double(base.powf(exponent))))
}

fn math_round_double(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = double_argument(args, 0, "Math.round")?;
    Ok(Some(Value::Long(value.round() as i64)))
}

fn math_round_float(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let value = float_argument(args, 0, "Math.round")?;
    Ok(Some(Value::Int(value.round() as i32)))
}

fn math_random(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    // A deterministic-per-process value derived from the clock; there is no seeded
    // generator in this slice.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos())
        .unwrap_or(0);
    Ok(Some(Value::Double(f64::from(nanos) / 1_000_000_000.0)))
}

fn math_abs_int(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        int_argument(args, 0, "Math.abs")?.wrapping_abs(),
    )))
}

fn math_abs_long(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        long_argument(args, 0, "Math.abs")?.wrapping_abs(),
    )))
}

fn math_abs_float(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Float(
        float_argument(args, 0, "Math.abs")?.abs(),
    )))
}

fn math_abs_double(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Double(
        double_argument(args, 0, "Math.abs")?.abs(),
    )))
}

fn math_max_int(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        int_argument(args, 0, "Math.max")?.max(int_argument(args, 1, "Math.max")?),
    )))
}

fn math_min_int(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(
        int_argument(args, 0, "Math.min")?.min(int_argument(args, 1, "Math.min")?),
    )))
}

fn math_max_long(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        long_argument(args, 0, "Math.max")?.max(long_argument(args, 1, "Math.max")?),
    )))
}

fn math_min_long(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(
        long_argument(args, 0, "Math.min")?.min(long_argument(args, 1, "Math.min")?),
    )))
}

fn math_max_float(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Float(
        float_argument(args, 0, "Math.max")?.max(float_argument(args, 1, "Math.max")?),
    )))
}

fn math_min_float(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Float(
        float_argument(args, 0, "Math.min")?.min(float_argument(args, 1, "Math.min")?),
    )))
}

fn math_max_double(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Double(
        double_argument(args, 0, "Math.max")?.max(double_argument(args, 1, "Math.max")?),
    )))
}

fn math_min_double(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Double(
        double_argument(args, 0, "Math.min")?.min(double_argument(args, 1, "Math.min")?),
    )))
}
