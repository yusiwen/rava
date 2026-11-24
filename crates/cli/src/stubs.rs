//! `rava compat stubs` and `rava compat link-check` — the M1.3 C0 surface.
//!
//! `stubs` writes one signature-only class file for every owner in the committed
//! manifest. `link-check` parses those files and resolves every class, field, method
//! and interface-method record against the members the stubs declare.
//!
//! Neither command reads the pinned jars or executes a class: the committed manifest
//! and `stdlib/classes/` are the whole input. Link-check failing on a missing stub is
//! deliberate; it never generates one.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rava_classfile::{ClassBuilder, ClassFile};
use rava_vm::{ClassSource, DirectorySource};

use crate::surface::{Counts, Member, Surface};
use crate::target::{self, TargetLock};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_STATIC: u16 = 0x0008;
const ACC_FINAL: u16 = 0x0010;
const ACC_SUPER: u16 = 0x0020;
const ACC_INTERFACE: u16 = 0x0200;
const ACC_ABSTRACT: u16 = 0x0400;
const ACC_PUBLIC_ABSTRACT: u16 = ACC_PUBLIC | ACC_ABSTRACT;

/// A class-level marker visible through `javap -v`; stubs are not the runtime library.
const STUB_SOURCE: &str = "RavaStub.java";

/// How many unresolved records a failed link-check prints before the total.
const SAMPLE_LIMIT: usize = 20;

/// A malformed manifest, an unwritable stub tree or a corrupt stub class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StubError(String);

impl StubError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for StubError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<target::LockError> for StubError {
    fn from(error: target::LockError) -> Self {
        Self(error.to_string())
    }
}

impl From<crate::surface::SurfaceError> for StubError {
    fn from(error: crate::surface::SurfaceError) -> Self {
        Self(error.to_string())
    }
}

/// The result of writing the stub tree.
struct StubReport {
    manifest: String,
    stubs: String,
    counts: Counts,
    files: usize,
}

/// The result of resolving the committed manifest against the stub tree.
pub(crate) struct LinkReport {
    manifest: String,
    stubs: String,
    pub(crate) counts: Counts,
    pub(crate) unresolved: Vec<String>,
}

/// The members grouped by owner, in the order the stubs declare them.
struct StubEntries<'a> {
    interfaces: BTreeSet<&'a str>,
    fields: BTreeMap<&'a str, BTreeSet<(&'a str, &'a str)>>,
    methods: BTreeMap<&'a str, BTreeSet<(&'a str, &'a str)>>,
}

impl<'a> StubEntries<'a> {
    fn collect(surface: &'a Surface) -> Self {
        let mut entries = Self {
            interfaces: BTreeSet::new(),
            fields: BTreeMap::new(),
            methods: BTreeMap::new(),
        };

        // `invokeinterface` only names interfaces, so an interface-method owner is the
        // one interface test the manifest can support. Classes that are interfaces but
        // are only named by `CONSTANT_Class` remain plain classes in the stub tree.
        for member in surface.interface_methods() {
            entries.interfaces.insert(member.owner.as_str());
        }
        for member in surface.fields() {
            entries
                .fields
                .entry(member.owner.as_str())
                .or_default()
                .insert((member.name.as_str(), member.descriptor.as_str()));
        }
        // A class can in principle carry both record kinds; the set keeps one declaration.
        for member in surface.methods() {
            entries
                .methods
                .entry(member.owner.as_str())
                .or_default()
                .insert((member.name.as_str(), member.descriptor.as_str()));
        }
        for member in surface.interface_methods() {
            entries
                .methods
                .entry(member.owner.as_str())
                .or_default()
                .insert((member.name.as_str(), member.descriptor.as_str()));
        }
        entries
    }
}

