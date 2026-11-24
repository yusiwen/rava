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
//!
//! M2.7 adds the exception table: `start_pc`/`end_pc`/`handler_pc` are resolved to the
//! same instruction indices at decode time, so handler search at run time is a range
//! check over instruction positions, never a byte-address comparison.
//!
//! M2.17 extends `ldc` to a `CONSTANT_Class` constant (a class literal), adds `ldc_w`,
//! and adds `if_acmpeq`/`if_acmpne` so a fixture can compare two class literals by
//! reference identity. A class constant keeps its name: the class is resolved through
//! the executing frame's loader, so the name cannot be turned into a class id at decode
//! time.
//!
//! M2.18 adds `invokedynamic` (`0xba`): its `CONSTANT_InvokeDynamic` supplies the
//! call-site name, descriptor and bootstrap method index, and the class's
//! `BootstrapMethods` entry is materialized into its typed handle and static arguments
//! here. Decoding therefore reports an out-of-range or malformed bootstrap reference as
//! a typed class-file error, before any execution. `lload`/`fload`/`dload` were added so
//! an assembled call site can pass a long/float/double argument to the concatenation
//! conversion.
//!
//! M2.19 adds `invokeinterface` (`0xb9`): its u2 interface-method reference, u1 argument
//! count and u1 reserved zero byte are decoded, and the interface method runs through the
//! receiver's runtime class or the lambda representation. `pop` (`0x57`) is decoded only
//! because a fixture that discards `List.add`'s boolean return needs it.
//!
//! M2.21 adds the static-field write and the reference-type surface: `putstatic`
//! (`0xb3`), `checkcast` (`0xc0`), `instanceof` (`0xc1`), the numeric `ldc`/`ldc_w`
//! constants, `ldc2_w` (`0x14`) for `long`/`double`, and the `lreturn`/`freturn`/
//! `dreturn` and `lcmp`/`fcmp<op>`/`dcmp<op>` instructions those constants need to be
//! returned and compared. It also adds `tableswitch` (`0xaa`) and `lookupswitch`
//! (`0xab`): their padding is checked against the alignment the JVMS requires, and the
//! default and case targets become instruction indices through the same two-pass
//! boundary map as every other branch. A `lookupswitch` whose keys are not strictly
//! ascending is a decode error, as is a target that is not an instruction boundary.

use std::collections::HashMap;

use rava_classfile::{BootstrapMethodEntry, ClassFile, Constant};
use rava_vm::ArrayKind;

use crate::error::InterpError;

/// One entry of a method's `Code` exception table, resolved to instruction indices.
///
/// `start` is inclusive and `end` is exclusive, the same half-open range as the class
/// file's `start_pc`/`end_pc` pair. `start`, `end` and `handler` are indices into
/// [`DecodedMethod::instructions`], not byte offsets. `catch_type` is the catch class's
/// binary name, or `None` for a catch-all entry (`catch_type = 0`, the shape `finally`
/// compiles to; `finally` duplication itself is not implemented).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionHandler {
    pub start: usize,
    pub end: usize,
    pub handler: usize,
    pub catch_type: Option<String>,
}

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

