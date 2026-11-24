//! The VM-owned heap: objects are addressed by a stable handle, never a host pointer.
//!
//! A handle carries a generation. Freeing a slot bumps the generation, so a stale
//! handle is rejected instead of silently addressing a reused slot.

use std::fmt;

use rava_classfile::metadata::AnnotationValue;

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

    /// A stable identity hash for this object handle.
    ///
    /// The handle's `(index, generation)` pair is the VM's object identity: it is fixed
    /// for the object's lifetime and a freed slot gets a new generation, so a reused
    /// index never repeats a live object's hash. The value is a deterministic mix of the
    /// two fields with odd multipliers; both steps are injective modulo 2^32, so two live
    /// handles that differ only in index get different hashes. No side table is needed
    /// because the input never changes, so `Object.hashCode` is stable by construction.
    ///
    /// This is a runtime identity hash, not a content hash: `java.lang.String` overrides
    /// `hashCode` with the `31*h + c` content algorithm and only reaches this value
    /// through an explicit `Object.hashCode` call.
    pub fn identity_hash(self) -> i32 {
        let index = self.index.wrapping_mul(0x9E37_79B1);
        let generation = self.generation.wrapping_mul(0x85EB_CA6B);
        (index ^ generation).wrapping_add(0x7F4A_7C15) as i32
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

/// A boxed primitive value (`java.lang.Integer.valueOf(int)`).
///
/// M2.10 models one box: `java.lang.Integer` holding its primitive `int`. A box is an
/// ordinary heap object rather than a second [`crate::Value`] variant, so it can live in
/// an `Object`-typed slot and `getClass` reports `java.lang.Integer`. There is no
/// `Boolean` or `Long` box yet and no small-value cache: every `valueOf` allocates a new
/// object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxedPrimitive {
    Integer(i32),
}

impl BoxedPrimitive {
    /// The internal class name of the box's runtime class.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Integer(_) => "java/lang/Integer",
        }
    }

    /// The dotted binary name of the box's runtime class.
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Integer(_) => "java.lang.Integer",
        }
    }

    /// The primitive `int` this box holds.
    pub fn as_int(self) -> Option<i32> {
        match self {
            Self::Integer(value) => Some(value),
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

/// One annotation instance (`java.lang.annotation.Annotation`).
///
/// The object stores its annotation type's class identity and the element values the
/// class file encodes, exactly as the M2.1 metadata model decoded them. A value the
/// annotation omits is not stored: the element method falls back to the annotation type's
/// `AnnotationDefault` when it is read. Annotation objects are allocated by
/// [`crate::Shared::annotation_object`]; they are ordinary heap objects, not permanent
/// roots.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationObject {
    /// The annotation type, resolved through the defining loader of the annotated class.
    pub class: ClassId,
    /// The encoded `element_name`/`element_value` pairs, in class-file order.
    pub values: Vec<AnnotationValue>,
}

/// One enum constant used as an annotation element value.
///
/// The VM does not execute the enum's `<clinit>` in this slice, so the constant is a heap
/// object that names the enum class and the constant. [`crate::Shared::enum_constant`]
/// caches one object per `(class, name)`, which keeps `==` on two reads of the same
/// annotation element true, as it is for the JDK's static enum fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EnumConstant {
    pub class: ClassId,
    pub name: String,
}

#[derive(Debug)]
pub enum HeapObject {
    Instance(Instance),
    Array(Array),
    /// A string object. Stored as a Rust `String` (valid UTF-8); the `java.lang.String`
    /// natives view it as UTF-16 code units through `String::encode_utf16`, so no second
    /// string representation exists. A lone surrogate cannot be stored in a Rust
    /// `String`, so a string operation that would produce one is a typed error.
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
    /// A boxed primitive value (`java.lang.Integer.valueOf(int)`). The client class is
    /// named by [`BoxedPrimitive::class_name`]; the object holds no `ClassId` of its own.
    Boxed(BoxedPrimitive),
    /// One annotation instance: its annotation type and its encoded element values.
    Annotation(AnnotationObject),
    /// One enum constant used as an annotation element value.
    Enum(EnumConstant),
    /// A `java.lang.StringBuilder` (`java.lang.AbstractStringBuilder` state).
    ///
    /// The builder stores its text as a Rust `String` (valid UTF-8), the same
    /// representation [`HeapObject::Str`] uses. The `java.lang.StringBuilder` natives view
    /// it as UTF-16 code units through `String::encode_utf16`, so `length` and `charAt`
    /// count and index units, and an operation that would leave a lone surrogate is a
    /// typed error, exactly as for a string.
    StringBuilder(String),
    /// A `java.util.ArrayList`: its elements in index order.
    ///
    /// An element is a [`Value`], so the list can hold a reference (including null) and
    /// that reference must be enumerated by [`crate::Shared::roots`]. Lists are ordinary
    /// heap objects, not permanent roots.
    List(Vec<Value>),
    /// A `java.util.HashMap`: its `(key, value)` entries in insertion order.
    ///
    /// A `Vec` of pairs is the whole representation: this slice has no hashing, tree
    /// bins or resizing, and insertion order is stable. Keys and values are [`Value`]s, so
    /// both can be references (null included) and [`crate::Shared::roots`] enumerates the
    /// non-null ones. Maps are ordinary heap objects, not permanent roots.
    Map(Vec<(Value, Value)>),
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

