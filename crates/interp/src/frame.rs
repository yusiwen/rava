//! A call frame: locals, operand stack, decoded instructions and the exception table.

use rava_vm::{ClassId, ObjRef, Value};

use crate::error::InterpError;
use crate::instr::{ExceptionHandler, Instr};

#[derive(Debug)]
pub struct Frame {
    pub class: ClassId,
    pub locals: Vec<Value>,
    pub stack: Vec<Value>,
    pub max_stack: usize,
    pub instructions: Vec<Instr>,
    /// The method's exception table; `athrow` and a propagating callee search it in this
    /// order.
    pub handlers: Vec<ExceptionHandler>,
    pub pc: usize,
    /// True for each local slot that is the empty second half of a wide (`long`/`double`)
    /// value.
    ///
    /// JVM locals are slots, and a category-2 value owns two adjacent slots. A frame keeps
    /// one `Value` in the first slot; storing a wide value marks the second slot as a hole:
    /// no load may read it, and it is never a reference root. Storing into the hole clears
    /// it and makes the slot usable again.
    holes: Vec<bool>,
}

impl Frame {
    pub fn new(
        class: ClassId,
        locals: Vec<Value>,
        instructions: Vec<Instr>,
        handlers: Vec<ExceptionHandler>,
        max_stack: u16,
    ) -> Self {
        Self {
            class,
            holes: vec![false; locals.len()],
            locals,
            stack: Vec::with_capacity(usize::from(max_stack)),
            max_stack: usize::from(max_stack),
            instructions,
            handlers,
            pc: 0,
        }
    }

    /// The frame's reference roots: every non-null `ObjRef` in its locals and on its
    /// operand stack. This is the only frame model; the interpreter publishes this
    /// snapshot at safepoint requests and call boundaries.
    ///
    /// A hole holds the placeholder of a wide value's second slot, not a value, so it is
    /// skipped even though the placeholder is never a reference.
    pub fn roots(&self) -> Vec<ObjRef> {
        let locals = self
            .locals
            .iter()
            .enumerate()
            .filter(|(index, _)| !self.is_hole(*index))
            .filter_map(|(_, value)| match value {
                Value::Ref(reference) if !reference.is_null() => Some(*reference),
                _ => None,
            });
        locals
            .chain(self.stack.iter().filter_map(|value| match value {
                Value::Ref(reference) if !reference.is_null() => Some(*reference),
                _ => None,
            }))
            .collect()
    }

    /// Read the value in local `index`.
    ///
    /// Reading a hole is a typed error: the slot is the second half of a wide value and
    /// carries no value of its own. The verifier should reject such a load; the
    /// interpreter rejects it too rather than reading the neighbouring slot.
    pub fn local(&self, index: u16) -> Result<Value, InterpError> {
        let slot = usize::from(index);
        if self.is_hole(slot) {
            return Err(InterpError::LocalHole(index));
        }
        self.locals.get(slot).copied().ok_or_else(|| outside(index))
    }

    /// Write local `index`. A store into a hole clears it, so the slot becomes usable.
    pub fn set_local(&mut self, index: u16, value: Value) -> Result<(), InterpError> {
        let slot = usize::from(index);
        let target = self.locals.get_mut(slot).ok_or_else(|| outside(index))?;
        *target = value;
        if let Some(hole) = self.holes.get_mut(slot) {
            *hole = false;
        }
        Ok(())
    }

    /// Write the first slot of a wide local and reserve its second slot as a hole.
    ///
    /// A `long`/`double` store owns two JVM slots, so writing at the last slot of the frame
    /// is a typed bounds error rather than a silently missing reservation.
    pub fn set_wide_local(&mut self, index: u16, value: Value) -> Result<(), InterpError> {
        if usize::from(index) >= self.locals.len() {
            return Err(outside(index));
        }
        let second = index.checked_add(1).ok_or_else(|| outside(index))?;
        if usize::from(second) >= self.locals.len() {
            return Err(outside(second));
        }
        self.set_local(index, value)?;
        self.mark_hole(second)
    }

    /// Mark local `index` as the empty second slot of a wide value.
    pub fn mark_hole(&mut self, index: u16) -> Result<(), InterpError> {
        let slot = usize::from(index);
        let placeholder = self.locals.get_mut(slot).ok_or_else(|| outside(index))?;
        *placeholder = Value::Int(0);
        let hole = self.holes.get_mut(slot).ok_or_else(|| outside(index))?;
        *hole = true;
        Ok(())
    }

    fn is_hole(&self, slot: usize) -> bool {
        self.holes.get(slot).copied().unwrap_or(false)
    }

    pub fn push(&mut self, value: Value) -> Result<(), InterpError> {
        if self.stack.len() >= self.max_stack {
            return Err(InterpError::Decode(
                "operand stack exceeds max_stack".to_owned(),
            ));
        }
        self.stack.push(value);
        Ok(())
    }

    pub fn pop(&mut self) -> Result<Value, InterpError> {
        self.stack.pop().ok_or(InterpError::StackUnderflow)
    }
}

fn outside(index: u16) -> InterpError {
    InterpError::Decode(format!("local {index} is outside the frame"))
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
            Vec::new(),
            4,
        );
        frame.push(Value::Ref(stacked)).unwrap();

        assert_eq!(frame.roots(), vec![local, stacked]);
    }

    #[test]
    fn a_hole_is_not_a_root_and_a_load_of_it_is_rejected() {
        let reference = ObjRef::new(6, 0);
        let mut frame = Frame::new(
            ClassId(0),
            vec![Value::Long(7), Value::Ref(reference)],
            Vec::new(),
            Vec::new(),
            2,
        );
        frame.mark_hole(1).unwrap();

        assert_eq!(frame.roots(), Vec::new());
        assert!(matches!(frame.local(1), Err(InterpError::LocalHole(1))));
        assert_eq!(frame.local(0).unwrap(), Value::Long(7));

        // A store into the hole clears it and makes the slot usable.
        frame.set_local(1, Value::Ref(reference)).unwrap();
        assert_eq!(frame.roots(), vec![reference]);
        assert_eq!(frame.local(1).unwrap(), Value::Ref(reference));
    }

    #[test]
    fn a_wide_store_reserves_the_next_slot_and_drops_its_root() {
        let reference = ObjRef::new(7, 0);
        let mut frame = Frame::new(
            ClassId(0),
            vec![Value::Ref(reference), Value::Ref(reference)],
            Vec::new(),
            Vec::new(),
            2,
        );

        frame.set_wide_local(0, Value::Long(1)).unwrap();

        assert_eq!(frame.roots(), Vec::new());
        assert_eq!(frame.local(0).unwrap(), Value::Long(1));
        assert!(matches!(frame.local(1), Err(InterpError::LocalHole(1))));
    }

    #[test]
    fn a_wide_store_at_the_last_slot_is_a_typed_bounds_error() {
        let mut frame = Frame::new(ClassId(0), vec![Value::Int(0)], Vec::new(), Vec::new(), 0);

        assert!(matches!(
            frame.set_wide_local(0, Value::Long(1)),
            Err(InterpError::Decode(message))
                if message.contains("local 1 is outside the frame")
        ));
        assert_eq!(frame.local(0).unwrap(), Value::Int(0));
    }
}