#[derive(Debug, Clone, PartialEq)]
pub enum Instr {
    Iconst(i32),
    /// `pop`: discard the top operand-stack value.
    Pop,
    ILoad(u8),
    /// `lload`/`lload_<n>`.
    LLoad(u8),
    /// `fload`/`fload_<n>`.
    FLoad(u8),
    /// `dload`/`dload_<n>`.
    DLoad(u8),
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
    /// `ldc`/`ldc_w` of a `CONSTANT_Class`: the class name or descriptor, resolved
    /// through the executing frame's loader when the instruction runs.
    LdcClass(String),
    /// `ldc`/`ldc_w` of a `CONSTANT_Integer`.
    LdcInt(i32),
    /// `ldc`/`ldc_w` of a `CONSTANT_Float`.
    LdcFloat(f32),
    /// `ldc2_w` of a `CONSTANT_Long`.
    LdcLong(i64),
    /// `ldc2_w` of a `CONSTANT_Double`.
    LdcDouble(f64),
    GetStatic {
        class: String,
        name: String,
        descriptor: String,
    },
    /// `putstatic`: write a static field. The owner is resolved through the executing
    /// frame's loader and the value is stored in that class's statics table.
    PutStatic {
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
    /// `invokeinterface`: an interface call through the receiver's runtime class.
    ///
    /// The interface method is resolved on the interface, then selected on the receiver's
    /// runtime class or a default method in the interface hierarchy. A lambda receiver
    /// answers through its metafactory target instead.
    InvokeInterface {
        class: String,
        name: String,
        descriptor: String,
    },
    /// `invokedynamic`: a call site whose target the class's `BootstrapMethods` entry
    /// supplies. The entry is resolved at decode time into its typed handle and static
    /// arguments; execution supports `StringConcatFactory` only.
    InvokeDynamic {
        /// The call-site name from `CONSTANT_InvokeDynamic` (`makeConcatWithConstants`).
        name: String,
        descriptor: String,
        bootstrap: BootstrapMethodEntry,
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
    /// `if_acmpeq`: compare two popped references, then jump to an index; `0xa5`.
    IfACmpEq(usize),
    /// `if_acmpne`: compare two popped references, then jump to an index; `0xa6`.
    IfACmpNe(usize),
    IfNull(usize),
    IfNonNull(usize),
    /// `tableswitch`: `low` is the first case key, so case `i` is `low + i` and its
    /// target is `targets[i]`. `default` is the target for every other key.
    TableSwitch {
        default: usize,
        low: i32,
        targets: Vec<usize>,
    },
    /// `lookupswitch`: `keys` is strictly ascending and `targets[i]` is key `keys[i]`'s
    /// target. `default` is the target for every other key.
    LookupSwitch {
        default: usize,
        keys: Vec<i32>,
        targets: Vec<usize>,
    },
    /// `checkcast`: the class operand's name, resolved through the executing frame's
    /// loader when the instruction runs.
    CheckCast {
        class: String,
    },
    /// `instanceof`: the class operand's name, resolved through the executing frame's
    /// loader when the instruction runs.
    InstanceOf {
        class: String,
    },
    /// `lcmp`: compare two `long` values and push -1, 0 or 1.
    LCmp,
    /// `fcmpl`: compare two `float` values, pushing -1 for NaN.
    FCmpL,
    /// `fcmpg`: compare two `float` values, pushing 1 for NaN.
    FCmpG,
    /// `dcmpl`: compare two `double` values, pushing -1 for NaN.
    DCmpL,
    /// `dcmpg`: compare two `double` values, pushing 1 for NaN.
    DCmpG,
    /// `athrow`: pop a reference and throw it; `0xbf`.
    Athrow,
    IReturn,
    /// `lreturn`: return a `long` value.
    LReturn,
    /// `freturn`: return a `float` value.
    FReturn,
    /// `dreturn`: return a `double` value.
    DReturn,
    AReturn,
    Return,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DecodedMethod {
    pub instructions: Vec<Instr>,
    /// The exception table, in class-file order. The order is the search order: the
    /// first matching entry wins (JVMS §4.7.3).
    pub handlers: Vec<ExceptionHandler>,
    pub max_stack: u16,
    pub max_locals: u16,
}

/// A control-flow target waiting for the second decode pass.
///
/// Every target is recorded as a byte offset and patched to an instruction index after
/// the whole method has been decoded. A switch contributes one default fixup and one
/// fixup per case; a branch contributes one.
#[derive(Debug, Clone, Copy)]
enum Fixup {
    Branch {
        instruction: usize,
        target: usize,
    },
    SwitchDefault {
        instruction: usize,
        target: usize,
    },
    SwitchCase {
        instruction: usize,
        case: usize,
        target: usize,
    },
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
    // `(instruction index, target byte offset)` for every control-flow instruction
    // decoded this pass.
    let mut fixups: Vec<Fixup> = Vec::new();
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
            0x16 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::LLoad(index)
            }
            0x17 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::FLoad(index)
            }
            0x18 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::DLoad(index)
            }
            0x19 => {
                let index = byte(code, pc)?;
                pc += 1;
                Instr::ALoad(index)
            }
            0x1a..=0x1d => Instr::ILoad(opcode - 0x1a),
            0x1e..=0x21 => Instr::LLoad(opcode - 0x1e),
            0x22..=0x25 => Instr::FLoad(opcode - 0x22),
            0x26..=0x29 => Instr::DLoad(opcode - 0x26),
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
            0x57 => Instr::Pop,
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
                fixups.push(Fixup::Branch {
                    instruction: instructions.len(),
                    target: target_offset(opcode_pc, offset)?,
                });
                Instr::If(CONDITIONS[usize::from(opcode - 0x99)], 0)
            }
            0x9f..=0xa4 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push(Fixup::Branch {
                    instruction: instructions.len(),
                    target: target_offset(opcode_pc, offset)?,
                });
                Instr::IfICmp(CONDITIONS[usize::from(opcode - 0x9f)], 0)
            }
            0xa5 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push(Fixup::Branch {
                    instruction: instructions.len(),
                    target: target_offset(opcode_pc, offset)?,
                });
                Instr::IfACmpEq(0)
            }
            0xa6 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push(Fixup::Branch {
                    instruction: instructions.len(),
                    target: target_offset(opcode_pc, offset)?,
                });
                Instr::IfACmpNe(0)
            }
            0xa7 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push(Fixup::Branch {
                    instruction: instructions.len(),
                    target: target_offset(opcode_pc, offset)?,
                });
                Instr::Goto(0)
            }
            0xaa => {
                let table = decode_tableswitch(code, opcode_pc)?;
                pc = table.next_pc;
                let instruction = instructions.len();
                fixups.push(Fixup::SwitchDefault {
                    instruction,
                    target: switch_target(opcode_pc, table.default)?,
                });
                for (case, offset) in table.offsets.iter().enumerate() {
                    fixups.push(Fixup::SwitchCase {
                        instruction,
                        case,
                        target: switch_target(opcode_pc, *offset)?,
                    });
                }
                Instr::TableSwitch {
                    default: 0,
                    low: table.low,
                    targets: vec![0; table.offsets.len()],
                }
            }
            0xab => {
                let lookup = decode_lookupswitch(code, opcode_pc)?;
                pc = lookup.next_pc;
                let instruction = instructions.len();
                fixups.push(Fixup::SwitchDefault {
                    instruction,
                    target: switch_target(opcode_pc, lookup.default)?,
                });
                for (case, offset) in lookup.offsets.iter().enumerate() {
                    fixups.push(Fixup::SwitchCase {
                        instruction,
                        case,
                        target: switch_target(opcode_pc, *offset)?,
                    });
                }
                Instr::LookupSwitch {
                    default: 0,
                    keys: lookup.keys,
                    targets: vec![0; lookup.offsets.len()],
                }
            }
            0xc0 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                Instr::CheckCast {
                    class: resolve_class(file, index)?,
                }
            }
            0xc1 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                Instr::InstanceOf {
                    class: resolve_class(file, index)?,
                }
            }
            0xc6 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push(Fixup::Branch {
                    instruction: instructions.len(),
                    target: target_offset(opcode_pc, offset)?,
                });
                Instr::IfNull(0)
            }
            0xc7 => {
                let offset = branch_offset(code, pc)?;
                pc += 2;
                fixups.push(Fixup::Branch {
                    instruction: instructions.len(),
                    target: target_offset(opcode_pc, offset)?,
                });
                Instr::IfNonNull(0)
            }
            0x94 => Instr::LCmp,
            0x95 => Instr::FCmpL,
            0x96 => Instr::FCmpG,
            0x97 => Instr::DCmpL,
            0x98 => Instr::DCmpG,
            0xac => Instr::IReturn,
            0xad => Instr::LReturn,
            0xae => Instr::FReturn,
            0xaf => Instr::DReturn,
            0xb0 => Instr::AReturn,
            0xb1 => Instr::Return,
            0xbf => Instr::Athrow,
            0x12 => {
                let index = u16::from(byte(code, pc)?);
                pc += 1;
                resolve_ldc(file, index, false)?
            }
            0x13 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                resolve_ldc(file, index, false)?
            }
            0x14 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                resolve_ldc(file, index, true)?
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
            0xb3 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                let (class, name, descriptor) = resolve_field(file, index)?;
                Instr::PutStatic {
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
            0xb9 => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                // The interface call's count operand is the argument-slot count including
                // the receiver (JVMS §6.5.invokeinterface). This interpreter keeps one
                // local per argument rather than widening long/double to two slots, so the
                // count is not used for dispatch; the fourth byte must still be zero.
                let _count = byte(code, pc)?;
                let zero = byte(code, pc + 1)?;
                pc += 2;
                if zero != 0 {
                    return Err(InterpError::Decode(format!(
                        "invokeinterface constant {index} has non-zero reserved byte 0x{zero:02x}"
                    )));
                }
                let (class, name, descriptor) = resolve_method(file, index, true)?;
                Instr::InvokeInterface {
                    class,
                    name,
                    descriptor,
                }
            }
            0xba => {
                let index = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                // JVMS §6.5.invokedynamic: the two bytes after the index are reserved and
                // must be zero. A non-zero value is malformed, not a second operand.
                let reserved = u16::from_be_bytes([byte(code, pc)?, byte(code, pc + 1)?]);
                pc += 2;
                if reserved != 0 {
                    return Err(InterpError::Decode(format!(
                        "invokedynamic constant {index} has non-zero reserved bytes 0x{reserved:04x}"
                    )));
                }
                let (name, descriptor, bootstrap_index) = resolve_invoke_dynamic(file, index)?;
                let bootstrap = file.bootstrap_method(bootstrap_index)?;
                Instr::InvokeDynamic {
                    name,
                    descriptor,
                    bootstrap,
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
    for fixup in fixups {
        match fixup {
            Fixup::Branch {
                instruction,
                target,
            } => {
                let target_index = boundary_index(&boundaries, target, "branch")?;
                instructions[instruction] = match &instructions[instruction] {
                    Instr::Goto(_) => Instr::Goto(target_index),
                    Instr::If(condition, _) => Instr::If(*condition, target_index),
                    Instr::IfICmp(condition, _) => Instr::IfICmp(*condition, target_index),
                    Instr::IfACmpEq(_) => Instr::IfACmpEq(target_index),
                    Instr::IfACmpNe(_) => Instr::IfACmpNe(target_index),
                    Instr::IfNull(_) => Instr::IfNull(target_index),
                    Instr::IfNonNull(_) => Instr::IfNonNull(target_index),
                    _ => {
                        return Err(InterpError::Decode(
                            "branch fixup without a branch instruction".to_owned(),
                        ));
                    }
                };
            }
            Fixup::SwitchDefault {
                instruction,
                target,
            } => {
                let target_index = boundary_index(&boundaries, target, "switch")?;
                match &mut instructions[instruction] {
                    Instr::TableSwitch { default, .. } | Instr::LookupSwitch { default, .. } => {
                        *default = target_index;
                    }
                    _ => {
                        return Err(InterpError::Decode(
                            "switch fixup without a switch instruction".to_owned(),
                        ));
                    }
                }
            }
            Fixup::SwitchCase {
                instruction,
                case,
                target,
            } => {
                let target_index = boundary_index(&boundaries, target, "switch")?;
                match &mut instructions[instruction] {
                    Instr::TableSwitch { targets, .. } | Instr::LookupSwitch { targets, .. } => {
                        let slot = targets.get_mut(case).ok_or_else(|| {
                            InterpError::Decode(format!(
                                "switch case {case} is outside its own jump table"
                            ))
                        })?;
                        *slot = target_index;
                    }
                    _ => {
                        return Err(InterpError::Decode(
                            "switch fixup without a switch instruction".to_owned(),
                        ));
                    }
                }
            }
        }
    }

    // The exception table follows the code array in the `Code` attribute. It is
    // resolved against the same boundaries map, so a handler can only name a real
    // instruction.
    let handlers = decode_handlers(file, attribute_data, end, code.len(), &boundaries)?;

    Ok(DecodedMethod {
        instructions,
        handlers,
        max_stack,
        max_locals,
    })
}

/// Decodes a `Code` attribute's exception table, which starts at byte `offset` (the end
/// of the code array).
///
/// `start_pc` and `handler_pc` must be instruction boundaries; `end_pc` must be a
/// boundary too, or the code length, which is one past the last instruction (JVMS
/// §4.7.3). A table entry that names any other byte, runs backwards, or has a
/// `catch_type` that is not a `CONSTANT_Class` is a decode error, not a silently ignored
/// handler.
fn decode_handlers(
    file: &ClassFile,
    data: &[u8],
    mut offset: usize,
    code_len: usize,
    boundaries: &HashMap<usize, usize>,
) -> Result<Vec<ExceptionHandler>, InterpError> {
    let count = table_u2(data, &mut offset)?;
    let mut handlers = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let start_pc = table_u2(data, &mut offset)?;
        let end_pc = table_u2(data, &mut offset)?;
        let handler_pc = table_u2(data, &mut offset)?;
        let catch_type = table_u2(data, &mut offset)?;

        let start = instruction_index(boundaries, start_pc, "start_pc")?;
        // `end_pc` is exclusive and may equal `code_length`, which is not itself a
        // boundary any instruction starts on.
        let end = if usize::from(end_pc) == code_len {
            boundaries.len()
        } else {
            instruction_index(boundaries, end_pc, "end_pc")?
        };
        let handler = instruction_index(boundaries, handler_pc, "handler_pc")?;
        if start >= end {
            return Err(InterpError::Decode(format!(
                "exception handler covers start_pc {start_pc} to end_pc {end_pc}, which is empty or reversed"
            )));
        }
        let catch_type = if catch_type == 0 {
            None
        } else {
            Some(resolve_class(file, catch_type)?)
        };
        handlers.push(ExceptionHandler {
            start,
            end,
            handler,
            catch_type,
        });
    }
    Ok(handlers)
}

