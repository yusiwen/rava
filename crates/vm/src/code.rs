//! Bytecode decoding (ADR-0003).
//!
//! A method body is decoded once, at load time, into [`Instr`] values with their operands
//! already read, plus a pc map back to byte offsets. Branch and switch targets are
//! translated from byte offsets to instruction indices in a second pass, so the dispatch
//! loop never computes an address.
//!
//! The enum grows slice by slice. An opcode outside what is implemented is
//! [`VmError::UnsupportedInstruction`] carrying the opcode and pc, which is an actionable
//! message rather than a misparse.

use rava_classfile::reader::Reader;

use crate::error::VmError;
use crate::heap::ArrayKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    IAdd,
    LAdd,
    FAdd,
    DAdd,
    ISub,
    LSub,
    FSub,
    DSub,
    IMul,
    LMul,
    FMul,
    DMul,
    IDiv,
    LDiv,
    FDiv,
    DDiv,
    IRem,
    LRem,
    FRem,
    DRem,
    IShl,
    LShl,
    IShr,
    LShr,
    IUShr,
    LUShr,
    IAnd,
    LAnd,
    IOr,
    LOr,
    IXor,
    LXor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    INeg,
    LNeg,
    FNeg,
    DNeg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Convert {
    I2L,
    I2F,
    I2D,
    L2I,
    L2F,
    L2D,
    F2I,
    F2L,
    F2D,
    D2I,
    D2L,
    D2F,
    I2B,
    I2C,
    I2S,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    LCmp,
    FCmpL,
    FCmpG,
    DCmpL,
    DCmpG,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Instr {
    Nop,
    AconstNull,
    Iconst(i32),
    Lconst(i64),
    Fconst(f32),
    Dconst(f64),
    Ldc(u16),
    /// `ldc2_w`: the entry is a `long` or a `double`.
    Ldc2(u16),
    Bipush(i8),
    Sipush(i16),
    Iload(u16),
    Lload(u16),
    Fload(u16),
    Dload(u16),
    Aload(u16),
    Istore(u16),
    Lstore(u16),
    Fstore(u16),
    Dstore(u16),
    Astore(u16),
    Pop,
    /// Pops two slots, which is two values unless the top one is a `long`/`double`.
    Pop2,
    Dup,
    /// `dup_x1`: duplicate the top value and insert it two slots down.
    DupX1,
    /// `dup_x2`: duplicate the top value and insert it below one or two values.
    DupX2,
    /// `dup2`: duplicate the top two slots (two values, or one `long`/`double`).
    Dup2,
    /// `dup2_x1`: duplicate two slots and insert them below one value.
    Dup2X1,
    /// `dup2_x2`: duplicate two slots and insert them below two or three slots.
    Dup2X2,
    /// `swap`: exchange the top two category-1 values.
    Swap,
    BinOp(BinOp),
    UnOp(UnOp),
    Convert(Convert),
    Compare(Compare),
    Iinc {
        slot: u16,
        increment: i16,
    },
    IfEq(u32),
    IfNe(u32),
    IfLt(u32),
    IfGe(u32),
    IfGt(u32),
    IfLe(u32),
    IfICmpEq(u32),
    IfICmpNe(u32),
    IfICmpLt(u32),
    IfICmpGe(u32),
    IfICmpGt(u32),
    IfICmpLe(u32),
    IfACmpEq(u32),
    IfACmpNe(u32),
    IfNull(u32),
    IfNonNull(u32),
    Goto(u32),
    TableSwitch(u16),
    LookupSwitch(u16),
    ArrayLength,
    ArrayLoad(ArrayKind),
    ArrayStore(ArrayKind),
    /// `newarray`: the operand is the JVMS §6.5 atype byte.
    NewArray(u8),
    /// `anewarray`: the operand is a `CONSTANT_Class` naming the *element* type.
    ANewArray(u16),
    MultiANewArray {
        class: u16,
        dimensions: u8,
    },
    GetStatic(u16),
    PutStatic(u16),
    GetField(u16),
    PutField(u16),
    New(u16),
    InvokeVirtual(u16),
    InvokeSpecial(u16),
    InvokeStatic(u16),
    InvokeInterface(u16),
    /// `invokedynamic`: the operand is a `CONSTANT_InvokeDynamic` index.
    InvokeDynamic(u16),
    /// `athrow`: the operand must be a `Throwable`.
    Athrow,
    MonitorEnter,
    MonitorExit,
    /// `instanceof`: the operand is a `CONSTANT_Class`.
    InstanceOf(u16),
    /// `checkcast`: the operand is a `CONSTANT_Class`.
    CheckCast(u16),
    Ireturn,
    Lreturn,
    Freturn,
    Dreturn,
    Areturn,
    Return,
}

/// One `exception_table` entry, already translated to instruction indices.
///
/// `start` is inclusive, `end` exclusive, and `handler` is where execution resumes. Keeping
/// them as indices means the handler search compares against `frame.pc` directly, exactly like
/// a branch target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExceptionHandler {
    pub start: usize,
    pub end: usize,
    pub handler: usize,
    pub catch_type: u16,
}

