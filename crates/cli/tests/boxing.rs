//! S23 acceptance: autoboxing works, and the bootstrap's objects survive a collection.
//!
//! `Integer.valueOf` initializes `Integer$IntegerCache`, whose `<clinit>` calls
//! `VM.getSavedProperty`; without the saved-properties map it threw `IllegalStateException`.
//! The fixture pins the path through a static and reads the value back.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, Shared, ThreadState, Value};

fn jdk_home() -> std::ffi::OsString {
    std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    })
}

fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-boxing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

#[test]
fn integer_value_of_returns_the_cached_value() {
    // BOX = Integer.valueOf(42); VALUE = BOX.intValue()
    let mut builder = ClassBuilder::new("Boxing", "java/lang/Object", 65);
    builder.field(0x0009, "BOX", "Ljava/lang/Integer;");
    builder.field(0x0009, "VALUE", "I");
    let box_field = builder.field_ref("Boxing", "BOX", "Ljava/lang/Integer;");
    let value_field = builder.field_ref("Boxing", "VALUE", "I");
    let value_of = builder.method_ref("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;");
    let int_value = builder.method_ref("java/lang/Integer", "intValue", "()I");
    let mut code = Vec::new();
    code.extend_from_slice(&[0x10, 42]); // bipush 42
    code.extend_from_slice(&[0xb8, (value_of >> 8) as u8, value_of as u8]);
    code.extend_from_slice(&[0xb3, (box_field >> 8) as u8, box_field as u8]);
    code.extend_from_slice(&[0xb2, (box_field >> 8) as u8, box_field as u8]);
    code.extend_from_slice(&[0xb6, (int_value >> 8) as u8, int_value as u8]);
    code.extend_from_slice(&[0xb3, (value_field >> 8) as u8, value_field as u8]);
    code.push(0xb1); // return
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((1, 1, code)));

    let dir = fixture_dir();
    std::fs::write(dir.join("Boxing.class"), builder.build()).expect("fixture class");
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), &dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "Boxing", &[])
        .unwrap_or_else(|error| panic!("Boxing failed: {error}"));
    let id = shared.class_ids["Boxing"];
    assert_eq!(
        shared.classes[id.index()].statics.get("VALUE"),
        Some(&Value::Int(42))
    );
}
