//! A deterministic, line-oriented dump of the metadata model.
//!
//! The dump exists so the C1 metadata gate (M2.2) can diff two views of the same class
//! without a reflection API on both sides: it prints exactly what the model decoded, in a
//! fixed order, with fixed spacing, and without a host path, a timestamp or a hash.
//!
//! Normalization rules, in one place:
//!
//! - Members keep class-file order; interfaces keep declaration order; annotations and
//!   their element pairs keep encoded declaration order.
//! - `NestMembers`, `PermittedSubclasses` and `InnerClasses` are sorted, because their
//!   class-file order carries no meaning and a diff over them should not.
//! - An unmodeled attribute is named, not dumped: `unknown_attributes` lists the
//!   attributes the model neither decoded nor delegated to the parser, so a missing
//!   decoder shows up in the diff instead of disappearing.
//! - `Code`, `BootstrapMethods` and the module attributes are the parser's and the
//!   interpreter's view, so they are never reported as unmodeled metadata.

use std::fmt::Arguments;

use super::decode::{
    CLASS_ATTRIBUTES, COMPONENT_ATTRIBUTES, FIELD_ATTRIBUTES, METHOD_ATTRIBUTES,
    NOT_METADATA_ATTRIBUTES,
};
use super::{
    Annotation, ClassMetadata, ElementValue, FieldMetadata, InnerClass, MethodMetadata,
    RawAttribute, RecordComponent, TypeAnnotation, TypeAnnotationTarget, TypePathEntry,
    TypePathKind,
};

/// Class access-flag names (JVMS §4.1).
const CLASS_FLAGS: &[(u16, &str)] = &[
    (0x0001, "public"),
    (0x0010, "final"),
    (0x0020, "super"),
    (0x0200, "interface"),
    (0x0400, "abstract"),
    (0x1000, "synthetic"),
    (0x2000, "annotation"),
    (0x4000, "enum"),
    (0x8000, "module"),
];

/// Field access-flag names (JVMS §4.5).
const FIELD_FLAGS: &[(u16, &str)] = &[
    (0x0001, "public"),
    (0x0002, "private"),
    (0x0004, "protected"),
    (0x0008, "static"),
    (0x0010, "final"),
    (0x0040, "volatile"),
    (0x0080, "transient"),
    (0x1000, "synthetic"),
    (0x4000, "enum"),
];

/// Method access-flag names (JVMS §4.6).
const METHOD_FLAGS: &[(u16, &str)] = &[
    (0x0001, "public"),
    (0x0002, "private"),
    (0x0004, "protected"),
    (0x0008, "static"),
    (0x0010, "final"),
    (0x0020, "synchronized"),
    (0x0040, "bridge"),
    (0x0080, "varargs"),
    (0x0100, "native"),
    (0x0400, "abstract"),
    (0x0800, "strict"),
    (0x1000, "synthetic"),
];

/// `InnerClasses` access-flag names (JVMS §4.7.6): the class flags plus the member-visible
/// `private`, `protected` and `static` bits.
const INNER_CLASS_FLAGS: &[(u16, &str)] = &[
    (0x0001, "public"),
    (0x0002, "private"),
    (0x0004, "protected"),
    (0x0008, "static"),
    (0x0010, "final"),
    (0x0200, "interface"),
    (0x0400, "abstract"),
    (0x1000, "synthetic"),
    (0x2000, "annotation"),
    (0x4000, "enum"),
];

/// `MethodParameters` access-flag names (JVMS §4.7.24).
const PARAMETER_FLAGS: &[(u16, &str)] = &[
    (0x0010, "final"),
    (0x1000, "synthetic"),
    (0x8000, "mandated"),
];

