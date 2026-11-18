//! Java-compatible conversion of floating-point values to text.
//!
//! `Double.toString` is specified down to the digits (JLS §20.6): the shortest decimal that
//! distinguishes the value, at least one digit before and after the point, positional form for
//! magnitudes in `[10^-3, 10^7)` and scientific form outside it, and the exact spellings `NaN`,
//! `Infinity`, `-Infinity`, `0.0` and `-0.0`.
//!
//! Rust's `{:e}` already produces the shortest round-tripping digits, which is the hard part; this
//! module only reformats them the way Java does. Before it existed, a concatenated `1.0` printed as
//! `1` (issue #25).

/// `Double.toString`.
pub fn double_to_string(value: f64) -> String {
    finite_to_string(value, value.is_nan(), value.is_infinite(), value == 0.0)
}

/// `Float.toString`.
pub fn float_to_string(value: f32) -> String {
    finite_to_string(
        f64::from(value),
        value.is_nan(),
        value.is_infinite(),
        value == 0.0,
    )
}

fn finite_to_string(value: f64, nan: bool, infinite: bool, zero: bool) -> String {
    if nan {
        return "NaN".to_owned();
    }
    if infinite {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    if zero {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .to_owned();
    }

    let negative = value < 0.0;
    // `{:e}` gives the shortest round-tripping mantissa: `123.456` -> `1.23456e2`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("{:e} has an exponent");
    let exponent: i32 = exponent.parse().expect("{:e} exponent is a number");
    let digits: String = mantissa
        .chars()
        .filter(|character| *character != '.')
        .collect();

    let mut text = String::new();
    if negative {
        text.push('-');
    }

    // Java switches to scientific form outside [10^-3, 10^7).
    if (-3..7).contains(&exponent) {
        if exponent < 0 {
            text.push_str("0.");
            for _ in 0..(-exponent - 1) {
                text.push('0');
            }
            text.push_str(&digits);
        } else {
            let split = exponent as usize + 1;
            if digits.len() <= split {
                text.push_str(&digits);
                for _ in digits.len()..split {
                    text.push('0');
                }
                text.push_str(".0");
            } else {
                text.push_str(&digits[..split]);
                text.push('.');
                text.push_str(&digits[split..]);
            }
        }
    } else {
        text.push_str(&digits[..1]);
        text.push('.');
        if digits.len() > 1 {
            text.push_str(&digits[1..]);
        } else {
            text.push('0');
        }
        text.push('E');
        text.push_str(&exponent.to_string());
    }
    text
}
