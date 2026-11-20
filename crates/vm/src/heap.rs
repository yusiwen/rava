//! The VM-owned heap: objects are addressed by index, never by pointer (ADR-0004).

use crate::class::ClassId;
use crate::value::Value;

/// A handle into the heap. Index 0 is the `Null` object, so every `ObjRef` is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjRef(pub u32);

impl ObjRef {
    pub const NULL: Self = Self(0);

    pub fn is_null(self) -> bool {
        self.0 == 0
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A heap entry.
///
/// `Str` and `Builder` carry their payload in Rust rather than in Java fields: the class
/// files only need to give javac the signatures, and the native implementations own the
/// state. Instances of ordinary classes keep a field vector indexed by the declaring
/// class's field order (filled in when field access lands).
#[derive(Debug)]
pub enum HeapObject {
    Null,
    /// An unreachable object that was swept. Its index stays valid so a stale reference
    /// cannot dangle; instead the slot may be handed to a later allocation, which is why
    /// stale references are a bug the VM cannot detect (a generation counter would).
    Free,
    Instance(Instance),
    Array(Array),
    /// A `LambdaMetafactory` call site's product: the implementation to call and the values it
    /// captured. Calling the functional interface method on it appends the call's arguments.
    Lambda(Lambda),
    /// A `java.lang.reflect.Field`: the class and the index of the declared field.
    FieldRef(ClassId, usize),
    /// A `java.lang.reflect.Method`: the class and the index of the declared method.
    MethodRef(ClassId, usize),
    /// A `java.lang.reflect.Constructor`: the class and the index of the declared method.
    ConstructorRef(ClassId, usize),
}

/// What an array holds. `Byte` doubles for `boolean`, as JVMS §2.2 does for the storage
/// size; `baload` on a boolean array yields 0 or 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayKind {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
    Reference,
}

impl ArrayKind {
    /// The value a freshly allocated element holds (JVMS §2.4 default values).
    pub fn default_value(self) -> Value {
        match self {
            Self::Long => Value::Long(0),
            Self::Float => Value::Float(0.0),
            Self::Double => Value::Double(0.0),
            Self::Reference => Value::NULL,
            Self::Boolean | Self::Byte | Self::Char | Self::Short | Self::Int => Value::Int(0),
        }
    }

    /// `baload` reads a boolean array as bytes; every other kind must match exactly.
    pub fn accepts(self, requested: Self) -> bool {
        match requested {
            Self::Byte => matches!(self, Self::Byte | Self::Boolean),
            other => self == other,
        }
    }
}

#[derive(Debug)]
pub struct Lambda {
    /// The functional interface the lambda implements: the return type of its call site
    /// (`()LPattern$CharPredicate;`). It is the lambda's runtime class for dispatch and
    /// `instanceof`.
    pub interface: ClassId,
    /// The interface method the lambda implements (`test`, `(I)Z`). A call to any *other*
    /// method on the lambda (an interface default such as `union`) must dispatch normally.
    pub sam_name: String,
    pub sam_descriptor: String,
    pub target_class: ClassId,
    pub target_method: usize,
    pub captured: Vec<Value>,
}

#[derive(Debug)]
pub struct Array {
    pub kind: ArrayKind,
    pub elements: Vec<Value>,
    /// The array's class descriptor: `[I`, `[Ljava/lang/String;`, `[[I`. It is what
    /// `Object.getClass` and `instanceof`/`checkcast` name the array by; without it a heap
    /// array has no runtime class and every array `checkcast` fails.
    ///
    /// `None` is only for an array a unit test allocates by hand without a class; every array
    /// the interpreter or the VM allocates carries a descriptor.
    pub class_name: Option<String>,
}

#[derive(Debug)]
pub struct Instance {
    pub class: ClassId,
    pub fields: Vec<Value>,
}
