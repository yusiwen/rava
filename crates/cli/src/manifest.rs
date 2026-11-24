//! `rava compat manifest` — the static surface manifest over the pinned jars.
//!
//! The manifest is the observed reference surface of ADR-0003, step 1: every class
//! reference and every field, method and interface-method reference to a platform
//! package (`java/`, `javax/`, `jdk/`, `sun/`) found in the constant pool of every
//! `.class` entry in the pinned jar set, plus every platform class named by a declared or
//! referenced field/method descriptor, a class/method/field `Signature` or an annotation's
//! encoded type descriptors. The descriptor and signature scan is the *descriptor
//! closure*: a type used only as `[Ljava/lang/annotation/ElementType;` never appears as a
//! `CONSTANT_Class`, so the constant-pool scan alone leaves it out of the surface. Records
//! are deduplicated and sorted, so the committed file is byte-for-byte reproducible.
//!
//! This slice is static only. It does not run the reference application. Annotations are
//! read for the type descriptors they encode (the annotation type, an enum constant's type,
//! a class literal), which is how `@Target(ElementType.TYPE)` contributes
//! `java/lang/annotation/ElementType`; annotation *behavior* belongs to the reflective
//! steps of ADR-0003 and later slices.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rava_classfile::metadata::{
    Annotation, ClassMetadata, ClassSignature, ClassTypeSignature, ElementValue, MethodSignature,
    TypeAnnotation, TypeArgument, TypeParameter, TypeSignature, parse_class_signature,
    parse_field_signature, parse_method_signature,
};
use rava_classfile::reader::Reader;
use rava_classfile::{Attribute, ClassFile, Constant};
use rava_vm::JarSource;

use crate::surface::{Counts, Member, Surface};
use crate::target::{self, JarLock, TargetLock};

/// Package prefixes in scope. A reference to anything else is not a platform
/// reference and is not recorded.
const PLATFORM_PREFIXES: [&str; 4] = ["java/", "javax/", "jdk/", "sun/"];

/// A missing jar, an unreadable class or an out-of-date manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(String);

impl ManifestError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<target::LockError> for ManifestError {
    fn from(error: target::LockError) -> Self {
        Self(error.to_string())
    }
}

impl Surface {
    /// Records the platform references of one class: the constant-pool owners and the
    /// descriptor/-signature closure around them.
    fn record(&mut self, class: &ClassFile) -> Result<(), ManifestError> {
        self.record_constant_pool(class)?;
        self.record_descriptor_closure(class)
    }

