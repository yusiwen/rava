//! `java.lang.invoke.StringConcatFactory` call sites (M2.18).
//!
//! Java 9 and later compile `+` string concatenation to `invokedynamic` with one of two
//! bootstrap methods: `makeConcat` when there is no constant text, and
//! `makeConcatWithConstants` when there is. The bootstrap is resolved by
//! [`rava_classfile::ClassFile::bootstrap_method`]; this module turns a resolved entry and
//! the call site's arguments into the concatenated, interned `String`.
//!
//! The conversion rules are the M2.9/M2.13 `String.valueOf` rules, not a second set: a
//! `String` contributes its content, null the text `"null"`, a boxed `Integer` its decimal
//! value, an object the VM's `ClassName@hex` shape, and a primitive its Java text. A
//! `char` must be a scalar value; a lone surrogate is a typed
//! [`crate::VmError::UnpairedSurrogate`] because the heap's string storage is UTF-8.
//!
//! The recipe uses the JVM's two markers: `\u0001` consumes the next dynamic argument and
//! `\u0002` consumes the next recipe constant. Every dynamic argument and every recipe
//! constant must be consumed exactly once; a recipe that names more or fewer than the call
//! site supplies is a typed error instead of a partial string.
//!
//! This module implements only `StringConcatFactory`. The interpreter routes a
//! `LambdaMetafactory` call site to its own lambda path; any other bootstrap that reaches
//! here is [`crate::VmError::UnsupportedBootstrapMethod`].

use rava_classfile::{BootstrapArgument, BootstrapMethodEntry};

use crate::VmError;
use crate::heap::ObjRef;
use crate::shared::Shared;
use crate::string::{double_text, float_text, object_text};
use crate::value::Value;

/// The internal name of the sole supported bootstrap owner.
pub const STRING_CONCAT_FACTORY: &str = "java/lang/invoke/StringConcatFactory";
/// The no-constants bootstrap method.
pub const MAKE_CONCAT: &str = "makeConcat";
/// The recipe-carrying bootstrap method.
pub const MAKE_CONCAT_WITH_CONSTANTS: &str = "makeConcatWithConstants";

/// Execute one resolved `invokedynamic` call site and return the interned result.
///
/// `descriptor` is the call site's method descriptor (from `CONSTANT_InvokeDynamic`);
/// `args` are the values popped from the operand stack, in call-site order.
pub fn invoke_call_site(
    shared: &Shared,
    bootstrap: &BootstrapMethodEntry,
    call_site_name: &str,
    descriptor: &str,
    args: &[Value],
) -> Result<ObjRef, VmError> {
    let handle = &bootstrap.handle;
    if handle.owner != STRING_CONCAT_FACTORY {
        return Err(unsupported(handle));
    }

    let descriptors = parameter_descriptors(descriptor)?;
    let result_descriptor = return_descriptor(descriptor)?;
    if result_descriptor != "Ljava/lang/String;" {
        return Err(VmError::Native(format!(
            "{call_site_name}{descriptor} does not return a String"
        )));
    }
    if descriptors.len() != args.len() {
        return Err(VmError::Native(format!(
            "{call_site_name} expects {} argument(s) by descriptor but received {}",
            descriptors.len(),
            args.len()
        )));
    }

    let text = match handle.name.as_str() {
        MAKE_CONCAT => {
            if !bootstrap.arguments.is_empty() {
                return Err(VmError::Native(format!(
                    "{call_site_name} has {} unexpected static argument(s)",
                    bootstrap.arguments.len()
                )));
            }
            concat_all(shared, &descriptors, args)?
        }
        MAKE_CONCAT_WITH_CONSTANTS => concat_with_recipe(
            shared,
            call_site_name,
            &descriptors,
            args,
            &bootstrap.arguments,
        )?,
        _ => return Err(unsupported(handle)),
    };

    // The result is interned, so repeated execution of one call site with equal input
    // produces the same object, matching `String.intern` and `ldc` of a string constant.
    Ok(shared.intern_string(&text))
}

