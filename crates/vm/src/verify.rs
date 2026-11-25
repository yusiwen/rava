//! Bytecode verification: the M0 dataflow skeleton over a `Code` attribute.
//!
//! The skeleton decodes every instruction boundary, propagates a frame of operand-stack
//! height and initialized local slots through a worklist, merges frames at branch
//! targets, and seeds exception handlers with a one-slot stack. It does not type-check
//! operands: the full verifier for generated classes lands at M3 on top of this shape.
//!
//! Opcodes covered: constants, local loads/stores (including the `wide` forms),
//! array loads/stores, stack manipulation, integer/long/float/double arithmetic and
//! conversions, comparisons and conditional/unconditional branches,
//! `tableswitch`/`lookupswitch`, field access, method invocation, object and array
//! creation, `athrow`, monitors, and all return forms. `jsr`/`ret` and reserved opcodes
//! return [`VerifyError::UnsupportedOpcode`] rather than being treated as no-ops.
//!
//! Effects are slot counts, not Java types: `dup` needs one slot and pushes two, `dup2`
//! needs two and pushes four. That is conservative and sufficient for the M0
//! invariants; M3 must add category-1/category-2 type states to accept every valid
//! method and to reject type-confused ones.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use rava_classfile::{ClassFile, Constant, Error as ClassFileError, Method};

/// `code_length` must be positive and less than 65536 (JVMS §4.7.3).
const MAX_CODE_LENGTH: u32 = 65_535;

/// One exception-table entry, byte offsets relative to the start of the code array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExceptionHandler {
    pub start_pc: u16,
    pub end_pc: u16,
    pub handler_pc: u16,
    pub catch_type: u16,
}

/// The parsed payload of a method's `Code` attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeAttribute {
    pub max_stack: u16,
    pub max_locals: u16,
    pub bytes: Vec<u8>,
    pub exception_table: Vec<ExceptionHandler>,
}

/// The verifier state at one instruction boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyFrame {
    /// Operand-stack height in JVM slots.
    pub stack: u16,
    /// Local slots that are definitely assigned on every path reaching this boundary.
    pub locals: BTreeSet<u16>,
}

/// A verified method: the frames the dataflow reached plus the parsed exception table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedMethod {
    pub max_stack: u16,
    pub max_locals: u16,
    pub code_len: usize,
    pub exception_table: Vec<ExceptionHandler>,
    /// Frames keyed by instruction byte offset. Unreachable instructions have no frame.
    pub frames: BTreeMap<usize, VerifyFrame>,
}

impl VerifiedMethod {
    pub fn frame_at(&self, pc: usize) -> Option<&VerifyFrame> {
        self.frames.get(&pc)
    }
}

/// Every way verifying a method can fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// The method has no `Code` attribute (native or abstract).
    NoCode,
    /// The `Code` payload itself is malformed or truncated.
    MalformedCode { detail: String },
    /// The method descriptor or a constant's descriptor cannot be parsed.
    MalformedDescriptor { descriptor: String, detail: String },
    /// An opcode outside the skeleton's covered set.
    UnsupportedOpcode { pc: usize, opcode: u8 },
    /// An instruction ended before its operands did.
    TruncatedOperand {
        pc: usize,
        opcode: u8,
        needed: usize,
        available: usize,
    },
    /// An instruction popped more values than the operand stack held.
    StackUnderflow { pc: usize, needed: u16, found: u16 },
    /// An instruction pushed past `max_stack`.
    StackOverflow {
        pc: usize,
        max_stack: u16,
        found: u32,
    },
    /// A local index (or a two-slot local) falls outside `max_locals`.
    InvalidLocal {
        pc: usize,
        index: u16,
        max_locals: u16,
    },
    /// A load reads a local that is not definitely assigned on this path.
    UninitializedLocal { pc: usize, index: u16 },
    /// A branch target is not an instruction boundary in this method.
    InvalidBranchTarget {
        pc: usize,
        target: i64,
        code_len: usize,
    },
    /// Two control-flow paths reach one boundary with different stack heights.
    StackHeightMismatch {
        pc: usize,
        existing: u16,
        incoming: u16,
    },
    /// An exception-table range or handler is not valid for this code array.
    InvalidExceptionTable { detail: String },
    /// Resolving a descriptor through the constant pool failed.
    ClassFile(ClassFileError),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoCode => write!(f, "method has no Code attribute"),
            Self::MalformedCode { detail } => write!(f, "malformed Code attribute: {detail}"),
            Self::MalformedDescriptor { descriptor, detail } => {
                write!(f, "malformed descriptor \"{descriptor}\": {detail}")
            }
            Self::UnsupportedOpcode { pc, opcode } => {
                write!(f, "unsupported opcode 0x{opcode:02x} at pc {pc}")
            }
            Self::TruncatedOperand {
                pc,
                opcode,
                needed,
                available,
            } => write!(
                f,
                "truncated operand for opcode 0x{opcode:02x} at pc {pc}: needed {needed} byte(s), {available} available"
            ),
            Self::StackUnderflow { pc, needed, found } => write!(
                f,
                "stack underflow at pc {pc}: instruction needs {needed} slot(s), the stack has {found}"
            ),
            Self::StackOverflow {
                pc,
                max_stack,
                found,
            } => write!(
                f,
                "stack overflow at pc {pc}: max_stack is {max_stack}, the instruction produces {found}"
            ),
            Self::InvalidLocal {
                pc,
                index,
                max_locals,
            } => write!(
                f,
                "invalid local index {index} at pc {pc}: max_locals is {max_locals}"
            ),
            Self::UninitializedLocal { pc, index } => {
                write!(f, "local {index} is not initialized at pc {pc}")
            }
            Self::InvalidBranchTarget {
                pc,
                target,
                code_len,
            } => write!(
                f,
                "branch at pc {pc} targets {target}, outside the instruction boundaries of {code_len} byte(s) of code"
            ),
            Self::StackHeightMismatch {
                pc,
                existing,
                incoming,
            } => write!(
                f,
                "stack height mismatch at pc {pc}: existing frame has {existing} slot(s), incoming frame has {incoming}"
            ),
            Self::InvalidExceptionTable { detail } => {
                write!(f, "invalid exception table: {detail}")
            }
            Self::ClassFile(error) => write!(f, "class-file error during verification: {error}"),
        }
    }
}

