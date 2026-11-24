//! Typed decoders for the attributes ADR-0006 lists.
//!
//! Every decoder takes the attribute it is decoding only for its `data`; the caller owns
//! the attribute name so that a failure can be reported as
//! [`Error::BadAttribute`](crate::Error::BadAttribute) with the name attached.

use std::collections::HashSet;

use crate::constant_pool::Constant;
use crate::error::Error;
use crate::model::{Attribute, ClassFile, Field, Method};
use crate::reader::Reader;

use super::{
    Annotation, AnnotationValue, ClassMetadata, ConstantValue, ElementValue, EnclosingMethod,
    FieldMetadata, InnerClass, LocalVariableTarget, MethodMetadata, MethodNameAndDescriptor,
    MethodParameter, RawAttribute, RecordComponent, TypeAnnotation, TypeAnnotationTarget,
    TypePathEntry, TypePathKind,
};

/// Class attributes this model decodes. An attribute with one of these names may appear at
/// most once (JVMS §4.7).
pub(super) const CLASS_ATTRIBUTES: &[&str] = &[
    "Signature",
    "SourceFile",
    "NestHost",
    "NestMembers",
    "PermittedSubclasses",
    "InnerClasses",
    "EnclosingMethod",
    "Record",
    "RuntimeVisibleAnnotations",
    "RuntimeInvisibleAnnotations",
    "RuntimeVisibleTypeAnnotations",
    "RuntimeInvisibleTypeAnnotations",
    "Deprecated",
];

/// Field attributes this model decodes.
pub(super) const FIELD_ATTRIBUTES: &[&str] = &[
    "Signature",
    "RuntimeVisibleAnnotations",
    "RuntimeInvisibleAnnotations",
    "RuntimeVisibleTypeAnnotations",
    "RuntimeInvisibleTypeAnnotations",
    "ConstantValue",
    "Deprecated",
];

/// Method and constructor attributes this model decodes.
pub(super) const METHOD_ATTRIBUTES: &[&str] = &[
    "Signature",
    "RuntimeVisibleAnnotations",
    "RuntimeInvisibleAnnotations",
    "RuntimeVisibleTypeAnnotations",
    "RuntimeInvisibleTypeAnnotations",
    "RuntimeVisibleParameterAnnotations",
    "RuntimeInvisibleParameterAnnotations",
    "AnnotationDefault",
    "Exceptions",
    "MethodParameters",
    "Deprecated",
];

/// `Record` component attributes this model decodes.
pub(super) const COMPONENT_ATTRIBUTES: &[&str] = &[
    "Signature",
    "RuntimeVisibleAnnotations",
    "RuntimeInvisibleAnnotations",
    "RuntimeVisibleTypeAnnotations",
    "RuntimeInvisibleTypeAnnotations",
];

/// Attributes that are the parser's or the interpreter's view, not metadata. The model
/// keeps them raw but never reports them as unmodeled metadata.
pub(super) const NOT_METADATA_ATTRIBUTES: &[&str] = &[
    "Code",
    "BootstrapMethods",
    "Module",
    "ModulePackages",
    "ModuleMainClass",
];

