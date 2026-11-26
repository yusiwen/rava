//! The VM-owned heap: objects are addressed by a stable handle, never a host pointer.
//!
//! A handle carries a generation. Freeing a slot bumps the generation, so a stale
//! handle is rejected instead of silently addressing a reused slot.

use std::collections::BTreeSet;
use std::fmt;

use crate::loader::LoaderId;

use rava_classfile::metadata::AnnotationValue;

use crate::futures::FutureTaskObject;
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

    /// The primitive kind this element kind stores, or `None` for a reference array.
    ///
    /// `boolean` and `byte` are distinct `ArrayKind`s (JVMS §6.5.newarray selects them
    /// with different `atype` values), so each maps to its own `PrimitiveKind`.
    pub fn primitive_kind(self) -> Option<PrimitiveKind> {
        Some(match self {
            Self::Boolean => PrimitiveKind::Boolean,
            Self::Byte => PrimitiveKind::Byte,
            Self::Char => PrimitiveKind::Char,
            Self::Short => PrimitiveKind::Short,
            Self::Int => PrimitiveKind::Int,
            Self::Long => PrimitiveKind::Long,
            Self::Float => PrimitiveKind::Float,
            Self::Double => PrimitiveKind::Double,
            Self::Reference => return None,
        })
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

/// One Java primitive type, `void` included.
///
/// A primitive class object (`int.class`, `Class.forName("int")`) is a
/// [`HeapObject::PrimitiveClass`] holding this kind. The kind is the whole identity: a
/// primitive has exactly one class object per VM, no defining loader, no class file and
/// no metadata, which is why it is not a [`ClassId`]. The nine names, descriptors and
/// modifiers match the JDK: every primitive reports the same
/// `public abstract final` modifiers (`0x0411`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PrimitiveKind {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
    Void,
}

impl PrimitiveKind {
    /// Every primitive kind in `boolean`, `byte`, `char`, `short`, `int`, `long`,
    /// `float`, `double`, `void` order.
    pub const ALL: [Self; 9] = [
        Self::Boolean,
        Self::Byte,
        Self::Char,
        Self::Short,
        Self::Int,
        Self::Long,
        Self::Float,
        Self::Double,
        Self::Void,
    ];

    /// The Java source name, which is also `Class.getName()` for the primitive and the
    /// spelling `Class.forName` accepts.
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
            Self::Void => "void",
        }
    }

    /// The JVM field descriptor and the return type of a `void` method (`V`).
    pub fn descriptor(self) -> &'static str {
        match self {
            Self::Boolean => "Z",
            Self::Byte => "B",
            Self::Char => "C",
            Self::Short => "S",
            Self::Int => "I",
            Self::Long => "J",
            Self::Float => "F",
            Self::Double => "D",
            Self::Void => "V",
        }
    }

    /// `Class.getModifiers()` for a primitive: `Modifier.PUBLIC | Modifier.ABSTRACT |
    /// Modifier.FINAL`, which is `0x0411` on the reference JDK for all nine kinds.
    pub fn modifiers(self) -> u16 {
        0x0411
    }

    /// The primitive whose Java name is `name`, if any.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The primitive whose JVM descriptor is `descriptor`, if any.
    ///
    /// A one-character field descriptor (`I`, not `[I`) is the whole match, so an array
    /// descriptor is never mistaken for its element primitive.
    pub fn from_descriptor(descriptor: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.descriptor() == descriptor)
    }

    /// The primitive element kind of an array whose component is this primitive.
    ///
    /// `void` is not a valid array component (JVMS §4.3.2), so it has no array kind.
    pub fn array_kind(self) -> Option<ArrayKind> {
        Some(match self {
            Self::Boolean => ArrayKind::Boolean,
            Self::Byte => ArrayKind::Byte,
            Self::Char => ArrayKind::Char,
            Self::Short => ArrayKind::Short,
            Self::Int => ArrayKind::Int,
            Self::Long => ArrayKind::Long,
            Self::Float => ArrayKind::Float,
            Self::Double => ArrayKind::Double,
            Self::Void => return None,
        })
    }

    /// The `java.lang` wrapper class that declares this primitive's `TYPE` field.
    pub fn wrapper_class(self) -> &'static str {
        match self {
            Self::Boolean => "java/lang/Boolean",
            Self::Byte => "java/lang/Byte",
            Self::Char => "java/lang/Character",
            Self::Short => "java/lang/Short",
            Self::Int => "java/lang/Integer",
            Self::Long => "java/lang/Long",
            Self::Float => "java/lang/Float",
            Self::Double => "java/lang/Double",
            Self::Void => "java/lang/Void",
        }
    }

    /// The primitive whose wrapper class is the internal name `name`, if any.
    pub fn from_wrapper_class(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.wrapper_class() == name)
    }
}

impl fmt::Display for PrimitiveKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One array class: its component type.
///
/// The component is the whole identity of the array class, so `int[]` is the array of
/// [`ClassRef::Primitive`]`(Int)` and `int[][]` is the array of that array. The
/// `ClassRef` does not carry a name: a component's descriptor and its Java name are read
/// from the class table by [`crate::Shared`], which is where a loaded class's binary name
/// lives. An array class object is [`HeapObject::ArrayClass`]; exactly one object exists
/// per `ArrayClass`, created by [`crate::Shared::array_class`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArrayClass {
    /// The component type. A nested array's component is itself an array class.
    pub component: ClassRef,
}

impl ArrayClass {
    pub fn new(component: ClassRef) -> Self {
        Self { component }
    }
}

/// A class identity: a loaded class, one of the nine primitives, or an array class.
///
/// This is the type of an array's component and the key of [`crate::Shared::array_class`].
/// It does not replace [`HeapObject::Class`] or [`HeapObject::PrimitiveClass`]: a class
/// object in the heap is still those kinds, and an array class object is
/// [`HeapObject::ArrayClass`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ClassRef {
    /// A loaded class, identified by its `(loader, name)` class id.
    Named(ClassId),
    /// A primitive type, which has no class file and no loader.
    Primitive(PrimitiveKind),
    /// An array class, identified by its component.
    Array(Box<ArrayClass>),
}

impl ClassRef {
    /// The array class whose component is `self`.
    pub fn array(component: ClassRef) -> Self {
        Self::Array(Box::new(ArrayClass::new(component)))
    }

    /// The `ArrayKind` this class ref stores when it is an array's component.
    ///
    /// A primitive component selects its primitive kind; a named class or a nested array
    /// selects a reference array. `void` is not a legal array component, so it is mapped
    /// to a reference array rather than inventing a primitive array kind; every caller
    /// builds the ref from a parsed descriptor, which rejects `[V`.
    pub fn array_kind(&self) -> ArrayKind {
        match self {
            Self::Primitive(kind) => kind.array_kind().unwrap_or(ArrayKind::Reference),
            Self::Named(_) | Self::Array(_) => ArrayKind::Reference,
        }
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
    /// The component class this array was allocated with.
    ///
    /// `newarray` stores a primitive kind, `anewarray`/`multianewarray` store the named
    /// class or nested array class the instruction names, and `String.toCharArray` stores
    /// `char`. The component is what `getClass()` and `Class.getComponentType()` report;
    /// `kind` stays the element representation the load/store opcodes check.
    pub component: ClassRef,
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
/// Every one of the eight primitive wrappers is modelled as one variant holding its
/// primitive: `Byte`/`Short`/`Integer`/`Long`/`Float`/`Double` are `java.lang.Number`
/// subclasses, `Character` and `Boolean` are not. A box is an ordinary heap object rather
/// than a second [`crate::Value`] variant, so it can live in an `Object`-typed slot and
/// `getClass` reports its boxed class. There is no small-value cache: every `valueOf`
/// allocates a new object, so `==` on two equal boxes is false where the JDK caches
/// small values.
///
/// The accessors here implement the Java conversion rules the wrappers define:
/// `byteValue`/`shortValue`/`intValue`/`longValue` follow the JDK's narrowing (a
/// `long` keeps its low bits, a `float`/`double` narrows through `int` exactly as
/// `(byte)`/`(short)`/`(int)` do, so a huge or NaN value saturates and then truncates),
/// `floatValue`/`doubleValue` widen (a `long` to `float` rounds), `equals` is per-box
/// value equality with `Float`/`Double` bit semantics, and `compareTo` orders a `NaN`
/// above every value and `0.0` above `-0.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoxedPrimitive {
    Byte(i8),
    Short(i16),
    Integer(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Character(char),
    Boolean(bool),
}

impl BoxedPrimitive {
    /// The internal class name of the box's runtime class.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Byte(_) => "java/lang/Byte",
            Self::Short(_) => "java/lang/Short",
            Self::Integer(_) => "java/lang/Integer",
            Self::Long(_) => "java/lang/Long",
            Self::Float(_) => "java/lang/Float",
            Self::Double(_) => "java/lang/Double",
            Self::Character(_) => "java/lang/Character",
            Self::Boolean(_) => "java/lang/Boolean",
        }
    }

