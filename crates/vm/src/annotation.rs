//! Annotation reflection — M2.11.
//!
//! This module owns four things:
//!
//! - **Annotation objects.** [`crate::HeapObject::Annotation`] carries the annotation
//!   type's class identity and the element values the class file encodes, exactly as the
//!   M2.1 metadata model decoded them. [`crate::Shared::annotation_object`] allocates one.
//! - **`AnnotatedElement` queries.** `Class`, `Field`, `Method` and `Constructor` each get
//!   `getDeclaredAnnotations`, `getAnnotations` and `getAnnotation(Class)`. The answers
//!   come from the same typed metadata the C1 gate compares, so there is no second copy of
//!   the annotations.
//! - **Annotation member dispatch.** `invokevirtual` on an annotation object resolves to
//!   its annotation type and then to this module: `annotationType`, `equals`, `hashCode`,
//!   `toString` and every element method are answered here. An element the class file
//!   omits falls back to the annotation type's `AnnotationDefault`.
//! - **Enum element values.** A `e` element value becomes a cached
//!   [`crate::EnumConstant`] object, so two reads of one annotation element are
//!   reference-identical and `java.lang.Enum.name` can report the constant.
//!
//! Deliberate limits of this slice: only runtime-visible annotations are exposed (a
//! `RuntimeInvisibleAnnotations` entry has no annotation object, as on the JDK); array
//! and primitive *class literals* (`[I`, `int.class`) are still typed errors in this
//! annotation path even though `Class` itself now models them; `getAnnotationsByType`/
//! repeatable containers, parameter annotations,
//! type annotations, `isAnnotationPresent`, records and generic `Type` graphs stay out.
//! `Annotation.equals` compares the effective element values (declared or default), so it
//! agrees with itself across two queries, but `hashCode` is a deterministic hash of that
//! same encoding rather than the JDK's `(127 * name.hashCode()) ^ valueHashCode` formula.
//! `toString` prints only the explicitly encoded elements, like the JDK.

use std::fmt::Write as _;

use rava_classfile::metadata::{Annotation, ElementValue};

use crate::VmError;
use crate::heap::{
    AnnotationObject, ArrayKind, ClassRef, HeapError, MemberKind, ObjRef, ReflectMember,
};
use crate::loader::{ClassId, LoaderId};
use crate::reflection::{allocate_reference_array, parameter_descriptors, return_descriptor};
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// Internal name of `java.lang.annotation.Annotation`.
const ANNOTATION_CLASS: &str = "java/lang/annotation/Annotation";

/// Descriptor of `Annotation[]`, the `AnnotatedElement` array return type.
const ANNOTATION_ARRAY: &str = "[Ljava/lang/annotation/Annotation;";

/// The descriptor `@Inherited` uses when it annotates an annotation type.
const INHERITED_DESCRIPTOR: &str = "Ljava/lang/annotation/Inherited;";

/// Registers the M2.11 annotation natives.
///
/// The entries are keyed by the class that declares the Java method. `Class`, `Field`,
/// `Method` and `Constructor` each declare the three `AnnotatedElement` queries in the C1
/// stub, so each owner gets its own entry; the annotation object's members are answered by
/// [`invoke_member`], which the interpreter consults for every annotation receiver before
/// it falls back to `Code`.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        ANNOTATION_CLASS,
        "annotationType",
        "()Ljava/lang/Class;",
        annotation_type,
    );
    shared.register_native(
        ANNOTATION_CLASS,
        "equals",
        "(Ljava/lang/Object;)Z",
        annotation_equals,
    );
    shared.register_native(ANNOTATION_CLASS, "hashCode", "()I", annotation_hash_code);
    shared.register_native(
        ANNOTATION_CLASS,
        "toString",
        "()Ljava/lang/String;",
        annotation_to_string,
    );

    for owner in [
        "java/lang/Class",
        "java/lang/reflect/Field",
        "java/lang/reflect/Method",
        "java/lang/reflect/Constructor",
    ] {
        shared.register_native(
            owner,
            "getDeclaredAnnotations",
            format!("(){ANNOTATION_ARRAY}"),
            get_declared_annotations,
        );
        shared.register_native(
            owner,
            "getAnnotations",
            format!("(){ANNOTATION_ARRAY}"),
            get_annotations,
        );
        shared.register_native(
            owner,
            "getAnnotation",
            format!("(Ljava/lang/Class;){}", &ANNOTATION_ARRAY[1..]),
            get_annotation,
        );
    }

    // `Field`, `Method` and `Constructor` do not redeclare `getAnnotations`: their stub
    // inherits it from `java.lang.reflect.AccessibleObject`, which is where virtual
    // resolution finds the declaration.
    shared.register_native(
        "java/lang/reflect/AccessibleObject",
        "getAnnotations",
        format!("(){ANNOTATION_ARRAY}"),
        get_annotations,
    );

    // `Enum.name` is what makes an `e` element value observable: the object stores the
    // constant's name, not the enum's static field value.
    shared.register_native("java/lang/Enum", "name", "()Ljava/lang/String;", enum_name);
    shared.register_native(
        "java/lang/Enum",
        "toString",
        "()Ljava/lang/String;",
        enum_name,
    );
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

