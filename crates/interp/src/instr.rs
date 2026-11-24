//! The decoded instruction form.
//!
//! Decoding happens once per method; the dispatch loop never reads raw bytes. The
//! subset here is the M0.5a integer core, M2.5's instance instructions and M2.6's
//! arrays and integer control flow: primitive and reference array creation, the array
//! length/load/store forms, `goto`, the `if<cond>`/`if_icmp<cond>`/`ifnull`/`ifnonnull`
//! branches and `iinc`.
//!
//! A branch carries the index of its target in [`DecodedMethod::instructions`], resolved
//! here from the class file's signed byte offset. The dispatch loop therefore never
//! computes a byte address, and a target that is not an instruction boundary is a decode
//! error rather than a runtime jump into the middle of an operand.

use std::collections::HashMap;

use rava_classfile::{ClassFile, Constant};
use rava_vm::ArrayKind;

use crate::error::InterpError;

/// The six integer comparisons shared by `if<cond>` and `if_icmp<cond>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntCondition {
    Eq,
    Ne,
    Lt,
    Ge,
    Gt,
    Le,
}

impl IntCondition {
    /// `left <op> right`, the comparison the corresponding opcode performs.
    pub fn satisfied(self, left: i32, right: i32) -> bool {
        match self {
            Self::Eq => left == right,
            Self::Ne => left != right,
            Self::Lt => left < right,
            Self::Ge => left >= right,
            Self::Gt => left > right,
            Self::Le => left <= right,
        }
    }
}

/// The conditions in JVMS opcode order: `if<cond>` is `0x99..=0x9e` and
/// `if_icmp<cond>` is `0x9f..=0xa4`, both starting with `eq` and ending with `le`.
const CONDITIONS: [IntCondition; 6] = [
    IntCondition::Eq,
    IntCondition::Ne,
    IntCondition::Lt,
    IntCondition::Ge,
    IntCondition::Gt,
    IntCondition::Le,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instr {
    Iconst(i32),
    ILoad(u8),
    ALoad(u8),
    IStore(u8),
    AStore(u8),
    Iinc(u8, i8),
    AconstNull,
    New {
        class: String,
    },
    Dup,
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
    GetField {
        class: String,
        name: String,
        descriptor: String,
    },
    PutField {
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
    InvokeSpecial {
        class: String,
        name: String,
        descriptor: String,
    },
    /// `newarray`: the primitive element kind the `atype` operand names.
    NewArray(ArrayKind),
    /// `anewarray`: the component class name or array descriptor from the class constant.
    ANewArray {
        component: String,
    },
    /// `multianewarray`: the full array descriptor and the number of dimensions allocated.
    MultiANewArray {
        component: String,
        dimensions: u8,
    },
    ArrayLength,
    /// `iaload`/`aaload`/...: the element kind the opcode addresses.
    ArrayLoad(ArrayKind),
    /// `iastore`/`aastore`/...: the element kind the opcode addresses.
    ArrayStore(ArrayKind),
    /// Jump to an instruction index; `0xa7`.
    Goto(usize),
    /// `ifeq`/`ifne`/...: compare the popped value with zero, then jump to an index.
    If(IntCondition, usize),
    /// `if_icmpeq`/...: compare two popped ints, then jump to an index.
    IfICmp(IntCondition, usize),
    IfNull(usize),
    IfNonNull(usize),
    IReturn,
    AReturn,
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
    // The byte offset of each decoded instruction, used to turn a branch's signed byte
    // offset into the instruction index the dispatch loop jumps to.
    let mut starts = Vec::new();
    // `(instruction index, target byte offset)` for every branch decoded this pass.
    let mut fixups: Vec<(usize, usize)> = Vec::new();
    let mut pc = 0usize;
    while pc < code.len() {
        let opcode_pc = pc;
        let opcode = code[pc];
        pc += 1;
        let instr = match opcode {
            0x01 => Instr::AconstNull,
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
            0x19 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::ALoad(index)
            }
            0x1a..=0x1d => Instr::ILoad(opcode - 0x1a),
            0x2a..=0x2d => Instr::ALoad(opcode - 0x2a),
            0x36 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::IStore(index)
            }
            0x3a => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::AStore(index)
            }
            0x3b..=0x3e => Instr::IStore(opcode - 0x3b),
            0x4b..=0x4e => Instr::AStore(opcode - 0x4b),
            0x59 => Instr::Dup,
            0x60 => Instr::IAdd,
            0x64 => Instr::ISub,
            0x68 => Instr::IMul,
            0x6c => Instr::IDiv,
            0x84 => {
                let index = byte(code, pc)?;
                let increment = i8::from_be_bytes([byte(code, pc + 1)?]);
                pc += 2;
                Instr::Iinc(index, increment)
            }
            0x2e..=0x35 => Instr::ArrayLoad(array_element(opcode)),
            0x4f..=0x56 => Instr::ArrayStore(array_element(opcode)),
            0xbe => Instr::ArrayLength,
            0xbc => {
                let atype = byte(code, pc)?;
                pc += 1;
                Instr::NewArray(primitive_array_kind(atype)?)
            }
            0xbd => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                Instr::ANewArray {
                    component: resolve_class(file, index)?,
                }
            }
            0xc5 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let dimensions = byte(code, pc)?;
                pc += 1;
                let component = resolve_class(file, index)?;
                if dimensions == 0 {
                    return Err(InterpError::Decode(
                        "multianewarray has zero dimensions".to_owned(),
                    ));
                }
                if !component.starts_with('[') {
                    return Err(InterpError::Decode(format!(
                        "multianewarray component {component} is not an array type"
                    )));
                }
                Instr::MultiANewArray {
                    component,
                    dimensions,
                }
            }
            0x99..=0x9e => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push((instructions.len(), target_offset(opcode_pc, offset)?));
                Instr::If(CONDITIONS[usize::from(opcode - 0x99)], 0)
            }
            0x9f..=0xa4 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push((instructions.len(), target_offset(opcode_pc, offset)?));
                Instr::IfICmp(CONDITIONS[usize::from(opcode - 0x9f)], 0)
            }
            0xa7 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push((instructions.len(), target_offset(opcode_pc, offset)?));
                Instr::Goto(0)
            }
            0xc6 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push((instructions.len(), target_offset(opcode_pc, offset)?));
                Instr::IfNull(0)
            }
            0xc7 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push((instructions.len(), target_offset(opcode_pc, offset)?));
                Instr::IfNonNull(0)
            }
            0xac => Instr::IReturn,
            0xb0 => Instr::AReturn,
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
            0xb4 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let (class, name, descriptor) = resolve_field(file, index)?;
                Instr::GetField {
                    class,
                    name,
                    descriptor,
                }
            }
            0xb5 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let (class, name, descriptor) = resolve_field(file, index)?;
                Instr::PutField {
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
            0xb7 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let (class, name, descriptor) = resolve_method(file, index, false)?;
                Instr::InvokeSpecial {
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
            0xbb => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                Instr::New {
                    class: resolve_class(file, index)?,
                }
            }
            other => {
                return Err(InterpError::Decode(format!(
                    "unsupported opcode 0x{other:02x}"
                )));
            }
        };
        starts.push(opcode_pc);
        instructions.push(instr);
    }

    // Resolve every branch's byte target to the index of the instruction that starts
    // there. The map is built after the pass because a forward branch names an offset
    // that has not been decoded yet when the branch itself is read.
    let mut boundaries = HashMap::with_capacity(starts.len());
    for (index, start) in starts.iter().enumerate() {
        boundaries.insert(*start, index);
    }
    for (index, target) in fixups {
        let target_index = boundaries.get(&target).copied().ok_or_else(|| {
            InterpError::Decode(format!(
                "branch target byte {target} is not an instruction boundary"
            ))
        })?;
        instructions[index] = match &instructions[index] {
            Instr::Goto(_) => Instr::Goto(target_index),
            Instr::If(condition, _) => Instr::If(*condition, target_index),
            Instr::IfICmp(condition, _) => Instr::IfICmp(*condition, target_index),
            Instr::IfNull(_) => Instr::IfNull(target_index),
            Instr::IfNonNull(_) => Instr::IfNonNull(target_index),
            _ => {
                return Err(InterpError::Decode(
                    "branch fixup without a branch instruction".to_owned(),
                ));
            }
        };
    }

    Ok(DecodedMethod {
        instructions,
        max_stack,
        max_locals,
    })
}

