//! `rava compat c1-stubs` and `rava compat metadata` — the M2.2 C1 signature gate.
//!
//! The C1 stub tree is a second, deliberately separate tree from the M1
//! `stdlib/classes/` signatures. For every manifest owner that Java 17 defines, the
//! generator reads that owner's release-17 signature from the host JDK's `lib/ct.sym`,
//! parses it with the same `rava-classfile` parser the runtime uses, sets the class-file
//! version to the pinned 61.0 and re-emits a class file that keeps the constant pool at
//! its original indices and every attribute payload verbatim. No JDK class-file bytes are
//! copied: the source is parsed into the model and written back out.
//!
//! `metadata --check` then compares the committed C1 tree with the release-17 signature
//! again, through the M2.1 normalized metadata dump, and exits 1 on any difference. The
//! only two normalizations are:
//!
//! - the class-file version line, because `ct.sym` stores the host JDK's version (69 for
//!   JDK 25) while the pinned target is 61.0; the release is already encoded in the entry's
//!   name and the committed stub is required to be 61.0;
//! - nothing else. Ordering, annotations, signatures and raw attributes are compared as
//!   the M2.1 dump renders them.
//!
//! A manifest owner that Java 17 does not define has no release-17 signature. Those owners
//! are listed in a committed file (`[c1].unreferenced`), get a header-only placeholder class
//! so the tree still covers the whole manifest, and are excluded from the metadata diff.
//! The generator fails, naming the owners, when that committed list and the archive disagree.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use rava_classfile::{ClassBuilder, ClassFile};

use crate::ct_sym::{CtSym, CtSymError, release_letter};
use crate::surface::{Counts, Surface};
use crate::target::{self, TargetLock};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_SUPER: u16 = 0x0020;
const ACC_ABSTRACT: u16 = 0x0400;
const PLACEHOLDER_ACCESS: u16 = ACC_PUBLIC | ACC_SUPER | ACC_ABSTRACT;
const PLACEHOLDER_SOURCE: &str = "RavaC1Unreferenced.java";
const SAMPLE_LIMIT: usize = 10;

/// A missing or inconsistent C1 input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct C1Error(String);