/// Renders the model as stable text. The result always ends with a newline.
pub fn dump(class: &ClassMetadata) -> String {
    let mut out = String::new();
    write_line(&mut out, format_args!("class {}", class.binary_name));
    write_line(
        &mut out,
        format_args!("version {}.{}", class.major_version, class.minor_version),
    );
    write_flags(&mut out, "access", class.access_flags, CLASS_FLAGS);
    write_line(
        &mut out,
        format_args!("super {}", class.super_class.as_deref().unwrap_or("none")),
    );

    write_line(
        &mut out,
        format_args!("interfaces {}", class.interfaces.len()),
    );
    for name in &class.interfaces {
        write_line(&mut out, format_args!("  interface {name}"));
    }

    if let Some(signature) = &class.signature {
        write_line(&mut out, format_args!("signature {signature}"));
    }
    if let Some(source_file) = &class.source_file {
        write_line(&mut out, format_args!("source_file {source_file}"));
    }
    if class.deprecated {
        write_line(&mut out, format_args!("deprecated true"));
    }
    if let Some(nest_host) = &class.nest_host {
        write_line(&mut out, format_args!("nest_host {nest_host}"));
    }

    write_line(
        &mut out,
        format_args!("nest_members {}", class.nest_members.len()),
    );
    let mut nest_members: Vec<&str> = class.nest_members.iter().map(String::as_str).collect();
    nest_members.sort_unstable();
    for name in nest_members {
        write_line(&mut out, format_args!("  nest_member {name}"));
    }

    if let Some(permitted) = &class.permitted_subclasses {
        write_line(
            &mut out,
            format_args!("permitted_subclasses {}", permitted.len()),
        );
        let mut permitted: Vec<&str> = permitted.iter().map(String::as_str).collect();
        permitted.sort_unstable();
        for name in permitted {
            write_line(&mut out, format_args!("  permitted {name}"));
        }
    }

    write_inner_classes(&mut out, &class.inner_classes);

    if let Some(enclosing) = &class.enclosing_method {
        match &enclosing.method {
            Some(method) => write_line(
                &mut out,
                format_args!(
                    "enclosing_method {} {}{}",
                    enclosing.class, method.name, method.descriptor
                ),
            ),
            None => write_line(
                &mut out,
                format_args!("enclosing_method {} none", enclosing.class),
            ),
        }
    }

    if let Some(components) = &class.record_components {
        write_line(
            &mut out,
            format_args!("record_components {}", components.len()),
        );
        for component in components {
            write_record_component(&mut out, component);
        }
    }

    write_annotations(&mut out, "", "annotations", &class.annotations);
    write_annotations(
        &mut out,
        "",
        "invisible_annotations",
        &class.invisible_annotations,
    );
    write_type_annotations(&mut out, "", "type_annotations", &class.type_annotations);
    write_type_annotations(
        &mut out,
        "",
        "invisible_type_annotations",
        &class.invisible_type_annotations,
    );

    write_line(&mut out, format_args!("fields {}", class.fields.len()));
    for field in &class.fields {
        write_field(&mut out, field);
    }

    write_line(&mut out, format_args!("methods {}", class.methods.len()));
    for method in &class.methods {
        write_method(&mut out, method);
    }

    write_unknown_attributes(&mut out, "", &class.attributes, CLASS_ATTRIBUTES);
    out
}

fn write_field(out: &mut String, field: &FieldMetadata) {
    write_line(
        out,
        format_args!("  field {} {}", field.name, field.descriptor),
    );
    write_flags(out, "    access", field.access_flags, FIELD_FLAGS);
    if let Some(signature) = &field.signature {
        write_line(out, format_args!("    signature {signature}"));
    }
    if let Some(constant) = &field.constant_value {
        write_line(out, format_args!("    constant_value {constant:?}"));
    }
    if field.deprecated {
        write_line(out, format_args!("    deprecated true"));
    }
    write_annotations(out, "    ", "annotations", &field.annotations);
    write_annotations(
        out,
        "    ",
        "invisible_annotations",
        &field.invisible_annotations,
    );
    write_type_annotations(out, "    ", "type_annotations", &field.type_annotations);
    write_type_annotations(
        out,
        "    ",
        "invisible_type_annotations",
        &field.invisible_type_annotations,
    );
    write_unknown_attributes(out, "    ", &field.attributes, FIELD_ATTRIBUTES);
}

