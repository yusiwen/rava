//! Errors from decoding and executing bytecode.

use std::fmt;

use rava_vm::VmError;

#[derive(Debug)]
pub enum InterpError {
    ClassFile(rava_classfile::Error),
    Vm(VmError),
    Heap(rava_vm::HeapError),
    Decode(String),
    MethodNotFound {
        class: String,
        name: String,
        descriptor: String,
    },
    FieldNotFound {
        class: String,
        name: String,
        descriptor: String,
    },
    FieldDescriptorMismatch {
        class: String,
        name: String,
        found: String,
    },
    StaticFieldAccess {
        class: String,
        name: String,
        descriptor: String,
    },
    NotAssignable {
        expected: String,
        found: String,
    },
    NativeOrAbstract {
        name: String,
        descriptor: String,
    },
    StackUnderflow,
    WrongValue,
    BadDescriptor(String),
    HeldLock(rava_vm::LockHeld),
}

impl fmt::Display for InterpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClassFile(error) => write!(f, "class-file error: {error}"),
            Self::Vm(error) => write!(f, "runtime error: {error}"),
            Self::Heap(error) => write!(f, "heap error: {error}"),
            Self::Decode(detail) => write!(f, "decode error: {detail}"),
            Self::MethodNotFound {
                class,
                name,
                descriptor,
            } => write!(f, "method {class}.{name}{descriptor} not found"),
            Self::FieldNotFound {
                class,
                name,
                descriptor,
            } => write!(f, "field {class}.{name}:{descriptor} not found"),
            Self::FieldDescriptorMismatch { class, name, found } => write!(
                f,
                "field {class}.{name} has descriptor {found}, not the requested one"
            ),
            Self::StaticFieldAccess {
                class,
                name,
                descriptor,
            } => write!(
                f,
                "static field {class}.{name}:{descriptor} was accessed with an instance field opcode"
            ),
            Self::NotAssignable { expected, found } => {
                write!(f, "{found} is not assignable to {expected}")
            }
            Self::NativeOrAbstract { name, descriptor } => {
                write!(f, "method {name}{descriptor} has no bytecode")
            }
            Self::StackUnderflow => write!(f, "operand stack underflow"),
            Self::WrongValue => write!(f, "operand has the wrong type"),
            Self::BadDescriptor(descriptor) => write!(f, "unsupported descriptor {descriptor}"),
            Self::HeldLock(error) => write!(f, "call-boundary invariant violated: {error}"),
        }
    }
}

impl std::error::Error for InterpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ClassFile(error) => Some(error),
            Self::Vm(error) => Some(error),
            Self::Heap(error) => Some(error),
            Self::HeldLock(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rava_classfile::Error> for InterpError {
    fn from(error: rava_classfile::Error) -> Self {
        Self::ClassFile(error)
    }
}

impl From<VmError> for InterpError {
    fn from(error: VmError) -> Self {
        Self::Vm(error)
    }
}

impl From<rava_vm::HeapError> for InterpError {
    fn from(error: rava_vm::HeapError) -> Self {
        Self::Heap(error)
    }
}

impl From<rava_vm::LockHeld> for InterpError {
    fn from(error: rava_vm::LockHeld) -> Self {
        Self::HeldLock(error)
    }
}