    /// The dotted binary name of the box's runtime class.
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Byte(_) => "java.lang.Byte",
            Self::Short(_) => "java.lang.Short",
            Self::Integer(_) => "java.lang.Integer",
            Self::Long(_) => "java.lang.Long",
            Self::Float(_) => "java.lang.Float",
            Self::Double(_) => "java.lang.Double",
            Self::Character(_) => "java.lang.Character",
            Self::Boolean(_) => "java.lang.Boolean",
        }
    }

    /// The one-character JVM field descriptor of this box's primitive.
    pub fn descriptor(self) -> &'static str {
        match self {
            Self::Byte(_) => "B",
            Self::Short(_) => "S",
            Self::Integer(_) => "I",
            Self::Long(_) => "J",
            Self::Float(_) => "F",
            Self::Double(_) => "D",
            Self::Character(_) => "C",
            Self::Boolean(_) => "Z",
        }
    }

    /// Whether this box is a `java.lang.Number` subclass.
    pub fn is_numeric(self) -> bool {
        !matches!(self, Self::Character(_) | Self::Boolean(_))
    }

    /// The `int` this box holds when it is an `Integer`; other kinds are `None`.
    pub fn as_int(self) -> Option<i32> {
        match self {
            Self::Integer(value) => Some(value),
            _ => None,
        }
    }

    /// `Number.byteValue()`: `(byte)` of the value.
    pub fn byte_value(self) -> Option<i8> {
        Some(match self {
            Self::Byte(value) => value,
            Self::Short(value) => value as i8,
            Self::Integer(value) => value as i8,
            Self::Long(value) => value as i8,
            // `(byte)` of a floating value is `(byte) intValue()` in the JDK: the `int`
            // conversion saturates and NaN becomes 0, then the low 8 bits are kept.
            Self::Float(value) => value as i32 as i8,
            Self::Double(value) => value as i32 as i8,
            Self::Character(_) | Self::Boolean(_) => return None,
        })
    }

    /// `Number.shortValue()`: `(short)` of the value.
    pub fn short_value(self) -> Option<i16> {
        Some(match self {
            Self::Byte(value) => i16::from(value),
            Self::Short(value) => value,
            Self::Integer(value) => value as i16,
            Self::Long(value) => value as i16,
            Self::Float(value) => value as i32 as i16,
            Self::Double(value) => value as i32 as i16,
            Self::Character(_) | Self::Boolean(_) => return None,
        })
    }

    /// `Number.intValue()`: `(int)` of the value.
    pub fn int_value(self) -> Option<i32> {
        Some(match self {
            Self::Byte(value) => i32::from(value),
            Self::Short(value) => i32::from(value),
            Self::Integer(value) => value,
            Self::Long(value) => value as i32,
            // Rust's float-to-int cast saturates and maps NaN to 0, the JDK's `(int)`.
            Self::Float(value) => value as i32,
            Self::Double(value) => value as i32,
            Self::Character(_) | Self::Boolean(_) => return None,
        })
    }

    /// `Number.longValue()`: `(long)` of the value.
    pub fn long_value(self) -> Option<i64> {
        Some(match self {
            Self::Byte(value) => i64::from(value),
            Self::Short(value) => i64::from(value),
            Self::Integer(value) => i64::from(value),
            Self::Long(value) => value,
            Self::Float(value) => value as i64,
            Self::Double(value) => value as i64,
            Self::Character(_) | Self::Boolean(_) => return None,
        })
    }

    /// `Number.floatValue()`: `(float)` of the value.
    pub fn float_value(self) -> Option<f32> {
        Some(match self {
            Self::Byte(value) => f32::from(value),
            Self::Short(value) => f32::from(value),
            Self::Integer(value) => value as f32,
            Self::Long(value) => value as f32,
            Self::Float(value) => value,
            Self::Double(value) => value as f32,
            Self::Character(_) | Self::Boolean(_) => return None,
        })
    }

    /// `Number.doubleValue()`: `(double)` of the value.
    pub fn double_value(self) -> Option<f64> {
        Some(match self {
            Self::Byte(value) => f64::from(value),
            Self::Short(value) => f64::from(value),
            Self::Integer(value) => f64::from(value),
            Self::Long(value) => value as f64,
            Self::Float(value) => f64::from(value),
            Self::Double(value) => value,
            Self::Character(_) | Self::Boolean(_) => return None,
        })
    }

    /// The `toString`/`String.valueOf` text of the boxed value.
    pub fn text(self) -> String {
        match self {
            Self::Byte(value) => value.to_string(),
            Self::Short(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            Self::Long(value) => value.to_string(),
            Self::Float(value) => crate::string::float_text(value),
            Self::Double(value) => crate::string::double_text(value),
            Self::Character(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
        }
    }

    /// `Wrapper.equals(Object)` between two boxes: same kind and Java value equality.
    ///
    /// `Float`/`Double` compare their canonical bits, so `NaN` equals `NaN` and `0.0`
    /// does not equal `-0.0`, exactly as `Float.equals`/`Double.equals` define.
    pub fn java_equals(self, other: Self) -> bool {
        match (self, other) {
            (Self::Byte(left), Self::Byte(right)) => left == right,
            (Self::Short(left), Self::Short(right)) => left == right,
            (Self::Integer(left), Self::Integer(right)) => left == right,
            (Self::Long(left), Self::Long(right)) => left == right,
            (Self::Float(left), Self::Float(right)) => {
                canonical_float_bits(left) == canonical_float_bits(right)
            }
            (Self::Double(left), Self::Double(right)) => {
                canonical_double_bits(left) == canonical_double_bits(right)
            }
            (Self::Character(left), Self::Character(right)) => left == right,
            (Self::Boolean(left), Self::Boolean(right)) => left == right,
            _ => false,
        }
    }

    /// `Wrapper.hashCode()` for the boxed value.
    pub fn java_hash_code(self) -> i32 {
        match self {
            Self::Byte(value) => i32::from(value),
            Self::Short(value) => i32::from(value),
            Self::Integer(value) => value,
            Self::Long(value) => {
                let bits = value as u64;
                (value ^ ((bits >> 32) as i64)) as i32
            }
            Self::Float(value) => canonical_float_bits(value) as i32,
            Self::Double(value) => {
                let bits = canonical_double_bits(value);
                (bits ^ (bits >> 32)) as i32
            }
            Self::Character(value) => i32::from(value as u16),
            Self::Boolean(value) => {
                if value {
                    1231
                } else {
                    1237
                }
            }
        }
    }

    /// `Wrapper.compareTo(same box)`: `None` for two different primitive kinds.
    ///
    /// `Float`/`Double` use the JDK's total order: a `NaN` compares greater than every
    /// value and equal to itself, and `0.0` compares greater than `-0.0`.
    pub fn java_compare(self, other: Self) -> Option<std::cmp::Ordering> {
        Some(match (self, other) {
            (Self::Byte(left), Self::Byte(right)) => left.cmp(&right),
            (Self::Short(left), Self::Short(right)) => left.cmp(&right),
            (Self::Integer(left), Self::Integer(right)) => left.cmp(&right),
            (Self::Long(left), Self::Long(right)) => left.cmp(&right),
            (Self::Float(left), Self::Float(right)) => float_order(left, right)?,
            (Self::Double(left), Self::Double(right)) => double_order(left, right)?,
            (Self::Character(left), Self::Character(right)) => left.cmp(&right),
            (Self::Boolean(left), Self::Boolean(right)) => left.cmp(&right),
            _ => return None,
        })
    }
}

/// `Float.floatToIntBits`: every `NaN` is the one canonical quiet-NaN bit pattern.
pub fn canonical_float_bits(value: f32) -> u32 {
    if value.is_nan() {
        0x7fc0_0000
    } else {
        value.to_bits()
    }
}

/// `Double.doubleToLongBits`: every `NaN` is the one canonical quiet-NaN bit pattern.
pub fn canonical_double_bits(value: f64) -> u64 {
    if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits()
    }
}

/// `Float.compare`: numeric order, then the canonical bit pattern for `NaN`/zeroes.
fn float_order(left: f32, right: f32) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if left < right {
        return Some(Ordering::Less);
    }
    if left > right {
        return Some(Ordering::Greater);
    }
    // `Float.compare` compares `floatToIntBits` as signed `int`s, so `0.0` (bits 0)
    // sorts above `-0.0` (bits `i32::MIN`) and every `NaN` above both.
    Some((canonical_float_bits(left) as i32).cmp(&(canonical_float_bits(right) as i32)))
}