/// A `tableswitch` or `lookupswitch` body, indexed by [`Instr::TableSwitch`] and
/// [`Instr::LookupSwitch`] so that the instruction itself stays small and `Copy`.
#[derive(Debug, Clone)]
pub struct SwitchTable {
    pub default: u32,
    /// `(key, instruction index)`, sorted by key.
    pub cases: Vec<(i32, u32)>,
}

#[derive(Debug)]
pub struct Code {
    pub instrs: Vec<Instr>,
    /// Byte offset of each instruction, in the same order as `instrs`.
    pub pcs: Vec<usize>,
    pub switches: Vec<SwitchTable>,
    pub max_stack: usize,
    pub max_locals: usize,
    pub handlers: Vec<ExceptionHandler>,
}

impl Code {
    /// Byte offset of instruction `index`, for stack traces and handler matching.
    pub fn pc(&self, index: usize) -> usize {
        self.pcs.get(index).copied().unwrap_or(0)
    }
}

/// Decodes a `Code` attribute payload.
pub fn decode(data: &[u8], method: &str) -> Result<Code, VmError> {
    let mut reader = Reader::new(data);
    let max_stack = usize::from(reader.u2()?);
    let max_locals = usize::from(reader.u2()?);
    let code_length = reader.u4()? as usize;
    let bytes = reader.bytes(code_length)?;

    let mut instrs = Vec::new();
    let mut pcs = Vec::new();
    let mut switches: Vec<SwitchTable> = Vec::new();
    let mut body = Reader::new(bytes);
    while !body.is_empty() {
        pcs.push(body.offset());
        instrs.push(decode_one(&mut body, &mut switches, method)?);
    }

    let handler_count = reader.u2()?;
    let mut handlers = Vec::with_capacity(usize::from(handler_count));
    for _ in 0..handler_count {
        handlers.push(ExceptionHandler {
            start: usize::from(reader.u2()?),
            end: usize::from(reader.u2()?),
            handler: usize::from(reader.u2()?),
            catch_type: reader.u2()?,
        });
    }

    // Nested attributes (LineNumberTable, StackMapTable, ...) are not read yet; they are
    // skipped by their declared length, which is why a newer compiler's output still loads.
    let attribute_count = reader.u2()?;
    for _ in 0..attribute_count {
        let _name_index = reader.u2()?;
        let length = reader.u4()? as usize;
        reader.skip(length)?;
    }

    translate_targets(&pcs, &mut instrs, &mut switches, &mut handlers, method)?;

    Ok(Code {
        instrs,
        pcs,
        switches,
        max_stack,
        max_locals,
        handlers,
    })
}

