//! S10 acceptance: `java.lang.String` is a real instance of the host class.
//!
//! A string used to be a Rust string in the heap, so the library's own `String` methods failed
//! at their first `getfield`. The representation is now the host class's: a `byte[] value` and
//! a `coder` byte. This slice covers what that makes runnable today — `length`, `charAt`, the
//! literal interning, and the VM's own text extraction — and the note records the wall the
//! remaining methods hit (`checkcast` on an array type, then `Unsafe` and `arraycopy`).

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
    let dir = std::env::temp_dir().join(format!("rava-strings-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

/// Builds `StringOps.main` and writes the class into `dir`.
fn build_fixture(dir: &Path) {
    let mut builder = ClassBuilder::new("StringOps", "java/lang/Object", 65);
    builder.field(0x0009, "S", "Ljava/lang/String;");
    builder.field(0x0009, "S2", "Ljava/lang/String;");
    builder.field(0x0009, "LEN", "I");
    builder.field(0x0009, "CHAR", "I");
    builder.field(0x0009, "SAME", "I");
    builder.field(0x0009, "HASH", "I");
    builder.field(0x0009, "EQ", "I");
    builder.field(0x0009, "CMP", "I");
    builder.field(0x0009, "CONCAT", "Ljava/lang/String;");
    builder.field(0x0009, "SUB", "Ljava/lang/String;");

    let s_field = builder.field_ref("StringOps", "S", "Ljava/lang/String;");
    let s2_field = builder.field_ref("StringOps", "S2", "Ljava/lang/String;");
    let len_field = builder.field_ref("StringOps", "LEN", "I");
    let char_field = builder.field_ref("StringOps", "CHAR", "I");
    let same_field = builder.field_ref("StringOps", "SAME", "I");
    let hash_field = builder.field_ref("StringOps", "HASH", "I");
    let eq_field = builder.field_ref("StringOps", "EQ", "I");
    let cmp_field = builder.field_ref("StringOps", "CMP", "I");
    let concat_field = builder.field_ref("StringOps", "CONCAT", "Ljava/lang/String;");
    let sub_field = builder.field_ref("StringOps", "SUB", "Ljava/lang/String;");

    let hello = builder.string("hello");
    let length = builder.method_ref("java/lang/String", "length", "()I");
    let char_at = builder.method_ref("java/lang/String", "charAt", "(I)C");
    let equals = builder.method_ref("java/lang/String", "equals", "(Ljava/lang/Object;)Z");
    let compare = builder.method_ref("java/lang/String", "compareTo", "(Ljava/lang/String;)I");
    let hash_code = builder.method_ref("java/lang/String", "hashCode", "()I");
    let concat = builder.method_ref(
        "java/lang/String",
        "concat",
        "(Ljava/lang/String;)Ljava/lang/String;",
    );
    let substring = builder.method_ref("java/lang/String", "substring", "(II)Ljava/lang/String;");
    let world = builder.string(" world");
    let plain_world = builder.string("world");

    let mut code = Vec::new();
    let ldc = |index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[0x13, (index >> 8) as u8, index as u8]);
    };
    let call = |opcode: u8, index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
    };

    // S = "hello"; S2 = "hello" — the same literal must intern to one object.
    ldc(hello, &mut code);
    call(0xb3, s_field, &mut code);
    ldc(hello, &mut code);
    call(0xb3, s2_field, &mut code);
    // SAME = (S == S2) ? 1 : 0
    call(0xb2, s_field, &mut code);
    call(0xb2, s2_field, &mut code);
    code.extend_from_slice(&[0xa6, 0x00, 0x07]); // if_acmpne -> L (pc 25)
    code.push(0x04); // iconst_1
    code.extend_from_slice(&[0xa7, 0x00, 0x04]); // goto -> store (offset 4)
    code.push(0x03); // L: iconst_0
    call(0xb3, same_field, &mut code); // store:
    // HASH = S.hashCode(); EQ = S.equals("hello"); CMP = S.compareTo("world")
    call(0xb2, s_field, &mut code);
    call(0xb6, hash_code, &mut code);
    call(0xb3, hash_field, &mut code);
    call(0xb2, s_field, &mut code);
    ldc(hello, &mut code);
    call(0xb6, equals, &mut code);
    call(0xb3, eq_field, &mut code);
    call(0xb2, s_field, &mut code);
    ldc(plain_world, &mut code);
    call(0xb6, compare, &mut code);
    call(0xb3, cmp_field, &mut code);
    // CONCAT = S.concat(" world"); SUB = S.substring(1, 4)
    call(0xb2, s_field, &mut code);
    ldc(world, &mut code);
    call(0xb6, concat, &mut code);
    call(0xb3, concat_field, &mut code);
    call(0xb2, s_field, &mut code);
    code.push(0x04); // iconst_1
    code.push(0x07); // iconst_4
    call(0xb6, substring, &mut code);
    call(0xb3, sub_field, &mut code);
    // LEN = S.length()
    call(0xb2, s_field, &mut code);
    call(0xb6, length, &mut code);
    call(0xb3, len_field, &mut code);
    // CHAR = S.charAt(1)
    call(0xb2, s_field, &mut code);
    code.push(0x04); // iconst_1
    call(0xb6, char_at, &mut code);
    call(0xb3, char_field, &mut code);
    code.push(0xb1); // return

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((3, 1, code)));
    std::fs::write(dir.join("StringOps.class"), builder.build()).expect("fixture class");
}

