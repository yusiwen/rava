//! The typed C1 metadata model behind reflection (ADR-0006).
//!
//! A [`ClassFile`] keeps attributes as raw bytes, which is right for parsing and wrong for
//! everything that compares metadata. This module decodes the attributes ADR-0006 lists —
//! annotations of every shape, generic signatures, records, sealed permits, nestmates,
//! inner and enclosing classes, exceptions, parameter annotations, parameter names and
//! source file — into a typed model, and [`dump`] renders that model as stable text for a
//! normalized JDK diff.
//!
//! Three properties are deliberate:
//!
//! - **Complete.** Every attribute is still copied verbatim into `attributes` on the class,
//!   field, method and record-component nodes, so an attribute this model does not decode
//!   is never lost and a later slice can add a decoder without re-reading the class file.
//! - **Validated.** Decoding checks bounds, constant-pool kinds, UTF-8 through the pool,
//!   element-value tags and ranges, `type_annotation` targets and paths, and that a payload
//!   is consumed exactly. A malformed attribute is an [`Error`], never a panic.
//! - **Ordered as encoded.** Annotations keep declaration order and annotation element
//!   pairs keep their encoded order; members keep class-file order. Only the sets whose
//!   order is semantically irrelevant (nest members, permitted subclasses, inner classes)
//!   are sorted, and only by the dump.
//!
//! Generic signatures are retained as the `Signature` string and parsed by
//! [`mod@signature`] into a typed graph; resolving that graph to loaded classes and the
//! reflective `java.lang.reflect.Type` objects is the VM's generic-reflection module.

mod decode;
mod dump;
mod signature;

pub use dump::dump;
pub use signature::{
    ClassSignature, ClassTypeSignature, MAX_TYPE_DEPTH, MethodSignature, PrimitiveSignature,
    SignatureError, TypeArgument, TypeParameter, TypeSignature, parse_class_signature,
    parse_field_signature, parse_method_signature,
};

use crate::error::Error;
use crate::model::ClassFile;

const ACC_PUBLIC: u16 = 0x0001;
const ACC_INTERFACE: u16 = 0x0200;
const ACC_ANNOTATION: u16 = 0x2000;
const ACC_ENUM: u16 = 0x4000;

/// One annotation, as encoded: its type descriptor and its element values in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    /// The annotation type's field descriptor, exactly as encoded (`Ldemo/Defaults;`).
    pub type_descriptor: String,
    /// Element values in encoded order; the model never sorts them.
    pub values: Vec<AnnotationValue>,
}

impl Annotation {
    /// The value of the pair named `name`, if the annotation encodes one.
    ///
    /// A missing pair means "use the element's `AnnotationDefault`", which is why this
    /// returns an `Option` instead of a defaulted value.
    pub fn value(&self, name: &str) -> Option<&ElementValue> {
        self.values
            .iter()
            .find(|pair| pair.name == name)
            .map(|pair| &pair.value)
    }
}

/// One `element_name_index`/`element_value` pair of an annotation (JVMS §4.7.16.1).
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationValue {
    pub name: String,
    pub value: ElementValue,
}

/// An `element_value` union (JVMS §4.7.16.1), tagged the way the class file tags it.
///
/// The tag is part of the variant, so `int` 1 and `byte` 1 do not compare equal: the
/// element's declared type decides the tag, and a model that normalized it would hide a
/// mismatched annotation from the C1 diff.
#[derive(Debug, Clone, PartialEq)]
pub enum ElementValue {
    /// `Z`, a `CONSTANT_Integer` interpreted as a boolean (`0` is false).
    Boolean(bool),
    /// `B`, a `CONSTANT_Integer` narrowed to a signed byte.
    Byte(i8),
    /// `C`, a `CONSTANT_Integer` read as one UTF-16 code unit. A lone surrogate is a
    /// legal code unit here and is not a Rust `char`, which is why this is not `char`.
    Char(u16),
    /// `S`, a `CONSTANT_Integer` narrowed to a signed short.
    Short(i16),
    /// `I`.
    Int(i32),
    /// `J`.
    Long(i64),
    /// `F`.
    Float(f32),
    /// `D`.
    Double(f64),
    /// `s`, a `CONSTANT_Utf8`.
    String(String),
    /// `c`, a field descriptor for the class literal (`Ljava/lang/String;`, `[I`).
    Class(String),
    /// `e`, an enum constant: the enum type's field descriptor and the constant name.
    Enum {
        type_descriptor: String,
        constant: String,
    },
    /// `@`, a nested annotation.
    Annotation(Annotation),
    /// `[`, an array; nested arrays keep their nesting.
    Array(Vec<ElementValue>),
}