/// An exception-table byte offset as an instruction index.
fn instruction_index(
    boundaries: &HashMap<usize, usize>,
    offset: u16,
    field: &str,
) -> Result<usize, InterpError> {
    boundaries
        .get(&usize::from(offset))
        .copied()
        .ok_or_else(|| {
            InterpError::Decode(format!(
                "exception handler {field} byte {offset} is not an instruction boundary"
            ))
        })
}

/// A control-flow byte target as an instruction index.
fn boundary_index(
    boundaries: &HashMap<usize, usize>,
    target: usize,
    kind: &str,
) -> Result<usize, InterpError> {
    boundaries.get(&target).copied().ok_or_else(|| {
        InterpError::Decode(format!(
            "{kind} target byte {target} is not an instruction boundary"
        ))
    })
}

/// A big-endian `u16` from the `Code` payload, advancing `offset`.
fn table_u2(data: &[u8], offset: &mut usize) -> Result<u16, InterpError> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| InterpError::Decode("Code attribute offset overflows".to_owned()))?;
    let bytes = data.get(*offset..end).ok_or_else(|| {
        InterpError::Decode("Code attribute is truncated inside its exception table".to_owned())
    })?;
    *offset = end;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
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

/// A decoded `tableswitch` operand block, still in byte offsets.
struct Tableswitch {
    default: i32,
    low: i32,
    offsets: Vec<i32>,
    /// The byte offset of the first instruction after the switch.
    next_pc: usize,
}