impl ClassMetadata {
    /// Decodes the C1 metadata of `file`.
    ///
    /// Every attribute payload is copied verbatim onto the node that owns it before it is
    /// decoded, so an attribute this model does not know survives into the result. A
    /// duplicate of a decoded attribute, or a payload that does not decode or does not
    /// consume exactly its length, is an error rather than a silently dropped attribute.
    pub fn from_class_file(file: &ClassFile) -> Result<Self, Error> {
        let mut metadata = Self {
            major_version: file.major_version,
            minor_version: file.minor_version,
            access_flags: file.access_flags,
            binary_name: file.this_class_name()?.to_owned(),
            super_class: file.super_class_name()?.map(str::to_owned),
            interfaces: file
                .interface_names()?
                .into_iter()
                .map(str::to_owned)
                .collect(),
            signature: None,
            annotations: Vec::new(),
            invisible_annotations: Vec::new(),
            type_annotations: Vec::new(),
            invisible_type_annotations: Vec::new(),
            nest_host: None,
            nest_members: Vec::new(),
            permitted_subclasses: None,
            inner_classes: Vec::new(),
            enclosing_method: None,
            record_components: None,
            source_file: None,
            deprecated: false,
            fields: Vec::new(),
            methods: Vec::new(),
            attributes: Vec::new(),
        };

        let mut seen: HashSet<&str> = HashSet::new();
        for attribute in &file.attributes {
            let name = file.attribute_name(attribute)?;
            metadata.attributes.push(raw(attribute, name));
            once(&mut seen, name, CLASS_ATTRIBUTES)?;
            if !CLASS_ATTRIBUTES.contains(&name) {
                continue;
            }
            match name {
                "Signature" => {
                    metadata.signature = Some(wrap(name, decode_utf8_attribute(file, attribute))?);
                }
                "SourceFile" => {
                    metadata.source_file =
                        Some(wrap(name, decode_utf8_attribute(file, attribute))?);
                }
                "NestHost" => {
                    metadata.nest_host = Some(wrap(name, decode_class_index(file, attribute))?);
                }
                "NestMembers" => {
                    metadata.nest_members = wrap(name, decode_class_list(file, attribute))?;
                }
                "PermittedSubclasses" => {
                    metadata.permitted_subclasses =
                        Some(wrap(name, decode_class_list(file, attribute))?);
                }
                "InnerClasses" => {
                    metadata.inner_classes = wrap(name, decode_inner_classes(file, attribute))?;
                }
                "EnclosingMethod" => {
                    metadata.enclosing_method =
                        Some(wrap(name, decode_enclosing_method(file, attribute))?);
                }
                "Record" => {
                    metadata.record_components = Some(wrap(name, decode_record(file, attribute))?);
                }
                "RuntimeVisibleAnnotations" => {
                    metadata.annotations = wrap(name, decode_annotations(file, attribute))?;
                }
                "RuntimeInvisibleAnnotations" => {
                    metadata.invisible_annotations =
                        wrap(name, decode_annotations(file, attribute))?;
                }
                "RuntimeVisibleTypeAnnotations" => {
                    metadata.type_annotations =
                        wrap(name, decode_type_annotations(file, attribute))?;
                }
                "RuntimeInvisibleTypeAnnotations" => {
                    metadata.invisible_type_annotations =
                        wrap(name, decode_type_annotations(file, attribute))?;
                }
                "Deprecated" => {
                    metadata.deprecated = wrap(name, decode_deprecated(attribute))?;
                }
                // `once` only admits names in CLASS_ATTRIBUTES, so this arm is unreachable
                // until the list and the match disagree; keeping it avoids a panic.
                _ => {}
            }
        }

        for field in &file.fields {
            metadata.fields.push(decode_field(file, field)?);
        }
        for method in &file.methods {
            metadata.methods.push(decode_method(file, method)?);
        }
        Ok(metadata)
    }
}

fn decode_field(file: &ClassFile, field: &Field) -> Result<FieldMetadata, Error> {
    let mut metadata = FieldMetadata {
        access_flags: field.access_flags,
        name: file.field_name(field)?.to_owned(),
        descriptor: file.field_descriptor(field)?.to_owned(),
        signature: None,
        annotations: Vec::new(),
        invisible_annotations: Vec::new(),
        type_annotations: Vec::new(),
        invisible_type_annotations: Vec::new(),
        constant_value: None,
        deprecated: false,
        attributes: Vec::new(),
    };

    let mut seen: HashSet<&str> = HashSet::new();
    for attribute in &field.attributes {
        let name = file.attribute_name(attribute)?;
        metadata.attributes.push(raw(attribute, name));
        once(&mut seen, name, FIELD_ATTRIBUTES)?;
        match name {
            "Signature" => {
                metadata.signature = Some(wrap(name, decode_utf8_attribute(file, attribute))?);
            }
            "RuntimeVisibleAnnotations" => {
                metadata.annotations = wrap(name, decode_annotations(file, attribute))?;
            }
            "RuntimeInvisibleAnnotations" => {
                metadata.invisible_annotations = wrap(name, decode_annotations(file, attribute))?;
            }
            "RuntimeVisibleTypeAnnotations" => {
                metadata.type_annotations = wrap(name, decode_type_annotations(file, attribute))?;
            }
            "RuntimeInvisibleTypeAnnotations" => {
                metadata.invisible_type_annotations =
                    wrap(name, decode_type_annotations(file, attribute))?;
            }
            "ConstantValue" => {
                metadata.constant_value = Some(wrap(name, decode_constant_value(file, attribute))?);
            }
            "Deprecated" => {
                metadata.deprecated = wrap(name, decode_deprecated(attribute))?;
            }
            _ => {}
        }
    }
    Ok(metadata)
}