/// One `type_annotation` (JVMS §4.7.20).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAnnotation {
    /// The encoded `target_type` byte, kept because it disambiguates targets that share a
    /// shape (`0x13` field, `0x14` method return, `0x15` method receiver) and because a
    /// normalized diff should compare it exactly.
    pub target_type: u8,
    pub target: TypeAnnotationTarget,
    /// The type path from the outermost type to the annotated part, outermost first.
    pub path: Vec<TypePathEntry>,
    pub annotation: Annotation,
}

/// The `target_info` union of a `type_annotation`, decoded per `target_type`.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnnotationTarget {
    /// `0x00`.
    ClassTypeParameter { index: u8 },
    /// `0x01`.
    MethodTypeParameter { index: u8 },
    /// `0x10`; index `0xFFFF` is the direct superclass, otherwise a superinterface by
    /// position in the `interfaces` array.
    Supertype { index: u16 },
    /// `0x11` (class) and `0x12` (method).
    TypeParameterBound {
        parameter_index: u8,
        bound_index: u8,
    },
    /// `0x13` (field), `0x14` (method return type) and `0x15` (method receiver).
    Empty,
    /// `0x16`.
    FormalParameter { index: u8 },
    /// `0x17`, an index into the `Exceptions` table.
    Throws { index: u16 },
    /// `0x40` (local variable) and `0x41` (resource variable).
    LocalVariable { table: Vec<LocalVariableTarget> },
    /// `0x42`, an index into the `Code` exception table.
    Catch { index: u16 },
    /// `0x43`..=`0x46`, a bytecode offset (`instanceof`, `new`, method reference, cast).
    Offset { offset: u16 },
    /// `0x47`..=`0x4B`, a bytecode offset plus the type-argument position.
    TypeArgument { offset: u16, argument_index: u8 },
}

/// One `localvar_target` table row (JVMS §4.7.20.1).
#[derive(Debug, Clone, PartialEq)]
pub struct LocalVariableTarget {
    pub start_pc: u16,
    pub length: u16,
    pub index: u16,
}

/// One entry of a `type_path` (JVMS §4.7.20.2).
#[derive(Debug, Clone, PartialEq)]
pub struct TypePathEntry {
    pub kind: TypePathKind,
    /// Always `0` unless `kind` is [`TypePathKind::TypeArgument`].
    pub argument_index: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypePathKind {
    /// `0`: a deeper array element.
    Array,
    /// `1`: an inner type.
    Nested,
    /// `2`: a wildcard bound.
    Wildcard,
    /// `3`: a type argument.
    TypeArgument,
}

/// One `InnerClasses` row (JVMS §4.7.6).
#[derive(Debug, Clone, PartialEq)]
pub struct InnerClass {
    /// Internal name of the inner class; never absent in a valid row.
    pub inner_class: String,
    /// `None` when the row names a class that is not a member of another class.
    pub outer_class: Option<String>,
    /// `None` for an anonymous or local class, which has no simple name in the class file.
    pub inner_name: Option<String>,
    pub access_flags: u16,
}

/// An `EnclosingMethod` attribute (JVMS §4.7.7).
#[derive(Debug, Clone, PartialEq)]
pub struct EnclosingMethod {
    /// Internal name of the class that declares the enclosing method.
    pub class: String,
    /// `None` when the class is immediately enclosed by a type declaration rather than by
    /// a method or initializer.
    pub method: Option<MethodNameAndDescriptor>,
}

/// The `NameAndType` an enclosing method names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodNameAndDescriptor {
    pub name: String,
    pub descriptor: String,
}

/// One `Record` component (JVMS §4.7.30), with its own signature and annotations.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordComponent {
    pub name: String,
    pub descriptor: String,
    pub signature: Option<String>,
    pub annotations: Vec<Annotation>,
    pub invisible_annotations: Vec<Annotation>,
    pub type_annotations: Vec<TypeAnnotation>,
    pub invisible_type_annotations: Vec<TypeAnnotation>,
    pub attributes: Vec<RawAttribute>,
}

/// One `MethodParameters` entry (JVMS §4.7.24).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParameter {
    /// `None` when `name_index` is zero, which is what a class compiled without
    /// `-parameters` records.
    pub name: Option<String>,
    pub access_flags: u16,
}

/// The `ConstantValue` of a field (JVMS §4.7.2).
#[derive(Debug, Clone, PartialEq)]
pub enum ConstantValue {
    Integer(i32),
    Float(f32),
    Long(i64),
    Double(f64),
    String(String),
}