/// A decoded `lookupswitch` operand block, still in byte offsets.
struct Lookupswitch {
    default: i32,
    keys: Vec<i32>,
    offsets: Vec<i32>,
    /// The byte offset of the first instruction after the switch.
    next_pc: usize,
}

/// The absolute byte offset a `tableswitch`/`lookupswitch` operand targets.
///
/// Switch offsets are signed 32-bit values relative to the opcode's own address, unlike
/// the signed 16-bit offsets of the ordinary branch instructions.
fn switch_target(opcode_pc: usize, offset: i32) -> Result<usize, InterpError> {
    let opcode_pc = isize::try_from(opcode_pc)
        .map_err(|_| InterpError::Decode("code offset does not fit an isize".to_owned()))?;
    let target = opcode_pc
        + isize::try_from(offset).map_err(|_| {
            InterpError::Decode(format!(
                "switch at byte {opcode_pc} has an offset wider than an isize"
            ))
        })?;
    usize::try_from(target).map_err(|_| {
        InterpError::Decode(format!(
            "switch at byte {opcode_pc} targets {target}, before the code start"
        ))
    })
}

/// The first operand byte of a switch, after its alignment padding.
///
/// JVMS §6.5 requires the operand to begin on a four-byte boundary measured from the
/// start of the code array and calls the intervening bytes null padding. A padding byte
/// that is not zero is therefore malformed, not something to ignore.
fn switch_operand_start(
    code: &[u8],
    opcode_pc: usize,
    mnemonic: &str,
) -> Result<usize, InterpError> {
    let after_opcode = opcode_pc + 1;
    let padding = (4 - (after_opcode % 4)) % 4;
    for index in 0..padding {
        let value = byte(code, after_opcode + index)?;
        if value != 0 {
            return Err(InterpError::Decode(format!(
                "{mnemonic} at byte {opcode_pc} has non-zero padding byte 0x{value:02x}"
            )));
        }
    }
    Ok(after_opcode + padding)
}