impl std::error::Error for VerifyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ClassFile(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ClassFileError> for VerifyError {
    fn from(error: ClassFileError) -> Self {
        Self::ClassFile(error)
    }
}

/// Parses a method's `Code` attribute into its header, bytecode and exception table.
///
/// Nested `Code` attributes (`LineNumberTable`, `StackMapTable`, ...) are validated as
/// length-prefixed byte strings and skipped; the dump and verifier only need the raw
/// payload lengths.
pub fn parse_code(file: &ClassFile, method: &Method) -> Result<CodeAttribute, VerifyError> {
    let attribute = file.code(method)?.ok_or(VerifyError::NoCode)?;
    let data = attribute.data.as_slice();
    if data.len() < 8 {
        return Err(VerifyError::MalformedCode {
            detail: format!(
                "Code attribute is {} byte(s), shorter than its 8-byte header",
                data.len()
            ),
        });
    }

    let max_stack = u16::from_be_bytes([data[0], data[1]]);
    let max_locals = u16::from_be_bytes([data[2], data[3]]);
    let code_length = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if code_length == 0 || code_length > MAX_CODE_LENGTH {
        return Err(VerifyError::MalformedCode {
            detail: format!("code_length {code_length} is outside 1..={MAX_CODE_LENGTH}"),
        });
    }
    let code_length = usize::try_from(code_length).map_err(|_| VerifyError::MalformedCode {
        detail: "code_length does not fit in usize".to_owned(),
    })?;

    let code_start = 8usize;
    let code_end =
        code_start
            .checked_add(code_length)
            .ok_or_else(|| VerifyError::MalformedCode {
                detail: "code_length overflows the Code attribute offset".to_owned(),
            })?;
    let bytes = data
        .get(code_start..code_end)
        .ok_or_else(|| VerifyError::MalformedCode {
            detail: format!(
                "Code attribute declares {code_length} code byte(s) but only {} are present",
                data.len().saturating_sub(code_start)
            ),
        })?
        .to_vec();

    let mut offset = code_end;
    let exception_count = read_u2(data, &mut offset, "exception table count")?;
    let mut exception_table = Vec::with_capacity(usize::from(exception_count));
    for _ in 0..exception_count {
        let start_pc = read_u2(data, &mut offset, "exception start_pc")?;
        let end_pc = read_u2(data, &mut offset, "exception end_pc")?;
        let handler_pc = read_u2(data, &mut offset, "exception handler_pc")?;
        let catch_type = read_u2(data, &mut offset, "exception catch_type")?;
        if catch_type != 0
            && !matches!(
                file.constant_pool
                    .get(catch_type)
                    .map_err(VerifyError::ClassFile)?,
                Constant::Class { .. }
            )
        {
            return Err(VerifyError::InvalidExceptionTable {
                detail: format!("catch_type {catch_type} is not a Class constant"),
            });
        }
        exception_table.push(ExceptionHandler {
            start_pc,
            end_pc,
            handler_pc,
            catch_type,
        });
    }

    let nested_count = read_u2(data, &mut offset, "nested attribute count")?;
    for _ in 0..nested_count {
        let _name_index = read_u2(data, &mut offset, "nested attribute name index")?;
        let length = read_u4(data, &mut offset, "nested attribute length")?;
        let length = usize::try_from(length).map_err(|_| VerifyError::MalformedCode {
            detail: "nested attribute length does not fit in usize".to_owned(),
        })?;
        offset = offset
            .checked_add(length)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| VerifyError::MalformedCode {
                detail: format!("nested Code attribute at byte {offset} is truncated"),
            })?;
    }

    if offset != data.len() {
        return Err(VerifyError::MalformedCode {
            detail: format!(
                "{} trailing byte(s) after the Code payload",
                data.len() - offset
            ),
        });
    }

    Ok(CodeAttribute {
        max_stack,
        max_locals,
        bytes,
        exception_table,
    })
}

