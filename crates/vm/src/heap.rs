//! The VM-owned heap: objects are addressed by a stable handle, never a host pointer.
//!
//! A handle carries a generation. Freeing a slot bumps the generation, so a stale
//! handle is rejected instead of silently addressing a reused slot.

use std::fmt;

use crate::loader::ClassId;
use crate::value::Value;

/// A stable handle into the heap. Index 0 is the null reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjRef {
    index: u32,
    generation: u32,
}

impl ObjRef {
    pub const NULL: Self = Self {
        index: 0,
        generation: 0,
    };

    pub fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub fn is_null(self) -> bool {
        self.index == 0
    }

    pub fn index(self) -> u32 {
        self.index
    }

    pub fn generation(self) -> u32 {
        self.generation
    }
}

impl fmt::Display for ObjRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "obj#{}@{}", self.index, self.generation)
    }
}

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
    pub fn default_value(self) -> Value {
        match self {
            Self::Long => Value::Long(0),
            Self::Float => Value::Float(0.0),
            Self::Double => Value::Double(0.0),
            Self::Reference => Value::Ref(ObjRef::NULL),
            Self::Boolean | Self::Byte | Self::Char | Self::Short | Self::Int => Value::Int(0),
        }
    }

    /// The Java element name used in diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            Self::Boolean => "boolean",
            Self::Byte => "byte",
            Self::Char => "char",
            Self::Short => "short",
            Self::Int => "int",
            Self::Long => "long",
            Self::Float => "float",
            Self::Double => "double",
            Self::Reference => "reference",
        }
    }
}

impl fmt::Display for ArrayKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One instance field's position in a class layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSlot {
    pub name: String,
    pub descriptor: String,
}

/// Instance layout: superclass fields first, then this class's declared fields.
///
/// The prefix property means a field index resolved in a superclass is valid in every
/// subclass. M0.5 builds this from class files; the heap only needs the slots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldLayout {
    slots: Vec<FieldSlot>,
}

impl FieldLayout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_parent(parent: &Self) -> Self {
        Self {
            slots: parent.slots.clone(),
        }
    }

    pub fn push(&mut self, name: impl Into<String>, descriptor: impl Into<String>) -> usize {
        let index = self.slots.len();
        self.slots.push(FieldSlot {
            name: name.into(),
            descriptor: descriptor.into(),
        });
        index
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn slot(&self, name: &str, descriptor: &str) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.name == name && slot.descriptor == descriptor)
    }

    pub fn slots(&self) -> &[FieldSlot] {
        &self.slots
    }

    pub fn defaults(&self) -> Vec<Value> {
        self.slots
            .iter()
            .map(|slot| Value::default_for(&slot.descriptor))
            .collect()
    }
}

#[derive(Debug)]
pub struct Instance {
    pub class: ClassId,
    pub fields: Vec<Value>,
}

#[derive(Debug)]
pub struct Array {
    pub kind: ArrayKind,
    pub elements: Vec<Value>,
}

/// The kind of reflected member a [`ReflectMember`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Field,
    Method,
    Constructor,
}

impl MemberKind {
    /// The internal class name of the member's `java.lang.reflect` class.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Field => "java/lang/reflect/Field",
            Self::Method => "java/lang/reflect/Method",
            Self::Constructor => "java/lang/reflect/Constructor",
        }
    }

    /// The binary name of the member's `java.lang.reflect` class.
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Field => "java.lang.reflect.Field",
            Self::Method => "java.lang.reflect.Method",
            Self::Constructor => "java.lang.reflect.Constructor",
        }
    }
}

/// A reflected field, method or constructor (`java.lang.reflect.Member`).
///
/// The object stores identity only: the declaring class, the member's name and its
/// descriptor. Modifiers, types and parameter lists are read from the class table when
/// a native asks for them, so there is no second copy of the metadata. Member objects
/// are created and cached by [`crate::Shared::reflect_member`], never by a caller
/// allocating directly.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReflectMember {
    pub kind: MemberKind,
    pub class: ClassId,
    pub name: String,
    pub descriptor: String,
}

#[derive(Debug)]
pub enum HeapObject {
    Instance(Instance),
    Array(Array),
    /// A minimal string object for `ldc` and native printing. The class-library
    /// `java.lang.String` representation arrives with the library slice.
    Str(String),
    /// A class object (`java.lang.Class`): one heap object per class identity.
    ///
    /// The heap stores only the identity; the class's name, modifiers, superclass and
    /// flags are read from the class table when a native asks for them. Class objects
    /// are created and cached by [`crate::Shared::class_object`], never by a caller
    /// allocating directly.
    Class(ClassId),
    /// A reflected `Field`, `Method` or `Constructor`: one heap object per member.
    ///
    /// Like a class object, the object holds identity only; the class's member record is
    /// read from the class table on demand. Cached by
    /// [`crate::Shared::reflect_member`], never allocated by a caller directly.
    Reflect(ReflectMember),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeapError {
    NullReference,
    StaleReference {
        reference: ObjRef,
        found_generation: u32,
    },
    IndexOutOfBounds {
        reference: ObjRef,
    },
    FieldOutOfBounds {
        class: ClassId,
        slot: usize,
    },
    WrongObjectKind {
        reference: ObjRef,
        expected: &'static str,
    },
}

impl fmt::Display for HeapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullReference => write!(f, "null reference"),
            Self::StaleReference {
                reference,
                found_generation,
            } => write!(
                f,
                "stale reference {reference} (slot generation is {found_generation})"
            ),
            Self::IndexOutOfBounds { reference } => {
                write!(f, "reference {reference} is outside the heap")
            }
            Self::FieldOutOfBounds { class, slot } => {
                write!(f, "class {} has no field slot {slot}", class.0)
            }
            Self::WrongObjectKind {
                reference,
                expected,
            } => write!(f, "reference {reference} is not {expected}"),
        }
    }
}