/// Rewrites branch and switch targets from byte offsets to instruction indices.
fn translate_targets(
    pcs: &[usize],
    instrs: &mut [Instr],
    switches: &mut [SwitchTable],
    handlers: &mut [ExceptionHandler],
    method: &str,
) -> Result<(), VmError> {
    let to_index = |byte: usize| -> Result<u32, VmError> {
        pcs.binary_search(&byte)
            .map(|index| index as u32)
            .map_err(|_| VmError::TruncatedCode {
                pc: byte,
                method: method.to_owned(),
            })
    };

    for instr in instrs.iter_mut() {
        let target = match instr {
            Instr::Goto(target)
            | Instr::IfEq(target)
            | Instr::IfNe(target)
            | Instr::IfLt(target)
            | Instr::IfGe(target)
            | Instr::IfGt(target)
            | Instr::IfLe(target)
            | Instr::IfICmpEq(target)
            | Instr::IfICmpNe(target)
            | Instr::IfICmpLt(target)
            | Instr::IfICmpGe(target)
            | Instr::IfICmpGt(target)
            | Instr::IfICmpLe(target)
            | Instr::IfACmpEq(target)
            | Instr::IfACmpNe(target)
            | Instr::IfNull(target)
            | Instr::IfNonNull(target) => target,
            _ => continue,
        };
        *target = to_index(*target as usize)?;
    }

    for table in switches.iter_mut() {
        table.default = to_index(table.default as usize)?;
        for (_, target) in table.cases.iter_mut() {
            *target = to_index(*target as usize)?;
        }
    }

    // An `end` byte offset need not be an instruction boundary: it can equal the end of the
    // code array, and the exclusive index is then simply "past the last instruction".
    for handler in handlers.iter_mut() {
        handler.start = to_index(handler.start)? as usize;
        handler.handler = to_index(handler.handler)? as usize;
        handler.end = pcs.partition_point(|pc| *pc < handler.end);
    }
    Ok(())
}