/// Verifies one method's bytecode and returns the dataflow result.
///
/// The method descriptor supplies the initially assigned local slots (parameters, plus
/// `this` for an instance method). A malformed descriptor or a parameter list that does
/// not fit `max_locals` is a typed error, not a panic.
pub fn verify_method(file: &ClassFile, method: &Method) -> Result<VerifiedMethod, VerifyError> {
    let code = parse_code(file, method)?;
    let descriptor = file.method_descriptor(method)?;
    let (argument_slots, _return_slots) = method_descriptor_slots(descriptor)?;
    let parameter_slots = if method.is_static() {
        argument_slots
    } else {
        argument_slots
            .checked_add(1)
            .ok_or_else(|| VerifyError::MalformedCode {
                detail: "parameter slot count overflows".to_owned(),
            })?
    };
    if parameter_slots > code.max_locals {
        return Err(VerifyError::MalformedCode {
            detail: format!(
                "method needs {parameter_slots} local slot(s) for its parameters, max_locals is {}",
                code.max_locals
            ),
        });
    }

    let instructions = decode(file, &code)?;
    validate_exception_table(&code, &instructions)?;

    let initial_locals = (0..parameter_slots).collect();
    let frames = dataflow(&code, &instructions, initial_locals)?;

    Ok(VerifiedMethod {
        max_stack: code.max_stack,
        max_locals: code.max_locals,
        code_len: code.bytes.len(),
        exception_table: code.exception_table,
        frames,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Effect {
    pop: u16,
    push: u16,
}

impl Effect {
    const fn new(pop: u16, push: u16) -> Self {
        Self { pop, push }
    }

    const fn none() -> Self {
        Self::new(0, 0)
    }

    const fn push(slots: u16) -> Self {
        Self::new(0, slots)
    }

    const fn pop(slots: u16) -> Self {
        Self::new(slots, 0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalAccess {
    None,
    Load { index: u16, slots: u16 },
    Store { index: u16, slots: u16 },
    Increment { index: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Flow {
    Fallthrough,
    /// Conditional branch: the target and the next instruction are both successors.
    Branch(i64),
    Goto(i64),
    /// `tableswitch`/`lookupswitch`: every case target and the default are successors,
    /// and control cannot fall through to the next instruction.
    Switch {
        default: i64,
        targets: Vec<i64>,
    },
    Return,
    Throw,
}

#[derive(Debug, Clone)]
struct Instruction {
    len: usize,
    effect: Effect,
    local: LocalAccess,
    flow: Flow,
}

fn decode(
    file: &ClassFile,
    code: &CodeAttribute,
) -> Result<BTreeMap<usize, Instruction>, VerifyError> {
    let bytes = code.bytes.as_slice();
    let mut instructions = BTreeMap::new();
    let mut pc = 0usize;

    while pc < bytes.len() {
        let opcode = bytes[pc];
        let mut next = pc + 1;
        let mut effect = Effect::none();
        let mut local = LocalAccess::None;
        let mut flow = Flow::Fallthrough;

        match opcode {
            0x00 => {}
            0x01..=0x08 => effect = Effect::push(1),
            0x09 | 0x0a => effect = Effect::push(2),
            0x0b..=0x0d => effect = Effect::push(1),
            0x0e | 0x0f => effect = Effect::push(2),
            0x10 => {
                let _value = operand_u1(bytes, &mut next, pc, opcode)?;
                effect = Effect::push(1);
            }
            0x11 => {
                let _value = operand_u2(bytes, &mut next, pc, opcode)?;
                effect = Effect::push(1);
            }
            0x12 => {
                let index = u16::from(operand_u1(bytes, &mut next, pc, opcode)?);
                effect = Effect::push(constant_slots(file, index)?);
            }
            0x13 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                effect = Effect::push(constant_slots(file, index)?);
            }
            0x14 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                effect = Effect::push(constant_slots(file, index)?);
            }
            0x15..=0x19 => {
                let index = u16::from(operand_u1(bytes, &mut next, pc, opcode)?);
                let slots = load_slots(opcode);
                local = LocalAccess::Load { index, slots };
                effect = Effect::push(slots);
            }
            0x1a..=0x1d => {
                let index = u16::from(opcode - 0x1a);
                local = LocalAccess::Load { index, slots: 1 };
                effect = Effect::push(1);
            }
            0x1e..=0x21 => {
                let index = u16::from(opcode - 0x1e);
                local = LocalAccess::Load { index, slots: 2 };
                effect = Effect::push(2);
            }
            0x22..=0x25 => {
                let index = u16::from(opcode - 0x22);
                local = LocalAccess::Load { index, slots: 1 };
                effect = Effect::push(1);
            }
            0x26..=0x29 => {
                let index = u16::from(opcode - 0x26);
                local = LocalAccess::Load { index, slots: 2 };
                effect = Effect::push(2);
            }
            0x2a..=0x2d => {
                let index = u16::from(opcode - 0x2a);
                local = LocalAccess::Load { index, slots: 1 };
                effect = Effect::push(1);
            }
            0x2e..=0x35 => {
                // Array loads pop the array reference and the index.
                effect = Effect::new(2, array_load_slots(opcode));
            }
            0x36..=0x3a => {
                let index = u16::from(operand_u1(bytes, &mut next, pc, opcode)?);
                let slots = store_slots(opcode);
                local = LocalAccess::Store { index, slots };
                effect = Effect::pop(slots);
            }
            0x3b..=0x3e => {
                let index = u16::from(opcode - 0x3b);
                local = LocalAccess::Store { index, slots: 1 };
                effect = Effect::pop(1);
            }
            0x3f..=0x42 => {
                let index = u16::from(opcode - 0x3f);
                local = LocalAccess::Store { index, slots: 2 };
                effect = Effect::pop(2);
            }
            0x43..=0x46 => {
                let index = u16::from(opcode - 0x43);
                local = LocalAccess::Store { index, slots: 1 };
                effect = Effect::pop(1);
            }
            0x47..=0x4a => {
                let index = u16::from(opcode - 0x47);
                local = LocalAccess::Store { index, slots: 2 };
                effect = Effect::pop(2);
            }
            0x4b..=0x4e => {
                let index = u16::from(opcode - 0x4b);
                local = LocalAccess::Store { index, slots: 1 };
                effect = Effect::pop(1);
            }
            0x4f..=0x56 => {
                // Array stores pop the array reference, the index and the value.
                effect = Effect::pop(2 + array_store_value_slots(opcode));
            }
            0x57 => effect = Effect::pop(1),
            0x58 => effect = Effect::pop(2),
            0x59 => effect = Effect::new(1, 2),
            0x5a => effect = Effect::new(2, 3),
            0x5b => effect = Effect::new(3, 4),
            0x5c => effect = Effect::new(2, 4),
            0x5d => effect = Effect::new(3, 5),
            0x5e => effect = Effect::new(4, 6),
            0x5f => effect = Effect::new(2, 2),
            0x60 | 0x62 | 0x64 | 0x66 | 0x68 | 0x6a | 0x6c | 0x6e | 0x70 | 0x72 | 0x7e | 0x80
            | 0x82 => effect = Effect::new(2, 1),
            0x61 | 0x63 | 0x65 | 0x67 | 0x69 | 0x6b | 0x6d | 0x6f | 0x71 | 0x73 | 0x7f | 0x81
            | 0x83 => effect = Effect::new(4, 2),
            0x74 | 0x76 => effect = Effect::new(1, 1),
            0x75 | 0x77 => effect = Effect::new(2, 2),
            0x78 | 0x7a | 0x7c => effect = Effect::new(2, 1),
            0x79 | 0x7b | 0x7d => effect = Effect::new(3, 2),
            0x84 => {
                let index = u16::from(operand_u1(bytes, &mut next, pc, opcode)?);
                let _increment = operand_i1(bytes, &mut next, pc, opcode)?;
                local = LocalAccess::Increment { index };
            }
            0x85 | 0x87 | 0x8c | 0x8d => effect = Effect::new(1, 2),
            0x86 | 0x8b | 0x91 | 0x92 | 0x93 => effect = Effect::new(1, 1),
            0x88 | 0x89 | 0x8e | 0x90 => effect = Effect::new(2, 1),
            0x8a | 0x8f => effect = Effect::new(2, 2),
            0x94 => effect = Effect::new(4, 1),
            0x95 | 0x96 => effect = Effect::new(2, 1),
            0x97 | 0x98 => effect = Effect::new(4, 1),
            0x99..=0x9e => {
                effect = Effect::pop(1);
                flow =
                    Flow::Branch(pc as i64 + i64::from(operand_i2(bytes, &mut next, pc, opcode)?));
            }
            0x9f..=0xa6 => {
                effect = Effect::pop(2);
                flow =
                    Flow::Branch(pc as i64 + i64::from(operand_i2(bytes, &mut next, pc, opcode)?));
            }
            0xa7 => {
                flow = Flow::Goto(pc as i64 + i64::from(operand_i2(bytes, &mut next, pc, opcode)?));
            }
            0xaa => {
                let (default, targets) = decode_tableswitch(bytes, &mut next, pc, opcode)?;
                effect = Effect::pop(1);
                flow = Flow::Switch { default, targets };
            }
            0xab => {
                let (default, targets) = decode_lookupswitch(bytes, &mut next, pc, opcode)?;
                effect = Effect::pop(1);
                flow = Flow::Switch { default, targets };
            }
            0xc4 => {
                let modified = operand_u1(bytes, &mut next, pc, opcode)?;
                match modified {
                    0x15..=0x19 => {
                        let index = operand_u2(bytes, &mut next, pc, opcode)?;
                        let slots = load_slots(modified);
                        local = LocalAccess::Load { index, slots };
                        effect = Effect::push(slots);
                    }
                    0x36..=0x3a => {
                        let index = operand_u2(bytes, &mut next, pc, opcode)?;
                        let slots = store_slots(modified);
                        local = LocalAccess::Store { index, slots };
                        effect = Effect::pop(slots);
                    }
                    0x84 => {
                        let index = operand_u2(bytes, &mut next, pc, opcode)?;
                        let _increment = operand_i2(bytes, &mut next, pc, opcode)?;
                        local = LocalAccess::Increment { index };
                    }
                    // `ret` and every other `wide`-modified opcode stay unsupported, so
                    // the method is rejected instead of executing the local-variable
                    // semantics the verifier does not model.
                    _ => {
                        return Err(VerifyError::UnsupportedOpcode {
                            pc,
                            opcode: modified,
                        });
                    }
                }
            }
            0xac | 0xae | 0xb0 => {
                effect = Effect::pop(1);
                flow = Flow::Return;
            }
            0xad | 0xaf => {
                effect = Effect::pop(2);
                flow = Flow::Return;
            }
            0xb1 => flow = Flow::Return,
            0xb2 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                effect = Effect::push(field_slots(file, index)?);
            }
            0xb3 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                effect = Effect::pop(field_slots(file, index)?);
            }
            0xb4 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                effect = Effect::new(1, field_slots(file, index)?);
            }
            0xb5 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                let slots = field_slots(file, index)?;
                effect = Effect::pop(1u16.saturating_add(slots));
            }
            0xb6..=0xb9 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                let receiver = opcode != 0xb8;
                effect = invoke_effect(file, index, receiver, false)?;
                if opcode == 0xb9 {
                    let _count = operand_u1(bytes, &mut next, pc, opcode)?;
                    let _zero = operand_u1(bytes, &mut next, pc, opcode)?;
                }
            }
            0xba => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                let _zero = operand_u2(bytes, &mut next, pc, opcode)?;
                effect = invoke_effect(file, index, false, true)?;
            }
            0xbb | 0xbd | 0xc0 | 0xc1 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                let _name = file.constant_pool.class_name(index)?;
                effect = match opcode {
                    0xbb => Effect::push(1),
                    _ => Effect::new(1, 1),
                };
            }
            0xbc => {
                let atype = operand_u1(bytes, &mut next, pc, opcode)?;
                if !(4..=11).contains(&atype) {
                    return Err(VerifyError::MalformedCode {
                        detail: format!("newarray atype {atype} is not a primitive array type"),
                    });
                }
                effect = Effect::new(1, 1);
            }
            0xbe => effect = Effect::new(1, 1),
            0xbf => {
                effect = Effect::pop(1);
                flow = Flow::Throw;
            }
            0xc2 | 0xc3 => effect = Effect::pop(1),
            0xc5 => {
                let index = operand_u2(bytes, &mut next, pc, opcode)?;
                let _name = file.constant_pool.class_name(index)?;
                let dimensions = operand_u1(bytes, &mut next, pc, opcode)?;
                if dimensions == 0 {
                    return Err(VerifyError::MalformedCode {
                        detail: "multianewarray has zero dimensions".to_owned(),
                    });
                }
                effect = Effect::new(u16::from(dimensions), 1);
            }
            0xc6 | 0xc7 => {
                effect = Effect::pop(1);
                flow =
                    Flow::Branch(pc as i64 + i64::from(operand_i2(bytes, &mut next, pc, opcode)?));
            }
            0xc8 => {
                flow = Flow::Goto(pc as i64 + i64::from(operand_i4(bytes, &mut next, pc, opcode)?));
            }
            _ => return Err(VerifyError::UnsupportedOpcode { pc, opcode }),
        }

        instructions.insert(
            pc,
            Instruction {
                len: next - pc,
                effect,
                local,
                flow,
            },
        );
        pc = next;
    }

    Ok(instructions)
}