fn unsupported(handle: &rava_classfile::BootstrapMethodHandle) -> VmError {
    VmError::UnsupportedBootstrapMethod {
        owner: handle.owner.clone(),
        name: handle.name.clone(),
        descriptor: handle.descriptor.clone(),
    }
}

/// `makeConcat`: concatenate every dynamic argument with `String.valueOf` semantics.
fn concat_all(shared: &Shared, descriptors: &[&str], args: &[Value]) -> Result<String, VmError> {
    let mut text = String::new();
    for (descriptor, value) in descriptors.iter().zip(args.iter()) {
        text.push_str(&argument_text(shared, descriptor, *value)?);
    }
    Ok(text)
}

/// `makeConcatWithConstants`: expand a recipe over the dynamic arguments and the
/// bootstrap's static arguments.
fn concat_with_recipe(
    shared: &Shared,
    call_site_name: &str,
    descriptors: &[&str],
    args: &[Value],
    recipe_arguments: &[BootstrapArgument],
) -> Result<String, VmError> {
    let Some(BootstrapArgument::String(recipe)) = recipe_arguments.first() else {
        return Err(VmError::Native(format!(
            "{call_site_name} has no String recipe argument"
        )));
    };
    let constants = &recipe_arguments[1..];

    let mut text = String::new();
    let mut dynamic = 0usize;
    let mut constant = 0usize;
    for marker in recipe.chars() {
        match marker {
            '\u{1}' => {
                let Some(descriptor) = descriptors.get(dynamic) else {
                    return Err(VmError::Native(format!(
                        "{call_site_name} recipe consumes more dynamic arguments than the {}-argument descriptor",
                        descriptors.len()
                    )));
                };
                let Some(value) = args.get(dynamic) else {
                    return Err(VmError::Native(format!(
                        "{call_site_name} is missing dynamic argument {dynamic}"
                    )));
                };
                text.push_str(&argument_text(shared, descriptor, *value)?);
                dynamic += 1;
            }
            '\u{2}' => {
                let Some(value) = constants.get(constant) else {
                    return Err(VmError::Native(format!(
                        "{call_site_name} recipe consumes more static constants than the {} supplied",
                        constants.len()
                    )));
                };
                text.push_str(&constant_text(value)?);
                constant += 1;
            }
            other => text.push(other),
        }
    }

    if dynamic != descriptors.len() {
        return Err(VmError::Native(format!(
            "{call_site_name} recipe consumes {dynamic} of {} dynamic argument(s)",
            descriptors.len()
        )));
    }
    if constant != constants.len() {
        return Err(VmError::Native(format!(
            "{call_site_name} recipe consumes {constant} of {} static constant(s)",
            constants.len()
        )));
    }
    Ok(text)
}

/// The text a non-string bootstrap constant contributes to a recipe.
///
/// A `String` is itself, a primitive is its Java text, and a `Class` contributes the
/// dotted binary name (this VM's `Class` objects have no JDK `Class.toString` shape; the
/// name is the useful static form). A class constant does not load or initialize the
/// class. A `MethodType` or `MethodHandle` argument cannot appear in a
/// `StringConcatFactory` recipe, so it is a typed error instead of a rendered value.
fn constant_text(argument: &BootstrapArgument) -> Result<String, VmError> {
    Ok(match argument {
        BootstrapArgument::String(text) => text.clone(),
        BootstrapArgument::Class(name) => name.replace('/', "."),
        BootstrapArgument::Integer(value) => value.to_string(),
        BootstrapArgument::Long(value) => value.to_string(),
        BootstrapArgument::Float(value) => float_text(*value),
        BootstrapArgument::Double(value) => double_text(*value),
        BootstrapArgument::MethodType(descriptor) => {
            return Err(VmError::Native(format!(
                "a StringConcatFactory recipe cannot use the MethodType constant {descriptor}"
            )));
        }
        BootstrapArgument::MethodHandle(handle) => {
            return Err(VmError::Native(format!(
                "a StringConcatFactory recipe cannot use the method handle {}.{}{}",
                handle.owner, handle.name, handle.descriptor
            )));
        }
    })
}