/// An attribute kept verbatim: its resolved name and its payload.
///
/// The model copies payloads so that a [`ClassMetadata`] is self-contained and can outlive
/// the buffer a class file was parsed from. `Code` is the only large attribute and is the
/// interpreter's view; the metadata model never decodes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAttribute {
    pub name: String,
    pub data: Vec<u8>,
}

/// The typed metadata of one class file.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassMetadata {
    pub major_version: u16,
    pub minor_version: u16,
    pub access_flags: u16,
    /// The internal form of the binary name from `this_class` (`demo/Kitchen`), which is
    /// what every other class file references. [`Self::java_name`] is `Class.getName()`.
    pub binary_name: String,
    pub super_class: Option<String>,
    /// Interfaces in declaration order; the order is semantic for default methods.
    pub interfaces: Vec<String>,
    /// The `Signature` attribute's string, or `None` when the class is not generic.
    pub signature: Option<String>,
    pub annotations: Vec<Annotation>,
    pub invisible_annotations: Vec<Annotation>,
    pub type_annotations: Vec<TypeAnnotation>,
    pub invisible_type_annotations: Vec<TypeAnnotation>,
    pub nest_host: Option<String>,
    pub nest_members: Vec<String>,
    /// `Some` when the class carries `PermittedSubclasses`; `None` for a class that is not
    /// sealed. Sealing is the attribute's presence, not a flag.
    pub permitted_subclasses: Option<Vec<String>>,
    pub inner_classes: Vec<InnerClass>,
    pub enclosing_method: Option<EnclosingMethod>,
    /// `Some` when the class carries a `Record` attribute, even with zero components.
    pub record_components: Option<Vec<RecordComponent>>,
    pub source_file: Option<String>,
    /// True when the `Deprecated` attribute is present. The `ACC_SYNTHETIC` flag is in
    /// `access_flags`, not here.
    pub deprecated: bool,
    pub fields: Vec<FieldMetadata>,
    pub methods: Vec<MethodMetadata>,
    pub attributes: Vec<RawAttribute>,
}

impl RecordComponent {
    /// Parse this record component's `Signature`, or `None` when its type is not generic.
    pub fn generic_type(&self) -> Result<Option<TypeSignature>, SignatureError> {
        self.signature
            .as_deref()
            .map(parse_field_signature)
            .transpose()
    }
}

impl ClassMetadata {
    /// Parse this class's `Signature`, or `None` when it is not generic.
    ///
    /// A malformed signature is a typed [`SignatureError`]; the string is not silently
    /// ignored, because a reflection client would otherwise see raw types where the class
    /// file declares generic ones.
    pub fn class_signature(&self) -> Result<Option<ClassSignature>, SignatureError> {
        self.signature
            .as_deref()
            .map(parse_class_signature)
            .transpose()
    }

    /// `Class.getName()`: package separators as `.`, nested classes still `$`-separated.
    pub fn java_name(&self) -> String {
        self.binary_name.replace('/', ".")
    }

    pub fn is_interface(&self) -> bool {
        self.access_flags & ACC_INTERFACE != 0
    }

    pub fn is_annotation_type(&self) -> bool {
        self.access_flags & ACC_ANNOTATION != 0
    }

    pub fn is_enum(&self) -> bool {
        self.access_flags & ACC_ENUM != 0
    }

    pub fn is_public(&self) -> bool {
        self.access_flags & ACC_PUBLIC != 0
    }

    /// `Class.isRecord()`: the `Record` attribute's presence.
    pub fn is_record(&self) -> bool {
        self.record_components.is_some()
    }

    /// `Class.isSealed()`: the `PermittedSubclasses` attribute's presence.
    pub fn is_sealed(&self) -> bool {
        self.permitted_subclasses.is_some()
    }

    pub fn annotation(&self, type_descriptor: &str) -> Option<&Annotation> {
        self.annotations
            .iter()
            .chain(&self.invisible_annotations)
            .find(|annotation| annotation.type_descriptor == type_descriptor)
    }

    pub fn field(&self, name: &str) -> Option<&FieldMetadata> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn method(&self, name: &str, descriptor: &str) -> Option<&MethodMetadata> {
        self.methods
            .iter()
            .find(|method| method.name == name && method.descriptor == descriptor)
    }

    /// Every method named `name`, in class-file order (constructors included).
    pub fn methods_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a MethodMetadata> {
        self.methods
            .iter()
            .filter(move |method| method.name == name)
    }