fn load_slots(opcode: u8) -> u16 {
    match opcode {
        0x16 | 0x18 => 2,
        _ => 1,
    }
}

fn store_slots(opcode: u8) -> u16 {
    match opcode {
        0x37 | 0x39 => 2,
        _ => 1,
    }
}

/// Skips the padding between a switch opcode and its first operand.
///
/// JVMS §6.5 aligns the operand on a four-byte boundary measured from the start of the
/// code array and requires the padding bytes to be zero; a non-zero byte is malformed,
/// not something to ignore.
fn skip_switch_padding(
    bytes: &[u8],
    next: &mut usize,
    pc: usize,
    opcode: u8,
) -> Result<(), VerifyError> {
    let padding = (4 - (*next % 4)) % 4;
    for _ in 0..padding {
        let value = operand_u1(bytes, next, pc, opcode)?;
        if value != 0 {
            return Err(VerifyError::MalformedCode {
                detail: format!(
                    "opcode 0x{opcode:02x} at pc {pc} has non-zero padding byte 0x{value:02x}"
                ),
            });
        }
    }
    Ok(())
}

/// Decodes a `tableswitch` operand block into the default and per-case target offsets.
///
/// The targets are absolute byte offsets; [`branch_target`] validates every one against
/// the instruction boundaries afterwards.
fn decode_tableswitch(
    bytes: &[u8],
    next: &mut usize,
    pc: usize,
    opcode: u8,
) -> Result<(i64, Vec<i64>), VerifyError> {
    skip_switch_padding(bytes, next, pc, opcode)?;
    let default = operand_i4(bytes, next, pc, opcode)?;
    let low = operand_i4(bytes, next, pc, opcode)?;
    let high = operand_i4(bytes, next, pc, opcode)?;
    if high < low {
        return Err(VerifyError::MalformedCode {
            detail: format!("tableswitch at pc {pc} has high {high} below low {low}"),
        });
    }
    let count = i64::from(high) - i64::from(low) + 1;
    let available = bytes.len().saturating_sub(*next);
    let needed = count * 4; // count is at most 2^32, so this cannot overflow i64
    if needed > available as i64 {
        return Err(truncated(
            pc,
            opcode,
            usize::try_from(needed).unwrap_or(usize::MAX),
            available,
        ));
    }
    let count = usize::try_from(count).map_err(|_| VerifyError::MalformedCode {
        detail: format!("tableswitch at pc {pc} has a jump table too large to index"),
    })?;
    let mut targets = Vec::with_capacity(count);
    for _ in 0..count {
        let offset = operand_i4(bytes, next, pc, opcode)?;
        targets.push(pc as i64 + i64::from(offset));
    }
    Ok((pc as i64 + i64::from(default), targets))
}