/// A big-endian signed 32-bit value from the code array.
fn i32_at(code: &[u8], offset: usize) -> Result<i32, InterpError> {
    Ok(i32::from_be_bytes([
        byte(code, offset)?,
        byte(code, offset + 1)?,
        byte(code, offset + 2)?,
        byte(code, offset + 3)?,
    ]))
}

/// Decode a `tableswitch` (`0xaa`) operand block.
///
/// The jump table is one entry per key in `low..=high`; the table must fit in the code
/// array, and `high` below `low` is malformed rather than an empty table.
fn decode_tableswitch(code: &[u8], opcode_pc: usize) -> Result<Tableswitch, InterpError> {
    let operand = switch_operand_start(code, opcode_pc, "tableswitch")?;
    let default = i32_at(code, operand)?;
    let low = i32_at(code, operand + 4)?;
    let high = i32_at(code, operand + 8)?;
    if high < low {
        return Err(InterpError::Decode(format!(
            "tableswitch at byte {opcode_pc} has high {high} below low {low}"
        )));
    }
    let count = i64::from(high) - i64::from(low) + 1;
    let table_start = operand + 12;
    let available = code.len().saturating_sub(table_start);
    let needed = count
        .checked_mul(4)
        .ok_or_else(|| InterpError::Decode("tableswitch jump table overflows".to_owned()))?;
    if needed > i64::try_from(available).unwrap_or(i64::MAX) {
        return Err(InterpError::Decode(format!(
            "tableswitch at byte {opcode_pc} is truncated: {count} jump offsets need {needed} bytes, {available} remain"
        )));
    }
    let count = usize::try_from(count)
        .map_err(|_| InterpError::Decode("tableswitch jump table is too large".to_owned()))?;
    let mut offsets = Vec::with_capacity(count);
    for index in 0..count {
        offsets.push(i32_at(code, table_start + index * 4)?);
    }
    Ok(Tableswitch {
        default,
        low,
        offsets,
        next_pc: table_start + count * 4,
    })
}

