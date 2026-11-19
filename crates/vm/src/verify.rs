//! A first bytecode verifier (G1).
//!
//! Three checks, each of which a malformed class file can violate:
//!
//! 1. **Stack depth**: the value-stack depth at every reachable instruction, derived from the
//!    instructions' effects, must never go negative and must never exceed `max_stack`.
//! 2. **Control flow**: every branch target must be a real instruction, and control must not fall
//!    off the end of the code array.
//! 3. **Locals**: every load, store and `iinc` must name a slot below `max_locals`.
//!
//! What this is **not**: JVMS §4.10.1's type-checking verifier. That one tracks a type per stack
//! slot and merges types at join points, which needs the decoded form to carry operand types — and
//! [`Instr`] deliberately does not (ADR-0003). The depth check below therefore counts *values*
//! rather than slots: a `long` occupies one entry here and two in the class file's accounting, so
//! the check is conservative in one direction and blind in the other. ADR-0014 records the trade.

use crate::code::{Code, Instr};
use crate::error::VmError;

/// One instruction's effect on the value stack, in values (not slots).
struct Effect {
    pop: usize,
    push: usize,
}

fn effect(instr: &Instr, code: &Code, class: &crate::Class) -> Result<Effect, VmError> {
    let pair = |pop: usize, push: usize| Effect { pop, push };
    Ok(match instr {
        Instr::Nop => pair(0, 0),
        Instr::AconstNull
        | Instr::Iconst(_)
        | Instr::Lconst(_)
        | Instr::Fconst(_)
        | Instr::Dconst(_)
        | Instr::Ldc(_)
        | Instr::Ldc2(_)
        | Instr::Bipush(_)
        | Instr::Sipush(_) => pair(0, 1),
        Instr::Iload(_) | Instr::Lload(_) | Instr::Fload(_) | Instr::Dload(_) | Instr::Aload(_) => {
            pair(0, 1)
        }
        Instr::Istore(_)
        | Instr::Lstore(_)
        | Instr::Fstore(_)
        | Instr::Dstore(_)
        | Instr::Astore(_) => pair(1, 0),
        Instr::Pop => pair(1, 0),
        // `pop2` removes two slots, or one value when that value is a long/double. The decoded
        // form cannot tell, so the conservative reading is two values.
        Instr::Pop2 => pair(2, 0),
        Instr::Dup => pair(1, 2),
        // Shifts pop a value and a shift distance; the rest pop two values. Both are two.
        Instr::BinOp(_) => pair(2, 1),
        Instr::UnOp(_) => pair(1, 1),
        Instr::Convert(_) => pair(1, 1),
        Instr::Compare(_) => pair(2, 1),
        Instr::Iinc { .. } => pair(0, 0),
        Instr::IfEq(_)
        | Instr::IfNe(_)
        | Instr::IfLt(_)
        | Instr::IfGe(_)
        | Instr::IfGt(_)
        | Instr::IfLe(_)
        | Instr::IfNull(_)
        | Instr::IfNonNull(_) => pair(1, 0),
        Instr::IfICmpEq(_)
        | Instr::IfICmpNe(_)
        | Instr::IfICmpLt(_)
        | Instr::IfICmpGe(_)
        | Instr::IfICmpGt(_)
        | Instr::IfICmpLe(_)
        | Instr::IfACmpEq(_)
        | Instr::IfACmpNe(_) => pair(2, 0),
        Instr::Goto(_) => pair(0, 0),
        Instr::TableSwitch(index) | Instr::LookupSwitch(index) => {
            let _ = code.switches.get(usize::from(*index));
            pair(1, 0)
        }
        Instr::ArrayLength => pair(1, 1),
        Instr::ArrayLoad(_) => pair(2, 1),
        Instr::ArrayStore(_) => pair(3, 0),
        Instr::NewArray(_) | Instr::ANewArray(_) => pair(1, 1),
        Instr::MultiANewArray { dimensions, .. } => pair(usize::from(*dimensions), 1),
        Instr::GetStatic(index) => {
            field_of(class, *index)?;
            pair(0, 1)
        }
        Instr::PutStatic(index) => {
            field_of(class, *index)?;
            pair(1, 0)
        }
        Instr::GetField(index) => {
            field_of(class, *index)?;
            pair(1, 1)
        }
        Instr::PutField(index) => {
            field_of(class, *index)?;
            pair(2, 0)
        }
        Instr::New(_) => pair(0, 1),
        Instr::InvokeVirtual(index) | Instr::InvokeSpecial(index) | Instr::InvokeStatic(index) => {
            let descriptor = method_descriptor(class, *index)?;
            let pops = crate::resolve::parameter_count(&descriptor)
                + usize::from(!matches!(instr, Instr::InvokeStatic(_)));
            pair(pops, usize::from(!descriptor.ends_with('V')))
        }
        Instr::InvokeInterface(index) => {
            let descriptor = method_descriptor(class, *index)?;
            pair(
                crate::resolve::parameter_count(&descriptor) + 1,
                usize::from(!descriptor.ends_with('V')),
            )
        }
        Instr::InvokeDynamic(index) => {
            let descriptor = indy_descriptor(class, *index)?;
            pair(
                crate::resolve::parameter_count(&descriptor),
                usize::from(!descriptor.ends_with('V')),
            )
        }
        Instr::Athrow => pair(1, 0),
        Instr::MonitorEnter | Instr::MonitorExit => pair(1, 0),
        Instr::InstanceOf(_) => pair(1, 1),
        Instr::CheckCast(_) => pair(0, 0),
        Instr::Ireturn | Instr::Lreturn | Instr::Freturn | Instr::Dreturn | Instr::Areturn => {
            pair(1, 0)
        }
        Instr::Return => pair(0, 0),
    })
}