/// Runs `rava compat stubs [--root <dir>]`.
pub fn stubs_command(args: &[String]) -> ExitCode {
    let root = match parse_root(args, print_stubs_usage) {
        Ok(Some(root)) => root,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return ExitCode::from(code),
    };

    match write_stubs(&root) {
        Ok(report) => {
            print_stub_report(&report);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Runs `rava compat link-check [--root <dir>]`.
pub fn link_check_command(args: &[String]) -> ExitCode {
    let root = match parse_root(args, print_link_check_usage) {
        Ok(Some(root)) => root,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return ExitCode::from(code),
    };

    match link_check(&root) {
        Ok(report) if report.unresolved.is_empty() => {
            print_link_report(&report);
            ExitCode::SUCCESS
        }
        Ok(report) => {
            print_link_report(&report);
            eprintln!(
                "error: link-check found {} unresolved record(s)",
                report.unresolved.len()
            );
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Parses `[--root <dir>]`; `None` means help was printed.
fn parse_root(args: &[String], usage: fn()) -> Result<Option<PathBuf>, u8> {
    let mut root = None;
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "-h" || argument == "--help" {
            usage();
            return Ok(None);
        } else if argument == "--root" {
            let Some(value) = args.get(index + 1) else {
                eprintln!("error: --root needs a directory");
                return Err(2);
            };
            root = Some(PathBuf::from(value));
            index += 2;
        } else if let Some(value) = argument.strip_prefix("--root=") {
            root = Some(PathBuf::from(value));
            index += 1;
        } else {
            eprintln!("error: unknown argument `{argument}`");
            usage();
            return Err(2);
        }
    }

    match root {
        Some(root) => Ok(Some(root)),
        None => match target::find_root() {
            Ok(root) => Ok(Some(root)),
            Err(error) => {
                eprintln!("error: {error}");
                Err(1)
            }
        },
    }
}

/// Writes a fresh `stdlib/classes` tree from the committed manifest.
fn write_stubs(root: &Path) -> Result<StubReport, StubError> {
    let (lock, surface) = load_surface(root)?;
    let files = write_stub_tree(&root.join(&lock.stubs_path), &lock, &surface)?;

    Ok(StubReport {
        manifest: lock.manifest_path,
        stubs: lock.stubs_path,
        counts: surface.counts(),
        files,
    })
}

/// Writes a complete stub tree into `stub_dir` and returns the number of class files.
///
/// The directory is generated output. Replacing it wholesale removes files for
/// classes that left the manifest and keeps the tree byte-identical across runs.
/// `compat check` calls this with a temporary directory, so the committed stub tree
/// is never rewritten by the gate.
pub(crate) fn write_stub_tree(
    stub_dir: &Path,
    lock: &TargetLock,
    surface: &Surface,
) -> Result<usize, StubError> {
    if stub_dir.exists() {
        fs::remove_dir_all(stub_dir).map_err(|error| {
            StubError::new(format!(
                "cannot clear stub directory {}: {error}",
                stub_dir.display()
            ))
        })?;
    }
    fs::create_dir_all(stub_dir).map_err(|error| {
        StubError::new(format!(
            "cannot create stub directory {}: {error}",
            stub_dir.display()
        ))
    })?;

    let major = class_major(lock)?;
    let entries = StubEntries::collect(surface);
    for owner in surface.classes() {
        let bytes = stub_class(owner, &entries, major)?;
        let path = stub_dir.join(format!("{owner}.class"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                StubError::new(format!(
                    "cannot create stub directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        fs::write(&path, bytes).map_err(|error| {
            StubError::new(format!("cannot write stub {}: {error}", path.display()))
        })?;
    }

    Ok(surface.counts().classes)
}

/// Reads and parses the committed manifest through the validated target lock.
pub(crate) fn load_surface(root: &Path) -> Result<(TargetLock, Surface), StubError> {
    let lock = target::load_target(root)?;
    let path = root.join(&lock.manifest_path);
    let text = fs::read_to_string(&path).map_err(|error| {
        StubError::new(format!(
            "missing surface manifest {}: {error}; run `rava compat manifest`",
            path.display()
        ))
    })?;
    let surface = Surface::parse(&text)?;
    Ok((lock, surface))
}

/// Builds one signature-only class file.
fn stub_class(owner: &str, entries: &StubEntries<'_>, major: u16) -> Result<Vec<u8>, StubError> {
    let interface = entries.interfaces.contains(owner);
    let access = if interface {
        ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT
    } else {
        ACC_PUBLIC | ACC_SUPER | ACC_ABSTRACT
    };
    let mut builder = if owner == "java/lang/Object" {
        ClassBuilder::root(owner, major)
    } else {
        ClassBuilder::new(owner, "java/lang/Object", major)
    }
    .with_access(access);
    builder.source_file(STUB_SOURCE);

    let field_access = if interface {
        ACC_PUBLIC | ACC_STATIC | ACC_FINAL
    } else {
        ACC_PUBLIC
    };
    if let Some(fields) = entries.fields.get(owner) {
        for (name, descriptor) in fields {
            builder.field(field_access, name, descriptor);
        }
    }

    if let Some(methods) = entries.methods.get(owner) {
        for (name, descriptor) in methods {
            match *name {
                "<init>" => {
                    if interface {
                        return Err(StubError::new(format!(
                            "interface {owner} declares a constructor {descriptor}"
                        )));
                    }
                    add_constructor(&mut builder, owner, descriptor)?;
                }
                "<clinit>" => {
                    builder.method(ACC_STATIC, name, descriptor, Some((0, 0, vec![0xb1])));
                }
                _ => {
                    builder.method(ACC_PUBLIC_ABSTRACT, name, descriptor, None);
                }
            }
        }
    }

    Ok(builder.build())
}

/// Adds a concrete constructor whose only work is the `Object` super call.
///
/// JVMS §4.6 forbids an abstract `<init>`, so the one method that cannot be
/// signature-only carries a minimal body. It is never executed by link-check.
fn add_constructor(
    builder: &mut ClassBuilder,
    owner: &str,
    descriptor: &str,
) -> Result<(), StubError> {
    let max_locals = parameter_slots(descriptor)?;
    if owner == "java/lang/Object" {
        builder.method(
            ACC_PUBLIC,
            "<init>",
            descriptor,
            Some((0, max_locals, vec![0xb1])),
        );
        return Ok(());
    }

    let super_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let [high, low] = super_init.to_be_bytes();
    let code = vec![0x2a, 0xb7, high, low, 0xb1]; // aload_0; invokespecial; return
    builder.method(
        ACC_PUBLIC,
        "<init>",
        descriptor,
        Some((1, max_locals, code)),
    );
    Ok(())
}

/// `max_locals` for a constructor: `this` plus one slot per parameter, two for
/// `long`/`double`. It also rejects a malformed descriptor before it reaches a class file.
fn parameter_slots(descriptor: &str) -> Result<u16, StubError> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return Err(StubError::new(format!(
            "malformed method descriptor `{descriptor}`: expected `(`"
        )));
    }

    let mut index = 1;
    let mut slots = 1u16; // `this`
    loop {
        let Some(&byte) = bytes.get(index) else {
            return Err(StubError::new(format!(
                "malformed method descriptor `{descriptor}`: missing `)`"
            )));
        };
        match byte {
            b')' => {
                index += 1;
                break;
            }
            b'B' | b'C' | b'F' | b'I' | b'S' | b'Z' => {
                slots = add_slots(slots, 1, descriptor)?;
                index += 1;
            }
            b'J' | b'D' => {
                slots = add_slots(slots, 2, descriptor)?;
                index += 1;
            }
            b'L' => {
                index = end_of_class_name(bytes, index + 1, descriptor)?;
                slots = add_slots(slots, 1, descriptor)?;
            }
            b'[' => {
                index += 1;
                while bytes.get(index) == Some(&b'[') {
                    index += 1;
                }
                match bytes.get(index) {
                    Some(b'L') => {
                        index = end_of_class_name(bytes, index + 1, descriptor)?;
                    }
                    Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
                        index += 1;
                    }
                    _ => {
                        return Err(StubError::new(format!(
                            "malformed method descriptor `{descriptor}`: bad array element"
                        )));
                    }
                }
                slots = add_slots(slots, 1, descriptor)?;
            }
            _ => {
                return Err(StubError::new(format!(
                    "malformed method descriptor `{descriptor}`: unexpected `{}`",
                    byte as char
                )));
            }
        }
    }

    match bytes.get(index) {
        Some(b'V') => {
            if index + 1 != bytes.len() {
                return Err(malformed_tail(descriptor));
            }
        }
        Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
            if index + 1 != bytes.len() {
                return Err(malformed_tail(descriptor));
            }
        }
        Some(b'L') => {
            if end_of_class_name(bytes, index + 1, descriptor)? != bytes.len() {
                return Err(malformed_tail(descriptor));
            }
        }
        Some(b'[') => {
            let mut cursor = index + 1;
            while bytes.get(cursor) == Some(&b'[') {
                cursor += 1;
            }
            match bytes.get(cursor) {
                Some(b'L') => {
                    if end_of_class_name(bytes, cursor + 1, descriptor)? != bytes.len() {
                        return Err(malformed_tail(descriptor));
                    }
                }
                Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
                    if cursor + 1 != bytes.len() {
                        return Err(malformed_tail(descriptor));
                    }
                }
                _ => return Err(malformed_tail(descriptor)),
            }
        }
        _ => return Err(malformed_tail(descriptor)),
    }

    Ok(slots)
}

fn add_slots(slots: u16, extra: u16, descriptor: &str) -> Result<u16, StubError> {
    slots.checked_add(extra).ok_or_else(|| {
        StubError::new(format!(
            "method descriptor `{descriptor}` has too many parameter slots"
        ))
    })
}

fn end_of_class_name(bytes: &[u8], start: usize, descriptor: &str) -> Result<usize, StubError> {
    match bytes[start..].iter().position(|&byte| byte == b';') {
        Some(offset) => Ok(start + offset + 1),
        None => Err(StubError::new(format!(
            "malformed method descriptor `{descriptor}`: unterminated class name"
        ))),
    }
}

fn malformed_tail(descriptor: &str) -> StubError {
    StubError::new(format!(
        "malformed method descriptor `{descriptor}`: bad return type"
    ))
}

/// Reads the class-file major version pinned by the target lock.
fn class_major(lock: &TargetLock) -> Result<u16, StubError> {
    lock.class_file
        .split('.')
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| {
            StubError::new(format!(
                "compat/targets.toml: class_file `{}` is not a major version",
                lock.class_file
            ))
        })
}

/// One stub class's declared fields and methods, with declaration counts so a
/// duplicate is visible rather than silently resolving.
#[derive(Debug, Default)]
struct ClassIndex {
    fields: BTreeMap<(String, String), usize>,
    methods: BTreeMap<(String, String), usize>,
}

impl ClassIndex {
    fn from_class(class: &ClassFile, owner: &str) -> Result<Self, StubError> {
        let mut index = Self::default();
        for field in &class.fields {
            let name = class
                .field_name(field)
                .map_err(|error| StubError::new(format!("stub {owner}: {error}")))?;
            let descriptor = class
                .field_descriptor(field)
                .map_err(|error| StubError::new(format!("stub {owner}: {error}")))?;
            *index
                .fields
                .entry((name.to_owned(), descriptor.to_owned()))
                .or_insert(0) += 1;
        }
        for method in &class.methods {
            let name = class
                .method_name(method)
                .map_err(|error| StubError::new(format!("stub {owner}: {error}")))?;
            let descriptor = class
                .method_descriptor(method)
                .map_err(|error| StubError::new(format!("stub {owner}: {error}")))?;
            *index
                .methods
                .entry((name.to_owned(), descriptor.to_owned()))
                .or_insert(0) += 1;
        }
        Ok(index)
    }
}

/// Resolves every manifest record against the stubs. It parses class files only; it
/// never asks the loader to define them, so no verification or execution happens.
pub(crate) fn link_check(root: &Path) -> Result<LinkReport, StubError> {
    let (lock, surface) = load_surface(root)?;
    let source = DirectorySource::new(root.join(&lock.stubs_path));
    let mut classes: BTreeMap<String, ClassIndex> = BTreeMap::new();
    let mut unresolved = Vec::new();

    for owner in surface.classes() {
        let bytes = source
            .read(owner)
            .map_err(|error| StubError::new(format!("cannot read stub {owner}: {error}")))?;
        match bytes {
            Some(bytes) => {
                let class = ClassFile::parse(&bytes).map_err(|error| {
                    StubError::new(format!("cannot parse stub {owner}: {error}"))
                })?;
                let actual = class
                    .this_class_name()
                    .map_err(|error| StubError::new(format!("stub {owner}: {error}")))?;
                if actual != owner {
                    // A misplaced file would otherwise index the wrong members under
                    // this owner and hide the defect.
                    unresolved.push(format!("class {owner} (declares {actual})"));
                    continue;
                }
                classes.insert(owner.to_owned(), ClassIndex::from_class(&class, owner)?);
            }
            None => unresolved.push(format!("class {owner} (missing stub)")),
        }
    }

    for member in surface.fields() {
        if let Some(problem) = resolve("field", member, &classes) {
            unresolved.push(problem);
        }
    }
    for member in surface.methods() {
        if let Some(problem) = resolve("method", member, &classes) {
            unresolved.push(problem);
        }
    }
    for member in surface.interface_methods() {
        if let Some(problem) = resolve("interface-method", member, &classes) {
            unresolved.push(problem);
        }
    }

    Ok(LinkReport {
        manifest: lock.manifest_path,
        stubs: lock.stubs_path,
        counts: surface.counts(),
        unresolved,
    })
}

fn resolve(kind: &str, member: &Member, classes: &BTreeMap<String, ClassIndex>) -> Option<String> {
    let Some(class) = classes.get(&member.owner) else {
        return Some(format!(
            "{kind} {} {} {} (missing class)",
            member.owner, member.name, member.descriptor
        ));
    };
    let declarations = if kind == "field" {
        &class.fields
    } else {
        &class.methods
    };
    match declarations.get(&(member.name.clone(), member.descriptor.clone())) {
        None => Some(format!(
            "{kind} {} {} {} (missing member)",
            member.owner, member.name, member.descriptor
        )),
        Some(1) => None,
        Some(count) => Some(format!(
            "{kind} {} {} {} (ambiguous: {count} declarations)",
            member.owner, member.name, member.descriptor
        )),
    }
}

fn print_stub_report(report: &StubReport) {
    println!("rava compat stubs");
    row("manifest", &report.manifest);
    row("stubs", &report.stubs);
    row("classes", &report.counts.classes.to_string());
    row("class files", &report.files.to_string());
    row("fields", &report.counts.fields.to_string());
    row("methods", &report.counts.methods.to_string());
    row(
        "interface-methods",
        &report.counts.interface_methods.to_string(),
    );
    println!("stubs: written");
}

fn print_link_report(report: &LinkReport) {
    println!("rava compat link-check");
    row("manifest", &report.manifest);
    row("stubs", &report.stubs);
    row("classes", &report.counts.classes.to_string());
    row("fields", &report.counts.fields.to_string());
    row("methods", &report.counts.methods.to_string());
    row(
        "interface-methods",
        &report.counts.interface_methods.to_string(),
    );
    row("unresolved", &report.unresolved.len().to_string());
    if report.unresolved.is_empty() {
        println!("link-check: ok");
        return;
    }

    let shown = report.unresolved.len().min(SAMPLE_LIMIT);
    println!();
    println!(
        "unresolved records (showing {shown} of {}):",
        report.unresolved.len()
    );
    for record in report.unresolved.iter().take(SAMPLE_LIMIT) {
        println!("  {record}");
    }
    println!("link-check: failed");
}

fn row(label: &str, value: &str) {
    println!("  {label:<18}{value}");
}

fn print_stubs_usage() {
    println!("usage: rava compat stubs [--root <dir>]");
    println!();
    println!("Writes a signature-only class file under [stubs].path for every owner in");
    println!("the committed surface manifest. Existing stub files are replaced. The");
    println!("command reads only the manifest and the target lock; it never fetches jars.");
}

fn print_link_check_usage() {
    println!("usage: rava compat link-check [--root <dir>]");
    println!();
    println!("Resolves every class, field, method and interface-method record in the");
    println!("committed manifest against the stub class files. It reads only the manifest,");
    println!("the target lock and the stubs; it never loads, executes or generates a class.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_constructor_slots() {
        assert_eq!(parameter_slots("()V").unwrap(), 1);
        assert_eq!(parameter_slots("(I)V").unwrap(), 2);
        assert_eq!(parameter_slots("(IJ)V").unwrap(), 4);
        assert_eq!(parameter_slots("(Ljava/lang/String;[D)V").unwrap(), 3);
        assert_eq!(
            parameter_slots("([[Ljava/lang/Object;Ljava/lang/String;)V").unwrap(),
            3
        );
    }

    #[test]
    fn rejects_a_malformed_descriptor() {
        for descriptor in ["I)V", "()", "(L)V", "(Ljava/lang/String)V", "([V)V"] {
            assert!(
                parameter_slots(descriptor).is_err(),
                "`{descriptor}` must be rejected"
            );
        }
    }
}
