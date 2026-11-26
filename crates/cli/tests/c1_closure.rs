//! The C1 tree is a manifest subset, not a closed hierarchy.
//!
//! `docs/explanation/c1-metadata.md` records the closure tolerance: the 1,047 committed
//! stubs leave 69 platform owners absent — 35 distinct superclass names over 44 edges and
//! 34 distinct interface names over 70 edges. Those owners are outside the observed
//! manifest, so the metadata diff stays 0. This test measures the profile instead of
//! trusting the recorded numbers: a stub added with an absent supertype, or a tree rebuilt
//! against a different manifest, fails it and the document is corrected in the same PR.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use rava_classfile::ClassFile;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/cli is inside the workspace")
        .to_path_buf()
}

/// The manifest path from `compat/targets.toml` (`[manifest].path`).
fn manifest_path(root: &Path) -> PathBuf {
    let text = fs::read_to_string(root.join("compat/targets.toml"))
        .expect("compat/targets.toml is readable");
    let table: toml::Table = text.parse().expect("compat/targets.toml parses");
    let relative = table["manifest"]["path"]
        .as_str()
        .expect("compat/targets.toml has [manifest].path");
    root.join(relative)
}

fn collect_classes(directory: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("the C1 tree is readable") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect_classes(&path, out);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("class") {
            out.push(path);
        }
    }
}

#[test]
fn the_c1_closure_profile_matches_the_documented_tolerance() {
    let root = repo_root();
    let manifest = fs::read_to_string(manifest_path(&root)).expect("the manifest is readable");
    let manifest_classes: BTreeSet<&str> = manifest
        .lines()
        .filter_map(|line| line.strip_prefix("class "))
        .collect();

    let mut files = Vec::new();
    collect_classes(&root.join("stdlib/c1/classes"), &mut files);
    let mut classes = BTreeSet::new();
    let mut entries = Vec::with_capacity(files.len());
    for file in &files {
        let bytes = fs::read(file).expect("a C1 stub is readable");
        let metadata = ClassFile::parse(&bytes)
            .expect("a C1 stub parses")
            .metadata()
            .expect("a C1 stub's metadata decodes");
        classes.insert(metadata.binary_name.clone());
        entries.push((
            metadata.binary_name,
            metadata.super_class,
            metadata.interfaces,
        ));
    }

    // A missing owner maps to the classes/interfaces that name it, so the value set is the
    // edge count for that owner.
    let mut missing_super: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut missing_interfaces: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (name, super_class, interfaces) in &entries {
        if let Some(super_class) = super_class.as_deref() {
            if !classes.contains(super_class) {
                missing_super
                    .entry(super_class)
                    .or_default()
                    .insert(name.as_str());
            }
        }
        for interface in interfaces {
            if !classes.contains(interface) {
                missing_interfaces
                    .entry(interface.as_str())
                    .or_default()
                    .insert(name.as_str());
            }
        }
    }
    let super_edges: usize = missing_super.values().map(BTreeSet::len).sum();
    let interface_edges: usize = missing_interfaces.values().map(BTreeSet::len).sum();
    let missing: BTreeSet<&str> = missing_super
        .keys()
        .chain(missing_interfaces.keys())
        .copied()
        .collect();
    let overlap: BTreeSet<&str> = missing_super
        .keys()
        .filter(|name| missing_interfaces.contains_key(*name))
        .copied()
        .collect();

    println!(
        "C1 closure: classes={} manifest={} missing_super={}/{} missing_interface={}/{} \
         missing_owners={} overlap={}",
        classes.len(),
        manifest_classes.len(),
        missing_super.len(),
        super_edges,
        missing_interfaces.len(),
        interface_edges,
        missing.len(),
        overlap.len()
    );

    assert_eq!(
        classes.len(),
        1_047,
        "the C1 tree should carry one stub per manifest owner"
    );
    assert_eq!(
        classes.len(),
        manifest_classes.len(),
        "the tree and the manifest disagree on the owner count"
    );
    assert_eq!(
        (missing_super.len(), super_edges),
        (35, 44),
        "the documented superclass profile changed: update docs/explanation/c1-metadata.md"
    );
    assert_eq!(
        (missing_interfaces.len(), interface_edges),
        (34, 70),
        "the documented interface profile changed: update docs/explanation/c1-metadata.md"
    );
    assert_eq!(
        missing.len(),
        69,
        "the documented omitted-owner count changed: update docs/explanation/c1-metadata.md"
    );
    assert!(
        overlap.is_empty(),
        "the two profile halves now overlap: {overlap:?}"
    );
    for name in [
        "java/lang/AbstractStringBuilder",
        "java/lang/Readable",
        "java/nio/channels/AsynchronousChannel",
    ] {
        assert!(
            missing.contains(name),
            "{name} should be one of the omitted owners"
        );
    }
    for name in &missing {
        assert!(
            !manifest_classes.contains(name),
            "{name} is a manifest owner and cannot be an omitted owner"
        );
    }
}