fn field_of(class: &crate::Class, index: u16) -> Result<String, VmError> {
    Ok(crate::resolve::field_ref(class, index)?.descriptor)
}

fn method_descriptor(class: &crate::Class, index: u16) -> Result<String, VmError> {
    Ok(crate::resolve::method_ref(class, index)?.descriptor)
}

fn indy_descriptor(class: &crate::Class, index: u16) -> Result<String, VmError> {
    crate::resolve::indy_descriptor(class, index)
}

/// Instructions after which control does not continue to the next instruction.
fn terminates(instr: &Instr) -> bool {
    matches!(
        instr,
        Instr::Athrow
            | Instr::Ireturn
            | Instr::Lreturn
            | Instr::Freturn
            | Instr::Dreturn
            | Instr::Areturn
            | Instr::Return
            | Instr::Goto(_)
            | Instr::TableSwitch(_)
            | Instr::LookupSwitch(_)
    )
}

/// Every successor instruction index.
fn successors(instr: &Instr, index: usize, code: &Code) -> Result<Vec<usize>, VmError> {
    let one = |target: u32| vec![target as usize];
    Ok(match instr {
        Instr::Goto(target) => one(*target),
        Instr::IfEq(target)
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
        | Instr::IfNonNull(target) => vec![*target as usize, index + 1],
        Instr::TableSwitch(table) | Instr::LookupSwitch(table) => {
            let table = code
                .switches
                .get(usize::from(*table))
                .ok_or_else(|| VmError::BadConstantPool(format!("missing switch {table}")))?;
            let mut targets = vec![table.default as usize];
            targets.extend(table.cases.iter().map(|(_, target)| *target as usize));
            targets
        }
        Instr::Athrow
        | Instr::Ireturn
        | Instr::Lreturn
        | Instr::Freturn
        | Instr::Dreturn
        | Instr::Areturn
        | Instr::Return => Vec::new(),
        // Falling past the last instruction is reported by the caller with a better message.
        _ if index + 1 >= code.instrs.len() => Vec::new(),
        _ => vec![index + 1],
    })
}

