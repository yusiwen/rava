//! Lazy constant-pool resolution.
//!
//! A `Fieldref` or `Methodref` is a pair of indices when the class file loads; it becomes
//! a name, a descriptor and a class only when the instruction runs. Keeping resolution
//! lazy is why `NoClassDefFoundError` can happen mid-method, and it is why a class can be
//! loaded without every class it mentions existing.

use rava_classfile::Constant;

use crate::class::Class;
use crate::error::VmError;

#[derive(Debug, Clone)]
pub struct FieldRef {
    pub class_name: String,
    pub name: String,
    pub descriptor: String,
}

#[derive(Debug, Clone)]
pub struct MethodRef {
    pub class_name: String,
    pub name: String,
    pub descriptor: String,
}

fn constants(class: &Class) -> Result<&rava_classfile::ClassFile, VmError> {
    class.file.as_ref().ok_or_else(|| {
        VmError::BadConstantPool(format!("{} is synthetic and has no constants", class.name))
    })
}

pub fn field_ref(class: &Class, index: u16) -> Result<FieldRef, VmError> {
    let pool = &constants(class)?.constant_pool;
    let (class_index, name_and_type_index) = match pool.get(index)? {
        Constant::FieldRef {
            class_index,
            name_and_type_index,
        } => (*class_index, *name_and_type_index),
        other => {
            return Err(VmError::BadConstantPool(format!(
                "entry {index} is {other:?}, expected a Fieldref"
            )));
        }
    };
    let class_name = pool.class_name(class_index)?.to_owned();
    let (name, descriptor) = pool.name_and_type(name_and_type_index)?;
    Ok(FieldRef {
        class_name,
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
    })
}

pub fn method_ref(class: &Class, index: u16) -> Result<MethodRef, VmError> {
    let pool = &constants(class)?.constant_pool;
    let (class_index, name_and_type_index) = match pool.get(index)? {
        Constant::MethodRef {
            class_index,
            name_and_type_index,
        }
        | Constant::InterfaceMethodRef {
            class_index,
            name_and_type_index,
        } => (*class_index, *name_and_type_index),
        other => {
            return Err(VmError::BadConstantPool(format!(
                "entry {index} is {other:?}, expected a Methodref or InterfaceMethodref"
            )));
        }
    };
    let class_name = pool.class_name(class_index)?.to_owned();
    let (name, descriptor) = pool.name_and_type(name_and_type_index)?;
    Ok(MethodRef {
        class_name,
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
    })
}