/// `Double.compare`: numeric order, then the canonical bit pattern for `NaN`/zeroes.
fn double_order(left: f64, right: f64) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    if left < right {
        return Some(Ordering::Less);
    }
    if left > right {
        return Some(Ordering::Greater);
    }
    // `Double.compare` compares `doubleToLongBits` as signed `long`s.
    Some((canonical_double_bits(left) as i64).cmp(&(canonical_double_bits(right) as i64)))
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

/// A `java.lang.reflect.Type` that is not a raw `Class`.
///
/// A raw class type (`String`, `int`, `String[]`) *is* a `Class` object in Java, because
/// `Class` implements `Type`; those types are not represented here. This enum covers the
/// four reflective interfaces the JDK creates separate objects for:
/// `ParameterizedType`, `TypeVariable`, `WildcardType` and `GenericArrayType`.
///
/// Every nested type is an [`ObjRef`] to another `Class` object or `Type` object. Type
/// objects are immutable and cached structurally by [`crate::Shared::type_object`], so two
/// reflections of the same signature return the same handle; the nested handles are
/// permanent roots through their own caches.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeObject {
    Parameterized(ParameterizedType),
    TypeVariable(TypeVariableType),
    Wildcard(WildcardType),
    GenericArray(GenericArrayType),
}

impl TypeObject {
    /// The internal name of the `java.lang.reflect` interface this object implements.
    ///
    /// This is the class identity [`crate::Shared::get_class`] reports and the class the
    /// interpreter dispatches its virtual methods from. The JDK's actual implementation
    /// classes are private (`sun.reflect.generics.reflectiveObjects.*Impl`); reporting the
    /// public interface is this VM's documented, deterministic shape, as it is for lambdas.
    pub fn class_name(&self) -> &'static str {
        match self {
            Self::Parameterized(_) => "java/lang/reflect/ParameterizedType",
            Self::TypeVariable(_) => "java/lang/reflect/TypeVariable",
            Self::Wildcard(_) => "java/lang/reflect/WildcardType",
            Self::GenericArray(_) => "java/lang/reflect/GenericArrayType",
        }
    }

    /// Every nested type handle this object owns, in a deterministic order.
    ///
    /// [`crate::Shared::roots`] reports them so a future collector sees the retention
    /// instead of inferring it from the type-object cache alone.
    pub fn references(&self) -> Vec<ObjRef> {
        match self {
            Self::Parameterized(parameterized) => {
                let mut references = Vec::new();
                references.extend(parameterized.owner);
                references.extend(parameterized.arguments.iter().copied());
                references
            }
            Self::TypeVariable(variable) => variable.bounds.clone(),
            Self::Wildcard(wildcard) => {
                let mut references = wildcard.upper.clone();
                references.extend(wildcard.lower.iter().copied());
                references
            }
            Self::GenericArray(array) => vec![array.component],
        }
    }
}

/// A `java.lang.reflect.ParameterizedType`: the raw class, its type arguments and the
/// owner type of a member class.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParameterizedType {
    /// The raw class (`java/util/List`), resolved through the declaring class's loader.
    pub raw: ClassRef,
    /// The owner type for a member class. `None` for a top-level type; the JDK reports
    /// the declaring class as a raw `Class` when the signature does not spell an owner.
    pub owner: Option<ObjRef>,
    pub arguments: Vec<ObjRef>,
}

/// A `java.lang.reflect.TypeVariable`: its name, its bounds and the declaration that
/// introduced it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TypeVariableType {
    pub name: String,
    pub bounds: Vec<ObjRef>,
    /// `None` only for a type-variable name whose declaration could not be recovered; a
    /// parsed class or method signature always supplies one.
    pub declaration: Option<TypeDeclaration>,
}

/// A `java.lang.reflect.WildcardType`: its upper and lower bounds.
///
/// `?` has the upper bound `Object` and no lower bound, `? extends X` has `[X]`, and
/// `? super X` has `[Object]` upper and `[X]` lower, exactly as the JDK reports them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WildcardType {
    pub upper: Vec<ObjRef>,
    pub lower: Vec<ObjRef>,
}

/// A `java.lang.reflect.GenericArrayType`: its component type.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GenericArrayType {
    pub component: ObjRef,
}

/// The declaration that introduced a type variable: a class, method or constructor.
///
/// The fields name the `ReflectMember` the VM caches for the declaration; a type variable
/// its `getGenericDeclaration` returns that member object (or the class object).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeDeclaration {
    Class(ClassId),
    Member {
        kind: MemberKind,
        class: ClassId,
        name: String,
        descriptor: String,
    },
}

impl TypeDeclaration {
    /// The member kind of this declaration, or `None` for a class declaration.
    pub fn member_kind(&self) -> Option<MemberKind> {
        match self {
            Self::Class(_) => None,
            Self::Member { kind, .. } => Some(*kind),
        }
    }
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
    /// The defining loader of the class that carries the annotation.
    ///
    /// An annotation's class-literal element (`@Repeatable(ComponentScans.class)`) is
    /// resolved through the loader of the annotated class, not the annotation type's
    /// loader. For `@Repeatable` the type is a bootstrap JDK class while the annotated
    /// class is an application class, so keeping this loader is what makes the literal
    /// resolve.
    pub loader: LoaderId,
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

/// How a lambda invokes its implementation method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LambdaTargetKind {
    /// A `REF_invokeStatic` implementation: the captured values and the SAM arguments are
    /// the method's arguments.
    Static,
    /// A `REF_invokeVirtual`, `REF_invokeSpecial` or `REF_invokeInterface` implementation:
    /// the first captured-or-call value is the receiver, the rest are the arguments.
    Instance,
    /// A `REF_newInvokeSpecial` implementation: the captured values and the SAM arguments
    /// are the constructor's arguments, and the SAM result is the new instance.
    Constructor,
    /// A native registry entry rather than a Java method.
    ///
    /// A VM factory such as `Comparator.comparingInt` returns a lambda whose body has no
    /// class-file method to point at; the target names a `(owner, name, descriptor)` the
    /// registry implements, and it receives the captured values followed by the SAM
    /// arguments like a static target.
    Native,
}

/// The implementation method a lambda calls when its SAM method is invoked.
///
/// The class identity is resolved through the defining loader of the class that executes
/// the `invokedynamic` instruction, so a child loader's target keeps its own identity.
#[derive(Debug, Clone, PartialEq)]
pub struct LambdaTarget {
    pub class: ClassId,
    pub name: String,
    pub descriptor: String,
    pub kind: LambdaTargetKind,
}

/// One lambda object produced by a `LambdaMetafactory` bootstrap.
///
/// The class identity is the interface the lambda implements, not a synthetic class. The
/// VM does not generate a class file for a lambda, so `getClass()` on a lambda reports the
/// functional interface's class object (documented deviation from the JDK, which reports
/// a hidden synthetic class such as `demo.LambdaProbe$$Lambda$1/0x...`). The `sam`
/// fields are the call-site name and the `samMethodType` descriptor the bootstrap was
/// given; they are checked when an `invokeinterface` calls the SAM. `captured` holds the
/// `invokedynamic` call-site arguments in order, exactly as the factory received them.
#[derive(Debug, Clone, PartialEq)]
pub struct LambdaObject {
    /// The functional interface the lambda implements.
    pub interface: ClassId,
    /// The SAM method name, which is the `invokedynamic` call-site name.
    pub sam_name: String,
    /// The SAM method descriptor, from the bootstrap's `samMethodType`.
    pub sam_descriptor: String,
    /// The bootstrap's `instantiatedMethodType`, retained for diagnostics.
    pub instantiated_descriptor: String,
    pub target: LambdaTarget,
    /// The captured `invokedynamic` arguments, in call-site order.
    pub captured: Vec<Value>,
}

/// One `java.lang.invoke.MethodHandles.Lookup` (M3.4).
///
/// The lookup is identity only: the class it looks up in and the access-mode bit set. The
/// native ABI does not carry a caller class, so `MethodHandles.lookup()` uses a documented
/// placeholder class and `MethodHandles.privateLookupIn` replaces it with the target class;
/// no caller frame is tracked. `modes` is the `Lookup.lookupModes()` bit set (see
/// [`crate::method_handles`] for the values this VM assigns).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LookupObject {
    /// The class the lookup resolves members in.
    pub lookup_class: ClassId,
    /// The access-mode bit set.
    pub modes: i32,
}