/// Decodes a `lookupswitch` operand block into the default and per-case target offsets.
///
/// The `(match, offset)` pairs must be strictly ascending by `match`
/// (JVMS §6.5.lookupswitch); an unsorted or duplicate key is malformed.
fn decode_lookupswitch(
    bytes: &[u8],
    next: &mut usize,
    pc: usize,
    opcode: u8,
) -> Result<(i64, Vec<i64>), VerifyError> {
    skip_switch_padding(bytes, next, pc, opcode)?;
    let default = operand_i4(bytes, next, pc, opcode)?;
    let pairs = operand_i4(bytes, next, pc, opcode)?;
    if pairs < 0 {
        return Err(VerifyError::MalformedCode {
            detail: format!("lookupswitch at pc {pc} has a negative pair count {pairs}"),
        });
    }
    let available = bytes.len().saturating_sub(*next);
    let needed = i64::from(pairs) * 8; // pairs is non-negative and at most 2^31
    if needed > available as i64 {
        return Err(truncated(
            pc,
            opcode,
            usize::try_from(needed).unwrap_or(usize::MAX),
            available,
        ));
    }
    let pairs = usize::try_from(pairs).map_err(|_| VerifyError::MalformedCode {
        detail: format!("lookupswitch at pc {pc} has a pair count too large to index"),
    })?;
    let mut previous: Option<i32> = None;
    let mut targets = Vec::with_capacity(pairs);
    for _ in 0..pairs {
        let key = operand_i4(bytes, next, pc, opcode)?;
        if let Some(previous) = previous {
            if key <= previous {
                return Err(VerifyError::MalformedCode {
                    detail: format!(
                        "lookupswitch at pc {pc} has key {key} after {previous}: keys must be strictly ascending"
                    ),
                });
            }
        }
        previous = Some(key);
        let offset = operand_i4(bytes, next, pc, opcode)?;
        targets.push(pc as i64 + i64::from(offset));
    }
    Ok((pc as i64 + i64::from(default), targets))
}