fn decode_one(
    reader: &mut Reader<'_>,
    switches: &mut Vec<SwitchTable>,
    method: &str,
) -> Result<Instr, VmError> {
    let pc = reader.offset();
    let opcode = reader.u1()?;
    let instr = match opcode {
        0x00 => Instr::Nop,
        0x01 => Instr::AconstNull,
        0x02 => Instr::Iconst(-1),
        0x03..=0x08 => Instr::Iconst(i32::from(opcode) - 0x03),
        0x09 => Instr::Lconst(0),
        0x0a => Instr::Lconst(1),
        0x0b..=0x0d => Instr::Fconst(f32::from(opcode - 0x0b)),
        0x0e => Instr::Dconst(0.0),
        0x0f => Instr::Dconst(1.0),
        0x10 => Instr::Bipush(reader.u1()? as i8),
        0x11 => Instr::Sipush(reader.u2()? as i16),
        0x12 => Instr::Ldc(u16::from(reader.u1()?)),
        0x13 => Instr::Ldc(reader.u2()?),
        0x14 => Instr::Ldc2(reader.u2()?),
        0x15 => Instr::Iload(u16::from(reader.u1()?)),
        0x16 => Instr::Lload(u16::from(reader.u1()?)),
        0x17 => Instr::Fload(u16::from(reader.u1()?)),
        0x18 => Instr::Dload(u16::from(reader.u1()?)),
        0x19 => Instr::Aload(u16::from(reader.u1()?)),
        0x1a..=0x1d => Instr::Iload(u16::from(opcode - 0x1a)),
        0x1e..=0x21 => Instr::Lload(u16::from(opcode - 0x1e)),
        0x22..=0x25 => Instr::Fload(u16::from(opcode - 0x22)),
        0x26..=0x29 => Instr::Dload(u16::from(opcode - 0x26)),
        0x2a..=0x2d => Instr::Aload(u16::from(opcode - 0x2a)),
        0x36 => Instr::Istore(u16::from(reader.u1()?)),
        0x37 => Instr::Lstore(u16::from(reader.u1()?)),
        0x38 => Instr::Fstore(u16::from(reader.u1()?)),
        0x39 => Instr::Dstore(u16::from(reader.u1()?)),
        0x3a => Instr::Astore(u16::from(reader.u1()?)),
        0x3b..=0x3e => Instr::Istore(u16::from(opcode - 0x3b)),
        0x3f..=0x42 => Instr::Lstore(u16::from(opcode - 0x3f)),
        0x43..=0x46 => Instr::Fstore(u16::from(opcode - 0x43)),
        0x47..=0x4a => Instr::Dstore(u16::from(opcode - 0x47)),
        0x4b..=0x4e => Instr::Astore(u16::from(opcode - 0x4b)),
        0x2e..=0x35 => Instr::ArrayLoad(array_kind(opcode).expect("covered by the range")),
        0x4f..=0x56 => Instr::ArrayStore(array_kind(opcode).expect("covered by the range")),
        0x57 => Instr::Pop,
        0x58 => Instr::Pop2,
        0x59 => Instr::Dup,
        0x5a => Instr::DupX1,
        0x5b => Instr::DupX2,
        0x5c => Instr::Dup2,
        0x5d => Instr::Dup2X1,
        0x5e => Instr::Dup2X2,
        0x5f => Instr::Swap,
        // 0x74..=0x77 are the negation instructions and are not binary.
        0x60..=0x73 | 0x78..=0x83 => Instr::BinOp(binop(opcode).expect("covered by the range")),
        0x74..=0x77 => Instr::UnOp(unop(opcode).expect("covered by the range")),
        0x84 => Instr::Iinc {
            slot: u16::from(reader.u1()?),
            increment: i16::from(reader.u1()? as i8),
        },
        0x85..=0x93 => Instr::Convert(convert(opcode).expect("covered by the range")),
        0x94..=0x98 => Instr::Compare(compare(opcode).expect("covered by the range")),
        0x99 => Instr::IfEq(absolute(reader, pc)?),
        0x9a => Instr::IfNe(absolute(reader, pc)?),
        0x9b => Instr::IfLt(absolute(reader, pc)?),
        0x9c => Instr::IfGe(absolute(reader, pc)?),
        0x9d => Instr::IfGt(absolute(reader, pc)?),
        0x9e => Instr::IfLe(absolute(reader, pc)?),
        0x9f => Instr::IfICmpEq(absolute(reader, pc)?),
        0xa0 => Instr::IfICmpNe(absolute(reader, pc)?),
        0xa1 => Instr::IfICmpLt(absolute(reader, pc)?),
        0xa2 => Instr::IfICmpGe(absolute(reader, pc)?),
        0xa3 => Instr::IfICmpGt(absolute(reader, pc)?),
        0xa4 => Instr::IfICmpLe(absolute(reader, pc)?),
        0xa5 => Instr::IfACmpEq(absolute(reader, pc)?),
        0xa6 => Instr::IfACmpNe(absolute(reader, pc)?),
        0xa7 => Instr::Goto(absolute(reader, pc)?),
        0xc8 => Instr::Goto(absolute_wide(reader, pc)?),
        0xaa => {
            let table = decode_table_switch(reader, pc)?;
            switches.push(table);
            Instr::TableSwitch((switches.len() - 1) as u16)
        }
        0xab => {
            let table = decode_lookup_switch(reader, pc)?;
            switches.push(table);
            Instr::LookupSwitch((switches.len() - 1) as u16)
        }
        0xac => Instr::Ireturn,
        0xad => Instr::Lreturn,
        0xae => Instr::Freturn,
        0xaf => Instr::Dreturn,
        0xb0 => Instr::Areturn,
        0xb1 => Instr::Return,
        0xb2 => Instr::GetStatic(reader.u2()?),
        0xb3 => Instr::PutStatic(reader.u2()?),
        0xb4 => Instr::GetField(reader.u2()?),
        0xb5 => Instr::PutField(reader.u2()?),
        0xb6 => Instr::InvokeVirtual(reader.u2()?),
        0xb7 => Instr::InvokeSpecial(reader.u2()?),
        0xb8 => Instr::InvokeStatic(reader.u2()?),
        0xb9 => {
            let constant = reader.u2()?;
            let _count = reader.u1()?;
            let _zero = reader.u1()?;
            Instr::InvokeInterface(constant)
        }
        0xbb => Instr::New(reader.u2()?),
        0xbf => Instr::Athrow,
        0xc0 => Instr::CheckCast(reader.u2()?),
        0xc2 => Instr::MonitorEnter,
        0xc3 => Instr::MonitorExit,
        0xc1 => Instr::InstanceOf(reader.u2()?),
        0xc6 => Instr::IfNull(absolute(reader, pc)?),
        0xc7 => Instr::IfNonNull(absolute(reader, pc)?),
        0xbc => Instr::NewArray(reader.u1()?),
        0xbd => Instr::ANewArray(reader.u2()?),
        0xbe => Instr::ArrayLength,
        0xc5 => Instr::MultiANewArray {
            class: reader.u2()?,
            dimensions: reader.u1()?,
        },
        0xba => {
            // A call-site index followed by two bytes that must be zero.
            let constant = reader.u2()?;
            let _zero = reader.u2()?;
            Instr::InvokeDynamic(constant)
        }
        0xc4 => decode_wide(reader, pc, method)?,
        other => {
            return Err(VmError::UnsupportedInstruction {
                opcode: other,
                pc,
                method: method.to_owned(),
            });
        }
    };
    Ok(instr)
}

