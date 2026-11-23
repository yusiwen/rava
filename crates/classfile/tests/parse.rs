//! Parser acceptance tests for M0.2.
//!
//! The fixture is a committed class file, so the assertions need no JDK at test time.

use std::path::{Path, PathBuf};

use rava_classfile::{ClassFile, Constant, Error};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture() -> PathBuf {
    manifest_dir().join("tests/fixtures/Hello.class")
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[test]
fn parses_the_fixture() {
    let class = ClassFile::parse(&read(&fixture())).expect("fixture parses");
    assert_eq!(class.major_version, 52);
    assert_eq!(class.minor_version, 0);
    assert_eq!(class.this_class_name().unwrap(), "Hello");
    assert_eq!(class.super_class_name().unwrap(), Some("java/lang/Object"));
}

#[test]
fn fixture_has_the_expected_structure() {
    let class = ClassFile::parse(&read(&fixture())).unwrap();

    let main = class
        .find_method("main", "([Ljava/lang/String;)V")
        .unwrap()
        .expect("Hello.main(String[]) exists");
    assert!(main.is_static());
    assert!(main.has_code());

    let code = class
        .code(main)
        .unwrap()
        .expect("main has a Code attribute");
    assert!(!code.data.is_empty(), "the Code payload must not be empty");

    let utf8: Vec<&str> = class
        .constant_pool
        .iter()
        .filter_map(|(_, entry)| match entry {
            Constant::Utf8(value) => Some(value.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        utf8.contains(&"Hello, rava"),
        "string constant survives decoding"
    );
    assert!(
        utf8.contains(&"java/lang/System"),
        "external class reference is recorded"
    );
}

#[test]
fn accepts_a_newer_class_file_version() {
    // A newer version must parse. Changing the major version is enough to prove the
    // parser has no version gate.
    let mut bytes = read(&fixture());
    bytes[6..8].copy_from_slice(&65u16.to_be_bytes());
    let class = ClassFile::parse(&bytes).expect("version 65 parses");
    assert_eq!(class.major_version, 65);
}

#[test]
fn rejects_a_bad_magic_number() {
    let mut bytes = read(&fixture());
    bytes[0] = 0x00;
    assert_eq!(
        ClassFile::parse(&bytes).unwrap_err(),
        Error::BadMagic { found: 0x00FE_BABE }
    );
}

#[test]
fn rejects_truncated_input() {
    let bytes = read(&fixture());
    let truncated = &bytes[..bytes.len() / 2];
    assert!(
        matches!(
            ClassFile::parse(truncated),
            Err(Error::UnexpectedEof { .. })
        ),
        "a truncated file must be an error, not a panic"
    );
}

#[test]
fn preserves_raw_attributes_for_later_metadata_layers() {
    let class = ClassFile::parse(&read(&fixture())).unwrap();
    let names: Vec<&str> = class
        .attributes
        .iter()
        .map(|attribute| class.attribute_name(attribute).unwrap())
        .collect();

    // The parser keeps top-level attributes as raw bytes; reflection and ASM-facing
    // views are layered on top later, not re-parsed from a lossy model.
    assert!(names.contains(&"SourceFile"));
    assert_eq!(class.source_file().unwrap().as_deref(), Some("Hello.java"));
}