impl std::error::Error for HeapError {}

#[derive(Debug)]
struct Slot {
    generation: u32,
    object: Option<HeapObject>,
}

#[derive(Debug)]
pub struct Heap {
    slots: Vec<Slot>,
    free: Vec<u32>,
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

impl Heap {
    pub fn new() -> Self {
        Self {
            slots: vec![Slot {
                generation: 0,
                object: None,
            }],
            free: Vec::new(),
        }
    }

    pub fn allocate_instance(&mut self, class: ClassId, layout: &FieldLayout) -> ObjRef {
        let fields = layout.defaults();
        self.allocate(HeapObject::Instance(Instance { class, fields }))
    }

    pub fn allocate_array(&mut self, kind: ArrayKind, length: usize) -> ObjRef {
        let elements = vec![kind.default_value(); length];
        self.allocate(HeapObject::Array(Array { kind, elements }))
    }

    pub fn allocate_string(&mut self, text: impl Into<String>) -> ObjRef {
        self.allocate(HeapObject::Str(text.into()))
    }

    /// Allocate a class object. Callers go through [`crate::Shared::class_object`],
    /// which caches one object per class identity.
    pub fn allocate_class(&mut self, class: ClassId) -> ObjRef {
        self.allocate(HeapObject::Class(class))
    }

    /// Allocate a reflected member. Callers go through
    /// [`crate::Shared::reflect_member`], which caches one object per member.
    pub fn allocate_reflect(&mut self, member: ReflectMember) -> ObjRef {
        self.allocate(HeapObject::Reflect(member))
    }

    pub fn string(&self, reference: ObjRef) -> Result<&str, HeapError> {
        match self.get(reference)? {
            HeapObject::Str(text) => Ok(text),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a string",
            }),
        }
    }

    /// The class identity behind a class object.
    pub fn class(&self, reference: ObjRef) -> Result<ClassId, HeapError> {
        match self.get(reference)? {
            HeapObject::Class(class) => Ok(*class),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a class object",
            }),
        }
    }

    /// The member identity behind a reflected member object.
    pub fn reflect(&self, reference: ObjRef) -> Result<&ReflectMember, HeapError> {
        match self.get(reference)? {
            HeapObject::Reflect(member) => Ok(member),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a reflected member",
            }),
        }
    }

    pub fn get(&self, reference: ObjRef) -> Result<&HeapObject, HeapError> {
        if reference.is_null() {
            return Err(HeapError::NullReference);
        }
        let slot = self
            .slots
            .get(reference.index() as usize)
            .ok_or(HeapError::IndexOutOfBounds { reference })?;
        if slot.generation != reference.generation() {
            return Err(HeapError::StaleReference {
                reference,
                found_generation: slot.generation,
            });
        }
        slot.object.as_ref().ok_or(HeapError::StaleReference {
            reference,
            found_generation: slot.generation,
        })
    }

    pub fn get_mut(&mut self, reference: ObjRef) -> Result<&mut HeapObject, HeapError> {
        if reference.is_null() {
            return Err(HeapError::NullReference);
        }
        let slot = self
            .slots
            .get_mut(reference.index() as usize)
            .ok_or(HeapError::IndexOutOfBounds { reference })?;
        if slot.generation != reference.generation() {
            return Err(HeapError::StaleReference {
                reference,
                found_generation: slot.generation,
            });
        }
        slot.object.as_mut().ok_or(HeapError::StaleReference {
            reference,
            found_generation: slot.generation,
        })
    }

    pub fn instance(&self, reference: ObjRef) -> Result<&Instance, HeapError> {
        match self.get(reference)? {
            HeapObject::Instance(instance) => Ok(instance),
            HeapObject::Array(_)
            | HeapObject::Str(_)
            | HeapObject::Class(_)
            | HeapObject::Reflect(_) => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an instance",
            }),
        }
    }

    pub fn instance_mut(&mut self, reference: ObjRef) -> Result<&mut Instance, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Instance(instance) => Ok(instance),
            HeapObject::Array(_)
            | HeapObject::Str(_)
            | HeapObject::Class(_)
            | HeapObject::Reflect(_) => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an instance",
            }),
        }
    }

    pub fn array(&self, reference: ObjRef) -> Result<&Array, HeapError> {
        match self.get(reference)? {
            HeapObject::Array(array) => Ok(array),
            HeapObject::Instance(_)
            | HeapObject::Str(_)
            | HeapObject::Class(_)
            | HeapObject::Reflect(_) => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an array",
            }),
        }
    }

    pub fn array_mut(&mut self, reference: ObjRef) -> Result<&mut Array, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Array(array) => Ok(array),
            HeapObject::Instance(_)
            | HeapObject::Str(_)
            | HeapObject::Class(_)
            | HeapObject::Reflect(_) => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an array",
            }),
        }
    }

    pub fn free(&mut self, reference: ObjRef) -> Result<(), HeapError> {
        if reference.is_null() {
            return Err(HeapError::NullReference);
        }
        let slot = self
            .slots
            .get_mut(reference.index() as usize)
            .ok_or(HeapError::IndexOutOfBounds { reference })?;
        if slot.generation != reference.generation() || slot.object.is_none() {
            return Err(HeapError::StaleReference {
                reference,
                found_generation: slot.generation,
            });
        }
        slot.object = None;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(reference.index());
        Ok(())
    }

    pub fn live_count(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.object.is_some())
            .count()
    }

    fn allocate(&mut self, object: HeapObject) -> ObjRef {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.object = Some(object);
            ObjRef::new(index, slot.generation)
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                object: Some(object),
            });
            ObjRef::new(index, 0)
        }
    }
}
