//! `CONSTANT_*` entries (JVMS §4.4).
//!
//! Every tag up to class-file version 65 is understood, including the ones this VM
//! cannot execute yet (`CONSTANT_Dynamic`, `CONSTANT_Module`, `CONSTANT_Package`).
//! Parsing them now keeps the enum complete, so a later version does not force the
//! `match` arms to be revisited (ADR-0001, R2).

use crate::error::Error;
use crate::mutf8;
use crate::reader::Reader;

pub const TAG_UTF8: u8 = 1;
pub const TAG_INTEGER: u8 = 3;
pub const TAG_FLOAT: u8 = 4;
pub const TAG_LONG: u8 = 5;
pub const TAG_DOUBLE: u8 = 6;
pub const TAG_CLASS: u8 = 7;
pub const TAG_STRING: u8 = 8;
pub const TAG_FIELDREF: u8 = 9;
pub const TAG_METHODREF: u8 = 10;
pub const TAG_INTERFACE_METHODREF: u8 = 11;
pub const TAG_NAME_AND_TYPE: u8 = 12;
pub const TAG_METHOD_HANDLE: u8 = 15;
pub const TAG_METHOD_TYPE: u8 = 16;
pub const TAG_DYNAMIC: u8 = 17;
pub const TAG_INVOKE_DYNAMIC: u8 = 18;
pub const TAG_MODULE: u8 = 19;
pub const TAG_PACKAGE: u8 = 20;

/// The reference kind byte of a `CONSTANT_MethodHandle` (JVMS §4.4.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    GetField,
    GetStatic,
    PutField,
    PutStatic,
    InvokeVirtual,
    InvokeStatic,
    InvokeSpecial,
    NewInvokeSpecial,
    InvokeInterface,
    Unknown(u8),
}

impl ReferenceKind {
    pub fn from_byte(value: u8) -> Self {
        match value {
            1 => Self::GetField,
            2 => Self::GetStatic,
            3 => Self::PutField,
            4 => Self::PutStatic,
            5 => Self::InvokeVirtual,
            6 => Self::InvokeStatic,
            7 => Self::InvokeSpecial,
            8 => Self::NewInvokeSpecial,
            9 => Self::InvokeInterface,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Constant {
    Utf8(String),
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    Class {
        name_index: u16,
    },
    String {
        string_index: u16,
    },
    FieldRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    MethodRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    InterfaceMethodRef {
        class_index: u16,
        name_and_type_index: u16,
    },
    NameAndType {
        name_index: u16,
        descriptor_index: u16,
    },
    MethodHandle {
        reference_kind: ReferenceKind,
        reference_index: u16,
    },
    MethodType {
        descriptor_index: u16,
    },
    /// `CONSTANT_Dynamic` (tag 17, class file ≥ 55). Parsed, not resolved.
    Dynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    /// `CONSTANT_InvokeDynamic` (tag 18, class file ≥ 51). Parsed, not resolved.
    InvokeDynamic {
        bootstrap_method_attr_index: u16,
        name_and_type_index: u16,
    },
    Module {
        name_index: u16,
    },
    Package {
        name_index: u16,
    },
}

/// The constant pool, indexed exactly as the class file indexes it.
///
/// Slot 0 is unusable, and a `long` or `double` occupies two slots whose second half
/// is also unusable (JVMS §4.4.5). Both are represented as `None`, so indices are never
/// shifted — a class file's index works verbatim, which is what the bytecode expects.
#[derive(Debug, Clone)]
pub struct ConstantPool {
    slots: Vec<Option<Constant>>,
}

impl ConstantPool {
    pub(crate) fn parse(r: &mut Reader<'_>) -> Result<Self, Error> {
        let declared = r.u2()?;
        let mut slots: Vec<Option<Constant>> = Vec::with_capacity(usize::from(declared));
        slots.push(None);

        let mut index: u16 = 1;
        while index < declared {
            let tag = r.u1()?;
            let (entry, wide) = read_entry(r, tag, index)?;
            slots.push(Some(entry));
            if wide {
                slots.push(None);
                index += 2;
            } else {
                index += 1;
            }
        }

        if slots.len() != usize::from(declared) {
            return Err(Error::PoolSizeMismatch {
                declared,
                parsed: slots.len(),
            });
        }
        Ok(Self { slots })
    }

    /// Number of slots, including the unusable ones.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.len() <= 1
    }

    pub fn get(&self, index: u16) -> Result<&Constant, Error> {
        self.slots
            .get(usize::from(index))
            .and_then(Option::as_ref)
            .ok_or(Error::BadConstantPoolIndex {
                index,
                pool_len: self.slots.len() as u16,
            })
    }

    pub fn utf8(&self, index: u16) -> Result<&str, Error> {
        match self.get(index)? {
            Constant::Utf8(value) => Ok(value),
            _ => Err(Error::WrongConstantKind {
                index,
                expected: "Utf8",
            }),
        }
    }

    /// Resolves a `CONSTANT_Class` entry to its binary name (`java/lang/Object`).
    pub fn class_name(&self, index: u16) -> Result<&str, Error> {
        match self.get(index)? {
            Constant::Class { name_index } => self.utf8(*name_index),
            _ => Err(Error::WrongConstantKind {
                index,
                expected: "Class",
            }),
        }
    }

    /// Resolves a `CONSTANT_Module` entry to its module name (JVMS §4.4.11).
    pub fn module_name(&self, index: u16) -> Result<&str, Error> {
        match self.get(index)? {
            Constant::Module { name_index } => self.utf8(*name_index),
            _ => Err(Error::WrongConstantKind {
                index,
                expected: "Module",
            }),
        }
    }

    /// Resolves a `CONSTANT_Package` entry to its internal package name (JVMS §4.4.12).
    pub fn package_name(&self, index: u16) -> Result<&str, Error> {
        match self.get(index)? {
            Constant::Package { name_index } => self.utf8(*name_index),
            _ => Err(Error::WrongConstantKind {
                index,
                expected: "Package",
            }),
        }
    }

    /// Resolves a `(name, descriptor)` pair from a `NameAndType`, method or field ref.
    pub fn name_and_type(&self, index: u16) -> Result<(&str, &str), Error> {
        match self.get(index)? {
            Constant::NameAndType {
                name_index,
                descriptor_index,
            } => Ok((self.utf8(*name_index)?, self.utf8(*descriptor_index)?)),
            _ => Err(Error::WrongConstantKind {
                index,
                expected: "NameAndType",
            }),
        }
    }

    /// Iterates over the usable entries, yielding `(index, entry)`.
    pub fn iter(&self) -> impl Iterator<Item = (u16, &Constant)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|entry| (index as u16, entry)))
    }
}

