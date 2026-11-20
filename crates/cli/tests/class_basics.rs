//! S13 acceptance: the `Class` query natives answer from the VM's class model.
//!
//! `isInstance` and `isAssignableFrom` are subtype tests, `getSuperclass` reaches the loader's
//! resolved link, and `getModifiers` reports the class file's access flags with the reflection
//! mask. The fixture calls all four through ordinary bytecode, so the test exercises the native
//! registration, the argument convention (the receiver is `args[0]`) and the class-object cache,
//! not just the functions in isolation.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, Shared, ThreadState, Value, VmError};

fn fixture_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rava-classbasics-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn class_path(dir: &Path) -> ClassPath {
    let home = std::env::var_os("JAVA21_HOME").expect("JAVA21_HOME");
    rava_cli::class_path(Path::new(&home), dir).expect("class path")
}

/// `ldc_w`: a constant-pool index is a `u16`, and a class object is a reference constant.
fn ldc_w(index: u16, code: &mut Vec<u8>) {
    code.extend_from_slice(&[0x13, (index >> 8) as u8, index as u8]);
}

/// `invokevirtual`/`invokespecial`/`invokestatic` share the two-byte operand shape.
fn invoke(opcode: u8, index: u16, code: &mut Vec<u8>) {
    code.extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
}

fn putstatic(index: u16, code: &mut Vec<u8>) {
    code.extend_from_slice(&[0xb3, (index >> 8) as u8, index as u8]);
}