/// One `java.lang.invoke.MethodType` (M3.4).
///
/// The type is a return type and an ordered parameter list, each a [`ClassRef`], so a
/// primitive, a loaded class and an array class are all representable and the type can be
/// turned back into a JVM method descriptor without a class lookup. A `Class` object
/// converts through `class_object_of(...).class_ref()`. Method types are ordinary heap
/// objects, not cached: two structural equal types are two objects, and `equals` compares
/// them structurally (documented deviation from the JDK's interning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodTypeObject {
    pub return_type: ClassRef,
    pub parameters: Vec<ClassRef>,
}

/// How a [`MethodHandleObject`] invokes its target method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    /// A `static` method: the arguments are the method's parameters.
    Static,
    /// An instance method with virtual dispatch: the first argument is the receiver and
    /// dispatch starts at its runtime class.
    Virtual,
    /// An instance method without virtual dispatch: the first argument is the receiver
    /// and the handle's owner declares the method.
    Special,
    /// A constructor: the arguments are the constructor's parameters and the result is a
    /// new instance of the owner.
    Constructor,
}

/// One `java.lang.invoke.MethodHandle` (M3.4).
///
/// The handle names its target by `(owner, name, descriptor)` rather than a resolved
/// pointer, so it survives a second definition and never duplicates method metadata.
/// `method_type` is the handle's *invocation* type, exactly as `MethodHandle.type()`
/// reports it: it includes the receiver as the first parameter for [`HandleKind::Virtual`]
/// and [`HandleKind::Special`], and it returns the owner class for
/// [`HandleKind::Constructor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodHandleObject {
    pub owner: ClassId,
    pub name: String,
    pub descriptor: String,
    pub kind: HandleKind,
    pub method_type: MethodTypeObject,
}

/// A `java.util.List`-family VM object: its runtime class and its elements in index
/// order.
///
/// The class is stored rather than derived from the shape, so `ArrayList`,
/// `LinkedList`, `ArrayDeque` and `CopyOnWriteArrayList` keep their own identity while
/// sharing one representation. [`crate::Shared::allocate_instance`] is the only creation
/// entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ListObject {
    pub class: ClassId,
    pub elements: Vec<Value>,
    /// The subclass's instance fields, in layout order. An exact `ArrayList` has none; a
    /// Spring subclass that adds `this$0` or a cache field keeps them here.
    pub fields: Vec<Value>,
    /// The maximum element count for a bounded queue, or [`usize::MAX`] otherwise.
    ///
    /// `LinkedBlockingQueue(int)` sets it; every other list-family container is
    /// unbounded, and a queue operation that only reads it (`remainingCapacity`,
    /// `offer`) is the sole consumer.
    pub capacity: usize,
}

/// A `java.util.Set`-family VM object: its runtime class and its elements in insertion
/// order.
///
/// A set stores each element once; the insertion order is what makes `LinkedHashSet`
/// observable. The class keeps `HashSet`, `LinkedHashSet` and `TreeSet` distinct.
#[derive(Debug, Clone, PartialEq)]
pub struct SetObject {
    pub class: ClassId,
    pub elements: Vec<Value>,
    /// The subclass's instance fields, in layout order.
    pub fields: Vec<Value>,
}

/// A `java.util.Map`-family VM object: its runtime class and its `(key, value)` entries
/// in insertion order.
///
/// `access_order` is the `LinkedHashMap` flag: when it is true, a lookup or an update of an
/// existing key moves that entry to the end of `entries`, which is the order `keySet`,
/// `values`, `entrySet` and an iteration observe. Every other map keeps it false, so its
/// entries stay in insertion order.
#[derive(Debug, Clone, PartialEq)]
pub struct MapObject {
    pub class: ClassId,
    pub entries: Vec<(Value, Value)>,
    /// `LinkedHashMap`'s access-order flag.
    pub access_order: bool,
    /// The subclass's instance fields, in layout order.
    pub fields: Vec<Value>,
}

/// A `java.util.Iterator` over a snapshot of values.
///
/// The iterator holds its own copy of the elements, so mutating the source collection
/// does not reorder an iteration in progress. `position` is the next index `next`
/// returns.
#[derive(Debug, Clone, PartialEq)]
pub struct IteratorObject {
    pub class: ClassId,
    pub items: Vec<Value>,
    pub position: usize,
    /// The list or set the snapshot was taken from, for `Iterator.remove`.
    ///
    /// A stream or empty iterator has no mutable source and stores null; removing from
    /// one is a typed error rather than a silent no-op.
    pub source: ObjRef,
}

/// One `java.util.Map.Entry`: a key and a value copied out of a map.
#[derive(Debug, Clone, PartialEq)]
pub struct MapEntry {
    pub key: Value,
    pub value: Value,
}

/// One `java.util.Enumeration`: a snapshot and a cursor.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumerationObject {
    pub items: Vec<Value>,
    pub position: usize,
}

/// A `java.util.stream.Stream` pipeline over an eagerly materialized element list.
///
/// The pipeline is eager: every intermediate operation runs while the stream is built
/// and produces a new `StreamObject`, so `filter`/`map`/`sorted`/`limit` never defer
/// work. Elements are [`Value`]s, so [`crate::Shared::roots`] enumerates their
/// references. There is no lazy source, no parallel execution and no `Spliterator`-backed
/// source after construction; `parallel` records what the caller asked for so
/// `BaseStream.isParallel` can report it.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamObject {
    pub elements: Vec<Value>,
    pub parallel: bool,
    /// The stream interface this object implements.
    ///
    /// `None` is the reference `java.util.stream.Stream`; a `Some(class)` is a primitive
    /// stream interface (`java.util.stream.IntStream` for `String.chars`). The object
    /// reports that class so an `invokeinterface` of the primitive stream's SAM resolves
    /// to its own natives.
    pub class: Option<ClassId>,
}

/// A `java.util.Spliterator` over a snapshot of values and a cursor.
///
/// The VM creates one for `Collection.spliterator` and `Spliterators.*`; a Java object
/// that implements `Spliterator` stays an ordinary `Instance` and is drained through its
/// own `tryAdvance`. `characteristics` is the bit set `Spliterator.characteristics`
/// returns, and `position` is the next index `tryAdvance` consumes.
#[derive(Debug, Clone, PartialEq)]
pub struct SpliteratorObject {
    pub elements: Vec<Value>,
    pub position: usize,
    pub characteristics: i32,
}

/// The kind of a `java.util.stream.Collector` the VM can execute.
///
/// The built-in kinds (`toList`, `toSet`, `joining`, `counting`, `toMap`) carry the result
/// shape directly. `ToCollection` keeps the supplier a caller passed, and `Custom` keeps
/// the four callbacks of `Collector.of`; both run Java code through the interpreter, so
/// [`CollectorObject::references`] reports every callback as a root.
#[derive(Debug, Clone, PartialEq)]
pub enum CollectorKind {
    /// `Collectors.toList()` / `Collectors.toUnmodifiableList()`.
    ToList,
    /// `Collectors.toSet()` / `Collectors.toUnmodifiableSet()`.
    ToSet,
    /// `Collectors.toCollection(Supplier)`: the supplier creates the result collection.
    ToCollection { supplier: ObjRef },
    /// `Collectors.joining(...)`: the delimiter plus the optional prefix and suffix.
    Joining {
        delimiter: String,
        prefix: String,
        suffix: String,
    },
    /// `Collectors.counting()`: the result is a boxed `Long`.
    Counting,
    /// `Collectors.toMap(keyMapper, valueMapper)`: the result is a `HashMap`.
    ToMap { key: ObjRef, value: ObjRef },
    /// `Collector.of(...)`: the callbacks are invoked through the interpreter.
    Custom {
        supplier: ObjRef,
        accumulator: ObjRef,
        combiner: ObjRef,
        finisher: ObjRef,
    },
}

/// One `java.util.stream.Collector`.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectorObject {
    pub kind: CollectorKind,
}

impl CollectorObject {
    /// Every reference this collector holds, for root enumeration.
    pub fn references(&self) -> Vec<ObjRef> {
        match &self.kind {
            CollectorKind::ToCollection { supplier } => vec![*supplier],
            CollectorKind::ToMap { key, value } => vec![*key, *value],
            CollectorKind::Custom {
                supplier,
                accumulator,
                combiner,
                finisher,
            } => vec![*supplier, *accumulator, *combiner, *finisher],
            CollectorKind::ToList
            | CollectorKind::ToSet
            | CollectorKind::Joining { .. }
            | CollectorKind::Counting => Vec::new(),
        }
    }
}