fn decode_method(file: &ClassFile, method: &Method) -> Result<MethodMetadata, Error> {
    let mut metadata = MethodMetadata {
        access_flags: method.access_flags,
        name: file.method_name(method)?.to_owned(),
        descriptor: file.method_descriptor(method)?.to_owned(),
        signature: None,
        annotations: Vec::new(),
        invisible_annotations: Vec::new(),
        type_annotations: Vec::new(),
        invisible_type_annotations: Vec::new(),
        parameter_annotations: None,
        invisible_parameter_annotations: None,
        annotation_default: None,
        exceptions: Vec::new(),
        parameters: None,
        deprecated: false,
        attributes: Vec::new(),
    };

    let mut seen: HashSet<&str> = HashSet::new();
    for attribute in &method.attributes {
        let name = file.attribute_name(attribute)?;
        metadata.attributes.push(raw(attribute, name));
        once(&mut seen, name, METHOD_ATTRIBUTES)?;
        match name {
            "Signature" => {
                metadata.signature = Some(wrap(name, decode_utf8_attribute(file, attribute))?);
            }
            "RuntimeVisibleAnnotations" => {
                metadata.annotations = wrap(name, decode_annotations(file, attribute))?;
            }
            "RuntimeInvisibleAnnotations" => {
                metadata.invisible_annotations = wrap(name, decode_annotations(file, attribute))?;
            }
            "RuntimeVisibleTypeAnnotations" => {
                metadata.type_annotations = wrap(name, decode_type_annotations(file, attribute))?;
            }
            "RuntimeInvisibleTypeAnnotations" => {
                metadata.invisible_type_annotations =
                    wrap(name, decode_type_annotations(file, attribute))?;
            }
            "RuntimeVisibleParameterAnnotations" => {
                metadata.parameter_annotations =
                    Some(wrap(name, decode_parameter_annotations(file, attribute))?);
            }
            "RuntimeInvisibleParameterAnnotations" => {
                metadata.invisible_parameter_annotations =
                    Some(wrap(name, decode_parameter_annotations(file, attribute))?);
            }
            "AnnotationDefault" => {
                metadata.annotation_default =
                    Some(wrap(name, decode_annotation_default(file, attribute))?);
            }
            "Exceptions" => {
                metadata.exceptions = wrap(name, decode_class_list(file, attribute))?;
            }
            "MethodParameters" => {
                metadata.parameters = Some(wrap(name, decode_method_parameters(file, attribute))?);
            }
            "Deprecated" => {
                metadata.deprecated = wrap(name, decode_deprecated(attribute))?;
            }
            _ => {}
        }
    }
    Ok(metadata)
}