impl C1Error {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for C1Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<target::LockError> for C1Error {
    fn from(error: target::LockError) -> Self {
        Self(error.to_string())
    }
}

impl From<crate::surface::SurfaceError> for C1Error {
    fn from(error: crate::surface::SurfaceError) -> Self {
        Self(error.to_string())
    }
}

impl From<CtSymError> for C1Error {
    fn from(error: CtSymError) -> Self {
        Self(error.to_string())
    }
}

/// The result of writing or scanning the C1 tree.
struct C1Report {
    manifest: String,
    stubs: String,
    unreferenced_list: String,
    ct_sym: PathBuf,
    release: String,
    counts: Counts,
    from_reference: usize,
    unreferenced: usize,
    files: usize,
}

/// The result of the normalized metadata comparison.
struct MetadataReport {
    manifest: String,
    stubs: String,
    ct_sym: Option<PathBuf>,
    release: String,
    classes: usize,
    from_reference: usize,
    unreferenced: usize,
    fields: usize,
    methods: usize,
    differences: Vec<Difference>,
}

struct Difference {
    owner: String,
    detail: String,
}

/// Runs `rava compat c1-stubs [--root <dir>] [--ct-sym <path>]`.
pub fn c1_stubs_command(args: &[String]) -> ExitCode {
    let options = match parse_options(args, false) {
        Ok(Some(options)) => options,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return ExitCode::from(code),
    };
    let root = match resolve_root(options.root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    let ct_sym = match resolve_ct_sym(options.ct_sym.as_deref()) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };

    match write_c1_tree(&root, &ct_sym) {
        Ok(report) => {
            print_c1_report(&report);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Runs `rava compat metadata [--check] [--root <dir>] [--ct-sym <path>]`.
pub fn metadata_command(args: &[String]) -> ExitCode {
    let options = match parse_options(args, true) {
        Ok(Some(options)) => options,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return ExitCode::from(code),
    };
    let root = match resolve_root(options.root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(1);
        }
    };
    let ct_sym = if options.check {
        match resolve_ct_sym(options.ct_sym.as_deref()) {
            Ok(path) => Some(path),
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::from(1);
            }
        }
    } else {
        None
    };

    match scan_metadata(&root, ct_sym.as_deref()) {
        Ok(report) => {
            print_metadata_report(&report, options.check);
            if options.check && !report.differences.is_empty() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

#[derive(Default)]
struct Options {
    root: Option<PathBuf>,
    ct_sym: Option<PathBuf>,
    check: bool,
}

/// Parses the shared options; `None` means help was printed.
fn parse_options(args: &[String], allow_check: bool) -> Result<Option<Options>, u8> {
    let mut options = Options::default();
    let mut index = 0;
    while index < args.len() {
        let argument = args[index].as_str();
        match argument {
            "-h" | "--help" => {
                print_usage(allow_check);
                return Ok(None);
            }
            "--check" if allow_check => {
                options.check = true;
                index += 1;
            }
            "--root" | "--ct-sym" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("error: {argument} needs a value");
                    return Err(2);
                };
                set_option(&mut options, argument, value)?;
                index += 2;
            }
            _ if argument.starts_with("--root=") => {
                let value = argument.trim_start_matches("--root=");
                options.root = Some(PathBuf::from(value));
                index += 1;
            }
            _ if argument.starts_with("--ct-sym=") => {
                let value = argument.trim_start_matches("--ct-sym=");
                options.ct_sym = Some(PathBuf::from(value));
                index += 1;
            }
            other => {
                eprintln!("error: unknown argument `{other}`");
                print_usage(allow_check);
                return Err(2);
            }
        }
    }
    Ok(Some(options))
}

fn set_option(options: &mut Options, name: &str, value: &str) -> Result<(), u8> {
    match name {
        "--root" => options.root = Some(PathBuf::from(value)),
        "--ct-sym" => options.ct_sym = Some(PathBuf::from(value)),
        other => {
            eprintln!("error: unknown argument `{other}`");
            return Err(2);
        }
    }
    Ok(())
}

fn resolve_root(root: Option<PathBuf>) -> Result<PathBuf, C1Error> {
    match root {
        Some(root) => Ok(root),
        None => target::find_root().map_err(C1Error::from),
    }
}

/// Finds `ct.sym`: the explicit flag, then `RAVA_CT_SYM`, then `$JAVA_HOME/lib/ct.sym`,
/// then the JDK that owns the first `java` executable on `PATH`.
fn resolve_ct_sym(explicit: Option<&Path>) -> Result<PathBuf, C1Error> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(C1Error::new(format!(
            "--ct-sym {} is not a file",
            path.display()
        )));
    }
    if let Some(value) = std::env::var_os("RAVA_CT_SYM") {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        return Err(C1Error::new(format!(
            "RAVA_CT_SYM names {} but that is not a file",
            path.display()
        )));
    }
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let path = PathBuf::from(home).join("lib/ct.sym");
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            let candidate = directory.join("java");
            if !candidate.is_file() {
                continue;
            }
            let Ok(java) = fs::canonicalize(&candidate) else {
                continue;
            };
            // `<jdk>/bin/java` -> `<jdk>/lib/ct.sym`.
            if let Some(jdk) = java.parent().and_then(Path::parent) {
                let path = jdk.join("lib/ct.sym");
                if path.is_file() {
                    return Ok(path);
                }
            }
        }
    }
    Err(C1Error::new(
        "cannot find ct.sym: pass --ct-sym <path>, set RAVA_CT_SYM, or set JAVA_HOME to a \
         JDK that has lib/ct.sym (the local acceptance uses the host JDK 25 archive)",
    ))
}

/// Opens a `ct.sym` and rejects an archive with no signatures for the pinned release,
/// which is what an older JDK's archive looks like.
fn open_reference(path: &Path, release: char) -> Result<CtSym, C1Error> {
    let ct_sym = CtSym::open(path, release)?;
    if ct_sym.is_empty() {
        return Err(C1Error::new(format!(
            "{} has no release-{} signatures; point --ct-sym, RAVA_CT_SYM or JAVA_HOME at a \
             JDK whose lib/ct.sym covers Java {}",
            path.display(),
            release,
            release_letter_to_major(release)
        )));
    }
    Ok(ct_sym)
}

fn release_letter_to_major(release: char) -> String {
    if let Some(digit) = release.to_digit(10) {
        digit.to_string()
    } else {
        ((release as u32 - 'A' as u32) + 10).to_string()
    }
}

