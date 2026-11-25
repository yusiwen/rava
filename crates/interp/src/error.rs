//! Errors from decoding and executing bytecode.

use std::fmt;

use rava_vm::{ArrayKind, MonitorError, ObjRef, VmError};

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
    /// A static field opcode named a field that is not `ACC_STATIC`.
    InstanceFieldAccess {
        class: String,
        name: String,
        descriptor: String,
    },
    /// A `putstatic` value's kind does not match the field's descriptor.
    FieldValueMismatch {
        class: String,
        name: String,
        descriptor: String,
    },
    /// `checkcast` rejected a value that is not assignable to its target type.
    ///
    /// The JVM throws a `ClassCastException`; this slice has no exception-object
    /// synthesis, so the mismatch is a typed error instead.
    ClassCast {
        expected: String,
        found: String,
    },
    NotAssignable {
        expected: String,
        found: String,
    },
    NativeOrAbstract {
        class: String,
        name: String,
        descriptor: String,
    },
    /// A lambda invocation or metafactory bootstrap did not match the shape the lambda
    /// object recorded (wrong SAM name/descriptor, target arity or value types).
    ///
    /// These are VM-shape errors, not Java exceptions: the slice has no exception-object
    /// synthesis, so a malformed lambda is reported as a typed error.
    Lambda(String),
    /// An array instruction read or wrote past the end, or used a negative index.
    ArrayIndexOutOfBounds {
        index: i32,
        length: usize,
    },
    /// An array instruction's element type does not match the array's `ArrayKind`.
    ///
    /// `expected` is the element name the opcode addresses; `baload`/`bastore` report
    /// `"boolean or byte"` because they accept both kinds.
    ArrayKindMismatch {
        expected: &'static str,
        found: ArrayKind,
    },
    /// `newarray`/`anewarray`/`multianewarray` was given a negative length.
    NegativeArraySize(i32),
    /// `athrow` popped a null reference.
    ///
    /// The JVM would raise a `NullPointerException`; this slice has no
    /// exception-object synthesis, so the condition is a typed error instead.
    NullThrow,
    /// `athrow` popped a value that is not a reference.
    NonReferenceThrow,
    /// A throwable left the frame that raised it.
    ///
    /// Carries the object `athrow` popped, unchanged: the frame's exception table did
    /// not catch it, and every caller that can catch it receives this same handle. An
    /// error surfacing from a top-level call is an uncaught exception.
    Thrown(ObjRef),
    StackUnderflow,
    WrongValue,
    /// A local load named the second slot of a wide (`long`/`double`) value.
    ///
    /// That slot is a hole: it holds no value of its own, and the verifier should reject
    /// the load. The interpreter reports a typed error rather than returning a stale
    /// neighbour.
    LocalHole(u16),
    /// The argument values handed to a method disagree with its descriptor.
    ArgumentCount {
        expected: usize,
        found: usize,
    },
    BadDescriptor(String),
    HeldLock(rava_vm::LockHeld),
    /// A monitor instruction failed: a null reference or an unlock this thread does not
    /// own. The JVM raises `NullPointerException`/`IllegalMonitorStateException`; this
    /// slice reports a typed error instead.
    Monitor(rava_vm::MonitorError),
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
            Self::InstanceFieldAccess {
                class,
                name,
                descriptor,
            } => write!(
                f,
                "instance field {class}.{name}:{descriptor} was accessed with a static field opcode"
            ),
            Self::FieldValueMismatch {
                class,
                name,
                descriptor,
            } => write!(
                f,
                "value on the operand stack does not match field {class}.{name}:{descriptor}"
            ),
            Self::ClassCast { expected, found } => {
                write!(f, "cannot cast {found} to {expected}")
            }
            Self::NotAssignable { expected, found } => {
                write!(f, "{found} is not assignable to {expected}")
            }
            Self::NativeOrAbstract {
                class,
                name,
                descriptor,
            } => {
                write!(f, "method {class}.{name}{descriptor} has no bytecode")
            }
            Self::Lambda(detail) => write!(f, "lambda error: {detail}"),
            Self::ArrayIndexOutOfBounds { index, length } => {
                write!(
                    f,
                    "array index {index} is outside an array of length {length}"
                )
            }
            Self::ArrayKindMismatch { expected, found } => {
                write!(f, "array kind mismatch: expected {expected}, found {found}")
            }
            Self::NegativeArraySize(size) => write!(f, "negative array size {size}"),
            Self::NullThrow => write!(f, "athrow of a null reference"),
            Self::NonReferenceThrow => write!(f, "athrow of a non-reference value"),
            Self::Thrown(throwable) => write!(f, "uncaught exception {throwable}"),
            Self::StackUnderflow => write!(f, "operand stack underflow"),
            Self::WrongValue => write!(f, "operand has the wrong type"),
            Self::LocalHole(index) => {
                write!(f, "local {index} is the empty second slot of a wide value")
            }
            Self::ArgumentCount { expected, found } => {
                write!(
                    f,
                    "method expects {expected} argument value(s), found {found}"
                )
            }
            Self::Monitor(MonitorError::Null) => {
                write!(f, "monitor instruction on a null reference")
            }
            Self::Monitor(MonitorError::NotOwned) => {
                write!(
                    f,
                    "monitorexit on a monitor the current thread does not own"
                )
            }
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
        match error {
            // A reflective invocation carries a Java throwable across the native ABI as
            // `VmError::Thrown`; at the interpreter boundary it becomes the same
            // `Thrown` the caller's exception table already understands.
            VmError::Thrown(throwable) => Self::Thrown(throwable),
            other => Self::Vm(other),
        }
    }
}

impl From<rava_vm::HeapError> for InterpError {
    fn from(error: rava_vm::HeapError) -> Self {
        Self::Heap(error)
    }
}

impl From<rava_vm::MonitorError> for InterpError {
    fn from(error: rava_vm::MonitorError) -> Self {
        Self::Monitor(error)
    }
}

impl From<rava_vm::LockHeld> for InterpError {
    fn from(error: rava_vm::LockHeld) -> Self {
        Self::HeldLock(error)
    }
}