/// The fixture's `main` stores every answer into a static field, so Rust can assert each one
/// without the program printing anything.
fn build_queries(dir: &Path) {
    let mut builder = ClassBuilder::new("ClassQueries", "java/lang/Object", 65);
    builder.field(0x0009, "OBJECT_INSTANCE", "I");
    builder.field(0x0009, "NULL_INSTANCE", "I");
    builder.field(0x0009, "THREAD_RUNNABLE", "I");
    builder.field(0x0009, "OBJECT_STRING", "I");
    builder.field(0x0009, "STRING_OBJECT", "I");
    builder.field(0x0009, "SUPERCLASS", "Ljava/lang/Class;");
    builder.field(0x0009, "RUNNABLE_MODIFIERS", "I");
    builder.field(0x0009, "STRING_MODIFIERS", "I");
    // The descriptor-only cases: a primitive class object has no class behind it.
    builder.field(0x0009, "INT_INSTANCE", "I");
    builder.field(0x0009, "INT_SELF", "I");
    builder.field(0x0009, "OBJECT_INT", "I");
    builder.field(0x0009, "INT_MODIFIERS", "I");
    builder.field(0x0009, "INT_SUPER", "Ljava/lang/Class;");

    let object_class = builder.class("java/lang/Object");
    let string_class = builder.class("java/lang/String");
    let runnable_class = builder.class("java/lang/Runnable");
    let thread_class = builder.class("java/lang/Thread");
    let int_class = builder.class("I");
    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let is_instance = builder.method_ref("java/lang/Class", "isInstance", "(Ljava/lang/Object;)Z");
    let is_assignable_from = builder.method_ref(
        "java/lang/Class",
        "isAssignableFrom",
        "(Ljava/lang/Class;)Z",
    );
    let get_superclass =
        builder.method_ref("java/lang/Class", "getSuperclass", "()Ljava/lang/Class;");
    let get_modifiers = builder.method_ref("java/lang/Class", "getModifiers", "()I");

    let field = |builder: &mut ClassBuilder, name: &str, descriptor: &str| {
        builder.field_ref("ClassQueries", name, descriptor)
    };
    let object_instance = field(&mut builder, "OBJECT_INSTANCE", "I");
    let null_instance = field(&mut builder, "NULL_INSTANCE", "I");
    let thread_runnable = field(&mut builder, "THREAD_RUNNABLE", "I");
    let object_string = field(&mut builder, "OBJECT_STRING", "I");
    let string_object = field(&mut builder, "STRING_OBJECT", "I");
    let superclass = field(&mut builder, "SUPERCLASS", "Ljava/lang/Class;");
    let runnable_modifiers = field(&mut builder, "RUNNABLE_MODIFIERS", "I");
    let string_modifiers = field(&mut builder, "STRING_MODIFIERS", "I");
    let int_instance = field(&mut builder, "INT_INSTANCE", "I");
    let int_self = field(&mut builder, "INT_SELF", "I");
    let object_int = field(&mut builder, "OBJECT_INT", "I");
    let int_modifiers = field(&mut builder, "INT_MODIFIERS", "I");
    let int_super = field(&mut builder, "INT_SUPER", "Ljava/lang/Class;");

    let mut code = Vec::new();
    // OBJECT_INSTANCE = Object.class.isInstance(new Object());
    ldc_w(object_class, &mut code);
    code.extend_from_slice(&[0xbb, (object_class >> 8) as u8, object_class as u8]);
    code.push(0x59); // dup
    invoke(0xb7, object_init, &mut code);
    invoke(0xb6, is_instance, &mut code);
    putstatic(object_instance, &mut code);
    // NULL_INSTANCE = Object.class.isInstance(null);
    ldc_w(object_class, &mut code);
    code.push(0x01); // aconst_null
    invoke(0xb6, is_instance, &mut code);
    putstatic(null_instance, &mut code);
    // THREAD_RUNNABLE = Runnable.class.isAssignableFrom(Thread.class);
    ldc_w(runnable_class, &mut code);
    ldc_w(thread_class, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    putstatic(thread_runnable, &mut code);
    // OBJECT_STRING = Object.class.isAssignableFrom(String.class);
    ldc_w(object_class, &mut code);
    ldc_w(string_class, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    putstatic(object_string, &mut code);
    // STRING_OBJECT = String.class.isAssignableFrom(Object.class);
    ldc_w(string_class, &mut code);
    ldc_w(object_class, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    putstatic(string_object, &mut code);
    // SUPERCLASS = String.class.getSuperclass();
    ldc_w(string_class, &mut code);
    invoke(0xb6, get_superclass, &mut code);
    putstatic(superclass, &mut code);
    // RUNNABLE_MODIFIERS = Runnable.class.getModifiers();
    ldc_w(runnable_class, &mut code);
    invoke(0xb6, get_modifiers, &mut code);
    putstatic(runnable_modifiers, &mut code);
    // STRING_MODIFIERS = String.class.getModifiers();
    ldc_w(string_class, &mut code);
    invoke(0xb6, get_modifiers, &mut code);
    putstatic(string_modifiers, &mut code);
    // INT_INSTANCE = int.class.isInstance(new Object());
    ldc_w(int_class, &mut code);
    code.extend_from_slice(&[0xbb, (object_class >> 8) as u8, object_class as u8]);
    code.push(0x59);
    invoke(0xb7, object_init, &mut code);
    invoke(0xb6, is_instance, &mut code);
    putstatic(int_instance, &mut code);
    // INT_SELF = int.class.isAssignableFrom(int.class);
    ldc_w(int_class, &mut code);
    ldc_w(int_class, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    putstatic(int_self, &mut code);
    // OBJECT_INT = Object.class.isAssignableFrom(int.class);
    ldc_w(object_class, &mut code);
    ldc_w(int_class, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    putstatic(object_int, &mut code);
    // INT_MODIFIERS = int.class.getModifiers();
    ldc_w(int_class, &mut code);
    invoke(0xb6, get_modifiers, &mut code);
    putstatic(int_modifiers, &mut code);
    // INT_SUPER = int.class.getSuperclass();
    ldc_w(int_class, &mut code);
    invoke(0xb6, get_superclass, &mut code);
    putstatic(int_super, &mut code);
    code.push(0xb1); // return

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((3, 1, code)));
    std::fs::write(dir.join("ClassQueries.class"), builder.build()).expect("fixture class");
}

fn int(statics: &std::collections::HashMap<String, Value>, name: &str) -> i32 {
    match statics.get(name) {
        Some(Value::Int(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn java_queries_the_class_model() {
    let dir = fixture_dir("queries");
    build_queries(&dir);
    let path = class_path(&dir);
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "ClassQueries", &[])
        .unwrap_or_else(|error| panic!("ClassQueries failed: {error}"));

    let id = *shared
        .class_ids
        .get("ClassQueries")
        .expect("ClassQueries loaded");
    let object_class = shared
        .load_class(&path, "java/lang/Object")
        .expect("Object");
    let expected_super = shared
        .class_object(&path, object_class)
        .expect("Object.class");
    let statics = &shared.classes[id.index()].statics;

    assert_eq!(
        int(statics, "OBJECT_INSTANCE"),
        1,
        "new Object() is an Object"
    );
    assert_eq!(
        int(statics, "NULL_INSTANCE"),
        0,
        "null is an instance of nothing"
    );
    assert_eq!(
        int(statics, "THREAD_RUNNABLE"),
        1,
        "Thread implements Runnable"
    );
    assert_eq!(
        int(statics, "OBJECT_STRING"),
        1,
        "String is a subtype of Object"
    );
    assert_eq!(
        int(statics, "STRING_OBJECT"),
        0,
        "Object is not a subtype of String"
    );

    let Some(Value::Ref(superclass)) = statics.get("SUPERCLASS") else {
        panic!("SUPERCLASS is {:?}", statics.get("SUPERCLASS"));
    };
    assert_eq!(
        *superclass, expected_super,
        "String.getSuperclass() is Object's cached class object"
    );

    let runnable = int(statics, "RUNNABLE_MODIFIERS");
    assert_ne!(
        runnable & 0x0200,
        0,
        "Runnable carries ACC_INTERFACE: {runnable:#06x}"
    );
    let string = int(statics, "STRING_MODIFIERS");
    assert_eq!(
        string & 0x0011,
        0x0011,
        "String is public and final: {string:#06x}"
    );
    assert_eq!(
        string & 0x0020,
        0,
        "ACC_SUPER is not a reflection modifier: {string:#06x}"
    );

    // Descriptor-only targets answer without a loaded class.
    assert_eq!(int(statics, "INT_INSTANCE"), 0, "no object is an int");
    assert_eq!(
        int(statics, "INT_SELF"),
        1,
        "a type is assignable from its identical descriptor"
    );
    assert_eq!(
        int(statics, "OBJECT_INT"),
        0,
        "a primitive is not assignable to Object"
    );
    assert_eq!(
        int(statics, "INT_MODIFIERS"),
        0,
        "no guessed primitive modifiers"
    );
    assert_eq!(
        statics.get("INT_SUPER"),
        Some(&Value::NULL),
        "a primitive class object has no superclass"
    );
}

/// `isAssignableFrom(null)` violates the method's contract; the native reports it instead of
/// answering `false` (which would make a `null` argument indistinguishable from a non-subtype).
#[test]
fn a_null_assignable_from_argument_is_an_error() {
    let mut builder = ClassBuilder::new("NullAssignable", "java/lang/Object", 65);
    let object_class = builder.class("java/lang/Object");
    let is_assignable_from = builder.method_ref(
        "java/lang/Class",
        "isAssignableFrom",
        "(Ljava/lang/Class;)Z",
    );

    let mut code = Vec::new();
    ldc_w(object_class, &mut code);
    code.push(0x01); // aconst_null
    invoke(0xb6, is_assignable_from, &mut code);
    code.push(0x57); // pop
    code.push(0xb1); // return

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((2, 1, code)));

    let dir = fixture_dir("null");
    std::fs::write(dir.join("NullAssignable.class"), builder.build()).expect("fixture class");
    let path = class_path(&dir);
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    let error = rava_interp::run_main(&mut shared, &mut thread, &path, "NullAssignable", &[])
        .expect_err("a null argument must fail");
    assert!(
        matches!(error, VmError::Native(ref message) if message.contains("isAssignableFrom(null)")),
        "unexpected error: {error:?}"
    );
}