    pub fn record_component(&self, name: &str) -> Option<&RecordComponent> {
        self.record_components
            .as_deref()?
            .iter()
            .find(|component| component.name == name)
    }
}

/// The typed metadata of one field.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldMetadata {
    pub access_flags: u16,
    pub name: String,
    pub descriptor: String,
    pub signature: Option<String>,
    pub annotations: Vec<Annotation>,
    pub invisible_annotations: Vec<Annotation>,
    pub type_annotations: Vec<TypeAnnotation>,
    pub invisible_type_annotations: Vec<TypeAnnotation>,
    pub constant_value: Option<ConstantValue>,
    pub deprecated: bool,
    pub attributes: Vec<RawAttribute>,
}

impl FieldMetadata {
    /// Parse this field's `Signature`, or `None` when its type is not generic.
    pub fn generic_type(&self) -> Result<Option<TypeSignature>, SignatureError> {
        self.signature
            .as_deref()
            .map(parse_field_signature)
            .transpose()
    }

    /// `java.lang.reflect.Modifier.isStatic`.
    pub fn is_static(&self) -> bool {
        self.access_flags & 0x0008 != 0
    }

    pub fn is_public(&self) -> bool {
        self.access_flags & ACC_PUBLIC != 0
    }
}

/// The typed metadata of one method or constructor.
///
/// Constructors are methods named `<init>`, exactly as the class file records them; there
/// is no separate constructor node, because `Method` and `Constructor` share every
/// attribute this model decodes.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodMetadata {
    pub access_flags: u16,
    pub name: String,
    pub descriptor: String,
    pub signature: Option<String>,
    pub annotations: Vec<Annotation>,
    pub invisible_annotations: Vec<Annotation>,
    pub type_annotations: Vec<TypeAnnotation>,
    pub invisible_type_annotations: Vec<TypeAnnotation>,
    /// One annotation list per encoded parameter (JVMS §4.7.18). `None` when the method
    /// has no `RuntimeVisibleParameterAnnotations` attribute, which is different from the
    /// attribute being present with no annotations.
    pub parameter_annotations: Option<Vec<Vec<Annotation>>>,
    pub invisible_parameter_annotations: Option<Vec<Vec<Annotation>>>,
    pub annotation_default: Option<ElementValue>,
    /// `Exceptions` in declaration order.
    pub exceptions: Vec<String>,
    /// `MethodParameters`, when the class was compiled with `-parameters`.
    pub parameters: Option<Vec<MethodParameter>>,
    pub deprecated: bool,
    pub attributes: Vec<RawAttribute>,
}

impl MethodMetadata {
    /// Parse this method's or constructor's `Signature`, or `None` when it is not generic.
    pub fn method_signature(&self) -> Result<Option<MethodSignature>, SignatureError> {
        self.signature
            .as_deref()
            .map(parse_method_signature)
            .transpose()
    }

    pub fn is_constructor(&self) -> bool {
        self.name == "<init>"
    }

    pub fn is_class_initializer(&self) -> bool {
        self.name == "<clinit>"
    }

    pub fn is_static(&self) -> bool {
        self.access_flags & 0x0008 != 0
    }

    pub fn is_abstract(&self) -> bool {
        self.access_flags & 0x0400 != 0
    }

    pub fn is_bridge(&self) -> bool {
        self.access_flags & 0x0040 != 0
    }

    pub fn is_varargs(&self) -> bool {
        self.access_flags & 0x0080 != 0
    }

    pub fn is_synthetic(&self) -> bool {
        self.access_flags & 0x1000 != 0
    }

    /// The `MethodParameters` names, with `None` for an unnamed parameter.
    pub fn parameter_names(&self) -> Option<Vec<Option<&str>>> {
        self.parameters.as_ref().map(|parameters| {
            parameters
                .iter()
                .map(|parameter| parameter.name.as_deref())
                .collect()
        })
    }
}

/// The metadata model of a parsed class.
///
/// Decoding is eager and fallible: a malformed attribute payload is reported as
/// [`Error::BadAttribute`] instead of being ignored, so the C1 gate cannot pass on a class
/// file the model only half understood.
impl ClassFile {
    pub fn metadata(&self) -> Result<ClassMetadata, Error> {
        ClassMetadata::from_class_file(self)
    }

    /// The deterministic normalized dump of this class, built from its metadata model.
    ///
    /// This is the artifact the C1 metadata gate compares; see [`dump`] for the
    /// normalization rules.
    pub fn metadata_dump(&self) -> Result<String, Error> {
        Ok(dump(&self.metadata()?))
    }
}