fn decode_record(file: &ClassFile, attribute: &Attribute) -> Result<Vec<RecordComponent>, Error> {
    let mut reader = Reader::new(&attribute.data);
    let count = reader.u2()?;
    let mut components = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let name = file.constant_pool.utf8(reader.u2()?)?.to_owned();
        let descriptor = file.constant_pool.utf8(reader.u2()?)?.to_owned();
        let attribute_count = reader.u2()?;
        let mut component = RecordComponent {
            name,
            descriptor,
            signature: None,
            annotations: Vec::new(),
            invisible_annotations: Vec::new(),
            type_annotations: Vec::new(),
            invisible_type_annotations: Vec::new(),
            attributes: Vec::new(),
        };
        let mut seen: HashSet<&str> = HashSet::new();
        for _ in 0..attribute_count {
            let nested = read_attribute(&mut reader)?;
            let nested_name = file.attribute_name(&nested)?;
            component.attributes.push(raw(&nested, nested_name));
            once(&mut seen, nested_name, COMPONENT_ATTRIBUTES)?;
            match nested_name {
                "Signature" => {
                    component.signature =
                        Some(wrap(nested_name, decode_utf8_attribute(file, &nested))?);
                }
                "RuntimeVisibleAnnotations" => {
                    component.annotations = wrap(nested_name, decode_annotations(file, &nested))?;
                }
                "RuntimeInvisibleAnnotations" => {
                    component.invisible_annotations =
                        wrap(nested_name, decode_annotations(file, &nested))?;
                }
                "RuntimeVisibleTypeAnnotations" => {
                    component.type_annotations =
                        wrap(nested_name, decode_type_annotations(file, &nested))?;
                }
                "RuntimeInvisibleTypeAnnotations" => {
                    component.invisible_type_annotations =
                        wrap(nested_name, decode_type_annotations(file, &nested))?;
                }
                _ => {}
            }
        }
        components.push(component);
    }
    finish(&reader)?;
    Ok(components)
}

fn decode_inner_classes(file: &ClassFile, attribute: &Attribute) -> Result<Vec<InnerClass>, Error> {
    let mut reader = Reader::new(&attribute.data);
    let count = reader.u2()?;
    let mut classes = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let inner_index = reader.u2()?;
        let outer_index = reader.u2()?;
        let name_index = reader.u2()?;
        let access_flags = reader.u2()?;
        if inner_index == 0 {
            return Err(malformed("InnerClasses row has no inner class"));
        }
        classes.push(InnerClass {
            inner_class: file.constant_pool.class_name(inner_index)?.to_owned(),
            outer_class: if outer_index == 0 {
                None
            } else {
                Some(file.constant_pool.class_name(outer_index)?.to_owned())
            },
            inner_name: if name_index == 0 {
                None
            } else {
                Some(file.constant_pool.utf8(name_index)?.to_owned())
            },
            access_flags,
        });
    }
    finish(&reader)?;
    Ok(classes)
}

fn decode_enclosing_method(
    file: &ClassFile,
    attribute: &Attribute,
) -> Result<EnclosingMethod, Error> {
    let mut reader = Reader::new(&attribute.data);
    let class_index = reader.u2()?;
    let method_index = reader.u2()?;
    finish(&reader)?;
    if class_index == 0 {
        return Err(malformed("EnclosingMethod has no enclosing class"));
    }
    let class = file.constant_pool.class_name(class_index)?.to_owned();
    let method = if method_index == 0 {
        None
    } else {
        let (name, descriptor) = file.constant_pool.name_and_type(method_index)?;
        Some(MethodNameAndDescriptor {
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        })
    };
    Ok(EnclosingMethod { class, method })
}

fn decode_annotations(file: &ClassFile, attribute: &Attribute) -> Result<Vec<Annotation>, Error> {
    let mut reader = Reader::new(&attribute.data);
    let count = reader.u2()?;
    let mut annotations = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        annotations.push(decode_annotation(file, &mut reader)?);
    }
    finish(&reader)?;
    Ok(annotations)
}

fn decode_annotation(file: &ClassFile, reader: &mut Reader<'_>) -> Result<Annotation, Error> {
    let type_descriptor = file.constant_pool.utf8(reader.u2()?)?.to_owned();
    let count = reader.u2()?;
    let mut values = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let name = file.constant_pool.utf8(reader.u2()?)?.to_owned();
        let value = decode_element_value(file, reader)?;
        values.push(AnnotationValue { name, value });
    }
    Ok(Annotation {
        type_descriptor,
        values,
    })
}

