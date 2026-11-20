//! S16 acceptance: `StringBuilder` runs through the host classes.
//!
//! With strings as instances (S10) and `System.arraycopy` (S14), `AbstractStringBuilder`'s
//! Java code works as written: the constructor allocates its `byte[] value`, `append` writes
//! through `String.getBytes` and grows with `Arrays.copyOf`, and `toString` calls the host
//! `String` constructor. The VM's old `HeapObject::Builder` shortcut is not used by anything.

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

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-sb-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn build_fixture(dir: &Path) {
    let mut builder = ClassBuilder::new("SbOps", "java/lang/Object", 65);
    builder.field(0x0009, "TEXT", "Ljava/lang/String;");
    builder.field(0x0009, "LEN", "I");
    builder.field(0x0009, "LONG", "Ljava/lang/String;");
    builder.field(0x0009, "LONG_LEN", "I");

    let text_field = builder.field_ref("SbOps", "TEXT", "Ljava/lang/String;");
    let len_field = builder.field_ref("SbOps", "LEN", "I");
    let long_field = builder.field_ref("SbOps", "LONG", "Ljava/lang/String;");
    let long_len_field = builder.field_ref("SbOps", "LONG_LEN", "I");

    let sb = builder.class("java/lang/StringBuilder");
    let init = builder.method_ref("java/lang/StringBuilder", "<init>", "()V");
    let append_string = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
    let append_char = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(C)Ljava/lang/StringBuilder;",
    );
    let append_int = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(I)Ljava/lang/StringBuilder;",
    );
    let to_string = builder.method_ref(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
    let length = builder.method_ref("java/lang/String", "length", "()I");
    let hello = builder.string("hello");
    let digits = builder.string("0123456789");

    let mut code = Vec::new();
    let ldc = |index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[0x13, (index >> 8) as u8, index as u8]);
    };
    let call = |opcode: u8, index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
    };
    let new_builder = |code: &mut Vec<u8>| {
        code.extend_from_slice(&[0xbb, (sb >> 8) as u8, sb as u8, 0x59]);
        call(0xb7, init, code);
    };

    // TEXT = new StringBuilder().append("hello").append(' ').append(42).toString()
    new_builder(&mut code);
    ldc(hello, &mut code);
    call(0xb6, append_string, &mut code);
    code.extend_from_slice(&[0x10, 32]); // bipush ' '
    call(0xb6, append_char, &mut code);
    code.extend_from_slice(&[0x10, 42]); // bipush 42
    call(0xb6, append_int, &mut code);
    call(0xb6, to_string, &mut code);
    call(0xb3, text_field, &mut code);
    // LEN = TEXT.length()
    call(0xb2, text_field, &mut code);
    call(0xb6, length, &mut code);
    call(0xb3, len_field, &mut code);

    // LONG = a builder grown past its first capacity: 25 appends of ten digits.
    new_builder(&mut code);
    for _ in 0..25 {
        ldc(digits, &mut code);
        call(0xb6, append_string, &mut code);
    }
    call(0xb6, to_string, &mut code);
    call(0xb3, long_field, &mut code);
    call(0xb2, long_field, &mut code);
    call(0xb6, length, &mut code);
    call(0xb3, long_len_field, &mut code);
    code.push(0xb1); // return

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((3, 1, code)));
    std::fs::write(dir.join("SbOps.class"), builder.build()).expect("fixture class");
}

fn run(dir: &Path) -> Shared {
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "SbOps", &[])
        .unwrap_or_else(|error| panic!("SbOps failed: {error}"));
    shared
}

fn text(shared: &Shared, name: &str) -> String {
    let id = shared.class_ids["SbOps"];
    let value = shared.classes[id.index()]
        .statics
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not set"));
    let Value::Ref(reference) = value else {
        panic!("{name} is {value:?}");
    };
    shared.string_text(*reference).expect("a String")
}

fn int(shared: &Shared, name: &str) -> i32 {
    let id = shared.class_ids["SbOps"];
    match shared.classes[id.index()].statics.get(name) {
        Some(Value::Int(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn append_and_to_string_run_on_the_host_classes() {
    let dir = fixture_dir("append");
    build_fixture(&dir);
    let shared = run(&dir);
    assert_eq!(text(&shared, "TEXT"), "hello 42");
    assert_eq!(int(&shared, "LEN"), 8);
}

#[test]
fn a_builder_grows_past_its_first_capacity() {
    let dir = fixture_dir("grow");
    build_fixture(&dir);
    let shared = run(&dir);
    assert_eq!(int(&shared, "LONG_LEN"), 250);
    assert_eq!(
        text(&shared, "LONG"),
        "0123456789".repeat(25),
        "every appended byte survives the growth"
    );
}
