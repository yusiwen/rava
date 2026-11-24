//! A deterministic, line-oriented metadata dump for a parsed class.
//!
//! The dump is the M0 proof that the parser retained what later layers need: every
//! field and method with its descriptor and access flags, and every attribute name with
//! its raw payload length. Attributes are printed by name, so annotations, `Signature`,
//! `Record`, nestmates and any future attribute appear without a new decoder.
//!
//! Fields and methods keep class-file declaration order. Attribute lines are sorted by
//! name and then by payload length, so the dump never depends on map iteration order.

use std::fmt::Write as _;

use rava_classfile::{Attribute, ClassFile, Error};

/// Formats a parsed class as stable text. The result always ends with a newline.
pub fn dump_class(file: &ClassFile) -> Result<String, Error> {
    let mut out = String::new();
    // Writing to a String is infallible; `let _ =` keeps the formatter calls terse.
    let _ = writeln!(out, "class {}", file.this_class_name()?);
    let _ = writeln!(out, "version {}.{}", file.major_version, file.minor_version);
    let _ = writeln!(out, "access_flags 0x{:04x}", file.access_flags);
    match file.super_class_name()? {
        Some(name) => {
            let _ = writeln!(out, "super {name}");
        }
        None => {
            let _ = writeln!(out, "super none");
        }
    }

    let mut interfaces = file.interface_names()?;
    interfaces.sort_unstable();
    let _ = writeln!(out, "interfaces {}", interfaces.len());
    for name in interfaces {
        let _ = writeln!(out, "  interface {name}");
    }

    let _ = writeln!(out, "fields {}", file.fields.len());
    for field in &file.fields {
        let _ = writeln!(out, "  field {}", file.field_name(field)?);
        let _ = writeln!(out, "    descriptor {}", file.field_descriptor(field)?);
        let _ = writeln!(out, "    access_flags 0x{:04x}", field.access_flags);
        let _ = writeln!(out, "    attributes {}", field.attributes.len());
        dump_attributes(&mut out, file, "      ", &field.attributes)?;
    }

    let _ = writeln!(out, "methods {}", file.methods.len());
    for method in &file.methods {
        let _ = writeln!(out, "  method {}", file.method_name(method)?);
        let _ = writeln!(out, "    descriptor {}", file.method_descriptor(method)?);
        let _ = writeln!(out, "    access_flags 0x{:04x}", method.access_flags);
        let _ = writeln!(out, "    attributes {}", method.attributes.len());
        dump_attributes(&mut out, file, "      ", &method.attributes)?;
    }

    let _ = writeln!(out, "attributes {}", file.attributes.len());
    dump_attributes(&mut out, file, "  ", &file.attributes)?;

    Ok(out)
}

fn dump_attributes(
    out: &mut String,
    file: &ClassFile,
    indent: &str,
    attributes: &[Attribute],
) -> Result<(), Error> {
    let mut entries = Vec::with_capacity(attributes.len());
    for attribute in attributes {
        entries.push((file.attribute_name(attribute)?, attribute.data.len()));
    }
    entries.sort_unstable_by(|left, right| left.0.cmp(right.0).then(left.1.cmp(&right.1)));
    for (name, length) in entries {
        let _ = writeln!(out, "{indent}{name} length {length}");
    }
    Ok(())
}