fn array_load_slots(opcode: u8) -> u16 {
    match opcode {
        0x2f | 0x31 => 2,
        _ => 1,
    }
}

fn array_store_value_slots(opcode: u8) -> u16 {
    match opcode {
        0x50 | 0x52 => 2,
        _ => 1,
    }
}

fn constant_slots(file: &ClassFile, index: u16) -> Result<u16, VerifyError> {
    match file.constant_pool.get(index)? {
        Constant::Long(_) | Constant::Double(_) => Ok(2),
        Constant::Dynamic {
            name_and_type_index,
            ..
        } => {
            let (_, descriptor) = file.constant_pool.name_and_type(*name_and_type_index)?;
            field_slots_of(descriptor)
        }
        Constant::Integer(_)
        | Constant::Float(_)
        | Constant::String { .. }
        | Constant::Class { .. }
        | Constant::MethodType { .. }
        | Constant::MethodHandle { .. } => Ok(1),
        _ => Err(VerifyError::MalformedCode {
            detail: format!("constant {index} cannot be loaded by ldc"),
        }),
    }
}

fn field_slots(file: &ClassFile, index: u16) -> Result<u16, VerifyError> {
    let Constant::FieldRef {
        name_and_type_index,
        ..
    } = file.constant_pool.get(index)?
    else {
        return Err(VerifyError::MalformedCode {
            detail: format!("constant {index} is not a Fieldref"),
        });
    };
    let (_, descriptor) = file.constant_pool.name_and_type(*name_and_type_index)?;
    field_slots_of(descriptor)
}

fn invoke_effect(
    file: &ClassFile,
    index: u16,
    receiver: bool,
    dynamic: bool,
) -> Result<Effect, VerifyError> {
    let (argument_slots, return_slots) = {
        let (name_and_type_index, constant_is_dynamic) = match file.constant_pool.get(index)? {
            Constant::MethodRef {
                name_and_type_index,
                ..
            }
            | Constant::InterfaceMethodRef {
                name_and_type_index,
                ..
            } => (*name_and_type_index, false),
            Constant::InvokeDynamic {
                name_and_type_index,
                ..
            } => (*name_and_type_index, true),
            _ => {
                return Err(VerifyError::MalformedCode {
                    detail: format!("constant {index} is not a method reference"),
                });
            }
        };
        if dynamic != constant_is_dynamic {
            return Err(VerifyError::MalformedCode {
                detail: format!(
                    "constant {index} is not the kind of method reference this opcode needs"
                ),
            });
        }
        let (_, descriptor) = file.constant_pool.name_and_type(name_and_type_index)?;
        let (arguments, result) = method_descriptor_slots(descriptor)?;
        (arguments, result)
    };
    let pop = if receiver {
        argument_slots
            .checked_add(1)
            .ok_or_else(|| VerifyError::MalformedCode {
                detail: "invocation slot count overflows".to_owned(),
            })?
    } else {
        argument_slots
    };
    Ok(Effect::new(pop, return_slots))
}

fn field_slots_of(descriptor: &str) -> Result<u16, VerifyError> {
    Ok(field_type(descriptor)?.0)
}

fn method_descriptor_slots(descriptor: &str) -> Result<(u16, u16), VerifyError> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return Err(malformed_descriptor(descriptor, "expected '('"));
    }

    let mut position = 1usize;
    let mut arguments = 0u16;
    while bytes.get(position) != Some(&b')') {
        if position >= bytes.len() {
            return Err(malformed_descriptor(
                descriptor,
                "missing ')' in method descriptor",
            ));
        }
        let (slots, used) = field_type(&descriptor[position..])?;
        arguments = arguments
            .checked_add(slots)
            .ok_or_else(|| malformed_descriptor(descriptor, "argument slot count overflows"))?;
        position += used;
    }
    position += 1;

    let return_slots = if bytes.get(position) == Some(&b'V') {
        position += 1;
        0
    } else {
        let (slots, used) = field_type(&descriptor[position..])?;
        position += used;
        slots
    };
    if position != bytes.len() {
        return Err(malformed_descriptor(
            descriptor,
            "trailing bytes after the return descriptor",
        ));
    }
    Ok((arguments, return_slots))
}

fn field_type(descriptor: &str) -> Result<(u16, usize), VerifyError> {
    let bytes = descriptor.as_bytes();
    let Some(first) = bytes.first() else {
        return Err(malformed_descriptor(descriptor, "empty field descriptor"));
    };
    match *first {
        b'B' | b'C' | b'F' | b'I' | b'S' | b'Z' => Ok((1, 1)),
        b'J' | b'D' => Ok((2, 1)),
        b'L' => {
            let Some(end) = bytes.iter().position(|byte| *byte == b';') else {
                return Err(malformed_descriptor(
                    descriptor,
                    "object type has no closing ';'",
                ));
            };
            if end <= 1 {
                return Err(malformed_descriptor(descriptor, "object type has no name"));
            }
            Ok((1, end + 1))
        }
        b'[' => {
            let mut component = 1usize;
            while bytes.get(component) == Some(&b'[') {
                component += 1;
            }
            if component >= bytes.len() {
                return Err(malformed_descriptor(
                    descriptor,
                    "array type has no component type",
                ));
            }
            let (_, used) = field_type(&descriptor[component..])?;
            Ok((1, component + used))
        }
        _ => Err(malformed_descriptor(
            descriptor,
            "unknown field-type character",
        )),
    }
}