fn read_entry(r: &mut Reader<'_>, tag: u8, index: u16) -> Result<(Constant, bool), Error> {
    let entry = match tag {
        TAG_UTF8 => {
            let length = usize::from(r.u2()?);
            let bytes = r.bytes(length)?;
            Constant::Utf8(mutf8::decode(bytes, index)?)
        }
        TAG_INTEGER => Constant::Integer(r.u4()? as i32),
        TAG_FLOAT => Constant::Float(f32::from_bits(r.u4()?)),
        TAG_LONG => return Ok((Constant::Long(r.u8()? as i64), true)),
        TAG_DOUBLE => return Ok((Constant::Double(f64::from_bits(r.u8()?)), true)),
        TAG_CLASS => Constant::Class {
            name_index: r.u2()?,
        },
        TAG_STRING => Constant::String {
            string_index: r.u2()?,
        },
        TAG_FIELDREF => Constant::FieldRef {
            class_index: r.u2()?,
            name_and_type_index: r.u2()?,
        },
        TAG_METHODREF => Constant::MethodRef {
            class_index: r.u2()?,
            name_and_type_index: r.u2()?,
        },
        TAG_INTERFACE_METHODREF => Constant::InterfaceMethodRef {
            class_index: r.u2()?,
            name_and_type_index: r.u2()?,
        },
        TAG_NAME_AND_TYPE => Constant::NameAndType {
            name_index: r.u2()?,
            descriptor_index: r.u2()?,
        },
        TAG_METHOD_HANDLE => Constant::MethodHandle {
            reference_kind: ReferenceKind::from_byte(r.u1()?),
            reference_index: r.u2()?,
        },
        TAG_METHOD_TYPE => Constant::MethodType {
            descriptor_index: r.u2()?,
        },
        TAG_DYNAMIC => Constant::Dynamic {
            bootstrap_method_attr_index: r.u2()?,
            name_and_type_index: r.u2()?,
        },
        TAG_INVOKE_DYNAMIC => Constant::InvokeDynamic {
            bootstrap_method_attr_index: r.u2()?,
            name_and_type_index: r.u2()?,
        },
        TAG_MODULE => Constant::Module {
            name_index: r.u2()?,
        },
        TAG_PACKAGE => Constant::Package {
            name_index: r.u2()?,
        },
        other => {
            return Err(Error::Unsupported {
                feature: "constant pool tag",
                detail: format!("tag {other} at entry {index}"),
            });
        }
    };
    Ok((entry, false))
}
