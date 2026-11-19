use std::fmt;

/// Everything the VM can refuse to do.
#[derive(Debug)]
pub enum VmError {
    ClassNotFound(String),
    MethodNotFound {
        class: String,
        name: String,
        descriptor: String,
    },
    FieldNotFound {
        class: String,
        name: String,
    },
    /// An opcode this slice has not implemented yet, named so the gap is actionable.
    UnsupportedInstruction {
        opcode: u8,
        pc: usize,
        method: String,
    },
    /// A constant-pool entry that does not have the shape the instruction needs.
    BadConstantPool(String),
    /// The method body ended in the middle of an instruction.
    TruncatedCode {
        pc: usize,
        method: String,
    },
    StackUnderflow {
        method: String,
        pc: usize,
        needed: usize,
    },
    /// A native method with no registered implementation.
    MissingNative(String),
    /// A Java-level exception in flight. It is control flow, not a VM failure: the handler
    /// search may still catch it in this frame or in a caller's.
    /// An array allocation with a negative size (JVMS §6.5 `newarray`).
    NegativeArraySize {
        length: i32,
    },
    /// A method body the verifier rejected. Separate from `BadConstantPool` so a load failure
    /// and a malformed constant are not reported as the same thing.
    VerifyError {
        detail: String,
    },
    /// A native asking the interpreter to throw: the native names the class and message, and
    /// the interpreter builds the throwable (it owns class loading and the constructor call).
    JavaException {
        class: String,
        message: Option<String>,
    },
    Thrown {
        object: crate::heap::ObjRef,
    },
    /// A feature this VM deliberately defers (M3 and later): reported by name so the gap is
    /// actionable, and never as a misparse (R8).
    Unsupported {
        feature: &'static str,
        detail: String,
    },
    /// A native method that refused.
    Native(String),
    /// An operand had the wrong type for the instruction.
    TypeMismatch {
        expected: &'static str,
        found: crate::value::Value,
    },
    /// An array access outside `0..length` (M1 turns this into
    /// `ArrayIndexOutOfBoundsException`).
    IndexOutOfBounds {
        index: i32,
        length: usize,
    },
    /// An arithmetic fault that Java would raise as an exception (M1 turns these into
    /// `ArithmeticException`; until then they abort the run with a message).
    Arithmetic(String),
    ClassFile(rava_classfile::Error),
    /// The host image could not be read (S4). Separate from `ClassNotFound`: an image whose
    /// index is unreadable is a broken installation, not a missing class.
    Image(crate::image::ImageError),
}

impl fmt::Display for VmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClassNotFound(name) => write!(f, "class not found: {name}"),
            Self::MethodNotFound {
                class,
                name,
                descriptor,
            } => write!(f, "method not found: {class}.{name}{descriptor}"),
            Self::FieldNotFound { class, name } => write!(f, "field not found: {class}.{name}"),
            Self::UnsupportedInstruction { opcode, pc, method } => write!(
                f,
                "unsupported opcode 0x{opcode:02X} at pc {pc} in {method}"
            ),
            Self::BadConstantPool(detail) => write!(f, "bad constant pool: {detail}"),
            Self::TruncatedCode { pc, method } => {
                write!(f, "truncated code at pc {pc} in {method}")
            }
            Self::StackUnderflow { method, pc, needed } => write!(
                f,
                "operand stack underflow in {method} at pc {pc}: needed {needed} value(s)"
            ),
            Self::MissingNative(key) => write!(f, "no native implementation for {key}"),
            Self::Thrown { object } => write!(f, "java exception in flight ({object:?})"),
            Self::NegativeArraySize { length } => {
                write!(f, "negative array size: {length}")
            }
            Self::VerifyError { detail } => write!(f, "verification failed: {detail}"),
            Self::JavaException { class, message } => match message {
                Some(message) => write!(f, "{class}: {message}"),
                None => write!(f, "{class}"),
            },
            Self::Unsupported { feature, detail } => {
                write!(f, "unsupported {feature}: {detail}")
            }
            Self::Native(detail) => write!(f, "native method failed: {detail}"),
            Self::TypeMismatch { expected, found } => {
                write!(f, "expected {expected}, found {found:?}")
            }
            Self::IndexOutOfBounds { index, length } => {
                write!(f, "array index {index} out of bounds for length {length}")
            }
            Self::Arithmetic(detail) => write!(f, "arithmetic: {detail}"),
            Self::ClassFile(error) => write!(f, "class file: {error}"),
            Self::Image(error) => write!(f, "image: {error}"),
        }
    }
}

impl std::error::Error for VmError {}

impl From<rava_classfile::Error> for VmError {
    fn from(error: rava_classfile::Error) -> Self {
        Self::ClassFile(error)
    }
}

impl From<crate::image::ImageError> for VmError {
    fn from(error: crate::image::ImageError) -> Self {
        Self::Image(error)
    }
}