fn run(dir: &Path) -> Shared {
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "StringOps", &[])
        .unwrap_or_else(|error| panic!("StringOps failed: {error}"));
    shared
}

fn int(shared: &Shared, name: &str) -> i32 {
    let id = shared.class_ids["StringOps"];
    match shared.classes[id.index()].statics.get(name) {
        Some(Value::Int(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

fn string(shared: &Shared, name: &str) -> (rava_vm::ObjRef, String) {
    let id = shared.class_ids["StringOps"];
    let value = shared.classes[id.index()]
        .statics
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not set"));
    let Value::Ref(reference) = value else {
        panic!("{name} is {value:?}");
    };
    (
        *reference,
        shared.string_text(*reference).expect("a String"),
    )
}

fn text(shared: &Shared, name: &str) -> String {
    string(shared, name).1
}

#[test]
fn the_string_methods_run_on_the_host_class() {
    let dir = fixture_dir("ops");
    build_fixture(&dir);
    let shared = run(&dir);
    assert_eq!(int(&shared, "LEN"), 5, "length");
    assert_eq!(int(&shared, "CHAR"), i32::from(b'e'), "charAt");
    assert_eq!(int(&shared, "HASH"), 99162322, "hashCode of \"hello\"");
    assert_eq!(int(&shared, "EQ"), 1, "equals");
    assert_eq!(int(&shared, "CMP"), -15, "compareTo(\"world\")");
    assert_eq!(text(&shared, "CONCAT"), "hello world", "concat");
    assert_eq!(text(&shared, "SUB"), "ell", "substring");
}

#[test]
fn a_string_literal_is_a_string_instance_and_is_interned() {
    let dir = fixture_dir("literal");
    build_fixture(&dir);
    let shared = run(&dir);

    assert_eq!(
        int(&shared, "SAME"),
        1,
        "ldc of the same literal is one object"
    );
    let (reference, text) = string(&shared, "S");
    assert_eq!(text, "hello");

    let class = shared.class_of(reference).expect("S has a class");
    assert_eq!(shared.classes[class.index()].name, "java/lang/String");
    // The bytes live in the host class's `value` field, with the Latin-1 coder.
    let value_slot = shared
        .class(class)
        .field_slot("value", "[B")
        .expect("String.value");
    let coder_slot = shared
        .class(class)
        .field_slot("coder", "B")
        .expect("String.coder");
    assert_eq!(
        shared.instance_field(reference, coder_slot).unwrap(),
        Value::Int(0)
    );
    let value = shared.instance_field(reference, value_slot).unwrap();
    let Value::Ref(bytes) = value else {
        panic!("value is {value:?}");
    };
    assert_eq!(shared.array(bytes).unwrap().elements.len(), 5);
}