    /// Records every platform owner named by a `CONSTANT_Class`, field, method or
    /// interface-method reference in one class's constant pool.
    fn record_constant_pool(&mut self, class: &ClassFile) -> Result<(), ManifestError> {
        let this = class.this_class_name().unwrap_or("<unknown>");
        let pool_error =
            |error: rava_classfile::Error| ManifestError::new(format!("class {this}: {error}"));

        for (_, constant) in class.constant_pool.entries() {
            match constant {
                Constant::Class { name_index } => {
                    let name = class.constant_pool.utf8(*name_index).map_err(pool_error)?;
                    // An array reference names a descriptor (`[Ljava/lang/String;`), not
                    // a class that can be linked as `class <name>`.
                    if name.starts_with('[') || !is_platform(name) {
                        continue;
                    }
                    self.classes.insert(name.to_owned());
                }
                Constant::FieldRef {
                    class_index,
                    name_and_type_index,
                } => {
                    if let Some(member) = member(class, *class_index, *name_and_type_index)? {
                        record_field_descriptor(&member.descriptor, &mut self.classes)
                            .map_err(|detail| reference_error(this, &member, detail))?;
                        self.fields.insert(member);
                    }
                }
                Constant::MethodRef {
                    class_index,
                    name_and_type_index,
                } => {
                    if let Some(member) = member(class, *class_index, *name_and_type_index)? {
                        record_method_descriptor(&member.descriptor, &mut self.classes)
                            .map_err(|detail| reference_error(this, &member, detail))?;
                        self.methods.insert(member);
                    }
                }
                Constant::InterfaceMethodRef {
                    class_index,
                    name_and_type_index,
                } => {
                    if let Some(member) = member(class, *class_index, *name_and_type_index)? {
                        record_method_descriptor(&member.descriptor, &mut self.classes)
                            .map_err(|detail| reference_error(this, &member, detail))?;
                        self.interface_methods.insert(member);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Records every platform class named by a class/method/field `Signature` and by a
    /// field or method descriptor, arrays unwrapped to their element type.
    ///
    /// A descriptor names its types as `Utf8` strings, so a type that appears only there
    /// (`[Ljava/lang/annotation/ElementType;`) has no `CONSTANT_Class` entry and would be
    /// outside the constant-pool surface. Signatures add the generic graph: type-parameter
    /// bounds, type arguments, wildcard bounds, nested `Outer<T>.Inner<U>` owners and `^`
    /// throws.
    ///
    /// Annotations contribute the descriptors they encode: the annotation type, an enum
    /// constant's type (`@Target(ElementType.TYPE)` names `ElementType` only there) and a
    /// `Class` literal. They are read from the metadata model because the raw attribute is
    /// not a descriptor string.
    fn record_descriptor_closure(&mut self, class: &ClassFile) -> Result<(), ManifestError> {
        let this = class.this_class_name().unwrap_or("<unknown>").to_owned();
        let class_error =
            |error: rava_classfile::Error| ManifestError::new(format!("class {this}: {error}"));
        let malformed = |detail: String| ManifestError::new(format!("class {this}: {detail}"));

        if let Some(text) = signature(class, &class.attributes).map_err(class_error)? {
            let signature = parse_class_signature(text).map_err(|error| {
                malformed(format!("malformed class Signature `{text}`: {error}"))
            })?;
            record_class_signature(&signature, &mut self.classes);
        }

        for field in &class.fields {
            let descriptor = class.field_descriptor(field).map_err(class_error)?;
            record_field_descriptor(descriptor, &mut self.classes).map_err(|detail| {
                malformed(format!(
                    "malformed field descriptor `{descriptor}`: {detail}"
                ))
            })?;
            if let Some(text) = signature(class, &field.attributes).map_err(class_error)? {
                let signature = parse_field_signature(text).map_err(|error| {
                    malformed(format!("malformed field Signature `{text}`: {error}"))
                })?;
                record_signature_type(&signature, &mut self.classes);
            }
        }

        for method in &class.methods {
            let descriptor = class.method_descriptor(method).map_err(class_error)?;
            record_method_descriptor(descriptor, &mut self.classes).map_err(|detail| {
                malformed(format!(
                    "malformed method descriptor `{descriptor}`: {detail}"
                ))
            })?;
            if let Some(text) = signature(class, &method.attributes).map_err(class_error)? {
                let signature = parse_method_signature(text).map_err(|error| {
                    malformed(format!("malformed method Signature `{text}`: {error}"))
                })?;
                record_method_signature(&signature, &mut self.classes);
            }
        }

        let metadata = class.metadata().map_err(class_error)?;
        record_metadata_annotations(&metadata, &mut self.classes)
            .map_err(|detail| malformed(format!("malformed annotation descriptor: {detail}")))?;

        Ok(())
    }

    /// Renders the committed manifest: a stable header, then one sorted record per line
    /// grouped by kind. It contains no timestamp and no host path.
    fn render(&self, lock: &TargetLock, jar_count: usize) -> String {
        let counts = self.counts();
        let mut output = String::new();
        output.push_str(
            "# rava surface manifest - generated by `rava compat manifest`; do not edit.\n",
        );
        output.push_str(&format!(
            "# target: java {} (class file {}), spring boot {}, spring framework {}, tomcat {}\n",
            lock.java, lock.class_file, lock.spring_boot, lock.spring_framework, lock.tomcat
        ));
        output.push_str(&format!(
            "# reference app: {} {} ({})\n",
            lock.app_path, lock.app_version, lock.app_main_class
        ));
        output.push_str(&format!("# jars: {jar_count}\n"));
        output.push_str(&format!("# classes: {}\n", counts.classes));
        output.push_str(&format!("# fields: {}\n", counts.fields));
        output.push_str(&format!("# methods: {}\n", counts.methods));
        output.push_str(&format!(
            "# interface-methods: {}\n",
            counts.interface_methods
        ));

        for name in &self.classes {
            output.push_str(&format!("class {name}\n"));
        }
        for member in &self.fields {
            output.push_str(&member.render("field"));
        }
        for member in &self.methods {
            output.push_str(&member.render("method"));
        }
        for member in &self.interface_methods {
            output.push_str(&member.render("interface-method"));
        }
        output
    }
}

/// `(owner, name, descriptor)` for a field/method/interface-method reference, or `None`
/// when the owner is not a platform class.
fn member(
    class: &ClassFile,
    class_index: u16,
    name_and_type_index: u16,
) -> Result<Option<Member>, ManifestError> {
    let this = class.this_class_name().unwrap_or("<unknown>");
    let pool_error =
        |error: rava_classfile::Error| ManifestError::new(format!("class {this}: {error}"));

    let owner = class
        .constant_pool
        .class_name(class_index)
        .map_err(pool_error)?;
    if !is_platform(owner) {
        return Ok(None);
    }
    let (name, descriptor) = class
        .constant_pool
        .name_and_type(name_and_type_index)
        .map_err(pool_error)?;
    Ok(Some(Member {
        owner: owner.to_owned(),
        name: name.to_owned(),
        descriptor: descriptor.to_owned(),
    }))
}

fn is_platform(name: &str) -> bool {
    PLATFORM_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Reports a malformed descriptor on a field/method/interface-method reference.
fn reference_error(this: &str, member: &Member, detail: &'static str) -> ManifestError {
    ManifestError::new(format!(
        "class {this}: malformed descriptor `{}` on {} {}: {detail}",
        member.descriptor, member.owner, member.name
    ))
}

/// The `Signature` attribute's string on a class, field or method, if present.
fn signature<'a>(
    class: &'a ClassFile,
    attributes: &'a [Attribute],
) -> Result<Option<&'a str>, rava_classfile::Error> {
    for attribute in attributes {
        if class.attribute_name(attribute)? != "Signature" {
            continue;
        }
        let mut reader = Reader::new(&attribute.data);
        let index = reader.u2()?;
        return Ok(Some(class.constant_pool.utf8(index)?));
    }
    Ok(None)
}

/// The deepest array nesting a descriptor may express. JVMS §4.3.2 caps a descriptor at
/// 255 dimensions; the limit keeps a hostile descriptor from exhausting the host stack.
const MAX_DESCRIPTOR_DEPTH: usize = 256;

/// A cursor over one field or method descriptor (`JVMS §4.3`).
///
/// It records the class name of every `L...;` type it consumes, after unwrapping arrays,
/// and rejects a descriptor that does not describe exactly one field type or one method.
struct Descriptor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Descriptor<'a> {
    fn new(descriptor: &'a str) -> Self {
        Self {
            bytes: descriptor.as_bytes(),
            position: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn at_end(&self) -> bool {
        self.position == self.bytes.len()
    }

    /// Consumes one `FieldType` and records the class it names, if any.
    fn field_type(
        &mut self,
        classes: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<(), &'static str> {
        if depth > MAX_DESCRIPTOR_DEPTH {
            return Err("array nesting is too deep");
        }
        match self.peek() {
            Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
                self.position += 1;
                Ok(())
            }
            Some(b'[') => {
                self.position += 1;
                self.field_type(classes, depth + 1)
            }
            Some(b'L') => {
                self.position += 1;
                let start = self.position;
                while let Some(byte) = self.peek() {
                    if byte == b';' {
                        break;
                    }
                    if byte == b'[' || byte == b'.' || byte == b'(' || byte == b')' {
                        return Err("a class descriptor contains an illegal character");
                    }
                    self.position += 1;
                }
                if self.peek() != Some(b';') {
                    return Err("a class descriptor is missing its `;`");
                }
                let name = std::str::from_utf8(&self.bytes[start..self.position])
                    .map_err(|_| "a class descriptor is not UTF-8")?;
                if name.is_empty() {
                    return Err("a class descriptor names no class");
                }
                if is_platform(name) {
                    classes.insert(name.to_owned());
                }
                self.position += 1;
                Ok(())
            }
            Some(_) => Err("unexpected character in a field type"),
            None => Err("unexpected end of descriptor"),
        }
    }
}

/// Records the class types of one field descriptor (`Ljava/lang/String;`, `[I`).
fn record_field_descriptor(
    descriptor: &str,
    classes: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    let mut cursor = Descriptor::new(descriptor);
    cursor.field_type(classes, 0)?;
    if !cursor.at_end() {
        return Err("trailing characters after the field type");
    }
    Ok(())
}

/// Records the class types of one method descriptor: every parameter type and the return
/// type, with `V` accepted only as the return.
fn record_method_descriptor(
    descriptor: &str,
    classes: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    let mut cursor = Descriptor::new(descriptor);
    if cursor.peek() != Some(b'(') {
        return Err("a method descriptor must start with `(`");
    }
    cursor.position += 1;
    loop {
        match cursor.peek() {
            Some(b')') => {
                cursor.position += 1;
                break;
            }
            Some(_) => cursor.field_type(classes, 0)?,
            None => return Err("a method descriptor is missing its `)`"),
        }
    }
    if cursor.peek() == Some(b'V') {
        cursor.position += 1;
    } else {
        cursor.field_type(classes, 0)?;
    }
    if !cursor.at_end() {
        return Err("trailing characters after the method descriptor");
    }
    Ok(())
}

/// Records every class type a parsed `TypeSignature` names, including nested owners,
/// type arguments and wildcard bounds.
fn record_signature_type(signature: &TypeSignature, classes: &mut BTreeSet<String>) {
    match signature {
        TypeSignature::Primitive(_) | TypeSignature::TypeVariable(_) => {}
        TypeSignature::Class(class) => record_class_type(class, classes),
        TypeSignature::Array(component) => record_signature_type(component, classes),
    }
}

fn record_class_type(class: &ClassTypeSignature, classes: &mut BTreeSet<String>) {
    if is_platform(&class.name) {
        classes.insert(class.name.clone());
    }
    if let Some(owner) = &class.owner {
        record_class_type(owner, classes);
    }
    for argument in &class.arguments {
        match argument {
            TypeArgument::Type(inner)
            | TypeArgument::Extends(inner)
            | TypeArgument::Super(inner) => record_signature_type(inner, classes),
            TypeArgument::Unbounded => {}
        }
    }
}

fn record_type_parameters(parameters: &[TypeParameter], classes: &mut BTreeSet<String>) {
    for parameter in parameters {
        for bound in parameter.bounds() {
            record_signature_type(bound, classes);
        }
    }
}

/// Records the closure of a class `Signature`: type-parameter bounds, superclass and
/// superinterfaces.
fn record_class_signature(signature: &ClassSignature, classes: &mut BTreeSet<String>) {
    record_type_parameters(&signature.type_parameters, classes);
    record_class_type(&signature.superclass, classes);
    for interface in &signature.interfaces {
        record_class_type(interface, classes);
    }
}

/// Records the closure of a method `Signature`: type-parameter bounds, parameter types,
/// the return type and `^` throws.
fn record_method_signature(signature: &MethodSignature, classes: &mut BTreeSet<String>) {
    record_type_parameters(&signature.type_parameters, classes);
    for parameter in &signature.parameters {
        record_signature_type(parameter, classes);
    }
    if let Some(result) = &signature.return_type {
        record_signature_type(result, classes);
    }
    for thrown in &signature.throws {
        record_signature_type(thrown, classes);
    }
}

/// Records every platform class the metadata model's annotations name: the annotation
/// type itself, enum-constant types, class literals and nested annotations, at class,
/// field, method, parameter, type-annotation and record-component level.
fn record_metadata_annotations(
    metadata: &ClassMetadata,
    classes: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    record_annotations(&metadata.annotations, classes)?;
    record_annotations(&metadata.invisible_annotations, classes)?;
    record_type_annotations(&metadata.type_annotations, classes)?;
    record_type_annotations(&metadata.invisible_type_annotations, classes)?;

    for field in &metadata.fields {
        record_annotations(&field.annotations, classes)?;
        record_annotations(&field.invisible_annotations, classes)?;
        record_type_annotations(&field.type_annotations, classes)?;
        record_type_annotations(&field.invisible_type_annotations, classes)?;
    }

    for method in &metadata.methods {
        record_annotations(&method.annotations, classes)?;
        record_annotations(&method.invisible_annotations, classes)?;
        record_type_annotations(&method.type_annotations, classes)?;
        record_type_annotations(&method.invisible_type_annotations, classes)?;
        for list in method.parameter_annotations.iter().flatten() {
            record_annotations(list, classes)?;
        }
        for list in method.invisible_parameter_annotations.iter().flatten() {
            record_annotations(list, classes)?;
        }
        if let Some(default) = &method.annotation_default {
            record_element_value(default, classes)?;
        }
    }

    for component in metadata.record_components.iter().flatten() {
        record_annotations(&component.annotations, classes)?;
        record_annotations(&component.invisible_annotations, classes)?;
        record_type_annotations(&component.type_annotations, classes)?;
        record_type_annotations(&component.invisible_type_annotations, classes)?;
    }

    Ok(())
}

fn record_annotations(
    annotations: &[Annotation],
    classes: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    for annotation in annotations {
        record_annotation(annotation, classes)?;
    }
    Ok(())
}

fn record_type_annotations(
    annotations: &[TypeAnnotation],
    classes: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    for annotation in annotations {
        record_annotation(&annotation.annotation, classes)?;
    }
    Ok(())
}

fn record_annotation(
    annotation: &Annotation,
    classes: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    record_field_descriptor(&annotation.type_descriptor, classes)?;
    for pair in &annotation.values {
        record_element_value(&pair.value, classes)?;
    }
    Ok(())
}

fn record_element_value(
    value: &ElementValue,
    classes: &mut BTreeSet<String>,
) -> Result<(), &'static str> {
    match value {
        ElementValue::Class(descriptor) => record_field_descriptor(descriptor, classes),
        ElementValue::Enum {
            type_descriptor, ..
        } => record_field_descriptor(type_descriptor, classes),
        ElementValue::Annotation(annotation) => record_annotation(annotation, classes),
        ElementValue::Array(values) => {
            for value in values {
                record_element_value(value, classes)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Scans every `.class` entry in every pinned jar below `root`.
fn scan(root: &Path, lock: &TargetLock, jar_lock: &JarLock) -> Result<Surface, ManifestError> {
    let jars_dir = root.join(&lock.jars_dir);
    let mut surface = Surface::default();

    // The jar lock is a `BTreeMap`, so the scan order is the sorted declared order.
    for name in jar_lock.entries.keys() {
        let path = jars_dir.join(name);
        if !path.is_file() {
            return Err(ManifestError::new(format!(
                "missing pinned jar {} (declared in {}); run `bash compat/fetch-jars.sh`",
                path.display(),
                lock.jars_lock
            )));
        }
        let jar = JarSource::open(&path).map_err(|error| {
            ManifestError::new(format!(
                "cannot read pinned jar {}: {error}",
                path.display()
            ))
        })?;

        for entry in jar.class_entry_names() {
            let bytes = jar.read_entry(entry).map_err(|error| {
                ManifestError::new(format!("cannot read {}!{entry}: {error}", path.display()))
            })?;
            let Some(bytes) = bytes else {
                return Err(ManifestError::new(format!(
                    "{}!{entry}: class entry disappeared while scanning",
                    path.display()
                )));
            };
            let class = ClassFile::parse(&bytes).map_err(|error| {
                ManifestError::new(format!("cannot parse {}!{entry}: {error}", path.display()))
            })?;
            surface.record(&class).map_err(|error| {
                ManifestError::new(format!("{}!{entry}: {error}", path.display()))
            })?;
        }
    }
    Ok(surface)
}

/// Runs `rava compat manifest [--check] [--root <dir>]`.
pub fn command(args: &[String]) -> ExitCode {
    let mut check = false;
    let mut root = None;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "-h" || argument == "--help" {
            print_usage();
            return ExitCode::SUCCESS;
        } else if argument == "--check" {
            check = true;
            index += 1;
        } else if argument == "--root" {
            let Some(value) = args.get(index + 1) else {
                eprintln!("error: --root needs a directory");
                return ExitCode::from(2);
            };
            root = Some(PathBuf::from(value));
            index += 2;
        } else if let Some(value) = argument.strip_prefix("--root=") {
            root = Some(PathBuf::from(value));
            index += 1;
        } else {
            eprintln!("error: unknown argument `{argument}`");
            print_usage();
            return ExitCode::from(2);
        }
    }

    let root = match root {
        Some(root) => root,
        None => match target::find_root() {
            Ok(root) => root,
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(1);
            }
        },
    };

    match manifest(&root, check) {
        Ok((path, counts)) => {
            print_report(&path, counts, check);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Regenerates the manifest in memory and byte-compares it with the committed file.
///
/// `compat check` uses this; `compat manifest --check` reaches it through
/// [`manifest`] so both commands compare the same bytes.
pub(crate) fn check(root: &Path) -> Result<Counts, ManifestError> {
    let (_, counts) = manifest(root, true)?;
    Ok(counts)
}

/// Generates the manifest in memory and either writes it or compares it with the
/// committed file. The comparison is over bytes, not lines.
fn manifest(root: &Path, check: bool) -> Result<(PathBuf, Counts), ManifestError> {
    let (lock, jar_lock) = target::load(root)?;
    let surface = scan(root, &lock, &jar_lock)?;
    let generated = surface.render(&lock, jar_lock.count);
    let path = root.join(&lock.manifest_path);

    if check {
        let committed = fs::read(&path).map_err(|error| {
            ManifestError::new(format!(
                "missing manifest {}: {error}; run `rava compat manifest`",
                lock.manifest_path
            ))
        })?;
        if committed != generated.as_bytes() {
            return Err(ManifestError::new(describe_difference(
                &lock.manifest_path,
                &committed,
                generated.as_bytes(),
            )));
        }
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ManifestError::new(format!(
                    "cannot create manifest directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        fs::write(&path, generated.as_bytes()).map_err(|error| {
            ManifestError::new(format!(
                "cannot write manifest {}: {error}",
                lock.manifest_path
            ))
        })?;
    }

    Ok((PathBuf::from(&lock.manifest_path), surface.counts()))
}

/// Explains the first line that differs from the committed manifest.
fn describe_difference(path: &str, committed: &[u8], generated: &[u8]) -> String {
    let committed = String::from_utf8_lossy(committed);
    let generated = String::from_utf8_lossy(generated);
    for (index, (old, new)) in committed.lines().zip(generated.lines()).enumerate() {
        if old != new {
            return format!(
                "manifest {path} is out of date at line {}: committed `{old}`, generated `{new}`; run `rava compat manifest`",
                index + 1
            );
        }
    }
    format!(
        "manifest {path} is out of date: {} committed lines, {} generated lines; run `rava compat manifest`",
        committed.lines().count(),
        generated.lines().count()
    )
}

fn print_report(path: &Path, counts: Counts, check: bool) {
    println!("rava compat manifest");
    row("path", &path.display().to_string());
    row("classes", &counts.classes.to_string());
    row("fields", &counts.fields.to_string());
    row("methods", &counts.methods.to_string());
    row("interface-methods", &counts.interface_methods.to_string());
    if check {
        println!("manifest: ok");
    } else {
        println!("manifest: written");
    }
}

fn row(label: &str, value: &str) {
    println!("  {label:<18}{value}");
}

fn print_usage() {
    println!("usage: rava compat manifest [--check] [--root <dir>]");
    println!();
    println!("Scans every .class entry in the pinned jars and records platform class,");
    println!("field, method and interface-method references. Without --check it writes");
    println!("the manifest pinned by [manifest].path in compat/targets.toml; with");
    println!("--check it regenerates in memory and byte-compares the committed file.");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> TargetLock {
        TargetLock {
            schema: 1,
            java: "17".to_owned(),
            class_file: "61.0".to_owned(),
            spring_boot: "3.5.16".to_owned(),
            spring_framework: "6.2.19".to_owned(),
            tomcat: "10.1.55".to_owned(),
            app_path: "reference-app".to_owned(),
            app_version: "0.1.0".to_owned(),
            app_main_class: "dev.rava.reference.ReferenceApplication".to_owned(),
            app_source_sha256: "a".repeat(64),
            jars_dir: "compat/jars".to_owned(),
            jars_lock: "compat/jars.lock".to_owned(),
            jar_count: 35,
            manifest_path: "compat/surface/test.txt".to_owned(),
            stubs_path: "stdlib/classes".to_owned(),
            c1_path: target::DEFAULT_C1_PATH.to_owned(),
            c1_unreferenced: target::DEFAULT_C1_UNREFERENCED.to_owned(),
        }
    }

    fn member(owner: &str, name: &str, descriptor: &str) -> Member {
        Member {
            owner: owner.to_owned(),
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        }
    }

    #[test]
    fn matches_only_the_pinned_platform_prefixes() {
        for name in [
            "java/lang/Object",
            "javax/sql/DataSource",
            "jdk/internal/misc/Unsafe",
            "sun/misc/Unsafe",
        ] {
            assert!(is_platform(name), "{name}");
        }
        for name in [
            "com/example/App",
            "org/springframework/core/ApplicationContext",
            "javafx/application/Application",
            "javax2/thing/Type",
            "javaxthing/Type",
        ] {
            assert!(!is_platform(name), "{name}");
        }
    }

    #[test]
    fn renders_groups_in_a_stable_order_with_counts() {
        let mut surface = Surface::default();
        // Insert out of order and twice: the sets deduplicate and sort.
        surface.classes.insert("java/lang/System".to_owned());
        surface.classes.insert("java/lang/Object".to_owned());
        surface.classes.insert("java/lang/Object".to_owned());
        surface
            .fields
            .insert(member("java/lang/System", "out", "Ljava/io/PrintStream;"));
        surface
            .fields
            .insert(member("java/lang/System", "out", "Ljava/io/PrintStream;"));
        surface
            .methods
            .insert(member("java/lang/Object", "<init>", "()V"));
        surface.methods.insert(member(
            "java/io/PrintStream",
            "println",
            "(Ljava/lang/String;)V",
        ));
        surface
            .interface_methods
            .insert(member("java/lang/Runnable", "run", "()V"));

        let text = surface.render(&lock(), 35);
        let header = "# classes: 2\n# fields: 1\n# methods: 2\n# interface-methods: 1\n";
        assert!(text.contains(header), "{text}");
        let records: Vec<&str> = text.lines().filter(|line| !line.starts_with('#')).collect();
        assert_eq!(
            records,
            vec![
                "class java/lang/Object",
                "class java/lang/System",
                "field java/lang/System out Ljava/io/PrintStream;",
                "method java/io/PrintStream println (Ljava/lang/String;)V",
                "method java/lang/Object <init> ()V",
                "interface-method java/lang/Runnable run ()V",
            ]
        );
        assert_eq!(text, surface.render(&lock(), 35), "render is reproducible");
    }

    #[test]
    fn header_carries_the_target_versions_and_never_a_host_path() {
        let surface = Surface::default();
        let text = surface.render(&lock(), 35);
        assert!(text.contains("java 17 (class file 61.0)"), "{text}");
        assert!(text.contains("spring boot 3.5.16"), "{text}");
        assert!(text.contains("spring framework 6.2.19"), "{text}");
        assert!(text.contains("tomcat 10.1.55"), "{text}");
        assert!(text.contains("# jars: 35"), "{text}");
        assert!(!text.contains("/home/"), "{text}");
    }

    fn classes(descriptor: &str) -> Vec<String> {
        let mut classes = BTreeSet::new();
        record_field_descriptor(descriptor, &mut classes).expect("field descriptor parses");
        classes.into_iter().collect()
    }

    fn method_classes(descriptor: &str) -> Vec<String> {
        let mut classes = BTreeSet::new();
        record_method_descriptor(descriptor, &mut classes).expect("method descriptor parses");
        classes.into_iter().collect()
    }

    #[test]
    fn field_descriptors_name_their_class_and_unwrap_arrays() {
        assert_eq!(classes("I"), Vec::<String>::new());
        assert_eq!(classes("Ljava/lang/String;"), ["java/lang/String"]);
        assert_eq!(
            classes("[[Ljava/lang/annotation/ElementType;"),
            ["java/lang/annotation/ElementType"]
        );
        assert_eq!(classes("[Ljava/util/Map$Entry;"), ["java/util/Map$Entry"]);
        // A non-platform type stays out of the platform surface.
        assert_eq!(
            classes("Lorg/springframework/core/Ordered;"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn method_descriptors_name_parameters_and_the_return_type() {
        assert_eq!(
            method_classes(
                "(Ljava/lang/annotation/ElementType;[Ljava/lang/String;I)Ljava/util/Map;"
            ),
            [
                "java/lang/String",
                "java/lang/annotation/ElementType",
                "java/util/Map"
            ]
        );
        assert_eq!(method_classes("()V"), Vec::<String>::new());
        assert_eq!(method_classes("()[I"), Vec::<String>::new());
    }

    #[test]
    fn malformed_descriptors_are_rejected() {
        for descriptor in [
            "",
            "X",
            "Ljava/lang/String",
            "Ljava/lang/String;;",
            "[",
            "[X",
        ] {
            let mut classes = BTreeSet::new();
            assert!(
                record_field_descriptor(descriptor, &mut classes).is_err(),
                "{descriptor}"
            );
        }
        for descriptor in [
            "",
            "I",
            "(",
            "()",
            "()X",
            "()Vextra",
            "(I",
            "(Ljava/lang/String;)",
        ] {
            let mut classes = BTreeSet::new();
            assert!(
                record_method_descriptor(descriptor, &mut classes).is_err(),
                "{descriptor}"
            );
        }
    }

    #[test]
    fn class_signatures_name_bounds_owners_and_interfaces() {
        let signature = parse_class_signature(
            "<T:Ljava/lang/Number;:Ljava/lang/Comparable<TT;>;>\
             Ljava/lang/Object;Ljava/util/function/Supplier<+Ljava/lang/CharSequence;>;",
        )
        .expect("class signature parses");
        let mut classes = BTreeSet::new();
        record_class_signature(&signature, &mut classes);
        assert_eq!(
            classes.into_iter().collect::<Vec<_>>(),
            [
                "java/lang/CharSequence",
                "java/lang/Comparable",
                "java/lang/Number",
                "java/lang/Object",
                "java/util/function/Supplier"
            ]
        );

        // A nested owner spelled as `Map<K,V>.Entry<B>` records the owner, its arguments
        // and the member type's own argument.
        let signature = parse_class_signature(
            "Ljava/lang/Object;Ljava/util/Map<Ljava/lang/String;Ljava/lang/Integer;>\
             .Entry<Ljava/lang/Boolean;>;",
        )
        .expect("class signature parses");
        let mut classes = BTreeSet::new();
        record_class_signature(&signature, &mut classes);
        for expected in [
            "java/lang/Boolean",
            "java/lang/Integer",
            "java/lang/String",
            "java/util/Map",
            "java/util/Map$Entry",
        ] {
            assert!(
                classes.contains(expected),
                "missing {expected}: {classes:?}"
            );
        }
    }

    #[test]
    fn method_signatures_name_parameters_results_and_throws() {
        let signature = parse_method_signature(
            "<E:Ljava/lang/Exception;>(Ljava/util/List<TE;>;[TE;)Ljava/lang/Object;^TE;^Ljava/io/IOException;",
        )
        .expect("method signature parses");
        let mut classes = BTreeSet::new();
        record_method_signature(&signature, &mut classes);
        assert_eq!(
            classes.into_iter().collect::<Vec<_>>(),
            [
                "java/io/IOException",
                "java/lang/Exception",
                "java/lang/Object",
                "java/util/List"
            ]
        );
    }

    #[test]
    fn field_signatures_name_arguments_and_arrays() {
        let signature =
            parse_field_signature("Ljava/util/Map<Ljava/lang/String;+Ljava/lang/Number;>;")
                .expect("field signature parses");
        let mut classes = BTreeSet::new();
        record_signature_type(&signature, &mut classes);
        assert_eq!(
            classes.into_iter().collect::<Vec<_>>(),
            ["java/lang/Number", "java/lang/String", "java/util/Map"]
        );
    }
}