fn write_method(out: &mut String, method: &MethodMetadata) {
    write_line(
        out,
        format_args!("  method {} {}", method.name, method.descriptor),
    );
    write_flags(out, "    access", method.access_flags, METHOD_FLAGS);
    if let Some(signature) = &method.signature {
        write_line(out, format_args!("    signature {signature}"));
    }
    if !method.exceptions.is_empty() {
        write_line(
            out,
            format_args!("    exceptions {}", method.exceptions.len()),
        );
        for exception in &method.exceptions {
            write_line(out, format_args!("      throws {exception}"));
        }
    }
    if let Some(parameters) = &method.parameters {
        write_line(out, format_args!("    parameters {}", parameters.len()));
        for parameter in parameters {
            let name = parameter.name.as_deref().unwrap_or("unnamed");
            let flags = parameter.access_flags;
            // Spell the parameter flags out too: `this$0` is `final mandated`, and a bare
            // hex word would hide that from the diff.
            let mut text = format!("      {name} flags 0x{flags:04x}");
            for (bit, flag_name) in PARAMETER_FLAGS {
                if flags & bit != 0 {
                    text.push(' ');
                    text.push_str(flag_name);
                }
            }
            write_line(out, format_args!("{text}"));
        }
    }
    if let Some(default) = &method.annotation_default {
        write_line(
            out,
            format_args!("    annotation_default {}", render_value(default)),
        );
    }
    if method.deprecated {
        write_line(out, format_args!("    deprecated true"));
    }
    write_annotations(out, "    ", "annotations", &method.annotations);
    write_annotations(
        out,
        "    ",
        "invisible_annotations",
        &method.invisible_annotations,
    );
    write_type_annotations(out, "    ", "type_annotations", &method.type_annotations);
    write_type_annotations(
        out,
        "    ",
        "invisible_type_annotations",
        &method.invisible_type_annotations,
    );
    if let Some(parameters) = &method.parameter_annotations {
        write_parameter_annotations(out, "    ", "parameter_annotations", parameters);
    }
    if let Some(parameters) = &method.invisible_parameter_annotations {
        write_parameter_annotations(out, "    ", "invisible_parameter_annotations", parameters);
    }
    write_unknown_attributes(out, "    ", &method.attributes, METHOD_ATTRIBUTES);
}

fn write_record_component(out: &mut String, component: &RecordComponent) {
    write_line(
        out,
        format_args!("  component {} {}", component.name, component.descriptor),
    );
    if let Some(signature) = &component.signature {
        write_line(out, format_args!("    signature {signature}"));
    }
    write_annotations(out, "    ", "annotations", &component.annotations);
    write_annotations(
        out,
        "    ",
        "invisible_annotations",
        &component.invisible_annotations,
    );
    write_type_annotations(out, "    ", "type_annotations", &component.type_annotations);
    write_type_annotations(
        out,
        "    ",
        "invisible_type_annotations",
        &component.invisible_type_annotations,
    );
    write_unknown_attributes(out, "    ", &component.attributes, COMPONENT_ATTRIBUTES);
}

fn write_inner_classes(out: &mut String, inner_classes: &[InnerClass]) {
    write_line(out, format_args!("inner_classes {}", inner_classes.len()));
    let mut entries: Vec<&InnerClass> = inner_classes.iter().collect();
    entries.sort_by(|left, right| {
        left.inner_class
            .cmp(&right.inner_class)
            .then_with(|| left.outer_class.cmp(&right.outer_class))
            .then_with(|| left.inner_name.cmp(&right.inner_name))
    });
    for entry in entries {
        let mut flags = String::new();
        for (bit, name) in INNER_CLASS_FLAGS {
            if entry.access_flags & bit != 0 {
                flags.push(' ');
                flags.push_str(name);
            }
        }
        write_line(
            out,
            format_args!(
                "  inner {} outer {} name {} access 0x{:04x}{}",
                entry.inner_class,
                entry.outer_class.as_deref().unwrap_or("none"),
                entry.inner_name.as_deref().unwrap_or("none"),
                entry.access_flags,
                flags,
            ),
        );
    }
}

