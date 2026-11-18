//! The values that live in locals and on the operand stack.

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
    /// The Java `null` reference. Index 0 of the heap is a `Null` object, so a reference
    /// is always a valid index (ADR-0004) and "is it null" is one comparison.
    pub const NULL: Self = Self::Ref(ObjRef::NULL);

    /// The zero value for a field of this descriptor (JVMS §2.4 default values).
    pub fn default_for(descriptor: &str) -> Self {
        match descriptor.as_bytes().first() {
            Some(b'J') => Self::Long(0),
            Some(b'F') => Self::Float(0.0),
            Some(b'D') => Self::Double(0.0),
            Some(b'L') | Some(b'[') => Self::NULL,
            _ => Self::Int(0),
        }
    }

    pub fn as_int(self) -> Option<i32> {
        match self {
            Self::Int(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_long(self) -> Option<i64> {
        match self {
            Self::Long(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_float(self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_double(self) -> Option<f64> {
        match self {
            Self::Double(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_ref(self) -> Option<ObjRef> {
        match self {
            Self::Ref(value) => Some(value),
            _ => None,
        }
    }
}