    /// Allocate a boxed `int`. Callers go through [`crate::Shared::box_int`], which is
    /// the one boxing entry point the reflection natives use.
    pub fn allocate_boxed_int(&mut self, value: i32) -> ObjRef {
        self.allocate(HeapObject::Boxed(BoxedPrimitive::Integer(value)))
    }

    /// Allocate an annotation instance. Callers go through
    /// [`crate::Shared::annotation_object`].
    pub fn allocate_annotation(&mut self, class: ClassId, values: Vec<AnnotationValue>) -> ObjRef {
        self.allocate(HeapObject::Annotation(AnnotationObject { class, values }))
    }

    /// Allocate an enum constant. Callers go through
    /// [`crate::Shared::enum_constant`], which caches one object per constant.
    pub fn allocate_enum_constant(&mut self, class: ClassId, name: impl Into<String>) -> ObjRef {
        self.allocate(HeapObject::Enum(EnumConstant {
            class,
            name: name.into(),
        }))
    }

    /// Allocate an empty `java.lang.StringBuilder`. The builder's class is named by
    /// [`crate::collections::STRING_BUILDER_CLASS`]; [`crate::Shared::allocate_instance`]
    /// is the entry point that creates one for `new` and reflection.
    pub fn allocate_string_builder(&mut self) -> ObjRef {
        self.allocate(HeapObject::StringBuilder(String::new()))
    }

    /// Allocate an empty `java.util.ArrayList`. The list's class is named by
    /// [`crate::collections::ARRAY_LIST_CLASS`].
    pub fn allocate_list(&mut self) -> ObjRef {
        self.allocate(HeapObject::List(Vec::new()))
    }

    /// Allocate an empty `java.util.HashMap`. The map's class is named by
    /// [`crate::maps::HASH_MAP_CLASS`]; [`crate::Shared::allocate_instance`] is the entry
    /// point that creates one for `new` and reflection.
    pub fn allocate_map(&mut self) -> ObjRef {
        self.allocate(HeapObject::Map(Vec::new()))
    }

    /// Every live object, in slot order.
    ///
    /// Root enumeration uses this to reach the references a container such as a list
    /// holds; there is no collector scanning object graphs yet.
    pub fn live_objects(&self) -> impl Iterator<Item = &HeapObject> {
        self.slots.iter().filter_map(|slot| slot.object.as_ref())
    }

    /// The text behind a `java.lang.StringBuilder` reference.
    pub fn string_builder(&self, reference: ObjRef) -> Result<&str, HeapError> {
        match self.get(reference)? {
            HeapObject::StringBuilder(text) => Ok(text),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a String builder",
            }),
        }
    }

    /// The mutable text behind a `java.lang.StringBuilder` reference.
    pub fn string_builder_mut(&mut self, reference: ObjRef) -> Result<&mut String, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::StringBuilder(text) => Ok(text),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a String builder",
            }),
        }
    }

    /// The elements behind a `java.util.ArrayList` reference.
    pub fn list(&self, reference: ObjRef) -> Result<&[Value], HeapError> {
        match self.get(reference)? {
            HeapObject::List(elements) => Ok(elements),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a list",
            }),
        }
    }

    /// The mutable elements behind a `java.util.ArrayList` reference.
    pub fn list_mut(&mut self, reference: ObjRef) -> Result<&mut Vec<Value>, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::List(elements) => Ok(elements),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a list",
            }),
        }
    }

    /// The `(key, value)` entries behind a `java.util.HashMap` reference.
    pub fn map(&self, reference: ObjRef) -> Result<&[(Value, Value)], HeapError> {
        match self.get(reference)? {
            HeapObject::Map(entries) => Ok(entries),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a map",
            }),
        }
    }

    /// The mutable `(key, value)` entries behind a `java.util.HashMap` reference.
    pub fn map_mut(&mut self, reference: ObjRef) -> Result<&mut Vec<(Value, Value)>, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Map(entries) => Ok(entries),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a map",
            }),
        }
    }

    /// The annotation instance behind a reference.
    pub fn annotation(&self, reference: ObjRef) -> Result<&AnnotationObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Annotation(annotation) => Ok(annotation),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an annotation",
            }),
        }
    }

    /// The enum constant behind a reference.
    pub fn enum_constant(&self, reference: ObjRef) -> Result<&EnumConstant, HeapError> {
        match self.get(reference)? {
            HeapObject::Enum(constant) => Ok(constant),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an enum constant",
            }),
        }
    }

    /// The boxed value behind a reference.
    pub fn boxed(&self, reference: ObjRef) -> Result<BoxedPrimitive, HeapError> {
        match self.get(reference)? {
            HeapObject::Boxed(boxed) => Ok(*boxed),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a boxed primitive",
            }),
        }
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
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an instance",
            }),
        }
    }

    pub fn instance_mut(&mut self, reference: ObjRef) -> Result<&mut Instance, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Instance(instance) => Ok(instance),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an instance",
            }),
        }
    }

    pub fn array(&self, reference: ObjRef) -> Result<&Array, HeapError> {
        match self.get(reference)? {
            HeapObject::Array(array) => Ok(array),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an array",
            }),
        }
    }

    pub fn array_mut(&mut self, reference: ObjRef) -> Result<&mut Array, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Array(array) => Ok(array),
            _ => Err(HeapError::WrongObjectKind {
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
