//! Errors from decoding and executing bytecode.

use std::fmt;

use rava_vm::VmError;

#[derive(Debug)]
pub enum InterpError {
    ClassFile(rava_classfile::Error),
    Vm(VmError),
    Decode(String),
    MethodNotFound {
        class: String,
        name: String,
        descriptor: String,
    },
    NativeOrAbstract {
        name: String,
        descriptor: String,
    },
    StackUnderflow,
    WrongValue,
    BadDescriptor(String),
}

impl fmt::Display for InterpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClassFile(error) => write!(f, "class-file error: {error}"),
            Self::Vm(error) => write!(f, "runtime error: {error}"),
            Self::Decode(detail) => write!(f, "decode error: {detail}"),
            Self::MethodNotFound {
                class,
                name,
                descriptor,
            } => write!(f, "method {class}.{name}{descriptor} not found"),
            Self::NativeOrAbstract { name, descriptor } => {
                write!(f, "method {name}{descriptor} has no bytecode")
            }
            Self::StackUnderflow => write!(f, "operand stack underflow"),
            Self::WrongValue => write!(f, "operand has the wrong type"),
            Self::BadDescriptor(descriptor) => write!(f, "unsupported descriptor {descriptor}"),
        }
    }
}

impl std::error::Error for InterpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ClassFile(error) => Some(error),
            Self::Vm(error) => Some(error),
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
