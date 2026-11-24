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
/// M2.10 models one box: `java.lang.Integer` holding its primitive `int`. A box is an
/// ordinary heap object rather than a second [`crate::Value`] variant, so it can live in
/// an `Object`-typed slot and `getClass` reports `java.lang.Integer`. There is no
/// `Boolean` or `Long` box yet and no small-value cache: every `valueOf` allocates a new
/// object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxedPrimitive {
    Integer(i32),
    Character(char),
    Boolean(bool),
}

impl BoxedPrimitive {
    /// The internal class name of the box's runtime class.
    pub fn class_name(self) -> &'static str {
        match self {
            Self::Integer(_) => "java/lang/Integer",
            Self::Character(_) => "java/lang/Character",
            Self::Boolean(_) => "java/lang/Boolean",
        }
    }

    /// The dotted binary name of the box's runtime class.
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Integer(_) => "java.lang.Integer",
            Self::Character(_) => "java.lang.Character",
            Self::Boolean(_) => "java.lang.Boolean",
        }
    }

    /// The primitive `int` this box holds.
    pub fn as_int(self) -> Option<i32> {
        match self {
            Self::Integer(value) => Some(value),
            Self::Character(_) => None,
            Self::Boolean(_) => None,
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

/// A `java.util.List`-family VM object: its runtime class and its elements in index
/// order.
///
/// The class is stored rather than derived from the shape, so `ArrayList`,
/// `LinkedList` and `CopyOnWriteArrayList` keep their own identity while sharing one
/// representation. [`crate::Shared::allocate_instance`] is the only creation entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ListObject {
    pub class: ClassId,
    pub elements: Vec<Value>,
    /// The subclass's instance fields, in layout order. An exact `ArrayList` has none; a
    /// Spring subclass that adds `this$0` or a cache field keeps them here.
    pub fields: Vec<Value>,
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
#[derive(Debug, Clone, PartialEq)]
pub struct MapObject {
    pub class: ClassId,
    pub entries: Vec<(Value, Value)>,
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
    /// One `Condition` bound to a lock.
    Condition(ConditionState),
    /// One `java.util.regex.Pattern`.
    Pattern(PatternObject),
    /// One `java.util.regex.Matcher`.
    Matcher(MatcherObject),
    /// One `java.util.Enumeration`.
    Enumeration(EnumerationObject),
    /// A lambda object created by a `LambdaMetafactory` bootstrap.
    ///
    /// The object stores the functional interface, the implementation method and the
    /// captured call-site arguments. Its class identity is the interface (see
    /// [`LambdaObject`]); its SAM dispatch is handled by the interpreter's
    /// `invokeinterface`, not by a generated class file. Lambdas are ordinary heap
    /// objects, not permanent roots, and [`crate::Shared::roots`] enumerates their
    /// captured references.
    Lambda(LambdaObject),
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

    /// Allocate a boxed `int`. Callers go through [`crate::Shared::box_int`], which is
    /// the one boxing entry point the reflection natives use.
    pub fn allocate_boxed_int(&mut self, value: i32) -> ObjRef {
        self.allocate(HeapObject::Boxed(BoxedPrimitive::Integer(value)))
    }

    /// Allocate a boxed `char`.
    pub fn allocate_boxed_char(&mut self, value: char) -> ObjRef {
        self.allocate(HeapObject::Boxed(BoxedPrimitive::Character(value)))
    }

    /// Allocate a boxed `boolean`.
    pub fn allocate_boxed_boolean(&mut self, value: bool) -> ObjRef {
        self.allocate(HeapObject::Boxed(BoxedPrimitive::Boolean(value)))
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
            fields,
        }))
    }

    /// Allocate an iterator over a snapshot of `items`.
    pub fn allocate_iterator(&mut self, class: ClassId, items: Vec<Value>) -> ObjRef {
        self.allocate(HeapObject::Iterator(IteratorObject {
            class,
            items,
            position: 0,
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
