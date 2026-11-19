//! The declared-versus-registered native report (issue #2).
//!
//! A program stops at the first native the VM cannot satisfy, one per run. This walks the
//! constant-pool class references from a program class instead, so the answer to "what will
//! `rava run` need?" exists before running anything.
//!
//! The walk is an over-approximation on purpose: every `CONSTANT_Class` entry is followed, not
//! just the ones a static analysis would prove reachable, because a missed native costs a run
//! and an extra one costs nothing. The cap exists so pointing the tool at a whole image is a
//! reported truncation rather than a hang.

use std::collections::{BTreeSet, HashMap, VecDeque};

use rava_classfile::{ClassFile, Constant};
use rava_vm::{ClassPath, NativeFn, VmError};

/// How many classes the walk visits before it stops and reports the truncation.
pub const DEFAULT_MAX_CLASSES: usize = 4096;

/// One native method the walk found, named the way the registry keys it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeMethod {
    pub owner: String,
    pub name: String,
    pub descriptor: String,
}

impl NativeMethod {
    pub fn key(&self) -> String {
        format!("{}.{}{}", self.owner, self.name, self.descriptor)
    }
}

/// What the walk found.
#[derive(Debug)]
pub struct Report {
    /// Classes whose constant pool was read.
    pub visited: usize,
    /// The walk hit [`DEFAULT_MAX_CLASSES`]-style cap and stopped early.
    pub truncated: bool,
    /// References the class path could not resolve; the report is incomplete around them.
    pub unresolved: usize,
    /// Every native declared by the visited classes, in `(owner, name, descriptor)` order.
    pub declared: Vec<NativeMethod>,
    /// The declared natives the registry does not cover.
    pub missing: Vec<NativeMethod>,
    /// Registered keys whose owner class exists and does not declare the native.
    pub dangling: Vec<String>,
    /// Registered keys whose owner class is not on the class path, so the check cannot judge.
    pub unverified: Vec<String>,
}

impl Report {
    /// True when every native the walk found has an implementation and no registration is a lie.
    pub fn ok(&self) -> bool {
        self.missing.is_empty() && self.dangling.is_empty()
    }
}

/// Walks from `program` and reconciles the declarations against `registered`.
pub fn collect(
    path: &ClassPath,
    program: &str,
    registered: &HashMap<String, NativeFn>,
    max_classes: usize,
) -> Result<Report, VmError> {
    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::from([program.to_owned()]);
    let mut declared: BTreeSet<NativeMethod> = BTreeSet::new();
    let mut unresolved = 0;
    let mut truncated = false;

    while let Some(name) = queue.pop_front() {
        if !visited.insert(name.clone()) {
            continue;
        }
        if visited.len() > max_classes {
            truncated = true;
            break;
        }
        let Some(bytes) = path.class_bytes(&name)? else {
            unresolved += 1;
            continue;
        };
        let file = ClassFile::parse(&bytes)?;
        for method in &file.methods {
            if method.is_native() {
                declared.insert(NativeMethod {
                    owner: name.clone(),
                    name: file.method_name(method)?.to_owned(),
                    descriptor: file.method_descriptor(method)?.to_owned(),
                });
            }
        }
        for reference in class_references(&file)? {
            if !visited.contains(&reference) {
                queue.push_back(reference);
            }
        }
    }

    let declared: Vec<NativeMethod> = declared.into_iter().collect();
    let missing: Vec<NativeMethod> = declared
        .iter()
        .filter(|method| !registered.contains_key(&method.key()))
        .cloned()
        .collect();

    let mut dangling = Vec::new();
    let mut unverified = Vec::new();
    for key in registered.keys() {
        match declares(path, key)? {
            Some(false) => dangling.push(key.clone()),
            None => unverified.push(key.clone()),
            Some(true) => {}
        }
    }
    dangling.sort();
    unverified.sort();

    Ok(Report {
        visited: visited.len(),
        truncated,
        unresolved,
        declared,
        missing,
        dangling,
        unverified,
    })
}

/// Every class name the constant pool references, including superclasses and interfaces.
fn class_references(file: &ClassFile) -> Result<BTreeSet<String>, VmError> {
    let pool = &file.constant_pool;
    let mut names = BTreeSet::new();
    for (index, constant) in pool.entries() {
        if matches!(constant, Constant::Class { .. }) {
            let name = pool.class_name(index)?;
            // Array descriptors are not classes to load; their element type is a separate entry.
            if !name.starts_with('[') {
                names.insert(name.to_owned());
            }
        }
    }
    Ok(names)
}

/// Whether the owner class declares `key` as a native.
///
/// `Ok(None)` means the owner class is not on the class path: the registration cannot be
/// judged, which is different from a registration that names something the class does not
/// declare. The registry key is `owner.name(descriptor)`, and binary names cannot contain `.`,
/// so the first dot ends the owner.
fn declares(path: &ClassPath, key: &str) -> Result<Option<bool>, VmError> {
    let Some((owner, rest)) = key.split_once('.') else {
        return Ok(Some(false));
    };
    let Some((name, tail)) = rest.split_once('(') else {
        return Ok(Some(false));
    };
    let descriptor = format!("({tail}");
    let Some(bytes) = path.class_bytes(owner)? else {
        return Ok(None);
    };
    let file = ClassFile::parse(&bytes)?;
    Ok(Some(file.methods.iter().any(|method| {
        method.is_native()
            && file.method_name(method).is_ok_and(|found| found == name)
            && file
                .method_descriptor(method)
                .is_ok_and(|found| found == descriptor)
    })))
}