/// Writes the C1 tree and returns its report.
fn write_c1_tree(root: &Path, ct_sym_path: &Path) -> Result<C1Report, C1Error> {
    let (lock, surface) = load_surface(root)?;
    let release = release_letter(java_major(&lock)?)?;
    let ct_sym = open_reference(ct_sym_path, release)?;
    let listed = read_unreferenced(root, &lock)?;
    let missing = unreferenced_owners(&surface, &ct_sym);
    check_unreferenced(&missing, &listed, root, &lock)?;

    let major = class_major(&lock)?;
    let directory = root.join(&lock.c1_path);
    replace_directory(&directory)?;

    let mut files = 0;
    let mut from_reference = 0;
    let mut unreferenced = 0;
    for owner in surface.classes() {
        let bytes = if missing.contains(owner) {
            unreferenced += 1;
            placeholder_class(owner, major)?
        } else {
            from_reference += 1;
            project_signature(&ct_sym, owner, major)?
        };
        let path = directory.join(format!("{owner}.class"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                C1Error::new(format!(
                    "cannot create C1 stub directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
        fs::write(&path, bytes).map_err(|error| {
            C1Error::new(format!("cannot write C1 stub {}: {error}", path.display()))
        })?;
        files += 1;
    }

    Ok(C1Report {
        manifest: lock.manifest_path,
        stubs: lock.c1_path,
        unreferenced_list: lock.c1_unreferenced,
        ct_sym: ct_sym.path().to_path_buf(),
        release: format!("{} ({release})", lock.java),
        counts: surface.counts(),
        from_reference,
        unreferenced,
        files,
    })
}

/// Project one release-17 signature into our own class file at the pinned version.
fn project_signature(ct_sym: &CtSym, owner: &str, major: u16) -> Result<Vec<u8>, C1Error> {
    let bytes = ct_sym.read(owner)?;
    let mut class = ClassFile::parse(&bytes).map_err(|error| {
        C1Error::new(format!(
            "ct.sym signature for {owner} does not parse: {error}"
        ))
    })?;
    let declared = class.this_class_name().map_err(|error| {
        C1Error::new(format!(
            "ct.sym signature for {owner} has no class name: {error}"
        ))
    })?;
    if declared != owner {
        return Err(C1Error::new(format!(
            "ct.sym entry for {owner} declares class {declared}; the index is wrong"
        )));
    }
    // `ct.sym` stores the host JDK's class-file version, not the release's. The release
    // is encoded in the entry name, so the projection sets the pinned target version.
    class.major_version = major;
    class.minor_version = 0;
    let written = class.write();
    // The emitted class must parse with the same parser before it is committed.
    ClassFile::parse(&written).map_err(|error| {
        C1Error::new(format!(
            "projected C1 stub for {owner} does not parse: {error}"
        ))
    })?;
    Ok(written)
}

/// A header-only class for a manifest owner Java 17 does not define.
fn placeholder_class(owner: &str, major: u16) -> Result<Vec<u8>, C1Error> {
    let mut builder =
        ClassBuilder::new(owner, "java/lang/Object", major).with_access(PLACEHOLDER_ACCESS);
    builder.source_file(PLACEHOLDER_SOURCE);
    let bytes = builder.build();
    ClassFile::parse(&bytes).map_err(|error| {
        C1Error::new(format!(
            "placeholder C1 stub for {owner} does not parse: {error}"
        ))
    })?;
    Ok(bytes)
}

fn load_surface(root: &Path) -> Result<(TargetLock, Surface), C1Error> {
    let lock = target::load_target(root)?;
    let path = root.join(&lock.manifest_path);
    let text = fs::read_to_string(&path).map_err(|error| {
        C1Error::new(format!(
            "missing surface manifest {}: {error}; run `rava compat manifest`",
            path.display()
        ))
    })?;
    Ok((lock, Surface::parse(&text)?))
}

fn java_major(lock: &TargetLock) -> Result<u32, C1Error> {
    lock.java.parse::<u32>().map_err(|_| {
        C1Error::new(format!(
            "compat/targets.toml: platform.java `{}` is not a major version",
            lock.java
        ))
    })
}

fn class_major(lock: &TargetLock) -> Result<u16, C1Error> {
    lock.class_file
        .split('.')
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| {
            C1Error::new(format!(
                "compat/targets.toml: class_file `{}` is not a major version",
                lock.class_file
            ))
        })
}

/// The manifest owners the release-17 archive does not define.
fn unreferenced_owners(surface: &Surface, ct_sym: &CtSym) -> BTreeSet<String> {
    surface
        .classes()
        .filter(|owner| !ct_sym.contains(owner))
        .map(str::to_owned)
        .collect()
}

fn read_unreferenced(root: &Path, lock: &TargetLock) -> Result<BTreeSet<String>, C1Error> {
    let path = root.join(&lock.c1_unreferenced);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(error) => {
            return Err(C1Error::new(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect())
}

/// Fails when the archive's unreferenced set and the committed list disagree.
fn check_unreferenced(
    missing: &BTreeSet<String>,
    listed: &BTreeSet<String>,
    root: &Path,
    lock: &TargetLock,
) -> Result<(), C1Error> {
    if missing == listed {
        return Ok(());
    }
    let not_listed: Vec<&str> = missing.difference(listed).map(String::as_str).collect();
    let stale: Vec<&str> = listed.difference(missing).map(String::as_str).collect();
    let path = root.join(&lock.c1_unreferenced);
    let mut message = String::from(
        "the manifest owners without a release-17 ct.sym signature do not match the committed list",
    );
    if !not_listed.is_empty() {
        message.push_str(&format!(
            "\n  not in {} ({}): {}",
            path.display(),
            not_listed.len(),
            not_listed.join(", ")
        ));
    }
    if !stale.is_empty() {
        message.push_str(&format!(
            "\n  listed in {} but resolved by Java 17 ({}): {}",
            path.display(),
            stale.len(),
            stale.join(", ")
        ));
    }
    Err(C1Error::new(message))
}

fn replace_directory(directory: &Path) -> Result<(), C1Error> {
    if directory.exists() {
        fs::remove_dir_all(directory).map_err(|error| {
            C1Error::new(format!(
                "cannot clear C1 stub directory {}: {error}",
                directory.display()
            ))
        })?;
    }
    fs::create_dir_all(directory).map_err(|error| {
        C1Error::new(format!(
            "cannot create C1 stub directory {}: {error}",
            directory.display()
        ))
    })
}

/// Scans the committed C1 tree; with a reference archive, also diffs it.
fn scan_metadata(root: &Path, ct_sym_path: Option<&Path>) -> Result<MetadataReport, C1Error> {
    let (lock, surface) = load_surface(root)?;
    let major = class_major(&lock)?;
    let directory = root.join(&lock.c1_path);
    if !directory.is_dir() {
        return Err(C1Error::new(format!(
            "missing C1 stub directory {}; run `rava compat c1-stubs`",
            directory.display()
        )));
    }
    let present = class_files(&directory)?;
    let expected: BTreeSet<String> = surface.classes().map(str::to_owned).collect();
    let extras: Vec<&String> = present.difference(&expected).collect();
    if !extras.is_empty() {
        return Err(C1Error::new(format!(
            "{} has {} class file(s) that are not manifest owners: {}",
            directory.display(),
            extras.len(),
            extras
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let release = release_letter(java_major(&lock)?)?;
    let ct_sym = match ct_sym_path {
        Some(path) => Some(open_reference(path, release)?),
        None => None,
    };
    let missing = ct_sym
        .as_ref()
        .map(|ct_sym| unreferenced_owners(&surface, ct_sym))
        .unwrap_or_default();
    if ct_sym.is_some() {
        let listed = read_unreferenced(root, &lock)?;
        check_unreferenced(&missing, &listed, root, &lock)?;
    }

    let mut fields = 0;
    let mut methods = 0;
    let mut from_reference = 0;
    let mut unreferenced = 0;
    let mut differences = Vec::new();
    for owner in surface.classes() {
        let path = directory.join(format!("{owner}.class"));
        let bytes = fs::read(&path).map_err(|error| {
            C1Error::new(format!("cannot read C1 stub {}: {error}", path.display()))
        })?;
        let stub = parse_stub(owner, &bytes, major)?;
        fields += stub.metadata.fields.len();
        methods += stub.metadata.methods.len();

        let Some(ct_sym) = ct_sym.as_ref() else {
            continue;
        };
        if missing.contains(owner) {
            unreferenced += 1;
            verify_placeholder(owner, &stub)?;
            continue;
        }
        from_reference += 1;
        let reference_bytes = ct_sym.read(owner)?;
        let reference = ClassFile::parse(&reference_bytes).map_err(|error| {
            C1Error::new(format!(
                "ct.sym signature for {owner} does not parse: {error}"
            ))
        })?;
        let mut reference_metadata = reference.metadata().map_err(|error| {
            C1Error::new(format!(
                "ct.sym metadata for {owner} does not decode: {error}"
            ))
        })?;
        // Version is the only normalization: `ct.sym` carries the host JDK's class-file
        // version, while the committed stub is the pinned target version. Both dumps must
        // render the same version before the comparison.
        reference_metadata.major_version = major;
        reference_metadata.minor_version = 0;
        let reference_dump = rava_classfile::metadata::dump(&reference_metadata);
        if let Some(detail) = first_difference(&stub.dump, &reference_dump) {
            differences.push(Difference {
                owner: owner.to_owned(),
                detail,
            });
        }
    }

    Ok(MetadataReport {
        manifest: lock.manifest_path,
        stubs: lock.c1_path,
        ct_sym: ct_sym.as_ref().map(|ct_sym| ct_sym.path().to_path_buf()),
        release: format!("{} ({release})", lock.java),
        classes: surface.counts().classes,
        from_reference,
        unreferenced,
        fields,
        methods,
        differences,
    })
}

struct ParsedStub {
    metadata: rava_classfile::ClassMetadata,
    dump: String,
}

fn parse_stub(owner: &str, bytes: &[u8], major: u16) -> Result<ParsedStub, C1Error> {
    let class = ClassFile::parse(bytes).map_err(|error| {
        C1Error::new(format!(
            "committed C1 stub for {owner} does not parse: {error}"
        ))
    })?;
    let declared = class.this_class_name().map_err(|error| {
        C1Error::new(format!(
            "committed C1 stub for {owner} has no class name: {error}"
        ))
    })?;
    if declared != owner {
        return Err(C1Error::new(format!(
            "committed C1 stub for {owner} declares class {declared}"
        )));
    }
    if class.major_version != major || class.minor_version != 0 {
        return Err(C1Error::new(format!(
            "committed C1 stub for {owner} is version {}.{}, expected {major}.0; run `rava compat c1-stubs`",
            class.major_version, class.minor_version
        )));
    }
    let metadata = class.metadata().map_err(|error| {
        C1Error::new(format!(
            "committed C1 stub for {owner} metadata does not decode: {error}"
        ))
    })?;
    let dump = rava_classfile::metadata::dump(&metadata);
    Ok(ParsedStub { metadata, dump })
}

/// An owner Java 17 does not define must stay a header-only placeholder: if the committed
/// stub has members or a different marker, the exclusion would hide real metadata.
fn verify_placeholder(owner: &str, stub: &ParsedStub) -> Result<(), C1Error> {
    if stub.metadata.fields.is_empty()
        && stub.metadata.methods.is_empty()
        && stub.metadata.source_file.as_deref() == Some(PLACEHOLDER_SOURCE)
    {
        return Ok(());
    }
    Err(C1Error::new(format!(
        "committed C1 stub for {owner} is not the header-only placeholder expected for an owner \
         Java 17 does not define; run `rava compat c1-stubs`"
    )))
}

/// Every `*.class` file below `directory`, as a class name relative to it.
fn class_files(directory: &Path) -> Result<BTreeSet<String>, C1Error> {
    let mut names = BTreeSet::new();
    collect_class_files(directory, directory, &mut names)?;
    Ok(names)
}

fn collect_class_files(
    root: &Path,
    directory: &Path,
    names: &mut BTreeSet<String>,
) -> Result<(), C1Error> {
    let entries = fs::read_dir(directory).map_err(|error| {
        C1Error::new(format!(
            "cannot read C1 stub directory {}: {error}",
            directory.display()
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            C1Error::new(format!("cannot read {}: {error}", directory.display()))
        })?;
        let path = entry.path();
        if entry
            .file_type()
            .map_err(|error| C1Error::new(format!("cannot inspect {}: {error}", path.display())))?
            .is_dir()
        {
            collect_class_files(root, &path, names)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| C1Error::new(format!("{} escaped the C1 stub tree", path.display())))?;
        let relative = relative.to_string_lossy();
        let Some(name) = relative.strip_suffix(".class") else {
            return Err(C1Error::new(format!(
                "{} is not a class file; the C1 stub tree holds class files only",
                path.display()
            )));
        };
        names.insert(name.to_owned());
    }
    Ok(())
}

fn first_difference(stub: &str, reference: &str) -> Option<String> {
    let stub_lines: Vec<&str> = stub.lines().collect();
    let reference_lines: Vec<&str> = reference.lines().collect();
    for (index, (stub_line, reference_line)) in stub_lines.iter().zip(&reference_lines).enumerate()
    {
        if stub_line != reference_line {
            return Some(format!(
                "line {}: C1 `{}` vs release-17 `{}`",
                index + 1,
                stub_line,
                reference_line
            ));
        }
    }
    if stub_lines.len() != reference_lines.len() {
        return Some(format!(
            "line count {} vs {}",
            stub_lines.len(),
            reference_lines.len()
        ));
    }
    None
}

fn print_c1_report(report: &C1Report) {
    println!("rava compat c1-stubs");
    row("manifest", &report.manifest);
    row("c1 stubs", &report.stubs);
    row("ct.sym", &report.ct_sym.display().to_string());
    row("release", &report.release);
    row("classes", &report.counts.classes.to_string());
    row("from ct.sym", &report.from_reference.to_string());
    row("unreferenced", &report.unreferenced.to_string());
    row("unreferenced list", &report.unreferenced_list);
    row("class files", &report.files.to_string());
    println!("c1 stubs: written");
}

fn print_metadata_report(report: &MetadataReport, check: bool) {
    println!("rava compat metadata");
    row("manifest", &report.manifest);
    row("c1 stubs", &report.stubs);
    if let Some(ct_sym) = &report.ct_sym {
        row("ct.sym", &ct_sym.display().to_string());
        row("release", &report.release);
    }
    row("classes", &report.classes.to_string());
    if report.ct_sym.is_some() {
        row("from ct.sym", &report.from_reference.to_string());
        row("unreferenced", &report.unreferenced.to_string());
    }
    row("fields", &report.fields.to_string());
    row("methods", &report.methods.to_string());
    if !check {
        println!("metadata: scanned");
        return;
    }
    row("differences", &report.differences.len().to_string());
    if report.differences.is_empty() {
        println!("metadata: ok");
        return;
    }

    let shown = report.differences.len().min(SAMPLE_LIMIT);
    println!();
    println!(
        "metadata differences (showing {shown} of {}):",
        report.differences.len()
    );
    for difference in report.differences.iter().take(SAMPLE_LIMIT) {
        println!("  {} {}", difference.owner, difference.detail);
    }
    println!("metadata: failed");
}

fn row(label: &str, value: &str) {
    println!("  {label:<18}{value}");
}

fn print_usage(allow_check: bool) {
    if allow_check {
        println!("usage: rava compat metadata [--check] [--root <dir>] [--ct-sym <path>]");
        println!();
        println!("Scans the committed C1 stub tree and prints its counts. With --check it");
        println!("also reads the release-17 signatures from ct.sym and compares the M2.1");
        println!("normalized dump for every manifest class that has a signature, exiting 1 on");
        println!("a difference. Owners Java 17 does not define are excluded, not hidden.");
        println!("The reference is found through --ct-sym, RAVA_CT_SYM or JAVA_HOME.");
    } else {
        println!("usage: rava compat c1-stubs [--root <dir>] [--ct-sym <path>]");
        println!();
        println!("Projects every manifest owner's release-17 ct.sym signature into");
        println!("stdlib/c1/classes as a version-61 class file. Manifest owners Java 17 does");
        println!("not define must be listed in [c1].unreferenced or the command fails naming");
        println!("them. The reference is found through --ct-sym, RAVA_CT_SYM or JAVA_HOME.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_difference_is_not_hidden_by_the_diff() {
        let left = "class demo/A\nversion 61.0\nfields 0\n";
        let right = "class demo/A\nversion 69.0\nfields 0\n";
        let difference = first_difference(left, right).expect("version differs");
        assert!(difference.starts_with("line 2:"), "{difference}");
    }

    #[test]
    fn identical_dumps_have_no_difference() {
        let dump = "class demo/A\nversion 61.0\nfields 0\n";
        assert_eq!(first_difference(dump, dump), None);
    }

    #[test]
    fn a_missing_trailing_line_is_a_difference() {
        let difference = first_difference("a\nb\n", "a\n").expect("line count differs");
        assert!(difference.contains("line count 2 vs 1"), "{difference}");
    }

    #[test]
    fn release_letter_matches_the_ct_sym_layout() {
        assert_eq!(release_letter(17).unwrap(), 'H');
    }
}