/// Reads a signed 16-bit relative branch offset and returns the absolute byte offset.
fn absolute(reader: &mut Reader<'_>, pc: usize) -> Result<u32, VmError> {
    let offset = reader.u2()? as i16;
    Ok((pc as i32 + i32::from(offset)) as u32)
}

/// Reads a signed 32-bit relative branch offset (`goto_w`) and returns the absolute byte offset.
fn absolute_wide(reader: &mut Reader<'_>, pc: usize) -> Result<u32, VmError> {
    let offset = reader.u4()? as i32;
    Ok((pc as i32 + offset) as u32)
}

/// Decodes the instruction after a `wide` prefix. The prefix only widens a local variable index
/// (u1 -> u2) and, for `iinc`, the increment (i8 -> i16); the instruction set itself is unchanged.
fn decode_wide(reader: &mut Reader<'_>, pc: usize, method: &str) -> Result<Instr, VmError> {
    let widened = reader.u1()?;
    Ok(match widened {
        0x15 => Instr::Iload(reader.u2()?),
        0x16 => Instr::Lload(reader.u2()?),
        0x17 => Instr::Fload(reader.u2()?),
        0x18 => Instr::Dload(reader.u2()?),
        0x19 => Instr::Aload(reader.u2()?),
        0x36 => Instr::Istore(reader.u2()?),
        0x37 => Instr::Lstore(reader.u2()?),
        0x38 => Instr::Fstore(reader.u2()?),
        0x39 => Instr::Dstore(reader.u2()?),
        0x3a => Instr::Astore(reader.u2()?),
        0x84 => Instr::Iinc {
            slot: reader.u2()?,
            increment: reader.u2()? as i16,
        },
        other => {
            return Err(VmError::UnsupportedInstruction {
                opcode: other,
                pc,
                method: method.to_owned(),
            });
        }
    })
}

fn align_switch(reader: &mut Reader<'_>) -> Result<(), VmError> {
    let padding = (4 - (reader.offset() % 4)) % 4;
    reader.skip(padding)?;
    Ok(())
}

fn decode_table_switch(reader: &mut Reader<'_>, pc: usize) -> Result<SwitchTable, VmError> {
    align_switch(reader)?;
    let default = (pc as i32 + reader.u4()? as i32) as u32;
    let low = reader.u4()? as i32;
    let high = reader.u4()? as i32;
    let count = (high - low + 1).max(0) as usize;
    let mut cases = Vec::with_capacity(count);
    for key in 0..count {
        let target = (pc as i32 + reader.u4()? as i32) as u32;
        cases.push((low + key as i32, target));
    }
    Ok(SwitchTable { default, cases })
}

