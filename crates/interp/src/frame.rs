//! A call frame: locals, operand stack and decoded instructions.

use rava_vm::{ClassId, ObjRef, Value};

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

    /// The frame's reference roots: every non-null `ObjRef` in its locals and on its
    /// operand stack. This is the only frame model; the interpreter publishes this
    /// snapshot at safepoint requests and call boundaries.
    pub fn roots(&self) -> Vec<ObjRef> {
        self.locals
            .iter()
            .chain(self.stack.iter())
            .filter_map(|value| match value {
                Value::Ref(reference) if !reference.is_null() => Some(*reference),
                _ => None,
            })
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_cover_locals_and_stack_and_skip_nulls() {
        let local = ObjRef::new(4, 1);
        let stacked = ObjRef::new(5, 0);
        let mut frame = Frame::new(
            ClassId(0),
            vec![Value::Ref(local), Value::Ref(ObjRef::NULL), Value::Int(7)],
            Vec::new(),
            4,
        );
        frame.push(Value::Ref(stacked)).unwrap();

        assert_eq!(frame.roots(), vec![local, stacked]);
    }
}
