//! The decoded instruction form.
//!
//! Decoding happens once per method; the dispatch loop never reads raw bytes. The
//! subset here is the M0.5a slice: integer constants, loads/stores, arithmetic,
//! `invokestatic` and the two returns.

use rava_classfile::{ClassFile, Constant};

use crate::error::InterpError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instr {
    Iconst(i32),
    ILoad(u8),
    IStore(u8),
    IAdd,
    ISub,
    IMul,
    IDiv,
    LdcString(String),
    GetStatic {
        class: String,
        name: String,
        descriptor: String,
    },
    InvokeStatic {
        class: String,
        name: String,
        descriptor: String,
    },
    InvokeVirtual {
        class: String,
        name: String,
        descriptor: String,
    },
    IReturn,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedMethod {
    pub instructions: Vec<Instr>,
    pub max_stack: u16,
    pub max_locals: u16,
}

pub fn decode(file: &ClassFile, attribute_data: &[u8]) -> Result<DecodedMethod, InterpError> {
    if attribute_data.len() < 8 {
        return Err(InterpError::Decode(
            "Code attribute is shorter than its header".to_owned(),
        ));
    }
    let max_stack = u16::from_be_bytes([attribute_data[0], attribute_data[1]]);
    let max_locals = u16::from_be_bytes([attribute_data[2], attribute_data[3]]);
    let length = u32::from_be_bytes([
        attribute_data[4],
        attribute_data[5],
        attribute_data[6],
        attribute_data[7],
    ]) as usize;
    let end = 8usize
        .checked_add(length)
        .ok_or_else(|| InterpError::Decode("Code length overflows".to_owned()))?;
    let code = attribute_data
        .get(8..end)
        .ok_or_else(|| InterpError::Decode("Code payload is truncated".to_owned()))?;

    let mut instructions = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let opcode = code[pc];
        pc += 1;
        let instr = match opcode {
            0x02..=0x08 => Instr::Iconst(i32::from(opcode) - 3),
            0x10 => {
                let value = i8::from_be_bytes([byte(code, pc)?]);
                pc += 1;
                Instr::Iconst(i32::from(value))
            }
            0x11 => {
                let value = i16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                Instr::Iconst(i32::from(value))
            }
            0x15 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::ILoad(index)
            }
            0x1a..=0x1d => Instr::ILoad(opcode - 0x1a),
            0x36 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::IStore(index)
            }
            0x3b..=0x3e => Instr::IStore(opcode - 0x3b),
            0x60 => Instr::IAdd,
            0x64 => Instr::ISub,
            0x68 => Instr::IMul,
            0x6c => Instr::IDiv,
            0xac => Instr::IReturn,
            0xb1 => Instr::Return,
            0x12 => {
                let index = byte(code, pc)?;
                pc += 1;
                resolve_ldc(file, u16::from(index))?
            }
            0xb2 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let (class, name, descriptor) = resolve_field(file, index)?;
                Instr::GetStatic {
                    class,
                    name,
                    descriptor,
                }
            }
            0xb6 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let (class, name, descriptor) = resolve_method(file, index, true)?;
                Instr::InvokeVirtual {
                    class,
                    name,
                    descriptor,
                }
            }
            0xb8 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let (class, name, descriptor) = resolve_method(file, index, false)?;
                Instr::InvokeStatic {
                    class,
                    name,
                    descriptor,
                }
            }
            other => {
                return Err(InterpError::Decode(format!(
                    "unsupported opcode 0x{other:02x}"
                )));
            }
        };
        instructions.push(instr);
    }

    Ok(DecodedMethod {
        instructions,
        max_stack,
        max_locals,
    })
}

fn resolve_ldc(file: &ClassFile, index: u16) -> Result<Instr, InterpError> {
    let Constant::String { string_index } = file.constant_pool.get(index)? else {
        return Err(InterpError::Decode(format!(
            "constant {index} is not a String"
        )));
    };
    Ok(Instr::LdcString(
        file.constant_pool.utf8(*string_index)?.to_owned(),
    ))
}

fn resolve_field(file: &ClassFile, index: u16) -> Result<(String, String, String), InterpError> {
    let Constant::FieldRef {
        class_index,
        name_and_type_index,
    } = file.constant_pool.get(index)?
    else {
        return Err(InterpError::Decode(format!(
            "constant {index} is not a Fieldref"
        )));
    };
    let class = file.constant_pool.class_name(*class_index)?.to_owned();
    let (name, descriptor) = file.constant_pool.name_and_type(*name_and_type_index)?;
    Ok((class, name.to_owned(), descriptor.to_owned()))
}

fn resolve_method(
    file: &ClassFile,
    index: u16,
    virtual_call: bool,
) -> Result<(String, String, String), InterpError> {
    let constant = file.constant_pool.get(index)?;
    let (class_index, name_and_type_index) = match constant {
        Constant::MethodRef {
            class_index,
            name_and_type_index,
        } => (*class_index, *name_and_type_index),
        Constant::InterfaceMethodRef {
            class_index,
            name_and_type_index,
        } if virtual_call => (*class_index, *name_and_type_index),
        _ => {
            return Err(InterpError::Decode(format!(
                "constant {index} is not a method reference"
            )));
        }
    };
    let class = file.constant_pool.class_name(class_index)?.to_owned();
    let (name, descriptor) = file.constant_pool.name_and_type(name_and_type_index)?;
    Ok((class, name.to_owned(), descriptor.to_owned()))
}

fn byte(code: &[u8], offset: usize) -> Result<u8, InterpError> {
    code.get(offset)
        .copied()
        .ok_or_else(|| InterpError::Decode("truncated operand".to_owned()))
}