fn malformed_descriptor(descriptor: &str, detail: &str) -> VerifyError {
    VerifyError::MalformedDescriptor {
        descriptor: descriptor.to_owned(),
        detail: detail.to_owned(),
    }
}

fn validate_exception_table(
    code: &CodeAttribute,
    instructions: &BTreeMap<usize, Instruction>,
) -> Result<(), VerifyError> {
    let code_len = code.bytes.len();
    for handler in &code.exception_table {
        let start = usize::from(handler.start_pc);
        let end = usize::from(handler.end_pc);
        let handler_pc = usize::from(handler.handler_pc);

        if start >= end {
            return Err(VerifyError::InvalidExceptionTable {
                detail: format!("start_pc {start} is not before end_pc {end}"),
            });
        }
        if end > code_len {
            return Err(VerifyError::InvalidExceptionTable {
                detail: format!("end_pc {end} is past the {code_len}-byte code array"),
            });
        }
        if !instructions.contains_key(&start) {
            return Err(VerifyError::InvalidExceptionTable {
                detail: format!("start_pc {start} is not an instruction boundary"),
            });
        }
        if end < code_len && !instructions.contains_key(&end) {
            return Err(VerifyError::InvalidExceptionTable {
                detail: format!("end_pc {end} is not an instruction boundary"),
            });
        }
        if !instructions.contains_key(&handler_pc) {
            return Err(VerifyError::InvalidExceptionTable {
                detail: format!("handler_pc {handler_pc} is not an instruction boundary"),
            });
        }
    }
    Ok(())
}

fn dataflow(
    code: &CodeAttribute,
    instructions: &BTreeMap<usize, Instruction>,
    initial_locals: BTreeSet<u16>,
) -> Result<BTreeMap<usize, VerifyFrame>, VerifyError> {
    if !instructions.contains_key(&0) {
        return Err(VerifyError::MalformedCode {
            detail: "method has no instruction at pc 0".to_owned(),
        });
    }

    let mut frames = BTreeMap::new();
    frames.insert(
        0,
        VerifyFrame {
            stack: 0,
            locals: initial_locals,
        },
    );
    let mut worklist = VecDeque::from([0usize]);

    while let Some(pc) = worklist.pop_front() {
        let instruction =
            instructions
                .get(&pc)
                .ok_or_else(|| VerifyError::InvalidBranchTarget {
                    pc,
                    target: i64::try_from(pc).unwrap_or(i64::MAX),
                    code_len: code.bytes.len(),
                })?;
        let frame = frames.get(&pc).cloned().ok_or(VerifyError::MalformedCode {
            detail: format!("worklist entry pc {pc} has no frame"),
        })?;

        if frame.stack < instruction.effect.pop {
            return Err(VerifyError::StackUnderflow {
                pc,
                needed: instruction.effect.pop,
                found: frame.stack,
            });
        }
        // Compute in 32 bits: a valid frame plus a `dup2`-sized push can exceed
        // u16 before the max_stack check rejects it.
        let stack = u32::from(frame.stack) - u32::from(instruction.effect.pop)
            + u32::from(instruction.effect.push);
        if stack > u32::from(code.max_stack) {
            return Err(VerifyError::StackOverflow {
                pc,
                max_stack: code.max_stack,
                found: stack,
            });
        }
        let stack = stack as u16; // bounded by max_stack, which is a u16

        let mut locals = frame.locals.clone();
        apply_local_access(pc, instruction.local, code.max_locals, &mut locals)?;

        // Every instruction inside a protected range can transfer control to its
        // handler with the exception on the stack and the locals it had assigned.
        for handler in &code.exception_table {
            let start = usize::from(handler.start_pc);
            let end = usize::from(handler.end_pc);
            if start <= pc && pc < end {
                let handler_pc = usize::from(handler.handler_pc);
                if code.max_stack < 1 {
                    return Err(VerifyError::StackOverflow {
                        pc: handler_pc,
                        max_stack: code.max_stack,
                        found: 1,
                    });
                }
                merge(
                    &mut frames,
                    &mut worklist,
                    handler_pc,
                    VerifyFrame {
                        stack: 1,
                        locals: locals.clone(),
                    },
                )?;
            }
        }

        let outgoing = VerifyFrame {
            stack,
            locals: locals.clone(),
        };
        match &instruction.flow {
            Flow::Fallthrough => {
                let next = pc + instruction.len;
                if next == code.bytes.len() {
                    return Err(VerifyError::MalformedCode {
                        detail: format!("control falls off the end after pc {pc}"),
                    });
                }
                merge(&mut frames, &mut worklist, next, outgoing.clone())?;
            }
            Flow::Branch(target) => {
                let target = branch_target(pc, *target, code, instructions)?;
                merge(&mut frames, &mut worklist, target, outgoing.clone())?;
                let next = pc + instruction.len;
                if next == code.bytes.len() {
                    return Err(VerifyError::MalformedCode {
                        detail: format!("conditional branch at pc {pc} falls off the end"),
                    });
                }
                merge(&mut frames, &mut worklist, next, outgoing.clone())?;
            }
            Flow::Goto(target) => {
                let target = branch_target(pc, *target, code, instructions)?;
                merge(&mut frames, &mut worklist, target, outgoing.clone())?;
            }
            Flow::Switch { default, targets } => {
                // A switch has no fallthrough successor: the default and every case
                // target receive the same outgoing frame.
                let default = branch_target(pc, *default, code, instructions)?;
                merge(&mut frames, &mut worklist, default, outgoing.clone())?;
                for target in targets {
                    let target = branch_target(pc, *target, code, instructions)?;
                    merge(&mut frames, &mut worklist, target, outgoing.clone())?;
                }
            }
            Flow::Return | Flow::Throw => {}
        }
    }

    Ok(frames)
}