fn write_annotations(out: &mut String, indent: &str, label: &str, annotations: &[Annotation]) {
    if annotations.is_empty() {
        return;
    }
    write_line(out, format_args!("{indent}{label} {}", annotations.len()));
    for annotation in annotations {
        write_line(
            out,
            format_args!("{indent}  {}", render_annotation(annotation)),
        );
    }
}

fn write_parameter_annotations(
    out: &mut String,
    indent: &str,
    label: &str,
    parameters: &[Vec<Annotation>],
) {
    write_line(out, format_args!("{indent}{label} {}", parameters.len()));
    for (index, annotations) in parameters.iter().enumerate() {
        write_line(out, format_args!("{indent}  parameter {index}"));
        for annotation in annotations {
            write_line(
                out,
                format_args!("{indent}    {}", render_annotation(annotation)),
            );
        }
    }
}

fn write_type_annotations(
    out: &mut String,
    indent: &str,
    label: &str,
    annotations: &[TypeAnnotation],
) {
    if annotations.is_empty() {
        return;
    }
    write_line(out, format_args!("{indent}{label} {}", annotations.len()));
    for annotation in annotations {
        write_line(
            out,
            format_args!("{indent}  {}", render_type_annotation(annotation)),
        );
    }
}

fn write_unknown_attributes(
    out: &mut String,
    indent: &str,
    attributes: &[RawAttribute],
    decoded: &[&str],
) {
    let mut names: Vec<&str> = attributes
        .iter()
        .map(|attribute| attribute.name.as_str())
        .filter(|name| !decoded.contains(name) && !NOT_METADATA_ATTRIBUTES.contains(name))
        .collect();
    if names.is_empty() {
        return;
    }
    names.sort_unstable();
    write_line(
        out,
        format_args!("{indent}unknown_attributes {}", names.len()),
    );
    for name in names {
        write_line(out, format_args!("{indent}  {name}"));
    }
}

fn render_type_annotation(type_annotation: &TypeAnnotation) -> String {
    let mut out = String::new();
    out.push_str(&format!("0x{:02x} ", type_annotation.target_type));
    match &type_annotation.target {
        TypeAnnotationTarget::ClassTypeParameter { index } => {
            out.push_str(&format!("class_type_parameter {index}"));
        }
        TypeAnnotationTarget::MethodTypeParameter { index } => {
            out.push_str(&format!("method_type_parameter {index}"));
        }
        TypeAnnotationTarget::Supertype { index } => {
            out.push_str(&format!("supertype {index}"));
        }
        TypeAnnotationTarget::TypeParameterBound {
            parameter_index,
            bound_index,
        } => out.push_str(&format!(
            "type_parameter_bound {parameter_index} {bound_index}"
        )),
        TypeAnnotationTarget::Empty => out.push_str("empty"),
        TypeAnnotationTarget::FormalParameter { index } => {
            out.push_str(&format!("formal_parameter {index}"));
        }
        TypeAnnotationTarget::Throws { index } => out.push_str(&format!("throws {index}")),
        TypeAnnotationTarget::LocalVariable { table } => {
            out.push_str(&format!("local_variable {}", table.len()));
            for entry in table {
                out.push_str(&format!(
                    " (start_pc={} length={} index={})",
                    entry.start_pc, entry.length, entry.index
                ));
            }
        }
        TypeAnnotationTarget::Catch { index } => out.push_str(&format!("catch {index}")),
        TypeAnnotationTarget::Offset { offset } => out.push_str(&format!("offset {offset}")),
        TypeAnnotationTarget::TypeArgument {
            offset,
            argument_index,
        } => out.push_str(&format!("type_argument {offset} {argument_index}")),
    }
    out.push(' ');
    render_path(&mut out, &type_annotation.path);
    out.push(' ');
    render_annotation_into(&mut out, &type_annotation.annotation);
    out
}