/// The `AnnotatedElement` a query receiver denotes.
enum AnnotatedElement {
    Class(ClassId),
    Member(ReflectMember),
}

/// Classify a receiver as a class object or a reflected member.
fn element_of(shared: &Shared, receiver: ObjRef) -> Result<AnnotatedElement, VmError> {
    let heap = shared.heap();
    match heap.get(receiver).map_err(heap_error)? {
        crate::HeapObject::Class(class) => Ok(AnnotatedElement::Class(*class)),
        crate::HeapObject::Reflect(member) => Ok(AnnotatedElement::Member(member.clone())),
        _ => Err(VmError::Native(format!(
            "{receiver} is not a class object or a reflected member"
        ))),
    }
}

/// The loader that resolves an element's annotation type descriptors.
fn element_loader(shared: &Shared, element: &AnnotatedElement) -> Result<LoaderId, VmError> {
    let class = match element {
        AnnotatedElement::Class(class) => *class,
        AnnotatedElement::Member(member) => member.class,
    };
    Ok(shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?
        .loader)
}

/// The runtime-visible annotations encoded on the element, in class-file order.
///
/// `getDeclaredAnnotations` and the member queries use this list; `Class.getAnnotations`
/// additionally merges inherited superclass annotations through [`inherited_annotations`].
fn declared_annotations(
    shared: &Shared,
    element: &AnnotatedElement,
) -> Result<Vec<Annotation>, VmError> {
    match element {
        AnnotatedElement::Class(class) => {
            let entry = shared
                .entry(*class)
                .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
            Ok(entry.metadata()?.annotations)
        }
        AnnotatedElement::Member(member) => {
            let entry = shared.entry(member.class).ok_or_else(|| {
                VmError::Loader(format!("class id {} is not loaded", member.class.0))
            })?;
            let metadata = entry.metadata()?;
            match member.kind {
                MemberKind::Field => {
                    let field = metadata
                        .field(&member.name)
                        .filter(|field| field.descriptor == member.descriptor)
                        .ok_or_else(|| {
                            VmError::Native(format!(
                                "class {} has no field {}:{}",
                                entry.name, member.name, member.descriptor
                            ))
                        })?;
                    Ok(field.annotations.clone())
                }
                MemberKind::Method | MemberKind::Constructor => {
                    let method = metadata
                        .method(&member.name, &member.descriptor)
                        .ok_or_else(|| {
                            VmError::Native(format!(
                                "class {} has no method {}:{}",
                                entry.name, member.name, member.descriptor
                            ))
                        })?;
                    Ok(method.annotations.clone())
                }
            }
        }
    }
}