fn apply_local_access(
    pc: usize,
    access: LocalAccess,
    max_locals: u16,
    locals: &mut BTreeSet<u16>,
) -> Result<(), VerifyError> {
    match access {
        LocalAccess::None => {}
        LocalAccess::Load { index, slots } | LocalAccess::Store { index, slots } => {
            let end = check_local_range(pc, index, slots, max_locals)?;
            if matches!(access, LocalAccess::Load { .. }) {
                for slot in index..end {
                    if !locals.contains(&slot) {
                        return Err(VerifyError::UninitializedLocal { pc, index });
                    }
                }
            } else {
                locals.extend(index..end);
            }
        }
        LocalAccess::Increment { index } => {
            check_local_range(pc, index, 1, max_locals)?;
            if !locals.contains(&index) {
                return Err(VerifyError::UninitializedLocal { pc, index });
            }
        }
    }
    Ok(())
}

fn check_local_range(
    pc: usize,
    index: u16,
    slots: u16,
    max_locals: u16,
) -> Result<u16, VerifyError> {
    match index.checked_add(slots) {
        Some(end) if end <= max_locals => Ok(end),
        _ => Err(VerifyError::InvalidLocal {
            pc,
            index,
            max_locals,
        }),
    }
}

fn merge(
    frames: &mut BTreeMap<usize, VerifyFrame>,
    worklist: &mut VecDeque<usize>,
    pc: usize,
    incoming: VerifyFrame,
) -> Result<(), VerifyError> {
    match frames.entry(pc) {
        Entry::Vacant(entry) => {
            entry.insert(incoming);
            worklist.push_back(pc);
        }
        Entry::Occupied(mut entry) => {
            let existing = entry.get_mut();
            if existing.stack != incoming.stack {
                return Err(VerifyError::StackHeightMismatch {
                    pc,
                    existing: existing.stack,
                    incoming: incoming.stack,
                });
            }
            let merged: BTreeSet<u16> = existing
                .locals
                .intersection(&incoming.locals)
                .copied()
                .collect();
            if merged != existing.locals {
                existing.locals = merged;
                worklist.push_back(pc);
            }
        }
    }
    Ok(())
}

fn branch_target(
    pc: usize,
    target: i64,
    code: &CodeAttribute,
    instructions: &BTreeMap<usize, Instruction>,
) -> Result<usize, VerifyError> {
    let Ok(target) = usize::try_from(target) else {
        return Err(VerifyError::InvalidBranchTarget {
            pc,
            target,
            code_len: code.bytes.len(),
        });
    };
    if !instructions.contains_key(&target) {
        return Err(VerifyError::InvalidBranchTarget {
            pc,
            target: i64::try_from(target).unwrap_or(i64::MAX),
            code_len: code.bytes.len(),
        });
    }
    Ok(target)
}

fn read_u2(data: &[u8], offset: &mut usize, context: &str) -> Result<u16, VerifyError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| VerifyError::MalformedCode {
            detail: format!("{context} offset overflows"),
        })?;
    let slice = data
        .get(*offset..end)
        .ok_or_else(|| VerifyError::MalformedCode {
            detail: format!("{context} at byte {} is truncated", *offset),
        })?;
    *offset = end;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u4(data: &[u8], offset: &mut usize, context: &str) -> Result<u32, VerifyError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| VerifyError::MalformedCode {
            detail: format!("{context} offset overflows"),
        })?;
    let slice = data
        .get(*offset..end)
        .ok_or_else(|| VerifyError::MalformedCode {
            detail: format!("{context} at byte {} is truncated", *offset),
        })?;
    *offset = end;
    Ok(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn operand_u1(bytes: &[u8], offset: &mut usize, pc: usize, opcode: u8) -> Result<u8, VerifyError> {
    let value = bytes
        .get(*offset)
        .copied()
        .ok_or_else(|| truncated(pc, opcode, 1, bytes.len().saturating_sub(*offset)))?;
    *offset += 1;
    Ok(value)
}

fn operand_i1(bytes: &[u8], offset: &mut usize, pc: usize, opcode: u8) -> Result<i8, VerifyError> {
    Ok(operand_u1(bytes, offset, pc, opcode)? as i8)
}

fn operand_u2(bytes: &[u8], offset: &mut usize, pc: usize, opcode: u8) -> Result<u16, VerifyError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| truncated(pc, opcode, 2, 0))?;
    let slice = bytes
        .get(*offset..end)
        .ok_or_else(|| truncated(pc, opcode, 2, bytes.len().saturating_sub(*offset)))?;
    *offset = end;
    Ok(u16::from_be_bytes([slice[0], slice[1]]))
}

fn operand_i2(bytes: &[u8], offset: &mut usize, pc: usize, opcode: u8) -> Result<i16, VerifyError> {
    Ok(operand_u2(bytes, offset, pc, opcode)? as i16)
}

fn operand_i4(bytes: &[u8], offset: &mut usize, pc: usize, opcode: u8) -> Result<i32, VerifyError> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| truncated(pc, opcode, 4, 0))?;
    let slice = bytes
        .get(*offset..end)
        .ok_or_else(|| truncated(pc, opcode, 4, bytes.len().saturating_sub(*offset)))?;
    *offset = end;
    Ok(i32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn truncated(pc: usize, opcode: u8, needed: usize, available: usize) -> VerifyError {
    VerifyError::TruncatedOperand {
        pc,
        opcode,
        needed,
        available,
    }
}