fn decode_lookup_switch(reader: &mut Reader<'_>, pc: usize) -> Result<SwitchTable, VmError> {
    align_switch(reader)?;
    let default = (pc as i32 + reader.u4()? as i32) as u32;
    let pairs = reader.u4()? as usize;
    let mut cases = Vec::with_capacity(pairs);
    for _ in 0..pairs {
        let key = reader.u4()? as i32;
        let target = (pc as i32 + reader.u4()? as i32) as u32;
        cases.push((key, target));
    }
    Ok(SwitchTable { default, cases })
}

/// The element kind an array instruction operates on: `iaload` is `Int`, `aaload` is
/// `Reference`, `baload` is `Byte` (and also reads boolean arrays).
fn array_kind(opcode: u8) -> Option<ArrayKind> {
    Some(match opcode {
        0x2e | 0x4f => ArrayKind::Int,
        0x2f | 0x50 => ArrayKind::Long,
        0x30 | 0x51 => ArrayKind::Float,
        0x31 | 0x52 => ArrayKind::Double,
        0x32 | 0x53 => ArrayKind::Reference,
        0x33 | 0x54 => ArrayKind::Byte,
        0x34 | 0x55 => ArrayKind::Char,
        0x35 | 0x56 => ArrayKind::Short,
        _ => return None,
    })
}

fn binop(opcode: u8) -> Option<BinOp> {
    Some(match opcode {
        0x60 => BinOp::IAdd,
        0x61 => BinOp::LAdd,
        0x62 => BinOp::FAdd,
        0x63 => BinOp::DAdd,
        0x64 => BinOp::ISub,
        0x65 => BinOp::LSub,
        0x66 => BinOp::FSub,
        0x67 => BinOp::DSub,
        0x68 => BinOp::IMul,
        0x69 => BinOp::LMul,
        0x6a => BinOp::FMul,
        0x6b => BinOp::DMul,
        0x6c => BinOp::IDiv,
        0x6d => BinOp::LDiv,
        0x6e => BinOp::FDiv,
        0x6f => BinOp::DDiv,
        0x70 => BinOp::IRem,
        0x71 => BinOp::LRem,
        0x72 => BinOp::FRem,
        0x73 => BinOp::DRem,
        0x78 => BinOp::IShl,
        0x79 => BinOp::LShl,
        0x7a => BinOp::IShr,
        0x7b => BinOp::LShr,
        0x7c => BinOp::IUShr,
        0x7d => BinOp::LUShr,
        0x7e => BinOp::IAnd,
        0x7f => BinOp::LAnd,
        0x80 => BinOp::IOr,
        0x81 => BinOp::LOr,
        0x82 => BinOp::IXor,
        0x83 => BinOp::LXor,
        // 0x74..=0x77 are ineg/lneg/fneg/dneg, handled as UnOp.
        _ => return None,
    })
}

fn unop(opcode: u8) -> Option<UnOp> {
    Some(match opcode {
        0x74 => UnOp::INeg,
        0x75 => UnOp::LNeg,
        0x76 => UnOp::FNeg,
        0x77 => UnOp::DNeg,
        _ => return None,
    })
}

fn convert(opcode: u8) -> Option<Convert> {
    Some(match opcode {
        0x85 => Convert::I2L,
        0x86 => Convert::I2F,
        0x87 => Convert::I2D,
        0x88 => Convert::L2I,
        0x89 => Convert::L2F,
        0x8a => Convert::L2D,
        0x8b => Convert::F2I,
        0x8c => Convert::F2L,
        0x8d => Convert::F2D,
        0x8e => Convert::D2I,
        0x8f => Convert::D2L,
        0x90 => Convert::D2F,
        0x91 => Convert::I2B,
        0x92 => Convert::I2C,
        0x93 => Convert::I2S,
        _ => return None,
    })
}

fn compare(opcode: u8) -> Option<Compare> {
    Some(match opcode {
        0x94 => Compare::LCmp,
        0x95 => Compare::FCmpL,
        0x96 => Compare::FCmpG,
        0x97 => Compare::DCmpL,
        0x98 => Compare::DCmpG,
        _ => return None,
    })
}
