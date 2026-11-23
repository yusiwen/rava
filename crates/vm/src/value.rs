//! A JVM value in a local slot or a field.
//!
//! References are handles into the VM heap, never host pointers.

use crate::heap::ObjRef;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Ref(ObjRef),
}

impl Value {
    /// The JVM default value for a field descriptor (JVMS §2.4).
    pub fn default_for(descriptor: &str) -> Self {
        match descriptor.as_bytes().first().copied() {
            Some(b'J') => Self::Long(0),
            Some(b'F') => Self::Float(0.0),
            Some(b'D') => Self::Double(0.0),
            Some(b'L' | b'[') => Self::Ref(ObjRef::NULL),
            _ => Self::Int(0),
        }
    }

    pub fn as_ref(self) -> Option<ObjRef> {
        match self {
            Self::Ref(reference) => Some(reference),
            _ => None,
        }
    }

    pub fn as_int(self) -> Option<i32> {
        match self {
            Self::Int(value) => Some(value),
            _ => None,
        }
    }
}