/// Verifies one method body. `Ok(())` means the three checks above hold.
pub fn verify(code: &Code, class: &crate::Class, qualified: &str) -> Result<(), VmError> {
    let count = code.instrs.len();
    if count == 0 {
        return Err(VmError::BadConstantPool(format!(
            "{qualified} has an empty body"
        )));
    }

    let mut depth: Vec<Option<usize>> = vec![None; count];
    depth[0] = Some(0);
    let mut worklist = vec![0usize];

    // A handler is entered with exactly one reference on the stack: the throwable (JVMS §4.10.1.6).
    // Seeding it is what makes a handler body that assumes an empty stack fail, which is the one
    // type rule a depth checker can afford (ADR-0014).
    for handler in &code.handlers {
        let start = handler.start;
        let end = handler.end;
        let target = handler.handler;
        if start >= end || end > count || target >= count {
            return Err(VmError::BadConstantPool(format!(
                "{qualified}: handler [{start}..{end}) -> {target} does not name real instructions"
            )));
        }
        match depth[target] {
            None => {
                depth[target] = Some(1);
                worklist.push(target);
            }
            Some(existing) if existing != 1 => {
                return Err(VmError::BadConstantPool(format!(
                    "{qualified}: handler at pc {} is entered with stack depth {existing}, expected 1",
                    code.pc(target)
                )));
            }
            Some(_) => {}
        }
    }

    while let Some(index) = worklist.pop() {
        let before = depth[index].expect("a queued instruction has a depth");
        let instruction = &code.instrs[index];
        check_locals(instruction, code, qualified)?;

        let effect = effect(instruction, code, class)?;
        if before < effect.pop {
            return Err(VmError::BadConstantPool(format!(
                "{qualified}: stack underflow at pc {} ({instruction:?} needs {}, has {before})",
                code.pc(index),
                effect.pop
            )));
        }
        let after = before - effect.pop + effect.push;
        if after > code.max_stack {
            return Err(VmError::BadConstantPool(format!(
                "{qualified}: stack depth {after} at pc {} exceeds max_stack {}",
                code.pc(index),
                code.max_stack
            )));
        }

        for successor in successors(instruction, index, code)? {
            if successor >= count {
                return Err(VmError::BadConstantPool(format!(
                    "{qualified}: control flow leaves the code array (pc {} -> {successor})",
                    code.pc(index)
                )));
            }
            match depth[successor] {
                None => {
                    depth[successor] = Some(after);
                    worklist.push(successor);
                }
                // Two paths to one instruction must agree on the stack shape; JVMS §4.9.1 makes an
                // inconsistent depth a verification failure, and it is the cheapest way to catch a
                // merge that a depth-only analysis can see.
                Some(existing) if existing != after => {
                    return Err(VmError::BadConstantPool(format!(
                        "{qualified}: inconsistent stack depth at pc {} ({existing} vs {after})",
                        code.pc(successor)
                    )));
                }
                Some(_) => {}
            }
        }

        // Falling off the end is only legal after a terminal instruction.
        if !terminates(instruction) && index + 1 >= count {
            return Err(VmError::BadConstantPool(format!(
                "{qualified}: control falls off the end at pc {}",
                code.pc(index)
            )));
        }
    }
    Ok(())
}

/// Locals: a load, store or `iinc` may only name a slot below `max_locals`.
fn check_locals(instruction: &Instr, code: &Code, qualified: &str) -> Result<(), VmError> {
    let slot = match instruction {
        Instr::Iload(slot)
        | Instr::Lload(slot)
        | Instr::Fload(slot)
        | Instr::Dload(slot)
        | Instr::Aload(slot)
        | Instr::Istore(slot)
        | Instr::Lstore(slot)
        | Instr::Fstore(slot)
        | Instr::Dstore(slot)
        | Instr::Astore(slot)
        | Instr::Iinc { slot, .. } => usize::from(*slot),
        _ => return Ok(()),
    };
    if slot >= code.max_locals {
        return Err(VmError::BadConstantPool(format!(
            "{qualified}: local {slot} at pc {} is outside max_locals {}",
            code.pc(0),
            code.max_locals
        )));
    }
    Ok(())
}

/// The stack effect of one instruction, in values `(pop, push)`.
///
/// Exposed for the disassembler, which prints the same numbers the verifier uses — so a reader of
/// `rava disasm` sees the table that decides whether a method is accepted.
pub fn stack_effect(
    instr: &Instr,
    code: &Code,
    class: &crate::Class,
) -> Result<(usize, usize), VmError> {
    let effect = effect(instr, code, class)?;
    Ok((effect.pop, effect.push))
}

/// The checks this verifier performs, for the CLI to print.
pub fn checks() -> &'static [&'static str] {
    &[
        "stack depth against max_stack",
        "branch targets and fall-through",
        "local variable indices",
    ]
}