fn decode_element_value(file: &ClassFile, reader: &mut Reader<'_>) -> Result<ElementValue, Error> {
    let tag = reader.u1()?;
    Ok(match tag {
        b'B' => {
            let value = const_integer(file, reader.u2()?)?;
            ElementValue::Byte(byte_value(value)?)
        }
        b'C' => {
            let value = const_integer(file, reader.u2()?)?;
            ElementValue::Char(char_value(value)?)
        }
        b'D' => ElementValue::Double(const_double(file, reader.u2()?)?),
        b'F' => ElementValue::Float(const_float(file, reader.u2()?)?),
        b'I' => ElementValue::Int(const_integer(file, reader.u2()?)?),
        b'J' => ElementValue::Long(const_long(file, reader.u2()?)?),
        b'S' => {
            let value = const_integer(file, reader.u2()?)?;
            ElementValue::Short(short_value(value)?)
        }
        b'Z' => ElementValue::Boolean(const_integer(file, reader.u2()?)? != 0),
        b's' => ElementValue::String(const_utf8(file, reader.u2()?)?),
        b'e' => ElementValue::Enum {
            type_descriptor: const_utf8(file, reader.u2()?)?,
            constant: const_utf8(file, reader.u2()?)?,
        },
        b'c' => ElementValue::Class(const_utf8(file, reader.u2()?)?),
        b'@' => ElementValue::Annotation(decode_annotation(file, reader)?),
        b'[' => {
            let count = reader.u2()?;
            let mut values = Vec::with_capacity(usize::from(count));
            for _ in 0..count {
                values.push(decode_element_value(file, reader)?);
            }
            ElementValue::Array(values)
        }
        other => {
            return Err(malformed(format!(
                "unknown element_value tag 0x{other:02x}"
            )));
        }
    })
}

fn decode_type_annotations(
    file: &ClassFile,
    attribute: &Attribute,
) -> Result<Vec<TypeAnnotation>, Error> {
    let mut reader = Reader::new(&attribute.data);
    let count = reader.u2()?;
    let mut annotations = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        annotations.push(decode_type_annotation(file, &mut reader)?);
    }
    finish(&reader)?;
    Ok(annotations)
}

fn decode_type_annotation(
    file: &ClassFile,
    reader: &mut Reader<'_>,
) -> Result<TypeAnnotation, Error> {
    let target_type = reader.u1()?;
    let target = match target_type {
        0x00 => TypeAnnotationTarget::ClassTypeParameter {
            index: reader.u1()?,
        },
        0x01 => TypeAnnotationTarget::MethodTypeParameter {
            index: reader.u1()?,
        },
        0x10 => TypeAnnotationTarget::Supertype {
            index: reader.u2()?,
        },
        0x11 | 0x12 => TypeAnnotationTarget::TypeParameterBound {
            parameter_index: reader.u1()?,
            bound_index: reader.u1()?,
        },
        0x13..=0x15 => TypeAnnotationTarget::Empty,
        0x16 => TypeAnnotationTarget::FormalParameter {
            index: reader.u1()?,
        },
        0x17 => TypeAnnotationTarget::Throws {
            index: reader.u2()?,
        },
        0x40 | 0x41 => {
            let table_length = reader.u2()?;
            let mut table = Vec::with_capacity(usize::from(table_length));
            for _ in 0..table_length {
                table.push(LocalVariableTarget {
                    start_pc: reader.u2()?,
                    length: reader.u2()?,
                    index: reader.u2()?,
                });
            }
            TypeAnnotationTarget::LocalVariable { table }
        }
        0x42 => TypeAnnotationTarget::Catch {
            index: reader.u2()?,
        },
        0x43..=0x46 => TypeAnnotationTarget::Offset {
            offset: reader.u2()?,
        },
        0x47..=0x4B => TypeAnnotationTarget::TypeArgument {
            offset: reader.u2()?,
            argument_index: reader.u1()?,
        },
        other => {
            return Err(malformed(format!(
                "unknown type_annotation target_type 0x{other:02x}"
            )));
        }
    };
    let path = decode_type_path(reader)?;
    let annotation = decode_annotation(file, reader)?;
    Ok(TypeAnnotation {
        target_type,
        target,
        path,
        annotation,
    })
}