/// What `ldc` and `ldc2_w` can push.
#[derive(Debug, Clone)]
pub enum ConstantValue {
    Int(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    String(String),
}

pub fn constant(class: &Class, index: u16) -> Result<ConstantValue, VmError> {
    let pool = &constants(class)?.constant_pool;
    Ok(match pool.get(index)? {
        Constant::Integer(value) => ConstantValue::Int(*value),
        Constant::Float(value) => ConstantValue::Float(*value),
        Constant::Long(value) => ConstantValue::Long(*value),
        Constant::Double(value) => ConstantValue::Double(*value),
        Constant::String { string_index } => {
            ConstantValue::String(pool.utf8(*string_index)?.to_owned())
        }
        Constant::Utf8(value) => ConstantValue::String(value.clone()),
        Constant::Dynamic { .. } => {
            return Err(VmError::Unsupported {
                feature: "condy (CONSTANT_Dynamic)",
                detail: format!(
                    "entry {index} needs a bootstrap method; the invokedynamic milestone (M3) implements them"
                ),
            });
        }
        other => {
            return Err(VmError::BadConstantPool(format!(
                "entry {index} is {other:?}, which ldc cannot push"
            )));
        }
    })
}

/// Parses a method descriptor and returns how many values the caller must push or pop.
///
/// Our operand stack holds one [`crate::Value`] per parameter even for `long` and
/// `double`, so this counts values, not JVM slots.
pub fn parameter_count(descriptor: &str) -> usize {
    let bytes = descriptor.as_bytes();
    let mut index = 1; // skip '('
    let mut count = 0;
    while index < bytes.len() && bytes[index] != b')' {
        while index < bytes.len() && bytes[index] == b'[' {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        if bytes[index] == b'L' {
            while index < bytes.len() && bytes[index] != b';' {
                index += 1;
            }
            index += 1;
        } else {
            index += 1;
        }
        count += 1;
    }
    count
}

/// The operand-stack slots a value of this type descriptor occupies (JVMS §2.6.2):
/// `long` and `double` take two, `void` takes none, everything else one.
pub fn type_slots(descriptor: &str) -> usize {
    match descriptor {
        "J" | "D" => 2,
        "V" => 0,
        _ => 1,
    }
}

/// The operand-stack slots the parameters of `descriptor` occupy.
///
/// This is [`parameter_count`] counted in slots, which is what `max_stack` is measured in.
/// One descriptor per parameter, in order (`(ILjava/lang/String;)[I` → `["I", "Ljava/lang/String;"]`).
pub fn parameter_descriptors(descriptor: &str) -> Vec<String> {
    let bytes = descriptor.as_bytes();
    let mut index = 1;
    let mut parameters = Vec::new();
    while index < bytes.len() && bytes[index] != b')' {
        let start = index;
        while index < bytes.len() && bytes[index] == b'[' {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }
        if bytes[index] == b'L' {
            while index < bytes.len() && bytes[index] != b';' {
                index += 1;
            }
            index += 1;
        } else {
            index += 1;
        }
        parameters.push(descriptor[start..index].to_owned());
    }
    parameters
}

pub fn parameter_slots(descriptor: &str) -> usize {
    let bytes = descriptor.as_bytes();
    let mut index = 1; // skip '('
    let mut slots = 0;
    while index < bytes.len() && bytes[index] != b')' {
        if bytes[index] == b'[' {
            while index < bytes.len() && bytes[index] == b'[' {
                index += 1;
            }
            // An array is a reference: one slot, whatever its element type.
            if index < bytes.len() && bytes[index] == b'L' {
                while index < bytes.len() && bytes[index] != b';' {
                    index += 1;
                }
            }
            index += 1;
            slots += 1;
        } else if bytes[index] == b'L' {
            while index < bytes.len() && bytes[index] != b';' {
                index += 1;
            }
            index += 1;
            slots += 1;
        } else {
            slots += if matches!(bytes[index], b'J' | b'D') {
                2
            } else {
                1
            };
            index += 1;
        }
    }
    slots
}

/// The return value's slots: zero for `void`, two for `long`/`double`.
pub fn return_slots(descriptor: &str) -> usize {
    match descriptor.split_once(')') {
        Some((_, returns)) => type_slots(returns),
        None => 0,
    }
}

/// The binary name of a `CONSTANT_Class` entry, used by `new`, `checkcast` and friends.
/// The descriptor of a `CONSTANT_InvokeDynamic` entry's name-and-type.
pub fn indy_descriptor(class: &Class, index: u16) -> Result<String, VmError> {
    let pool = &constants(class)?.constant_pool;
    let name_and_type = match pool.get(index)? {
        Constant::InvokeDynamic {
            name_and_type_index,
            ..
        } => *name_and_type_index,
        other => {
            return Err(VmError::BadConstantPool(format!(
                "entry {index} is {other:?}, expected an InvokeDynamic"
            )));
        }
    };
    let (_, descriptor) = pool.name_and_type(name_and_type)?;
    Ok(descriptor.to_owned())
}

pub fn class_constant(class: &Class, index: u16) -> Result<String, VmError> {
    let pool = &constants(class)?.constant_pool;
    match pool.get(index)? {
        Constant::Class { .. } => Ok(pool.class_name(index)?.to_owned()),
        other => Err(VmError::BadConstantPool(format!(
            "entry {index} is {other:?}, expected a Class constant"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_parameter_slots() {
        assert_eq!(parameter_slots("()V"), 0);
        assert_eq!(parameter_slots("(I)V"), 1);
        assert_eq!(parameter_slots("(IJ)V"), 3);
        assert_eq!(parameter_slots("(JD)V"), 4);
        assert_eq!(parameter_slots("(Ljava/lang/String;)V"), 1);
        assert_eq!(
            parameter_slots("([J)V"),
            1,
            "an array of longs is a reference"
        );
        assert_eq!(parameter_slots("([[I[Ljava/lang/Object;JZ)V"), 5);
        assert_eq!(type_slots("J"), 2);
        assert_eq!(type_slots("V"), 0);
        assert_eq!(type_slots("[D"), 1);
        assert_eq!(return_slots("(I)J"), 2);
        assert_eq!(return_slots("(I)V"), 0);
        assert_eq!(return_slots("(I)Ljava/lang/String;"), 1);
    }

    #[test]
    fn counts_parameters() {
        assert_eq!(parameter_count("()V"), 0);
        assert_eq!(parameter_count("(I)V"), 1);
        assert_eq!(parameter_count("(Ljava/lang/String;)V"), 1);
        assert_eq!(parameter_count("(IJ)Ljava/lang/String;"), 2);
        assert_eq!(parameter_count("([Ljava/lang/String;)V"), 1);
        assert_eq!(parameter_count("([[I[Ljava/lang/Object;)V"), 2);
        assert_eq!(parameter_count("(ILjava/lang/String;JZ)V"), 4);
    }
}