/// An internal `java.util.function.Consumer` that appends its argument to `destination`.
///
/// A VM native cannot execute Java code, so draining a Java `Spliterator` needs a
/// consumer the Java `tryAdvance` can call back into. This object is that consumer: its
/// class identity is `java.util.function.Consumer`, `accept` is a registry native, and
/// the value it receives is pushed onto the destination stream's elements.
#[derive(Debug, Clone, PartialEq)]
pub struct SinkObject {
    pub destination: ObjRef,
}

/// One `java.text.MessageFormat`: the pattern its constructor or `applyPattern` stored.
///
/// The pattern is kept verbatim and parsed on each format call, so `toPattern` returns the
/// caller's text and `applyPattern` is a plain replacement. The Java `Locale` argument is
/// accepted but not stored; see [`crate::format`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFormatObject {
    pub pattern: String,
}

/// One `java.util.regex.Pattern`: its Java source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternObject {
    pub source: String,
}

/// One `java.util.regex.Matcher`: the pattern source, the input, the search position and
/// the groups of the most recent match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatcherObject {
    pub pattern: String,
    pub input: String,
    pub search_from: usize,
    pub matched: bool,
    pub start: usize,
    pub end: usize,
    pub groups: Vec<Option<(usize, usize)>>,
}

/// One `java.util.concurrent.locks.ReentrantLock`.
///
/// Single-threaded until M5: the state records ownership and reentrancy so `isLocked`,
/// `isHeldByCurrentThread` and `getHoldCount` have the JDK's answers, while `lock` and
/// `unlock` never block because there is only one thread.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LockState {
    pub held: bool,
    pub count: usize,
}

/// One `java.util.BitSet`.
///
/// The JDK stores a `long[]`; a sorted set of the set bit indices is the same observable
/// value for `get`/`set` and avoids modelling the word layout. CGLIB builds one while it
/// emits an `equals` helper, and Spring's `MimeType` builds its token table from two.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BitSetObject {
    pub bits: BTreeSet<u32>,
}

/// One `java.util.concurrent.locks.Condition` created by `ReentrantLock.newCondition`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConditionState {
    pub lock: ObjRef,
}

/// One `java.util.concurrent.atomic` value.
///
/// The VM is single-threaded until M5, so a plain cell is enough; the read-modify-write
/// operations are still atomic with respect to instruction dispatch, and the cell is the
/// state a later lock implementation will guard.
#[derive(Debug, Clone, PartialEq)]
pub enum AtomicCell {
    Boolean(bool),
    Integer(i32),
    Long(i64),
    Reference(Value),
}

impl AtomicCell {
    /// The internal name of the class this cell reports.
    pub fn class_name(&self) -> &'static str {
        match self {
            Self::Boolean(_) => "java/util/concurrent/atomic/AtomicBoolean",
            Self::Integer(_) => "java/util/concurrent/atomic/AtomicInteger",
            Self::Long(_) => "java/util/concurrent/atomic/AtomicLong",
            Self::Reference(_) => "java/util/concurrent/atomic/AtomicReference",
        }
    }
}

/// The nine `java.util.logging.Level` constants.
///
/// A level is a tiny immutable value, not an `Instance`, because the C1 `Level` stub has
/// no constructor `Code` to initialize one. The integer values match the JDK's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LevelKind {
    All,
    Finest,
    Finer,
    Fine,
    Config,
    Info,
    Warning,
    Severe,
    Off,
}

impl LevelKind {
    pub const ALL: [LevelKind; 9] = [
        LevelKind::All,
        LevelKind::Finest,
        LevelKind::Finer,
        LevelKind::Fine,
        LevelKind::Config,
        LevelKind::Info,
        LevelKind::Warning,
        LevelKind::Severe,
        LevelKind::Off,
    ];