fn decode_type_path(reader: &mut Reader<'_>) -> Result<Vec<TypePathEntry>, Error> {
    let length = reader.u1()?;
    let mut path = Vec::with_capacity(usize::from(length));
    for _ in 0..length {
        let kind_byte = reader.u1()?;
        let argument_index = reader.u1()?;
        let kind = match kind_byte {
            0 => TypePathKind::Array,
            1 => TypePathKind::Nested,
            2 => TypePathKind::Wildcard,
            3 => TypePathKind::TypeArgument,
            other => return Err(malformed(format!("unknown type_path kind {other}"))),
        };
        if kind != TypePathKind::TypeArgument && argument_index != 0 {
            return Err(malformed(format!(
                "type_path {kind:?} carries type argument {argument_index}"
            )));
        }
        path.push(TypePathEntry {
            kind,
            argument_index,
        });
    }
    Ok(path)
}

fn decode_parameter_annotations(
    file: &ClassFile,
    attribute: &Attribute,
) -> Result<Vec<Vec<Annotation>>, Error> {
    let mut reader = Reader::new(&attribute.data);
    let count = reader.u1()?;
    let mut parameters = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let annotation_count = reader.u2()?;
        let mut annotations = Vec::with_capacity(usize::from(annotation_count));
        for _ in 0..annotation_count {
            annotations.push(decode_annotation(file, &mut reader)?);
        }
        parameters.push(annotations);
    }
    finish(&reader)?;
    Ok(parameters)
}

fn decode_annotation_default(
    file: &ClassFile,
    attribute: &Attribute,
) -> Result<ElementValue, Error> {
    let mut reader = Reader::new(&attribute.data);
    let value = decode_element_value(file, &mut reader)?;
    finish(&reader)?;
    Ok(value)
}

fn decode_method_parameters(
    file: &ClassFile,
    attribute: &Attribute,
) -> Result<Vec<MethodParameter>, Error> {
    let mut reader = Reader::new(&attribute.data);
    let count = reader.u1()?;
    let mut parameters = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let name_index = reader.u2()?;
        let access_flags = reader.u2()?;
        let name = if name_index == 0 {
            None
        } else {
            Some(file.constant_pool.utf8(name_index)?.to_owned())
        };
        parameters.push(MethodParameter { name, access_flags });
    }
    finish(&reader)?;
    Ok(parameters)
}

fn decode_constant_value(file: &ClassFile, attribute: &Attribute) -> Result<ConstantValue, Error> {
    let mut reader = Reader::new(&attribute.data);
    let index = reader.u2()?;
    finish(&reader)?;
    Ok(match file.constant_pool.get(index)? {
        Constant::Integer(value) => ConstantValue::Integer(*value),
        Constant::Float(value) => ConstantValue::Float(*value),
        Constant::Long(value) => ConstantValue::Long(*value),
        Constant::Double(value) => ConstantValue::Double(*value),
        Constant::String { string_index } => {
            ConstantValue::String(file.constant_pool.utf8(*string_index)?.to_owned())
        }
        _ => {
            return Err(Error::WrongConstantKind {
                index,
                expected: "Integer, Float, Long, Double or String",
            });
        }
    })
}

fn decode_utf8_attribute(file: &ClassFile, attribute: &Attribute) -> Result<String, Error> {
    let mut reader = Reader::new(&attribute.data);
    let index = reader.u2()?;
    finish(&reader)?;
    Ok(file.constant_pool.utf8(index)?.to_owned())
}

fn decode_class_index(file: &ClassFile, attribute: &Attribute) -> Result<String, Error> {
    let mut reader = Reader::new(&attribute.data);
    let index = reader.u2()?;
    finish(&reader)?;
    if index == 0 {
        return Err(malformed("class index must not be zero"));
    }
    Ok(file.constant_pool.class_name(index)?.to_owned())
}

fn decode_class_list(file: &ClassFile, attribute: &Attribute) -> Result<Vec<String>, Error> {
    let mut reader = Reader::new(&attribute.data);
    let count = reader.u2()?;
    let mut names = Vec::with_capacity(usize::from(count));
    for _ in 0..count {
        let index = reader.u2()?;
        if index == 0 {
            return Err(malformed("class index must not be zero"));
        }
        names.push(file.constant_pool.class_name(index)?.to_owned());
    }
    finish(&reader)?;
    Ok(names)
}