/// Decode a `lookupswitch` (`0xab`) operand block.
///
/// The `(match, offset)` pairs must be strictly ascending by `match`: a sorted table is
/// what JVMS §6.5.lookupswitch requires, so an unsorted or duplicate key is a malformed
/// switch, not something to search linearly.
fn decode_lookupswitch(code: &[u8], opcode_pc: usize) -> Result<Lookupswitch, InterpError> {
    let operand = switch_operand_start(code, opcode_pc, "lookupswitch")?;
    let default = i32_at(code, operand)?;
    let pairs = i32_at(code, operand + 4)?;
    if pairs < 0 {
        return Err(InterpError::Decode(format!(
            "lookupswitch at byte {opcode_pc} has a negative pair count {pairs}"
        )));
    }
    let count = usize::try_from(pairs)
        .map_err(|_| InterpError::Decode("lookupswitch pair count is too large".to_owned()))?;
    let pairs_start = operand + 8;
    let available = code.len().saturating_sub(pairs_start);
    let needed = count
        .checked_mul(8)
        .ok_or_else(|| InterpError::Decode("lookupswitch table overflows".to_owned()))?;
    if needed > available {
        return Err(InterpError::Decode(format!(
            "lookupswitch at byte {opcode_pc} is truncated: {count} pairs need {needed} bytes, {available} remain"
        )));
    }
    let mut keys = Vec::with_capacity(count);
    let mut offsets = Vec::with_capacity(count);
    let mut previous: Option<i32> = None;
    for index in 0..count {
        let pair = pairs_start + index * 8;
        let key = i32_at(code, pair)?;
        if let Some(previous) = previous {
            if key <= previous {
                return Err(InterpError::Decode(format!(
                    "lookupswitch at byte {opcode_pc} has key {key} after {previous}: keys must be strictly ascending"
                )));
            }
        }
        previous = Some(key);
        keys.push(key);
        offsets.push(i32_at(code, pair + 4)?);
    }
    Ok(Lookupswitch {
        default,
        keys,
        offsets,
        next_pc: pairs_start + count * 8,
    })
}