/// The text one dynamic argument contributes, according to its parameter descriptor.
fn argument_text(shared: &Shared, descriptor: &str, value: Value) -> Result<String, VmError> {
    match descriptor {
        "Z" => Ok(if int_value(value, descriptor)? == 0 {
            "false".to_owned()
        } else {
            "true".to_owned()
        }),
        "B" | "S" | "I" => Ok(int_value(value, descriptor)?.to_string()),
        "C" => {
            let unit = int_value(value, descriptor)?;
            let unit = u16::try_from(unit).map_err(|_| mismatch(descriptor))?;
            let character = char::from_u32(u32::from(unit)).ok_or(VmError::UnpairedSurrogate {
                method: "StringConcatFactory",
            })?;
            Ok(character.to_string())
        }
        "J" => match value {
            Value::Long(value) => Ok(value.to_string()),
            _ => Err(mismatch(descriptor)),
        },
        "F" => match value {
            Value::Float(value) => Ok(float_text(value)),
            _ => Err(mismatch(descriptor)),
        },
        "D" => match value {
            Value::Double(value) => Ok(double_text(value)),
            _ => Err(mismatch(descriptor)),
        },
        descriptor if descriptor.starts_with('L') || descriptor.starts_with('[') => match value {
            Value::Ref(reference) => object_text(shared, reference, "StringConcatFactory"),
            _ => Err(mismatch(descriptor)),
        },
        _ => Err(VmError::Native(format!(
            "unsupported StringConcatFactory parameter descriptor {descriptor}"
        ))),
    }
}

fn int_value(value: Value, descriptor: &str) -> Result<i32, VmError> {
    match value {
        Value::Int(value) => Ok(value),
        _ => Err(mismatch(descriptor)),
    }
}

fn bad_descriptor(descriptor: &str) -> VmError {
    VmError::Native(format!("malformed descriptor {descriptor}"))
}

fn mismatch(descriptor: &str) -> VmError {
    VmError::Native(format!(
        "StringConcatFactory argument does not match descriptor {descriptor}"
    ))
}

/// The descriptor's parameter field descriptors, in order.
fn parameter_descriptors(descriptor: &str) -> Result<Vec<&str>, VmError> {
    let (arguments, _) = split_descriptor(descriptor)?;
    let bytes = arguments.as_bytes();
    let mut descriptors = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let end = field_descriptor_end(bytes, start)?;
        descriptors.push(&arguments[start..end]);
        start = end;
    }
    Ok(descriptors)
}

/// The descriptor's return field descriptor.
fn return_descriptor(descriptor: &str) -> Result<&str, VmError> {
    let (_, result) = split_descriptor(descriptor)?;
    let end = field_descriptor_end(result.as_bytes(), 0)?;
    if end != result.len() {
        return Err(bad_descriptor(descriptor));
    }
    Ok(result)
}

fn split_descriptor(descriptor: &str) -> Result<(&str, &str), VmError> {
    let Some(rest) = descriptor.strip_prefix('(') else {
        return Err(bad_descriptor(descriptor));
    };
    let Some((arguments, result)) = rest.split_once(')') else {
        return Err(bad_descriptor(descriptor));
    };
    Ok((arguments, result))
}

/// The exclusive end of one field descriptor starting at `start`.
fn field_descriptor_end(bytes: &[u8], start: usize) -> Result<usize, VmError> {
    let mut position = start;
    while bytes.get(position) == Some(&b'[') {
        position += 1;
    }
    match bytes.get(position) {
        Some(b'Z' | b'B' | b'C' | b'S' | b'I' | b'J' | b'F' | b'D') => Ok(position + 1),
        Some(b'L') => {
            let mut cursor = position + 1;
            while let Some(byte) = bytes.get(cursor) {
                if *byte == b';' {
                    return Ok(cursor + 1);
                }
                cursor += 1;
            }
            Err(bad_descriptor(&String::from_utf8_lossy(bytes)))
        }
        _ => Err(bad_descriptor(&String::from_utf8_lossy(bytes))),
    }
}