/// The `Deprecated` attribute is a marker: JVMS §4.7.15 fixes its length at zero.
fn decode_deprecated(attribute: &Attribute) -> Result<bool, Error> {
    if attribute.data.is_empty() {
        Ok(true)
    } else {
        Err(malformed("Deprecated attribute must have an empty payload"))
    }
}

fn const_integer(file: &ClassFile, index: u16) -> Result<i32, Error> {
    match file.constant_pool.get(index)? {
        Constant::Integer(value) => Ok(*value),
        _ => Err(Error::WrongConstantKind {
            index,
            expected: "Integer",
        }),
    }
}

fn const_float(file: &ClassFile, index: u16) -> Result<f32, Error> {
    match file.constant_pool.get(index)? {
        Constant::Float(value) => Ok(*value),
        _ => Err(Error::WrongConstantKind {
            index,
            expected: "Float",
        }),
    }
}

fn const_long(file: &ClassFile, index: u16) -> Result<i64, Error> {
    match file.constant_pool.get(index)? {
        Constant::Long(value) => Ok(*value),
        _ => Err(Error::WrongConstantKind {
            index,
            expected: "Long",
        }),
    }
}

fn const_double(file: &ClassFile, index: u16) -> Result<f64, Error> {
    match file.constant_pool.get(index)? {
        Constant::Double(value) => Ok(*value),
        _ => Err(Error::WrongConstantKind {
            index,
            expected: "Double",
        }),
    }
}

fn const_utf8(file: &ClassFile, index: u16) -> Result<String, Error> {
    Ok(file.constant_pool.utf8(index)?.to_owned())
}

fn byte_value(value: i32) -> Result<i8, Error> {
    i8::try_from(value)
        .map_err(|_| malformed(format!("byte element value {value} is out of range")))
}

fn short_value(value: i32) -> Result<i16, Error> {
    i16::try_from(value)
        .map_err(|_| malformed(format!("short element value {value} is out of range")))
}

fn char_value(value: i32) -> Result<u16, Error> {
    u16::try_from(value)
        .map_err(|_| malformed(format!("char element value {value} is out of range")))
}

fn read_attribute(reader: &mut Reader<'_>) -> Result<Attribute, Error> {
    let name_index = reader.u2()?;
    // A u32 payload length widens losslessly on every supported target; the reader
    // bounds-checks the slice it returns.
    let length = reader.u4()? as usize;
    let data = reader.bytes(length)?.to_vec();
    Ok(Attribute { name_index, data })
}

fn raw(attribute: &Attribute, name: &str) -> RawAttribute {
    RawAttribute {
        name: name.to_owned(),
        data: attribute.data.clone(),
    }
}

fn malformed(detail: impl Into<String>) -> Error {
    Error::MalformedAttribute {
        detail: detail.into(),
    }
}

fn wrap<T>(name: &str, result: Result<T, Error>) -> Result<T, Error> {
    result.map_err(|cause| Error::BadAttribute {
        name: name.to_owned(),
        cause: Box::new(cause),
    })
}

/// Rejects a second occurrence of an attribute the model decodes (JVMS §4.7 allows at
/// most one of each).
fn once<'a>(seen: &mut HashSet<&'a str>, name: &'a str, decoded: &[&str]) -> Result<(), Error> {
    if !decoded.contains(&name) {
        return Ok(());
    }
    if seen.insert(name) {
        return Ok(());
    }
    Err(Error::BadAttribute {
        name: name.to_owned(),
        cause: Box::new(malformed("duplicate attribute")),
    })
}

/// A payload must be consumed exactly: a trailing byte means the decoder and the file
/// disagree about the attribute's shape.
fn finish(reader: &Reader<'_>) -> Result<(), Error> {
    if reader.is_empty() {
        Ok(())
    } else {
        Err(malformed(format!(
            "{} trailing byte(s) in attribute payload",
            reader.remaining()
        )))
    }
}