/// Resolve `ldc`/`ldc_w`/`ldc2_w`'s constant-pool index to its instruction.
///
/// The one-slot forms (`ldc`, `ldc_w`) decode a `CONSTANT_String` (interned when it
/// executes), a `CONSTANT_Class` (a class literal, resolved through the executing
/// frame's loader), a `CONSTANT_Integer` or a `CONSTANT_Float`. `ldc2_w` decodes a
/// `CONSTANT_Long` or `CONSTANT_Double`. Any other pairing is a typed decode error: a
/// `long`/`double` through `ldc` is not a legal encoding, and this slice has no
/// `MethodType`/`MethodHandle`/`Dynamic` value.
fn resolve_ldc(file: &ClassFile, index: u16, wide: bool) -> Result<Instr, InterpError> {
    let constant = file.constant_pool.get(index)?;
    match (wide, constant) {
        (false, Constant::String { string_index }) => Ok(Instr::LdcString(
            file.constant_pool.utf8(*string_index)?.to_owned(),
        )),
        (false, Constant::Class { name_index }) => Ok(Instr::LdcClass(
            file.constant_pool.utf8(*name_index)?.to_owned(),
        )),
        (false, Constant::Integer(value)) => Ok(Instr::LdcInt(*value)),
        (false, Constant::Float(value)) => Ok(Instr::LdcFloat(*value)),
        (true, Constant::Long(value)) => Ok(Instr::LdcLong(*value)),
        (true, Constant::Double(value)) => Ok(Instr::LdcDouble(*value)),
        (_, other) => Err(InterpError::Decode(format!(
            "{} of an unsupported {} constant",
            if wide { "ldc2_w" } else { "ldc" },
            constant_kind(other)
        ))),
    }
}

/// The class-file name of a constant kind, for an error that names what it rejected.
fn constant_kind(constant: &Constant) -> &'static str {
    match constant {
        Constant::Utf8(_) => "Utf8",
        Constant::Integer(_) => "Integer",
        Constant::Float(_) => "Float",
        Constant::Long(_) => "Long",
        Constant::Double(_) => "Double",
        Constant::Class { .. } => "Class",
        Constant::String { .. } => "String",
        Constant::FieldRef { .. } => "Fieldref",
        Constant::MethodRef { .. } => "Methodref",
        Constant::InterfaceMethodRef { .. } => "InterfaceMethodref",
        Constant::NameAndType { .. } => "NameAndType",
        Constant::MethodHandle { .. } => "MethodHandle",
        Constant::MethodType { .. } => "MethodType",
        Constant::Dynamic { .. } => "Dynamic",
        Constant::InvokeDynamic { .. } => "InvokeDynamic",
        Constant::Module { .. } => "Module",
        Constant::Package { .. } => "Package",
    }
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

/// Resolve a `CONSTANT_InvokeDynamic` to its call-site name, descriptor and bootstrap
/// method index.
///
/// The bootstrap index names an entry of the class's `BootstrapMethods` attribute; it is
/// passed back so `decode` can materialize the typed entry through
/// [`ClassFile::bootstrap_method`], which reports a malformed or out-of-range reference as
/// a typed class-file error.
fn resolve_invoke_dynamic(
    file: &ClassFile,
    index: u16,
) -> Result<(String, String, u16), InterpError> {
    let Constant::InvokeDynamic {
        bootstrap_method_attr_index,
        name_and_type_index,
    } = file.constant_pool.get(index)?
    else {
        return Err(InterpError::Decode(format!(
            "constant {index} is not an InvokeDynamic"
        )));
    };
    let (name, descriptor) = file.constant_pool.name_and_type(*name_and_type_index)?;
    Ok((
        name.to_owned(),
        descriptor.to_owned(),
        *bootstrap_method_attr_index,
    ))
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