/// Annotations inherited from superclasses, nearest first.
///
/// JVMS does not inherit annotations; `Class.getAnnotations` does, for an annotation type
/// that is itself annotated with `@Inherited`. The nearest declaration of an annotation
/// type wins, so a superclass annotation is skipped once the same type is already present
/// on a closer class.
fn inherited_annotations(
    shared: &Shared,
    class: ClassId,
    declared: &[Annotation],
) -> Result<Vec<Annotation>, VmError> {
    let mut inherited = Vec::new();
    let mut present: Vec<String> = declared
        .iter()
        .map(|annotation| annotation.type_descriptor.clone())
        .collect();
    let mut current = class;
    loop {
        let entry = shared
            .entry(current)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", current.0)))?;
        let Some(super_name) = entry.file.super_class_name()? else {
            break;
        };
        let super_name = super_name.to_owned();
        let loader = entry.loader;
        let super_class = shared.load_class(loader, &super_name)?;
        let metadata = shared
            .entry(super_class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", super_class.0)))?
            .metadata()?;
        for annotation in &metadata.annotations {
            if present.contains(&annotation.type_descriptor) {
                continue;
            }
            if !is_inherited(shared, loader, &annotation.type_descriptor)? {
                continue;
            }
            present.push(annotation.type_descriptor.clone());
            inherited.push(annotation.clone());
        }
        current = super_class;
    }
    Ok(inherited)
}

/// Whether the annotation type is annotated with `@Inherited`.
fn is_inherited(shared: &Shared, loader: LoaderId, type_descriptor: &str) -> Result<bool, VmError> {
    let class = load_descriptor_class(shared, loader, type_descriptor)?;
    let metadata = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?
        .metadata()?;
    Ok(metadata
        .annotations
        .iter()
        .any(|annotation| annotation.type_descriptor == INHERITED_DESCRIPTOR))
}

/// The annotations `getAnnotations` answers: declared ones, then inherited ones for a
/// class receiver. A member has no superclass view, so its list is the declared list.
fn visible_annotations(
    shared: &Shared,
    element: &AnnotatedElement,
    include_inherited: bool,
) -> Result<Vec<Annotation>, VmError> {
    let declared = declared_annotations(shared, element)?;
    if !include_inherited {
        return Ok(declared);
    }
    match element {
        AnnotatedElement::Class(class) => {
            let inherited = inherited_annotations(shared, *class, &declared)?;
            let mut merged = declared;
            merged.extend(inherited);
            Ok(merged)
        }
        AnnotatedElement::Member(_) => Ok(declared),
    }
}

/// Allocate one annotation object per encoded annotation.
fn annotation_objects(
    shared: &Shared,
    loader: LoaderId,
    annotations: &[Annotation],
) -> Result<Vec<ObjRef>, VmError> {
    let mut objects = Vec::with_capacity(annotations.len());
    for annotation in annotations {
        let class = load_descriptor_class(shared, loader, &annotation.type_descriptor)?;
        let object = shared.annotation_object(class, annotation.values.clone())?;
        objects.push(object);
    }
    Ok(objects)
}

/// Allocate the `Annotation[]` result for an `AnnotatedElement` query.
fn annotation_array(
    shared: &Shared,
    element: &AnnotatedElement,
    include_inherited: bool,
) -> Result<Value, VmError> {
    let loader = element_loader(shared, element)?;
    let annotations = visible_annotations(shared, element, include_inherited)?;
    let objects = annotation_objects(shared, loader, &annotations)?;
    let component = ClassRef::Named(shared.load_library_class(ANNOTATION_CLASS)?);
    let array = allocate_reference_array(shared, component, &objects)?;
    Ok(Value::Ref(array))
}

fn get_declared_annotations(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let element = element_of(shared, receiver)?;
    Ok(Some(annotation_array(shared, &element, false)?))
}

fn get_annotations(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let element = element_of(shared, receiver)?;
    Ok(Some(annotation_array(shared, &element, true)?))
}

/// `AnnotatedElement.getAnnotation(Class)`: the annotation of that type, or null.
///
/// The search follows `getAnnotations`, so `Class.getAnnotation` sees an inherited
/// annotation while a member query does not. A non-class argument is a typed error; a
/// class that is not an annotation type simply finds nothing.
fn get_annotation(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let element = element_of(shared, receiver)?;
    let Some(Value::Ref(wanted)) = args.first().copied() else {
        return Err(VmError::Native(
            "getAnnotation expects a Class argument".to_owned(),
        ));
    };
    let wanted = {
        let heap = shared.heap();
        heap.class(wanted).map_err(heap_error)?
    };
    let loader = element_loader(shared, &element)?;
    let include_inherited = matches!(element, AnnotatedElement::Class(_));
    let annotations = visible_annotations(shared, &element, include_inherited)?;
    for annotation in &annotations {
        let class = load_descriptor_class(shared, loader, &annotation.type_descriptor)?;
        if class != wanted {
            continue;
        }
        let object = shared.annotation_object(class, annotation.values.clone())?;
        return Ok(Some(Value::Ref(object)));
    }
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

/// Answer one annotation instance's member, or `None` when the name is not an annotation
/// member (so `Object.getClass` still resolves through the annotation type).
///
/// The interpreter calls this for every receiver that is a
/// [`crate::HeapObject::Annotation`], before it resolves the method on the class chain,
/// because `annotationType` lives on the `Annotation` interface rather than on the
/// annotation type's superclass chain.
pub fn invoke_member(
    shared: &Shared,
    annotation: &AnnotationObject,
    name: &str,
    descriptor: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    match (name, descriptor) {
        ("annotationType", "()Ljava/lang/Class;") => {
            let object = shared.class_object(annotation.class)?;
            Ok(Some(Value::Ref(object)))
        }
        ("equals", "(Ljava/lang/Object;)Z") => {
            let Some(Value::Ref(other)) = args.first().copied() else {
                return Err(VmError::Native(
                    "Annotation.equals expects an Object argument".to_owned(),
                ));
            };
            let equal = match shared.annotation(other)? {
                Some(other) => {
                    annotation_key(shared, annotation)? == annotation_key(shared, &other)?
                }
                None => false,
            };
            Ok(Some(Value::Int(i32::from(equal))))
        }
        ("hashCode", "()I") => {
            let key = annotation_key(shared, annotation)?;
            let mut hash: u32 = 0x811C_9DC5;
            for byte in key.as_bytes() {
                hash ^= u32::from(*byte);
                hash = hash.wrapping_mul(0x0100_0193);
            }
            Ok(Some(Value::Int(hash as i32)))
        }
        ("toString", "()Ljava/lang/String;") => {
            let text = annotation_text(shared, annotation)?;
            Ok(Some(Value::Ref(shared.heap().allocate_string(text))))
        }
        _ => element_member(shared, annotation, name, descriptor),
    }
}

/// Answer an element method (`name()`), falling back to `AnnotationDefault`.
fn element_member(
    shared: &Shared,
    annotation: &AnnotationObject,
    name: &str,
    descriptor: &str,
) -> Result<Option<Value>, VmError> {
    let entry = shared
        .entry(annotation.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", annotation.class.0)))?;
    let metadata = entry.metadata()?;
    let Some(method) = metadata
        .methods
        .iter()
        .find(|method| method.name == name && is_element_descriptor(&method.descriptor))
    else {
        return Ok(None);
    };
    let expected = return_descriptor(&method.descriptor)?;
    if format!("(){expected}") != descriptor {
        return Err(VmError::Native(format!(
            "annotation element {} has descriptor (){expected}, not {descriptor}",
            method.name
        )));
    }
    let Some(value) = effective_element(annotation, method) else {
        return Err(VmError::Native(format!(
            "annotation {} element {name} has neither a value nor a default",
            entry.name
        )));
    };
    let value = element_to_value(shared, entry.loader, expected, &value)?;
    Ok(Some(value))
}

/// Whether a method descriptor is an annotation element method's `()T` shape.
fn is_element_descriptor(descriptor: &str) -> bool {
    parameter_descriptors(descriptor)
        .map(|parameters| parameters.is_empty())
        .unwrap_or(false)
}

/// The element's encoded value, or the annotation type's declared default.
fn effective_element(
    annotation: &AnnotationObject,
    method: &rava_classfile::MethodMetadata,
) -> Option<ElementValue> {
    annotation
        .values
        .iter()
        .find(|pair| pair.name == method.name)
        .map(|pair| pair.value.clone())
        .or_else(|| method.annotation_default.clone())
}

/// Decode one annotation element value into a runtime value.
///
/// `descriptor` is the element's declared type and decides an array's `ArrayKind`. Every
/// other variant is self-describing; `Class` uses its own literal descriptor because a
/// `Class[]` element's declared type is `Ljava/lang/Class;` while each literal names the
/// class it denotes.
fn element_to_value(
    shared: &Shared,
    loader: LoaderId,
    descriptor: &str,
    value: &ElementValue,
) -> Result<Value, VmError> {
    match value {
        ElementValue::Boolean(value) => Ok(Value::Int(i32::from(*value))),
        ElementValue::Byte(value) => Ok(Value::Int(i32::from(*value))),
        ElementValue::Char(value) => Ok(Value::Int(i32::from(*value))),
        ElementValue::Short(value) => Ok(Value::Int(i32::from(*value))),
        ElementValue::Int(value) => Ok(Value::Int(*value)),
        ElementValue::Long(value) => Ok(Value::Long(*value)),
        ElementValue::Float(value) => Ok(Value::Float(*value)),
        ElementValue::Double(value) => Ok(Value::Double(*value)),
        ElementValue::String(text) => Ok(Value::Ref(shared.intern_string(text))),
        ElementValue::Class(literal) => {
            let object = class_object_for_descriptor(shared, loader, literal)?;
            Ok(Value::Ref(object))
        }
        ElementValue::Enum {
            type_descriptor,
            constant,
        } => {
            let class = load_descriptor_class(shared, loader, type_descriptor)?;
            let object = shared.enum_constant(class, constant)?;
            Ok(Value::Ref(object))
        }
        ElementValue::Annotation(nested) => {
            let class = load_descriptor_class(shared, loader, &nested.type_descriptor)?;
            let object = shared.annotation_object(class, nested.values.clone())?;
            Ok(Value::Ref(object))
        }
        ElementValue::Array(items) => {
            let element_descriptor = descriptor.strip_prefix('[').ok_or_else(|| {
                VmError::Native(format!(
                    "annotation array value has non-array descriptor {descriptor}"
                ))
            })?;
            let kind = array_kind_of(element_descriptor)?;
            // The array's component is the declared element type, so `getClass()` on an
            // annotation element array reports the same array class as a member type.
            let component = shared.class_ref_for_descriptor(loader, element_descriptor)?;
            debug_assert_eq!(component.array_kind(), kind);
            // Convert before taking the heap lock: `element_to_value` allocates, so the
            // two lock scopes must not nest.
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(element_to_value(shared, loader, element_descriptor, item)?);
            }
            let mut heap = shared.heap();
            let array = heap.allocate_array(component, values.len());
            {
                let allocated = heap.array_mut(array).map_err(heap_error)?;
                for (slot, value) in allocated.elements.iter_mut().zip(values) {
                    *slot = value;
                }
            }
            Ok(Value::Ref(array))
        }
    }
}

/// The `ArrayKind` an array element descriptor addresses.
fn array_kind_of(descriptor: &str) -> Result<ArrayKind, VmError> {
    Ok(match descriptor.as_bytes().first().copied() {
        Some(b'I') => ArrayKind::Int,
        Some(b'J') => ArrayKind::Long,
        Some(b'F') => ArrayKind::Float,
        Some(b'D') => ArrayKind::Double,
        Some(b'B') => ArrayKind::Byte,
        Some(b'C') => ArrayKind::Char,
        Some(b'S') => ArrayKind::Short,
        Some(b'Z') => ArrayKind::Boolean,
        Some(b'L' | b'[') => ArrayKind::Reference,
        _ => {
            return Err(VmError::Native(format!(
                "bad annotation array element descriptor {descriptor}"
            )));
        }
    })
}

/// Resolve an object descriptor (`Lname;`) to its class identity.
fn load_descriptor_class(
    shared: &Shared,
    loader: LoaderId,
    descriptor: &str,
) -> Result<ClassId, VmError> {
    let internal = descriptor
        .strip_prefix('L')
        .and_then(|rest| rest.strip_suffix(';'))
        .ok_or_else(|| {
            VmError::Native(format!(
                "annotation descriptor {descriptor} does not name an object class"
            ))
        })?;
    shared.load_class(loader, internal)
}

/// Resolve an object descriptor to its `Class` object.
///
/// The annotation path predates the M2.15/M2.16 class identity and stays on object
/// descriptors only, so an array or primitive class literal (`[I`, `int.class`) is a
/// typed error here rather than a wrong `Class` object.
fn class_object_for_descriptor(
    shared: &Shared,
    loader: LoaderId,
    descriptor: &str,
) -> Result<ObjRef, VmError> {
    let Some(internal) = descriptor
        .strip_prefix('L')
        .and_then(|rest| rest.strip_suffix(';'))
    else {
        return Err(VmError::Native(format!(
            "class literals for {descriptor} are not supported yet (M2.11)"
        )));
    };
    let class = shared.load_class(loader, internal)?;
    shared.class_object(class)
}

/// A deterministic encoding of an annotation instance's effective element values.
///
/// The key resolves defaults, so an explicitly encoded value that equals its default
/// compares equal to an omitted element. It is the basis of `Annotation.equals` and
/// `hashCode`; it is never exposed to Java code.
fn annotation_key(shared: &Shared, annotation: &AnnotationObject) -> Result<String, VmError> {
    let entry = shared
        .entry(annotation.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", annotation.class.0)))?;
    let metadata = entry.metadata()?;
    // The class identity, not just the binary name: two loaders may define the same
    // annotation type name as two distinct types.
    let mut key = format!("{}:{}", annotation.class.0, metadata.binary_name);
    for method in &metadata.methods {
        if !is_element_descriptor(&method.descriptor)
            || method.is_constructor()
            || method.is_class_initializer()
        {
            continue;
        }
        let descriptor = return_descriptor(&method.descriptor)?;
        let Some(value) = effective_element(annotation, method) else {
            continue;
        };
        let _ = write!(key, "|{}:{descriptor}=", method.name);
        write_key(&mut key, &value);
    }
    Ok(key)
}

fn write_key(key: &mut String, value: &ElementValue) {
    match value {
        ElementValue::Boolean(value) => {
            let _ = write!(key, "Z{value}");
        }
        ElementValue::Byte(value) => {
            let _ = write!(key, "B{value}");
        }
        ElementValue::Char(value) => {
            let _ = write!(key, "C{value}");
        }
        ElementValue::Short(value) => {
            let _ = write!(key, "S{value}");
        }
        ElementValue::Int(value) => {
            let _ = write!(key, "I{value}");
        }
        ElementValue::Long(value) => {
            let _ = write!(key, "J{value}");
        }
        ElementValue::Float(value) => {
            let _ = write!(key, "F{}", value.to_bits());
        }
        ElementValue::Double(value) => {
            let _ = write!(key, "D{}", value.to_bits());
        }
        ElementValue::String(text) => {
            let _ = write!(key, "s{text:?}");
        }
        ElementValue::Class(descriptor) => {
            let _ = write!(key, "c{descriptor}");
        }
        ElementValue::Enum {
            type_descriptor,
            constant,
        } => {
            let _ = write!(key, "e{type_descriptor}.{constant}");
        }
        ElementValue::Annotation(nested) => {
            let _ = write!(key, "@{}(", nested.type_descriptor);
            for pair in &nested.values {
                let _ = write!(key, "{}=", pair.name);
                write_key(key, &pair.value);
                key.push(',');
            }
            key.push(')');
        }
        ElementValue::Array(items) => {
            key.push('[');
            for item in items {
                write_key(key, item);
                key.push(',');
            }
            key.push(']');
        }
    }
}

/// `Annotation.toString`: `@dotted.Type(name="x", count=1)` over the encoded elements.
fn annotation_text(shared: &Shared, annotation: &AnnotationObject) -> Result<String, VmError> {
    let entry = shared
        .entry(annotation.class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", annotation.class.0)))?;
    let mut text = String::from("@");
    text.push_str(&entry.name.replace('/', "."));
    if !annotation.values.is_empty() {
        text.push('(');
        for (index, pair) in annotation.values.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            let _ = write!(text, "{}=", pair.name);
            write_text(&mut text, &pair.value);
        }
        text.push(')');
    }
    Ok(text)
}

fn write_text(text: &mut String, value: &ElementValue) {
    match value {
        ElementValue::Boolean(value) => {
            let _ = write!(text, "{value}");
        }
        ElementValue::Byte(value) => {
            let _ = write!(text, "{value}");
        }
        ElementValue::Char(value) => {
            let _ = write!(text, "{}", char::from_u32(u32::from(*value)).unwrap_or('?'));
        }
        ElementValue::Short(value) => {
            let _ = write!(text, "{value}");
        }
        ElementValue::Int(value) => {
            let _ = write!(text, "{value}");
        }
        ElementValue::Long(value) => {
            let _ = write!(text, "{value}");
        }
        ElementValue::Float(value) => {
            let _ = write!(text, "{value:?}");
        }
        ElementValue::Double(value) => {
            let _ = write!(text, "{value:?}");
        }
        ElementValue::String(value) => {
            let _ = write!(text, "{value:?}");
        }
        ElementValue::Class(descriptor) => {
            let name = descriptor
                .strip_prefix('L')
                .and_then(|rest| rest.strip_suffix(';'))
                .unwrap_or(descriptor);
            let _ = write!(text, "class {}", name.replace('/', "."));
        }
        ElementValue::Enum {
            type_descriptor,
            constant,
        } => {
            let name = type_descriptor
                .strip_prefix('L')
                .and_then(|rest| rest.strip_suffix(';'))
                .unwrap_or(type_descriptor);
            let _ = write!(text, "{}.{constant}", name.replace('/', "."));
        }
        ElementValue::Annotation(nested) => {
            let name = nested
                .type_descriptor
                .strip_prefix('L')
                .and_then(|rest| rest.strip_suffix(';'))
                .unwrap_or(&nested.type_descriptor);
            let _ = write!(text, "@{}", name.replace('/', "."));
            if !nested.values.is_empty() {
                text.push('(');
                for (index, pair) in nested.values.iter().enumerate() {
                    if index > 0 {
                        text.push_str(", ");
                    }
                    let _ = write!(text, "{}=", pair.name);
                    write_text(text, &pair.value);
                }
                text.push(')');
            }
        }
        ElementValue::Array(items) => {
            text.push('{');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    text.push_str(", ");
                }
                write_text(text, item);
            }
            text.push('}');
        }
    }
}

fn annotation_type(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let annotation = annotation_receiver(shared, receiver)?;
    let object = shared.class_object(annotation.class)?;
    Ok(Some(Value::Ref(object)))
}

fn annotation_equals(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let annotation = annotation_receiver(shared, receiver)?;
    invoke_member(shared, &annotation, "equals", "(Ljava/lang/Object;)Z", args)
}

fn annotation_hash_code(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let annotation = annotation_receiver(shared, receiver)?;
    invoke_member(shared, &annotation, "hashCode", "()I", &[])
}

fn annotation_to_string(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let annotation = annotation_receiver(shared, receiver)?;
    invoke_member(shared, &annotation, "toString", "()Ljava/lang/String;", &[])
}

/// The annotation instance behind a registered `Annotation` native's receiver.
fn annotation_receiver(shared: &Shared, receiver: ObjRef) -> Result<AnnotationObject, VmError> {
    shared
        .annotation(receiver)?
        .ok_or_else(|| VmError::Native(format!("{receiver} is not an annotation instance")))
}

/// `Enum.name()` and `Enum.toString()`: the stored constant name.
fn enum_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let constant = {
        let heap = shared.heap();
        heap.enum_constant(receiver).map_err(heap_error)?.clone()
    };
    Ok(Some(Value::Ref(shared.intern_string(&constant.name))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_descriptor_shape() {
        assert!(is_element_descriptor("()Ljava/lang/String;"));
        assert!(!is_element_descriptor("(I)V"));
    }

    #[test]
    fn array_kinds_follow_the_element_descriptor() {
        assert_eq!(array_kind_of("I").unwrap(), ArrayKind::Int);
        assert_eq!(array_kind_of("[I").unwrap(), ArrayKind::Reference);
        assert_eq!(
            array_kind_of("Ljava/lang/String;").unwrap(),
            ArrayKind::Reference
        );
        assert!(array_kind_of("Q").is_err());
    }
}