/// The element kind an array load/store opcode addresses.
///
/// `baload`/`bastore` are reported as [`ArrayKind::Byte`]; the executor accepts either a
/// `byte[]` or a `boolean[]` for those two opcodes.
fn array_element(opcode: u8) -> ArrayKind {
    match opcode {
        0x2e | 0x4f => ArrayKind::Int,
        0x2f | 0x50 => ArrayKind::Long,
        0x30 | 0x51 => ArrayKind::Float,
        0x31 | 0x52 => ArrayKind::Double,
        0x32 | 0x53 => ArrayKind::Reference,
        0x33 | 0x54 => ArrayKind::Byte,
        0x34 | 0x55 => ArrayKind::Char,
        0x35 | 0x56 => ArrayKind::Short,
        _ => unreachable!("only array load/store opcodes reach array_element"),
    }
}

/// `newarray`'s `atype` operand (JVMS §6.5.newarray) as an [`ArrayKind`].
fn primitive_array_kind(atype: u8) -> Result<ArrayKind, InterpError> {
    Ok(match atype {
        4 => ArrayKind::Boolean,
        5 => ArrayKind::Char,
        6 => ArrayKind::Float,
        7 => ArrayKind::Double,
        8 => ArrayKind::Byte,
        9 => ArrayKind::Short,
        10 => ArrayKind::Int,
        11 => ArrayKind::Long,
        other => {
            return Err(InterpError::Decode(format!(
                "newarray atype {other} is not a primitive array type"
            )));
        }
    })
}

/// A signed branch offset, read from the two bytes that follow the opcode.
fn branch_offset(code: &[u8], offset: usize) -> Result<i16, InterpError> {
    Ok(i16::from_be_bytes([
        byte(code, offset)?,
        byte(code, offset + 1)?,
    ]))
}

/// The absolute byte offset a branch at `opcode_pc` targets (JVMS §6.5: relative to the
/// opcode's own address).
fn target_offset(opcode_pc: usize, offset: i16) -> Result<usize, InterpError> {
    let opcode_pc = isize::try_from(opcode_pc)
        .map_err(|_| InterpError::Decode("code offset does not fit an isize".to_owned()))?;
    let target = opcode_pc + isize::from(offset);
    usize::try_from(target).map_err(|_| {
        InterpError::Decode(format!(
            "branch at byte {opcode_pc} targets {target}, before the code start"
        ))
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

fn resolve_class(file: &ClassFile, index: u16) -> Result<String, InterpError> {
    let Constant::Class { name_index } = file.constant_pool.get(index)? else {
        return Err(InterpError::Decode(format!(
            "constant {index} is not a Class"
        )));
    };
    Ok(file.constant_pool.utf8(*name_index)?.to_owned())
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
