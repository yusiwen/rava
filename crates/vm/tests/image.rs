//! S4 acceptance: read the host JDK's module image.
//!
//! The image is `$JAVA21_HOME/lib/modules`, the JDK the devShell pins. A machine without
//! that variable is not a machine this acceptance covers, so the tests say so instead of
//! skipping: a skipped gate is indistinguishable from a passing one in CI.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use rava_classfile::ClassFile;
use rava_vm::image::{Entry, Image, ImageError, MAGIC};
use rava_vm::{ClassPath, Shared};

/// The JDK home the devShell exports (`flake.nix`) and CI inherits.
fn jdk_home() -> PathBuf {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    PathBuf::from(home)
}

fn image_path() -> PathBuf {
    jdk_home().join("lib/modules")
}

/// Opening the image reads 140 MB, so one open serves every test in this binary.
fn image() -> &'static Arc<Image> {
    static IMAGE: OnceLock<Arc<Image>> = OnceLock::new();
    IMAGE.get_or_init(|| {
        let path = image_path();
        Arc::new(Image::open(&path).unwrap_or_else(|error| {
            panic!("cannot read the host image {}: {error}", path.display())
        }))
    })
}

#[test]
fn header_describes_the_index() {
    let header = image().header();
    assert_eq!(header.magic, MAGIC);
    assert_eq!(header.major_version, 1);
    assert_eq!(header.minor_version, 0);
    assert!(header.resource_count > 0, "the index is empty");
    assert!(header.table_length >= header.resource_count);
    // The index must fit in the file, and the content must start after it.
    let len = std::fs::metadata(image_path())
        .expect("image metadata")
        .len() as usize;
    assert!(header.index_size() < len, "index does not fit in the image");
}

#[test]
fn every_index_slot_names_a_distinct_resource() {
    let mut names: Vec<String> = Vec::new();
    for entry in image().entries() {
        names.push(entry.expect("entry decodes").name);
    }
    assert_eq!(
        names.len(),
        image().header().resource_count as usize,
        "the table length and the resource count disagree with the offsets table"
    );
    names.sort_unstable();
    names.dedup();
    assert_eq!(
        names.len(),
        image().header().resource_count as usize,
        "two slots name the same resource"
    );
}

#[test]
fn enumerates_java_base() {
    let entries = image()
        .module_entries("java.base")
        .expect("java.base reads");
    let object = entries
        .iter()
        .find(|entry| entry.name == "/java.base/java/lang/Object.class")
        .unwrap_or_else(|| panic!("java/lang/Object.class is not in java.base"));
    assert_eq!(object.module, "java.base");
    // The whole boot library, not a handful of stubs: the devShell's image holds 7,572
    // resources in java.base, of which 7,559 are classes (patch releases move the first
    // number, so it is a bound rather than an equality).
    assert!(
        entries.len() > 7_000,
        "java.base holds only {} resources",
        entries.len()
    );
    // Sorted by name, so the test doubles as a check that ordering holds.
    assert!(entries.windows(2).all(|pair| pair[0].name <= pair[1].name));
}

#[test]
fn lookup_agrees_with_the_scan() {
    let scanned: Option<Entry> = image()
        .entries()
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.name == "/java.base/java/lang/String.class");
    let scanned = scanned.expect("String is in java.base");
    let bytes = image()
        .class("java/lang/String")
        .expect("lookup reads")
        .expect("String is in java.base");
    assert!(bytes.len() > 1_000, "String.class is {} bytes", bytes.len());
    assert_eq!(scanned.module, "java.base");
}

#[test]
fn missing_resources_are_none_not_an_error() {
    assert!(
        image()
            .class("java/lang/ThisClassDoesNotExist")
            .expect("lookup reads")
            .is_none()
    );
    assert!(
        image()
            .resource("/java.base/java/lang/ThisClassDoesNotExist.class")
            .expect("lookup reads")
            .is_none()
    );
}

#[test]
fn loads_object_class_from_the_image() {
    let bytes = image()
        .class("java/lang/Object")
        .expect("lookup reads")
        .expect("Object is in the image");
    let file = ClassFile::parse(bytes).expect("the parser reads the host class");
    assert_eq!(file.major_version, 65, "the host image is class-file 65");
    assert_eq!(file.this_class_name().unwrap(), "java/lang/Object");
    assert_eq!(file.super_class_name().unwrap(), None);
    let hash_code = file
        .methods
        .iter()
        .find(|method| file.method_name(method).unwrap() == "hashCode")
        .expect("Object.hashCode");
    assert!(hash_code.is_native(), "Object.hashCode is native");
}

#[test]
fn class_path_loads_from_the_image() {
    let mut shared = Shared::new();
    let path = ClassPath::new("/nonexistent/bootstrap", "/nonexistent/app")
        .with_image(Arc::clone(image()));
    let id = shared
        .load_class(&path, "java/lang/Object")
        .expect("Object loads from lib/modules");
    let class = &shared.classes[id.index()];
    assert_eq!(class.name, "java/lang/Object");
    assert!(
        class
            .methods
            .iter()
            .any(|method| method.name == "hashCode" && method.is_native),
        "the loaded class carries the native flag"
    );
}

#[test]
fn rejects_bytes_that_are_not_an_image() {
    let class_file = image()
        .class("java/lang/Object")
        .expect("lookup reads")
        .expect("Object is in the image")
        .to_vec();
    match Image::from_bytes(class_file) {
        Err(ImageError::NotAnImage { magic }) => {
            assert_eq!(magic, u32::from_ne_bytes([0xCA, 0xFE, 0xBA, 0xBE]))
        }
        other => panic!("a class file passed as an image: {other:?}"),
    }
}

#[test]
fn rejects_a_truncated_index() {
    // A header that promises two table slots, two locations and two strings, with no tables
    // behind it. This is the shape of a copy interrupted by a full disk.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC.to_ne_bytes());
    bytes.extend_from_slice(&0x0001_0000u32.to_ne_bytes());
    bytes.extend_from_slice(&0u32.to_ne_bytes());
    bytes.extend_from_slice(&2u32.to_ne_bytes());
    bytes.extend_from_slice(&2u32.to_ne_bytes());
    bytes.extend_from_slice(&8u32.to_ne_bytes());
    bytes.extend_from_slice(&8u32.to_ne_bytes());
    match Image::from_bytes(bytes) {
        Err(ImageError::Truncated { .. }) => {}
        other => panic!("a truncated index was accepted: {other:?}"),
    }
}
