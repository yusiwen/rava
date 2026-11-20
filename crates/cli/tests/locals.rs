//! S15 regression: locals are addressed by JVM slot, not by value.
//!
//! A `long`/`double` occupies two local slots (JVMS §2.6.1), so a parameter after one starts two
//! slots later. While `invoke` bound arguments by value position, `aload_2` in a `(J, String)`
//! method read an untouched NULL slot — the bare `NullPointerException` from
//! `StringConcatHelper.mix` that blocked `String.concat`. The fixture pins one method per shape;
//! each reads only the parameter *after* the wide one.

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
    let dir = std::env::temp_dir().join(format!("rava-locals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn build_fixture(dir: &Path) {
    let mut builder = ClassBuilder::new("Locals", "java/lang/Object", 65);
    for (name, descriptor) in [("SUM", "J"), ("PICK", "I"), ("PICKD", "I"), ("FIRST", "J")] {
        builder.field(0x0009, name, descriptor);
    }
    let sum_field = builder.field_ref("Locals", "SUM", "J");
    let pick_field = builder.field_ref("Locals", "PICK", "I");
    let pickd_field = builder.field_ref("Locals", "PICKD", "I");
    let first_field = builder.field_ref("Locals", "FIRST", "J");

    let length = builder.method_ref("java/lang/String", "length", "()I");
    let abcd = builder.string("abcd");
    let abc = builder.string("abc");

    // static long sum(long a, long b) { return a + b; }
    builder.method(
        0x0009,
        "sum",
        "(JJ)J",
        Some((4, 4, vec![0x1e, 0x20, 0x61, 0xad])),
    );
    // static int pick(long a, String s) { return s.length(); }
    builder.method(
        0x0009,
        "pick",
        "(JLjava/lang/String;)I",
        Some((
            1,
            3,
            vec![
                0x2c, // aload_2
                0xb6,
                (length >> 8) as u8,
                length as u8, // invokevirtual String.length
                0xac,         // ireturn
            ],
        )),
    );
    // static int pickDouble(double d, String s) { return s.length(); }
    builder.method(
        0x0009,
        "pickDouble",
        "(DLjava/lang/String;)I",
        Some((
            1,
            4,
            vec![
                0x2c, // aload_2
                0xb6,
                (length >> 8) as u8,
                length as u8,
                0xac,
            ],
        )),
    );
    // static long first(long a, int b) { return a + b; }
    builder.method(
        0x0009,
        "first",
        "(JI)J",
        Some((4, 3, vec![0x1e, 0x1c, 0x85, 0x61, 0xad])), // lload_0, iload_2, i2l, ladd, lreturn
    );

    let mut code = Vec::new();
    let ldc2 = |index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[0x14, (index >> 8) as u8, index as u8]);
    };
    let ldc = |index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[0x13, (index >> 8) as u8, index as u8]);
    };
    let call = |opcode: u8, index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
    };
    let sum = builder.method_ref("Locals", "sum", "(JJ)J");
    let pick = builder.method_ref("Locals", "pick", "(JLjava/lang/String;)I");
    let pickd = builder.method_ref("Locals", "pickDouble", "(DLjava/lang/String;)I");
    let first = builder.method_ref("Locals", "first", "(JI)J");
    let seven = builder.long(7);
    let thirty_five = builder.long(35);
    let one_long = builder.long(1);
    let forty = builder.long(40);
    let one_point_five = builder.double(1.5);

    // SUM = sum(7L, 35L)
    ldc2(seven, &mut code);
    ldc2(thirty_five, &mut code);
    call(0xb8, sum, &mut code);
    call(0xb3, sum_field, &mut code);
    // PICK = pick(1L, "abcd")
    ldc2(one_long, &mut code);
    ldc(abcd, &mut code);
    call(0xb8, pick, &mut code);
    call(0xb3, pick_field, &mut code);
    // PICKD = pickDouble(1.5, "abc")
    ldc2(one_point_five, &mut code);
    ldc(abc, &mut code);
    call(0xb8, pickd, &mut code);
    call(0xb3, pickd_field, &mut code);
    // FIRST = first(40L, 2)
    ldc2(forty, &mut code);
    code.extend_from_slice(&[0x10, 2]); // bipush 2
    call(0xb8, first, &mut code);
    call(0xb3, first_field, &mut code);
    code.push(0xb1); // return

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((4, 1, code)));
    std::fs::write(dir.join("Locals.class"), builder.build()).expect("fixture class");
}

fn run(dir: &Path) -> Shared {
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "Locals", &[])
        .unwrap_or_else(|error| panic!("Locals failed: {error}"));
    shared
}

#[test]
fn a_wide_parameter_does_not_hide_the_next_one() {
    let dir = fixture_dir();
    build_fixture(&dir);
    let shared = run(&dir);
    let id = shared.class_ids["Locals"];
    let statics = &shared.classes[id.index()].statics;
    assert_eq!(statics.get("SUM"), Some(&Value::Long(42)), "sum(7L, 35L)");
    assert_eq!(
        statics.get("PICK"),
        Some(&Value::Int(4)),
        "pick(1L, \"abcd\")"
    );
    assert_eq!(
        statics.get("PICKD"),
        Some(&Value::Int(3)),
        "pickDouble(1.5, \"abc\")"
    );
    assert_eq!(
        statics.get("FIRST"),
        Some(&Value::Long(42)),
        "first(40L, 2)"
    );
}
