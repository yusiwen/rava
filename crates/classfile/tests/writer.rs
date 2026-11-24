//! M2.2: the faithful class-file writer and its parse/write round-trip.
//!
//! The fixtures are the committed Java 17 class files from the M2.1 metadata suite, so
//! the round-trip covers annotations, type annotations, generic signatures, records,
//! sealed permits, nestmates, inner and anonymous classes, parameter names and constant
//! values without needing a JDK at test time.

use std::fs;
use std::path::PathBuf;

use rava_classfile::{ClassBuilder, ClassFile, Constant, ConstantPool};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo")
}

fn fixture_names() -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(fixture_dir())
        .expect("read fixture directory")
        .map(|entry| {
            let path = entry.expect("fixture entry").path();
            path.file_stem()
                .expect("fixture stem")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "the fixture directory must not be empty");
    names
}

fn read(name: &str) -> Vec<u8> {
    let path = fixture_dir().join(format!("{name}.class"));
    fs::read(&path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[test]
fn a_parsed_class_file_writes_back_byte_identically() {
    for name in fixture_names() {
        let bytes = read(&name);
        let class = ClassFile::parse(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let written = class.write();
        assert_eq!(
            written,
            bytes,
            "{name} did not round-trip byte for byte ({} vs {} bytes)",
            written.len(),
            bytes.len()
        );
    }
}

#[test]
fn the_metadata_dump_survives_a_round_trip() {
    for name in fixture_names() {
        let bytes = read(&name);
        let parsed = ClassFile::parse(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
        let before = parsed.metadata_dump().unwrap();
        let rewritten = ClassFile::parse(&parsed.write()).unwrap();
        assert_eq!(rewritten.metadata_dump().unwrap(), before, "{name}");
    }
}

#[test]
fn rewriting_the_version_preserves_pool_indices_and_long_double_slots() {
    let mut builder = ClassBuilder::new("demo/Slots", "java/lang/Object", 61);
    // Long and double each consume two constant-pool slots; the writer must not shift
    // the indices that follow them.
    let long_index = builder.long(7);
    let double_index = builder.double(2.5);
    builder.field(0x0009, "VALUE", "J");
    builder.field(0x0009, "RATIO", "D");
    let bytes = builder.build();

    let mut class = ClassFile::parse(&bytes).expect("synthetic class parses");
    let pool_before: Vec<(u16, Constant)> = class
        .constant_pool
        .entries()
        .map(|(index, constant)| (index, constant.clone()))
        .collect();
    assert!(matches!(
        class.constant_pool.get(long_index).unwrap(),
        Constant::Long(7)
    ));
    assert!(matches!(
        class.constant_pool.get(double_index).unwrap(),
        Constant::Double(value) if *value == 2.5
    ));

    class.major_version = 69;
    let rewritten = ClassFile::parse(&class.write()).expect("rewritten class parses");
    assert_eq!(rewritten.major_version, 69);
    let pool_after: Vec<(u16, Constant)> = rewritten
        .constant_pool
        .entries()
        .map(|(index, constant)| (index, constant.clone()))
        .collect();
    assert_eq!(pool_after, pool_before);
    assert_eq!(rewritten.constant_pool.len(), class.constant_pool.len());
}

#[test]
fn an_unknown_attribute_payload_survives_verbatim() {
    // `ClassBuilder` emits no annotations, so this pins the writer's rule on a raw payload
    // that no decoder owns: the bytes are repeated exactly.
    let bytes = read("Kitchen");
    let mut class = ClassFile::parse(&bytes).expect("fixture parses");
    let expected_length = {
        let marker = class
            .attributes
            .iter_mut()
            .find(|attribute| attribute.data.len() % 2 == 0)
            .expect("a class attribute");
        marker.data.extend_from_slice(&[0xAA, 0x55]);
        marker.data.len()
    };
    let rewritten = ClassFile::parse(&class.write()).expect("rewritten class parses");
    let attribute = rewritten
        .attributes
        .iter()
        .find(|attribute| attribute.data.ends_with(&[0xAA, 0x55]))
        .expect("the extended payload is present");
    assert_eq!(attribute.data.len(), expected_length);
}

#[test]
fn pool_entries_are_written_at_their_original_indices() {
    let bytes = read("Kitchen");
    let class = ClassFile::parse(&bytes).expect("fixture parses");
    let rewritten = ClassFile::parse(&class.write()).expect("rewritten class parses");
    let pairs = |pool: &ConstantPool| -> Vec<(u16, Constant)> {
        pool.entries()
            .map(|(index, constant)| (index, constant.clone()))
            .collect()
    };
    assert_eq!(pairs(&rewritten.constant_pool), pairs(&class.constant_pool));
}