fn render_path(out: &mut String, path: &[TypePathEntry]) {
    out.push_str("path=[");
    for (index, entry) in path.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        match entry.kind {
            TypePathKind::Array => out.push_str("ARRAY"),
            TypePathKind::Nested => out.push_str("INNER_TYPE"),
            TypePathKind::Wildcard => out.push_str("WILDCARD"),
            TypePathKind::TypeArgument => {
                out.push_str(&format!("TYPE_ARGUMENT({})", entry.argument_index));
            }
        }
    }
    out.push(']');
}

fn render_annotation(annotation: &Annotation) -> String {
    let mut out = String::new();
    render_annotation_into(&mut out, annotation);
    out
}

fn render_annotation_into(out: &mut String, annotation: &Annotation) {
    out.push('@');
    out.push_str(&annotation.type_descriptor);
    out.push('(');
    for (index, pair) in annotation.values.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(&pair.name);
        out.push('=');
        render_value_into(out, &pair.value);
    }
    out.push(')');
}

/// Renders one element value as `tag:value`, the way a class file tags it.
fn render_value(value: &ElementValue) -> String {
    let mut out = String::new();
    render_value_into(&mut out, value);
    out
}

fn render_value_into(out: &mut String, value: &ElementValue) {
    match value {
        ElementValue::Boolean(value) => out.push_str(if *value { "Z:true" } else { "Z:false" }),
        ElementValue::Byte(value) => out.push_str(&format!("B:{value}")),
        ElementValue::Char(value) => {
            out.push_str("C:");
            render_char(out, *value);
        }
        ElementValue::Short(value) => out.push_str(&format!("S:{value}")),
        ElementValue::Int(value) => out.push_str(&format!("I:{value}")),
        ElementValue::Long(value) => out.push_str(&format!("J:{value}")),
        ElementValue::Float(value) => out.push_str(&format!("F:{value}")),
        ElementValue::Double(value) => out.push_str(&format!("D:{value}")),
        ElementValue::String(value) => {
            out.push_str("s:");
            render_string(out, value);
        }
        ElementValue::Class(descriptor) => out.push_str(&format!("c:{descriptor}")),
        ElementValue::Enum {
            type_descriptor,
            constant,
        } => out.push_str(&format!("e:{type_descriptor}.{constant}")),
        ElementValue::Annotation(annotation) => render_annotation_into(out, annotation),
        ElementValue::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                render_value_into(out, value);
            }
            out.push(']');
        }
    }
}

fn render_char(out: &mut String, value: u16) {
    match char::from_u32(u32::from(value)) {
        Some('\'') => out.push_str("'\\''"),
        Some('\\') => out.push_str("'\\\\'"),
        Some(character) if character.is_control() => out.push_str(&format!("'\\u{value:04x}'")),
        Some(character) => {
            out.push('\'');
            out.push(character);
            out.push('\'');
        }
        None => out.push_str(&format!("'\\u{value:04x}'")),
    }
}

fn render_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

fn write_flags(out: &mut String, label: &str, flags: u16, names: &[(u16, &str)]) {
    let mut text = format!("{label} 0x{flags:04x}");
    for (bit, name) in names {
        if flags & bit != 0 {
            text.push(' ');
            text.push_str(name);
        }
    }
    write_line(out, format_args!("{text}"));
}

fn write_line(out: &mut String, args: Arguments<'_>) {
    out.push_str(&args.to_string());
    out.push('\n');
}
