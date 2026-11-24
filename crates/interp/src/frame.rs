//! A call frame: locals, operand stack and decoded instructions.

use rava_vm::{ClassId, Value};

use crate::instr::Instr;

#[derive(Debug)]
pub struct Frame {
    pub class: ClassId,
    pub locals: Vec<Value>,
    pub stack: Vec<Value>,
    pub max_stack: usize,
    pub instructions: Vec<Instr>,
    pub pc: usize,
}

impl Frame {
    pub fn new(
        class: ClassId,
        locals: Vec<Value>,
        instructions: Vec<Instr>,
        max_stack: u16,
    ) -> Self {
        Self {
            class,
            locals,
            stack: Vec::with_capacity(usize::from(max_stack)),
            max_stack: usize::from(max_stack),
            instructions,
            pc: 0,
        }
    }

    pub fn push(&mut self, value: Value) -> Result<(), crate::error::InterpError> {
        if self.stack.len() >= self.max_stack {
            return Err(crate::error::InterpError::Decode(
                "operand stack exceeds max_stack".to_owned(),
            ));
        }
        self.stack.push(value);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<Value, crate::error::InterpError> {
        self.stack
            .pop()
            .ok_or(crate::error::InterpError::StackUnderflow)
    }
}