    /// The JDK constant name (`SEVERE`, `WARNING`, ...).
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Finest => "FINEST",
            Self::Finer => "FINER",
            Self::Fine => "FINE",
            Self::Config => "CONFIG",
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Severe => "SEVERE",
            Self::Off => "OFF",
        }
    }

    /// The JDK numeric level.
    pub fn value(self) -> i32 {
        match self {
            Self::All => i32::MIN,
            Self::Finest => 300,
            Self::Finer => 400,
            Self::Fine => 500,
            Self::Config => 700,
            Self::Info => 800,
            Self::Warning => 900,
            Self::Severe => 1000,
            Self::Off => i32::MAX,
        }
    }
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
    /// A primitive class object (`int.class`, `Class.forName("int")`).
    ///
    /// The object holds only a [`PrimitiveKind`]. There is exactly one object per kind:
    /// [`crate::Shared::primitive_class`] creates and caches them, and
    /// [`crate::Shared::roots`] reports the cache as roots. A primitive class object is
    /// distinct from [`HeapObject::Class`], which stays the representation of a loaded
    /// class's identity.
    PrimitiveClass(PrimitiveKind),
    /// An array class object (`int[]`, `String[][]`).
    ///
    /// The object holds an [`ArrayClass`], which is its component type. There is exactly
    /// one object per array class: [`crate::Shared::array_class`] creates and caches them,
    /// and [`crate::Shared::roots`] reports the cache as roots. An array class object is
    /// distinct from [`HeapObject::Class`] and [`HeapObject::PrimitiveClass`], which stay
    /// the representations of a loaded class and a primitive.
    ArrayClass(ArrayClass),
    /// A `java.lang.reflect.Type` that is not a raw `Class`: a parameterized type, a type
    /// variable, a wildcard or a generic array type.
    ///
    /// A raw class type stays a `Class`/`PrimitiveClass`/`ArrayClass` object because
    /// `java.lang.Class` itself implements `Type`. Cached by
    /// [`crate::Shared::type_object`], never allocated by a caller directly.
    Type(TypeObject),
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
    /// A `java.lang.StringBuffer` (the same `java.lang.AbstractStringBuilder` state as
    /// [`HeapObject::StringBuilder`]).
    ///
    /// `MessageFormat`'s `format(Object, StringBuffer, FieldPosition)` form appends to one
    /// of these. The stored text is valid UTF-8, exactly as for a builder.
    StringBuffer(String),
    /// One `java.text.MessageFormat`: its pattern text.
    ///
    /// The C1 stub declares no instance field for the pattern, so the pattern lives in this
    /// dedicated heap shape. A `Locale` is not stored: the formatting subset is
    /// locale-independent.
    MessageFormat(MessageFormatObject),
    /// A `java.util.List`-family object: its runtime class and elements in index order.
    ///
    /// An element is a [`Value`], so the list can hold a reference (including null) and
    /// that reference must be enumerated by [`crate::Shared::roots`]. Lists are ordinary
    /// heap objects, not permanent roots.
    List(ListObject),
    /// A `java.util.Set`-family object: its runtime class and insertion-ordered elements.
    Set(SetObject),
    /// A `java.util.Map`-family object: its runtime class and insertion-ordered entries.
    ///
    /// A `Vec` of pairs is the whole representation: this slice has no hashing, tree
    /// bins or resizing, and insertion order is stable. Keys and values are [`Value`]s, so
    /// both can be references (null included) and [`crate::Shared::roots`] enumerates the
    /// non-null ones. Maps are ordinary heap objects, not permanent roots.
    Map(MapObject),
    /// A `java.util.Iterator` over a snapshot of values.
    Iterator(IteratorObject),
    /// One `java.util.Map.Entry`.
    MapEntry(MapEntry),
    /// One `java.util.logging.Level` constant.
    LogLevel(LevelKind),
    /// One `java.util.concurrent.atomic` cell.
    Atomic(AtomicCell),
    /// One `ReentrantLock`.
    Lock(LockState),
    /// One `java.util.concurrent.FutureTask` (see [`crate::futures::FutureTaskObject`]).
    FutureTask(FutureTaskObject),
    /// One `java.util.BitSet`: the indices of its set bits.
    BitSet(BitSetObject),
    /// One `Condition` bound to a lock.
    Condition(ConditionState),
    /// One `java.util.regex.Pattern`.
    Pattern(PatternObject),
    /// One `java.util.regex.Matcher`.
    Matcher(MatcherObject),
    /// One `java.util.Enumeration`.
    Enumeration(EnumerationObject),
    /// One eagerly evaluated `java.util.stream.Stream` pipeline.
    Stream(StreamObject),
    /// One `java.util.Spliterator` cursor over a snapshot of values.
    Spliterator(SpliteratorObject),
    /// One `java.util.stream.Collector`.
    Collector(CollectorObject),
    /// One `java.util.Optional`: an absent value or one heap value.
    ///
    /// A present value is a [`Value`], so [`crate::Shared::roots`] enumerates a reference
    /// it holds. `Optional.empty()` allocates a fresh object each call rather than the
    /// JDK's singleton; identity of two empties is not relied on anywhere.
    Optional(Option<Value>),
    /// An internal `java.util.function.Consumer` that appends to a stream (see
    /// [`SinkObject`]). It is never built by application code.
    Sink(SinkObject),
    /// A lambda object created by a `LambdaMetafactory` bootstrap.
    ///
    /// The object stores the functional interface, the implementation method and the
    /// captured call-site arguments. Its class identity is the interface (see
    /// [`LambdaObject`]); its SAM dispatch is handled by the interpreter's
    /// `invokeinterface`, not by a generated class file. Lambdas are ordinary heap
    /// objects, not permanent roots, and [`crate::Shared::roots`] enumerates their
    /// captured references.
    Lambda(LambdaObject),
    /// One `java.lang.invoke.MethodHandles.Lookup` (see [`LookupObject`]).
    Lookup(LookupObject),
    /// One `java.lang.invoke.MethodType` (see [`MethodTypeObject`]).
    MethodType(MethodTypeObject),
    /// One `java.lang.invoke.MethodHandle` (see [`MethodHandleObject`]).
    MethodHandle(MethodHandleObject),
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

    pub fn allocate_array(&mut self, component: ClassRef, length: usize) -> ObjRef {
        let kind = component.array_kind();
        let elements = vec![kind.default_value(); length];
        self.allocate(HeapObject::Array(Array {
            kind,
            component,
            elements,
        }))
    }

    /// Allocate an array class object. Callers go through
    /// [`crate::Shared::array_class`], which caches one object per component.
    pub fn allocate_array_class(&mut self, component: ClassRef) -> ObjRef {
        self.allocate(HeapObject::ArrayClass(ArrayClass::new(component)))
    }

    pub fn allocate_string(&mut self, text: impl Into<String>) -> ObjRef {
        self.allocate(HeapObject::Str(text.into()))
    }

    /// Allocate a class object. Callers go through [`crate::Shared::class_object`],
    /// which caches one object per class identity.
    pub fn allocate_class(&mut self, class: ClassId) -> ObjRef {
        self.allocate(HeapObject::Class(class))
    }

    /// Allocate a primitive class object. Callers go through
    /// [`crate::Shared::primitive_class`], which caches one object per primitive kind.
    pub fn allocate_primitive_class(&mut self, kind: PrimitiveKind) -> ObjRef {
        self.allocate(HeapObject::PrimitiveClass(kind))
    }

    /// Allocate a `java.lang.reflect.Type` object. Callers go through
    /// [`crate::Shared::type_object`], which caches one object per structural type.
    pub fn allocate_type(&mut self, ty: TypeObject) -> ObjRef {
        self.allocate(HeapObject::Type(ty))
    }

    /// Allocate a reflected member. Callers go through
    /// [`crate::Shared::reflect_member`], which caches one object per member.
    pub fn allocate_reflect(&mut self, member: ReflectMember) -> ObjRef {
        self.allocate(HeapObject::Reflect(member))
    }

    /// Allocate a boxed primitive. Callers go through [`crate::Shared::boxed_value`] or the
    /// wrappers' `valueOf` natives, which are the one boxing entry point the reflective
    /// invocation path uses.
    pub fn allocate_boxed(&mut self, boxed: BoxedPrimitive) -> ObjRef {
        self.allocate(HeapObject::Boxed(boxed))
    }

    /// Allocate a boxed `byte`.
    pub fn allocate_boxed_byte(&mut self, value: i8) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Byte(value))
    }

    /// Allocate a boxed `short`.
    pub fn allocate_boxed_short(&mut self, value: i16) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Short(value))
    }

    /// Allocate a boxed `int`.
    pub fn allocate_boxed_int(&mut self, value: i32) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Integer(value))
    }

    /// Allocate a boxed `long`.
    pub fn allocate_boxed_long(&mut self, value: i64) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Long(value))
    }

    /// Allocate a boxed `float`.
    pub fn allocate_boxed_float(&mut self, value: f32) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Float(value))
    }

    /// Allocate a boxed `double`.
    pub fn allocate_boxed_double(&mut self, value: f64) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Double(value))
    }

    /// Allocate a boxed `char`.
    pub fn allocate_boxed_char(&mut self, value: char) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Character(value))
    }

    /// Allocate a boxed `boolean`.
    pub fn allocate_boxed_boolean(&mut self, value: bool) -> ObjRef {
        self.allocate_boxed(BoxedPrimitive::Boolean(value))
    }

    /// Allocate an annotation instance. Callers go through
    /// [`crate::Shared::annotation_object`].
    pub fn allocate_annotation(
        &mut self,
        class: ClassId,
        loader: LoaderId,
        values: Vec<AnnotationValue>,
    ) -> ObjRef {
        self.allocate(HeapObject::Annotation(AnnotationObject {
            class,
            loader,
            values,
        }))
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

    /// Allocate an empty `java.lang.StringBuffer`. [`crate::Shared::allocate_instance`] is
    /// the entry point that creates one for `new` and reflection.
    pub fn allocate_string_buffer(&mut self) -> ObjRef {
        self.allocate(HeapObject::StringBuffer(String::new()))
    }

    /// Allocate a `java.text.MessageFormat` holding `pattern`.
    pub fn allocate_message_format(&mut self, pattern: impl Into<String>) -> ObjRef {
        self.allocate(HeapObject::MessageFormat(MessageFormatObject {
            pattern: pattern.into(),
        }))
    }

    /// Allocate an empty list of the given runtime class.
    pub fn allocate_list(&mut self, class: ClassId) -> ObjRef {
        self.allocate_list_with_fields(class, Vec::new())
    }

    /// Allocate a list whose subclass fields start at their default values.
    pub fn allocate_list_with_fields(&mut self, class: ClassId, fields: Vec<Value>) -> ObjRef {
        self.allocate(HeapObject::List(ListObject {
            class,
            elements: Vec::new(),
            fields,
            capacity: usize::MAX,
        }))
    }

    /// Allocate an empty set of the given runtime class.
    pub fn allocate_set(&mut self, class: ClassId) -> ObjRef {
        self.allocate_set_with_fields(class, Vec::new())
    }

    /// Allocate a set whose subclass fields start at their default values.
    pub fn allocate_set_with_fields(&mut self, class: ClassId, fields: Vec<Value>) -> ObjRef {
        self.allocate(HeapObject::Set(SetObject {
            class,
            elements: Vec::new(),
            fields,
        }))
    }

    /// Allocate an empty map of the given runtime class.
    pub fn allocate_map(&mut self, class: ClassId) -> ObjRef {
        self.allocate_map_with_fields(class, Vec::new())
    }

    /// Allocate a map whose subclass fields start at their default values.
    pub fn allocate_map_with_fields(&mut self, class: ClassId, fields: Vec<Value>) -> ObjRef {
        self.allocate(HeapObject::Map(MapObject {
            class,
            entries: Vec::new(),
            access_order: false,
            fields,
        }))
    }

    /// Allocate an iterator over a snapshot of `items`.
    pub fn allocate_iterator(&mut self, class: ClassId, items: Vec<Value>) -> ObjRef {
        self.allocate_iterator_with_source(class, items, ObjRef::NULL)
    }

    /// Allocate an iterator that remembers the collection its snapshot came from.
    ///
    /// `Iterator.remove()` removes the last returned element from `source` and from the
    /// snapshot, which is what Spring's `AnnotationMetadata.getAnnotatedMethods` set
    /// expects while selecting a `@Bean` method.
    pub fn allocate_iterator_with_source(
        &mut self,
        class: ClassId,
        items: Vec<Value>,
        source: ObjRef,
    ) -> ObjRef {
        self.allocate(HeapObject::Iterator(IteratorObject {
            class,
            items,
            position: 0,
            source,
        }))
    }

    /// Allocate a `Map.Entry` holding one pair.
    pub fn allocate_map_entry(&mut self, key: Value, value: Value) -> ObjRef {
        self.allocate(HeapObject::MapEntry(MapEntry { key, value }))
    }

    /// Allocate one `java.util.logging.Level` constant.
    pub fn allocate_log_level(&mut self, kind: LevelKind) -> ObjRef {
        self.allocate(HeapObject::LogLevel(kind))
    }

    /// Allocate one atomic cell.
    pub fn allocate_atomic(&mut self, cell: AtomicCell) -> ObjRef {
        self.allocate(HeapObject::Atomic(cell))
    }

    /// Allocate one unlocked reentrant lock.
    pub fn allocate_lock(&mut self) -> ObjRef {
        self.allocate(HeapObject::Lock(LockState::default()))
    }

    /// Allocate one empty `java.util.BitSet`.
    pub fn allocate_bit_set(&mut self) -> ObjRef {
        self.allocate(HeapObject::BitSet(BitSetObject::default()))
    }

    /// The state behind a `BitSet` reference.
    pub fn bit_set(&self, reference: ObjRef) -> Result<&BitSetObject, HeapError> {
        match self.get(reference)? {
            HeapObject::BitSet(bits) => Ok(bits),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a BitSet",
            }),
        }
    }

    /// The mutable state behind a `BitSet` reference.
    pub fn bit_set_mut(&mut self, reference: ObjRef) -> Result<&mut BitSetObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::BitSet(bits) => Ok(bits),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a BitSet",
            }),
        }
    }

    /// Allocate one `FutureTask` whose callable is filled in by its constructor.
    pub fn allocate_future_task(&mut self, callable: ObjRef) -> ObjRef {
        self.allocate(HeapObject::FutureTask(FutureTaskObject::new(callable)))
    }

    /// The state behind a `FutureTask` reference.
    pub fn future_task(&self, reference: ObjRef) -> Result<&FutureTaskObject, HeapError> {
        match self.get(reference)? {
            HeapObject::FutureTask(task) => Ok(task),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a FutureTask",
            }),
        }
    }

    /// The mutable state behind a `FutureTask` reference.
    pub fn future_task_mut(
        &mut self,
        reference: ObjRef,
    ) -> Result<&mut FutureTaskObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::FutureTask(task) => Ok(task),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a FutureTask",
            }),
        }
    }

    /// Allocate one condition bound to `lock`.
    pub fn allocate_condition(&mut self, lock: ObjRef) -> ObjRef {
        self.allocate(HeapObject::Condition(ConditionState { lock }))
    }

    /// Allocate an enumeration over a snapshot of values.
    pub fn allocate_enumeration(&mut self, items: Vec<Value>) -> ObjRef {
        self.allocate(HeapObject::Enumeration(EnumerationObject {
            items,
            position: 0,
        }))
    }

    /// The mutable state behind an enumeration reference.
    pub fn enumeration_mut(
        &mut self,
        reference: ObjRef,
    ) -> Result<&mut EnumerationObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Enumeration(enumeration) => Ok(enumeration),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an Enumeration",
            }),
        }
    }

    /// Allocate one eagerly materialized stream pipeline.
    pub fn allocate_stream(
        &mut self,
        elements: Vec<Value>,
        parallel: bool,
        class: Option<ClassId>,
    ) -> ObjRef {
        self.allocate(HeapObject::Stream(StreamObject {
            elements,
            parallel,
            class,
        }))
    }

    /// The state behind a stream reference.
    pub fn stream(&self, reference: ObjRef) -> Result<&StreamObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Stream(stream) => Ok(stream),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Stream",
            }),
        }
    }

    /// The mutable state behind a stream reference.
    pub fn stream_mut(&mut self, reference: ObjRef) -> Result<&mut StreamObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Stream(stream) => Ok(stream),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Stream",
            }),
        }
    }

    /// Allocate a spliterator over a snapshot of `elements`.
    pub fn allocate_spliterator(&mut self, elements: Vec<Value>, characteristics: i32) -> ObjRef {
        self.allocate(HeapObject::Spliterator(SpliteratorObject {
            elements,
            position: 0,
            characteristics,
        }))
    }

    /// The state behind a spliterator reference.
    pub fn spliterator(&self, reference: ObjRef) -> Result<&SpliteratorObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Spliterator(spliterator) => Ok(spliterator),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Spliterator",
            }),
        }
    }

    /// The mutable state behind a spliterator reference.
    pub fn spliterator_mut(
        &mut self,
        reference: ObjRef,
    ) -> Result<&mut SpliteratorObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Spliterator(spliterator) => Ok(spliterator),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Spliterator",
            }),
        }
    }

    /// Allocate one collector.
    pub fn allocate_collector(&mut self, kind: CollectorKind) -> ObjRef {
        self.allocate(HeapObject::Collector(CollectorObject { kind }))
    }

    /// The state behind a collector reference.
    pub fn collector(&self, reference: ObjRef) -> Result<&CollectorObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Collector(collector) => Ok(collector),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Collector",
            }),
        }
    }

    /// Allocate one optional holding `value`, or one absent optional.
    pub fn allocate_optional(&mut self, value: Option<Value>) -> ObjRef {
        self.allocate(HeapObject::Optional(value))
    }

    /// The value an optional holds, or `None` when it is absent.
    pub fn optional(&self, reference: ObjRef) -> Result<Option<Value>, HeapError> {
        match self.get(reference)? {
            HeapObject::Optional(value) => Ok(*value),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an Optional",
            }),
        }
    }

    /// Allocate the internal consumer that appends to `destination`.
    pub fn allocate_sink(&mut self, destination: ObjRef) -> ObjRef {
        self.allocate(HeapObject::Sink(SinkObject { destination }))
    }

    /// The destination behind an internal consumer reference.
    pub fn sink(&self, reference: ObjRef) -> Result<&SinkObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Sink(sink) => Ok(sink),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a consumer sink",
            }),
        }
    }

    /// Allocate one compiled-pattern object.
    pub fn allocate_pattern(&mut self, source: String) -> ObjRef {
        self.allocate(HeapObject::Pattern(PatternObject { source }))
    }

    /// Allocate one matcher over `input`.
    pub fn allocate_matcher(&mut self, pattern: String, input: String) -> ObjRef {
        self.allocate(HeapObject::Matcher(MatcherObject {
            pattern,
            input,
            search_from: 0,
            matched: false,
            start: 0,
            end: 0,
            groups: Vec::new(),
        }))
    }

    /// The state behind a matcher reference.
    pub fn matcher(&self, reference: ObjRef) -> Result<&MatcherObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Matcher(matcher) => Ok(matcher),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Matcher",
            }),
        }
    }

    /// The mutable state behind a matcher reference.
    pub fn matcher_mut(&mut self, reference: ObjRef) -> Result<&mut MatcherObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Matcher(matcher) => Ok(matcher),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Matcher",
            }),
        }
    }

    /// The source text behind a pattern reference.
    pub fn pattern(&self, reference: ObjRef) -> Result<&str, HeapError> {
        match self.get(reference)? {
            HeapObject::Pattern(pattern) => Ok(&pattern.source),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a Pattern",
            }),
        }
    }

    /// The state behind a lock reference.
    pub fn lock_state(&self, reference: ObjRef) -> Result<&LockState, HeapError> {
        match self.get(reference)? {
            HeapObject::Lock(state) => Ok(state),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a ReentrantLock",
            }),
        }
    }

    /// The mutable state behind a lock reference.
    pub fn lock_state_mut(&mut self, reference: ObjRef) -> Result<&mut LockState, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Lock(state) => Ok(state),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a ReentrantLock",
            }),
        }
    }

    /// The cell behind an atomic reference.
    pub fn atomic(&self, reference: ObjRef) -> Result<&AtomicCell, HeapError> {
        match self.get(reference)? {
            HeapObject::Atomic(cell) => Ok(cell),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an atomic cell",
            }),
        }
    }

    /// The mutable cell behind an atomic reference.
    pub fn atomic_mut(&mut self, reference: ObjRef) -> Result<&mut AtomicCell, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Atomic(cell) => Ok(cell),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an atomic cell",
            }),
        }
    }

    /// Allocate a lambda object. Callers go through [`crate::Shared::allocate_lambda`],
    /// which is the one creation entry point the metafactory path uses.
    pub fn allocate_lambda(&mut self, lambda: LambdaObject) -> ObjRef {
        self.allocate(HeapObject::Lambda(lambda))
    }

    /// Allocate one `MethodHandles.Lookup`. Callers go through
    /// [`crate::method_handles`], which is the one creation entry point.
    pub fn allocate_lookup(&mut self, lookup: LookupObject) -> ObjRef {
        self.allocate(HeapObject::Lookup(lookup))
    }

    /// Allocate one `MethodType`. Method types are ordinary heap objects: no cache owns
    /// them and a second construction returns a second object.
    pub fn allocate_method_type(&mut self, method_type: MethodTypeObject) -> ObjRef {
        self.allocate(HeapObject::MethodType(method_type))
    }

    /// Allocate one `MethodHandle`, holding its target identity and invocation type.
    pub fn allocate_method_handle(&mut self, handle: MethodHandleObject) -> ObjRef {
        self.allocate(HeapObject::MethodHandle(handle))
    }

    /// The state behind a `MethodHandles.Lookup` reference.
    pub fn lookup(&self, reference: ObjRef) -> Result<&LookupObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Lookup(lookup) => Ok(lookup),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a MethodHandles.Lookup",
            }),
        }
    }

    /// The state behind a `MethodType` reference.
    pub fn method_type(&self, reference: ObjRef) -> Result<&MethodTypeObject, HeapError> {
        match self.get(reference)? {
            HeapObject::MethodType(method_type) => Ok(method_type),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a MethodType",
            }),
        }
    }

    /// The state behind a `MethodHandle` reference.
    pub fn method_handle(&self, reference: ObjRef) -> Result<&MethodHandleObject, HeapError> {
        match self.get(reference)? {
            HeapObject::MethodHandle(handle) => Ok(handle),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a MethodHandle",
            }),
        }
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

    /// The text behind a `java.lang.StringBuffer` reference.
    pub fn string_buffer(&self, reference: ObjRef) -> Result<&str, HeapError> {
        match self.get(reference)? {
            HeapObject::StringBuffer(text) => Ok(text),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a String buffer",
            }),
        }
    }

    /// The mutable text behind a `java.lang.StringBuffer` reference.
    pub fn string_buffer_mut(&mut self, reference: ObjRef) -> Result<&mut String, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::StringBuffer(text) => Ok(text),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a String buffer",
            }),
        }
    }

    /// The state behind a `java.text.MessageFormat` reference.
    pub fn message_format(&self, reference: ObjRef) -> Result<&MessageFormatObject, HeapError> {
        match self.get(reference)? {
            HeapObject::MessageFormat(format) => Ok(format),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a MessageFormat",
            }),
        }
    }

    /// The mutable state behind a `java.text.MessageFormat` reference.
    pub fn message_format_mut(
        &mut self,
        reference: ObjRef,
    ) -> Result<&mut MessageFormatObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::MessageFormat(format) => Ok(format),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a MessageFormat",
            }),
        }
    }

    /// The runtime class of an instance or VM container.
    pub fn object_class(&self, reference: ObjRef) -> Result<ClassId, HeapError> {
        match self.get(reference)? {
            HeapObject::Instance(instance) => Ok(instance.class),
            HeapObject::List(list) => Ok(list.class),
            HeapObject::Set(set) => Ok(set.class),
            HeapObject::Map(map) => Ok(map.class),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an instance or container",
            }),
        }
    }

    /// The instance fields behind an instance or VM container.
    pub fn object_fields(&self, reference: ObjRef) -> Result<&[Value], HeapError> {
        match self.get(reference)? {
            HeapObject::Instance(instance) => Ok(&instance.fields),
            HeapObject::List(list) => Ok(&list.fields),
            HeapObject::Set(set) => Ok(&set.fields),
            HeapObject::Map(map) => Ok(&map.fields),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an instance or container",
            }),
        }
    }

    /// The mutable instance fields behind an instance or VM container.
    pub fn object_fields_mut(&mut self, reference: ObjRef) -> Result<&mut Vec<Value>, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Instance(instance) => Ok(&mut instance.fields),
            HeapObject::List(list) => Ok(&mut list.fields),
            HeapObject::Set(set) => Ok(&mut set.fields),
            HeapObject::Map(map) => Ok(&mut map.fields),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an instance or container",
            }),
        }
    }

    /// The elements behind a list reference.
    pub fn list(&self, reference: ObjRef) -> Result<&[Value], HeapError> {
        match self.get(reference)? {
            HeapObject::List(list) => Ok(&list.elements),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a list",
            }),
        }
    }

    /// The mutable elements behind a list reference.
    pub fn list_mut(&mut self, reference: ObjRef) -> Result<&mut Vec<Value>, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::List(list) => Ok(&mut list.elements),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a list",
            }),
        }
    }

    /// The element capacity of a list-family container.
    ///
    /// An ordinary list is unbounded and reports [`usize::MAX`]; a bounded
    /// `LinkedBlockingQueue` reports the capacity its constructor was given.
    pub fn list_capacity(&self, reference: ObjRef) -> Result<usize, HeapError> {
        match self.get(reference)? {
            HeapObject::List(list) => Ok(list.capacity),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a list",
            }),
        }
    }

    /// Set a list-family container's element capacity.
    pub fn set_list_capacity(
        &mut self,
        reference: ObjRef,
        capacity: usize,
    ) -> Result<(), HeapError> {
        match self.get_mut(reference)? {
            HeapObject::List(list) => {
                list.capacity = capacity;
                Ok(())
            }
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a list",
            }),
        }
    }

    /// The elements behind a set reference.
    pub fn set(&self, reference: ObjRef) -> Result<&[Value], HeapError> {
        match self.get(reference)? {
            HeapObject::Set(set) => Ok(&set.elements),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a set",
            }),
        }
    }

    /// The mutable elements behind a set reference.
    pub fn set_mut(&mut self, reference: ObjRef) -> Result<&mut Vec<Value>, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Set(set) => Ok(&mut set.elements),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a set",
            }),
        }
    }

    /// The `(key, value)` entries behind a map reference.
    pub fn map(&self, reference: ObjRef) -> Result<&[(Value, Value)], HeapError> {
        match self.get(reference)? {
            HeapObject::Map(map) => Ok(&map.entries),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a map",
            }),
        }
    }

    /// The runtime class of a VM container, if `reference` is one.
    pub fn container_class(&self, reference: ObjRef) -> Result<ClassId, HeapError> {
        match self.get(reference)? {
            HeapObject::List(list) => Ok(list.class),
            HeapObject::Set(set) => Ok(set.class),
            HeapObject::Map(map) => Ok(map.class),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a list, set or map",
            }),
        }
    }

    /// The mutable `(key, value)` entries behind a map reference.
    pub fn map_mut(&mut self, reference: ObjRef) -> Result<&mut Vec<(Value, Value)>, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Map(map) => Ok(&mut map.entries),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a map",
            }),
        }
    }

    /// The `LinkedHashMap` access-order flag behind a map reference.
    pub fn map_access_order(&self, reference: ObjRef) -> Result<bool, HeapError> {
        match self.get(reference)? {
            HeapObject::Map(map) => Ok(map.access_order),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a map",
            }),
        }
    }

    /// Set the `LinkedHashMap` access-order flag behind a map reference.
    pub fn set_map_access_order(
        &mut self,
        reference: ObjRef,
        access_order: bool,
    ) -> Result<(), HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Map(map) => {
                map.access_order = access_order;
                Ok(())
            }
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a map",
            }),
        }
    }

    /// The mutable iterator state behind an iterator reference.
    pub fn iterator_mut(&mut self, reference: ObjRef) -> Result<&mut IteratorObject, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Iterator(iterator) => Ok(iterator),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an iterator",
            }),
        }
    }

    /// The two values behind a `Map.Entry` reference.
    pub fn map_entry(&self, reference: ObjRef) -> Result<&MapEntry, HeapError> {
        match self.get(reference)? {
            HeapObject::MapEntry(entry) => Ok(entry),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a map entry",
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

    /// The lambda object behind a reference.
    pub fn lambda(&self, reference: ObjRef) -> Result<&LambdaObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Lambda(lambda) => Ok(lambda),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a lambda",
            }),
        }
    }

    pub fn string_mut(&mut self, reference: ObjRef) -> Result<&mut String, HeapError> {
        match self.get_mut(reference)? {
            HeapObject::Str(text) => Ok(text),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a String",
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

    /// The primitive kind behind a primitive class object.
    ///
    /// A loaded class object is a different heap object kind, so this reports a typed
    /// error for it rather than inventing a primitive.
    pub fn primitive_class(&self, reference: ObjRef) -> Result<PrimitiveKind, HeapError> {
        match self.get(reference)? {
            HeapObject::PrimitiveClass(kind) => Ok(*kind),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a primitive class object",
            }),
        }
    }

    /// The array class behind an array class object.
    ///
    /// A loaded class object and a primitive class object are different heap kinds, so
    /// this reports a typed error for them rather than inventing a component.
    pub fn array_class(&self, reference: ObjRef) -> Result<&ArrayClass, HeapError> {
        match self.get(reference)? {
            HeapObject::ArrayClass(array) => Ok(array),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "an array class object",
            }),
        }
    }

    /// The type behind a `java.lang.reflect.Type` object.
    ///
    /// A raw class type is a class object, not this heap kind, so it reports a typed
    /// error here; callers that accept both use [`crate::generic`].
    pub fn type_object(&self, reference: ObjRef) -> Result<&TypeObject, HeapError> {
        match self.get(reference)? {
            HeapObject::Type(ty) => Ok(ty),
            _ => Err(HeapError::WrongObjectKind {
                reference,
                expected: "a reflected type",
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
