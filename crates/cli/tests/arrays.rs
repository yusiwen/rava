//! S11 acceptance: an array is an object whose runtime class is its array class.
//!
//! A heap array used to carry only its element kind and contents, so `Shared::class_of`
//! answered `None`: `Object.getClass()` failed on an array, `checkcast [B` was reported as
//! `class not found: [B`, and `Class.getComponentType()` returned `null`. Every array the
//! interpreter or the VM allocates now carries its class descriptor, and the class-object
//! natives, `instanceof`/`checkcast` and the `Class` queries read it.
//!
//! The fixture calls all of those through ordinary bytecode and stores the answers in static
//! fields, so the test exercises the native registration and the interpreter paths rather than
//! the Rust helpers in isolation.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, Shared, ThreadState, Value};

fn host_path() -> ClassPath {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    rava_cli::class_path(Path::new(&home), Path::new("/nonexistent")).expect("class path")
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-arrays-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn class_path(dir: &Path) -> ClassPath {
    let home = std::env::var_os("JAVA21_HOME").expect("JAVA21_HOME");
    rava_cli::class_path(Path::new(&home), dir).expect("class path")
}

// --- a very small instruction assembler -------------------------------------------------------
//
// The builder writes a method body as raw bytes; labels are not its job yet. Branch offsets are
// relative to the opcode, so a wrong pair of bytes lands inside an instruction (S10's bug). The
// two helpers below record the opcode position and patch the offset once the target is known.

fn op(code: &mut Vec<u8>, opcode: u8) {
    code.push(opcode);
}

fn op_u16(code: &mut Vec<u8>, opcode: u8, operand: u16) {
    code.extend_from_slice(&[opcode, (operand >> 8) as u8, operand as u8]);
}

fn ldc_w(index: u16, code: &mut Vec<u8>) {
    op_u16(code, 0x13, index);
}

fn invoke(opcode: u8, index: u16, code: &mut Vec<u8>) {
    op_u16(code, opcode, index);
}

fn field(opcode: u8, index: u16, code: &mut Vec<u8>) {
    op_u16(code, opcode, index);
}

fn iconst(value: i32, code: &mut Vec<u8>) {
    match value {
        -1 => op(code, 0x02),
        0..=5 => op(code, 0x03 + value as u8),
        -128..=127 => {
            op(code, 0x10);
            code.push(value as i8 as u8);
        }
        _ => {
            op(code, 0x11);
            code.extend_from_slice(&(value as i16).to_be_bytes());
        }
    }
}

fn newarray(atype: u8, code: &mut Vec<u8>) {
    code.extend_from_slice(&[0xbc, atype]);
}

fn anewarray(class: u16, code: &mut Vec<u8>) {
    op_u16(code, 0xbd, class);
}

fn instance_of(class: u16, code: &mut Vec<u8>) {
    op_u16(code, 0xc1, class);
}

fn checkcast(class: u16, code: &mut Vec<u8>) {
    op_u16(code, 0xc0, class);
}

fn multianewarray(class: u16, dimensions: u8, code: &mut Vec<u8>) {
    code.extend_from_slice(&[0xc5, (class >> 8) as u8, class as u8, dimensions]);
}

/// `new T(); dup; <init>` — leaves one reference.
fn new_object(class: u16, init: u16, code: &mut Vec<u8>) {
    op_u16(code, 0xbb, class);
    op(code, 0x59);
    invoke(0xb7, init, code);
}

/// Emits a forward branch and returns the opcode position for [`patch`].
fn branch(opcode: u8, code: &mut Vec<u8>) -> usize {
    let start = code.len();
    code.extend_from_slice(&[opcode, 0x00, 0x00]);
    start
}

fn patch(start: usize, code: &mut [u8]) {
    let offset = (code.len() - start) as u16;
    code[start + 1..start + 3].copy_from_slice(&offset.to_be_bytes());
}

/// Consumes two references and stores `1` when they are identical, else `0` (`if_acmpeq`).
fn store_boolean(field_index: u16, code: &mut Vec<u8>) {
    let equal = branch(0xa5, code); // if_acmpeq
    iconst(0, code);
    let done = branch(0xa7, code);
    patch(equal, code);
    iconst(1, code);
    patch(done, code);
    field(0xb3, field_index, code); // putstatic
}

fn build_fixture(dir: &Path) {
    let mut builder = ClassBuilder::new("ArrayClasses", "java/lang/Object", 65);
    for (name, descriptor) in [
        ("INTS_CLASS", "Ljava/lang/Class;"),
        ("INT_ARRAY_CLASS", "Ljava/lang/Class;"),
        ("NAME_INT", "Ljava/lang/String;"),
        ("NAME_STRING", "Ljava/lang/String;"),
        ("MULTI_NAME", "Ljava/lang/String;"),
        ("SAME", "I"),
        ("IS_INT", "I"),
        ("IS_STR_EXACT", "I"),
        ("IS_STR_OBJ", "I"),
        ("IS_INT_OBJ", "I"),
        ("IS_OBJ_STRING", "I"),
        ("IS_NULL_INT", "I"),
        ("IS_OBJECT", "I"),
        ("CHECKCAST_LEN", "I"),
        ("COMPONENT_INT", "I"),
        ("COMPONENT_STRING", "I"),
        ("IS_INSTANCE_INT", "I"),
        ("IS_INSTANCE_OBJ", "I"),
        ("IS_INSTANCE_LOADED", "I"),
        ("ASSIGNABLE_OBJECT", "I"),
        ("ASSIGNABLE_COVARIANT", "I"),
        ("ASSIGNABLE_EXACT", "I"),
        ("ASSIGNABLE_NOT", "I"),
        ("MODIFIERS", "I"),
        ("SUPER_OBJECT", "I"),
        ("MULTI_DEEP_LEN", "I"),
        ("MULTI_PARTIAL_LEN", "I"),
    ] {
        builder.field(0x0009, name, descriptor);
    }

    let int_array = builder.class("[I");
    // `int.class` in real class files is `getstatic java/lang/Integer.TYPE`; a `CONSTANT_Class`
    // named "I" is a class named I (JVMS §4.4.1), which S27 now resolves as one.
    let int_type = builder.field_ref("java/lang/Integer", "TYPE", "Ljava/lang/Class;");
    let int_matrix = builder.class("[[I");
    let byte_array = builder.class("[B");
    let string_array = builder.class("[Ljava/lang/String;");
    let object_array = builder.class("[Ljava/lang/Object;");
    let object_class = builder.class("java/lang/Object");
    let string_class = builder.class("java/lang/String");

    let get_class = builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");
    let get_component_type =
        builder.method_ref("java/lang/Class", "getComponentType", "()Ljava/lang/Class;");
    let get_modifiers = builder.method_ref("java/lang/Class", "getModifiers", "()I");
    let get_superclass =
        builder.method_ref("java/lang/Class", "getSuperclass", "()Ljava/lang/Class;");
    let is_instance = builder.method_ref("java/lang/Class", "isInstance", "(Ljava/lang/Object;)Z");
    let is_assignable_from = builder.method_ref(
        "java/lang/Class",
        "isAssignableFrom",
        "(Ljava/lang/Class;)Z",
    );
    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");

    let f = |builder: &mut ClassBuilder, name: &str, descriptor: &str| {
        builder.field_ref("ArrayClasses", name, descriptor)
    };
    let ints_class = f(&mut builder, "INTS_CLASS", "Ljava/lang/Class;");
    let int_array_class = f(&mut builder, "INT_ARRAY_CLASS", "Ljava/lang/Class;");
    let name_int = f(&mut builder, "NAME_INT", "Ljava/lang/String;");
    let name_string = f(&mut builder, "NAME_STRING", "Ljava/lang/String;");
    let multi_name = f(&mut builder, "MULTI_NAME", "Ljava/lang/String;");
    let same = f(&mut builder, "SAME", "I");
    let is_int = f(&mut builder, "IS_INT", "I");
    let is_str_exact = f(&mut builder, "IS_STR_EXACT", "I");
    let is_str_obj = f(&mut builder, "IS_STR_OBJ", "I");
    let is_int_obj = f(&mut builder, "IS_INT_OBJ", "I");
    let is_obj_string = f(&mut builder, "IS_OBJ_STRING", "I");
    let is_null_int = f(&mut builder, "IS_NULL_INT", "I");
    let is_object = f(&mut builder, "IS_OBJECT", "I");
    let checkcast_len = f(&mut builder, "CHECKCAST_LEN", "I");
    let component_int = f(&mut builder, "COMPONENT_INT", "I");
    let component_string = f(&mut builder, "COMPONENT_STRING", "I");
    let is_instance_int = f(&mut builder, "IS_INSTANCE_INT", "I");
    let is_instance_obj = f(&mut builder, "IS_INSTANCE_OBJ", "I");
    let is_instance_loaded = f(&mut builder, "IS_INSTANCE_LOADED", "I");
    let assignable_object = f(&mut builder, "ASSIGNABLE_OBJECT", "I");
    let assignable_covariant = f(&mut builder, "ASSIGNABLE_COVARIANT", "I");
    let assignable_exact = f(&mut builder, "ASSIGNABLE_EXACT", "I");
    let assignable_not = f(&mut builder, "ASSIGNABLE_NOT", "I");
    let modifiers = f(&mut builder, "MODIFIERS", "I");
    let super_object = f(&mut builder, "SUPER_OBJECT", "I");
    let multi_deep_len = f(&mut builder, "MULTI_DEEP_LEN", "I");
    let multi_partial_len = f(&mut builder, "MULTI_PARTIAL_LEN", "I");

    let mut code = Vec::new();
    // INTS_CLASS = new int[3].getClass(); INT_ARRAY_CLASS = int[].class
    iconst(3, &mut code);
    newarray(10, &mut code); // T_INT
    invoke(0xb6, get_class, &mut code);
    field(0xb3, ints_class, &mut code);
    ldc_w(int_array, &mut code);
    field(0xb3, int_array_class, &mut code);
    // SAME = (new int[3].getClass() == int[].class) ? 1 : 0
    field(0xb2, ints_class, &mut code);
    field(0xb2, int_array_class, &mut code);
    store_boolean(same, &mut code);
    // NAME_INT = new int[3].getClass().getName()
    field(0xb2, ints_class, &mut code);
    invoke(0xb6, get_name, &mut code);
    field(0xb3, name_int, &mut code);
    // NAME_STRING = new String[2].getClass().getName()
    iconst(2, &mut code);
    anewarray(string_class, &mut code);
    invoke(0xb6, get_class, &mut code);
    invoke(0xb6, get_name, &mut code);
    field(0xb3, name_string, &mut code);
    // MULTI_NAME = new int[2][3].getClass().getName() — multianewarray names the full type.
    iconst(2, &mut code);
    iconst(3, &mut code);
    multianewarray(int_matrix, 2, &mut code);
    invoke(0xb6, get_class, &mut code);
    invoke(0xb6, get_name, &mut code);
    field(0xb3, multi_name, &mut code);
    // MULTI_DEEP_LEN = new int[2][3], then row 1's length; the outer level holds arrays.
    iconst(2, &mut code);
    iconst(3, &mut code);
    multianewarray(int_matrix, 2, &mut code);
    iconst(1, &mut code);
    op(&mut code, 0x32); // aaload
    op(&mut code, 0xbe); // arraylength
    field(0xb3, multi_deep_len, &mut code);
    // MULTI_PARTIAL_LEN = ((int[][]) new int[1][])[0] = new int[3]; length 3.
    // A partial multianewarray allocates an array of [I, so its elements are references.
    iconst(1, &mut code);
    multianewarray(int_matrix, 1, &mut code);
    op(&mut code, 0x59); // dup
    iconst(0, &mut code);
    iconst(3, &mut code);
    newarray(10, &mut code);
    op(&mut code, 0x53); // aastore
    iconst(0, &mut code);
    op(&mut code, 0x32); // aaload
    op(&mut code, 0xbe); // arraylength
    field(0xb3, multi_partial_len, &mut code);

    // instanceof: true for the array's own type, false for an incompatible one.
    iconst(1, &mut code);
    newarray(10, &mut code);
    instance_of(int_array, &mut code);
    field(0xb3, is_int, &mut code);
    iconst(1, &mut code);
    anewarray(string_class, &mut code);
    instance_of(string_array, &mut code);
    field(0xb3, is_str_exact, &mut code);
    // String[] is a subtype of Object[] (covariance), int[] is not.
    iconst(1, &mut code);
    anewarray(string_class, &mut code);
    instance_of(object_array, &mut code);
    field(0xb3, is_str_obj, &mut code);
    iconst(1, &mut code);
    newarray(10, &mut code);
    instance_of(object_array, &mut code);
    field(0xb3, is_int_obj, &mut code);
    // Object[] is not a subtype of String[].
    iconst(1, &mut code);
    anewarray(object_class, &mut code);
    instance_of(string_array, &mut code);
    field(0xb3, is_obj_string, &mut code);
    // null instanceof int[] is false.
    op(&mut code, 0x01);
    instance_of(int_array, &mut code);
    field(0xb3, is_null_int, &mut code);
    // Every array is an Object.
    iconst(1, &mut code);
    newarray(10, &mut code);
    instance_of(object_class, &mut code);
    field(0xb3, is_object, &mut code);

    // CHECKCAST_LEN = ((byte[]) new byte[4]).length — a successful `checkcast [B`.
    iconst(4, &mut code);
    newarray(8, &mut code); // T_BYTE
    checkcast(byte_array, &mut code);
    op(&mut code, 0xbe); // arraylength
    field(0xb3, checkcast_len, &mut code);

    // COMPONENT_INT = int[].class.getComponentType() == int.class
    ldc_w(int_array, &mut code);
    invoke(0xb6, get_component_type, &mut code);
    op_u16(&mut code, 0xb2, int_type);
    store_boolean(component_int, &mut code);
    // COMPONENT_STRING = String[].class.getComponentType() == String.class
    ldc_w(string_array, &mut code);
    invoke(0xb6, get_component_type, &mut code);
    ldc_w(string_class, &mut code);
    store_boolean(component_string, &mut code);

    // int[].class.isInstance(new int[0]) / isInstance(new Object())
    ldc_w(int_array, &mut code);
    iconst(0, &mut code);
    newarray(10, &mut code);
    invoke(0xb6, is_instance, &mut code);
    field(0xb3, is_instance_int, &mut code);
    ldc_w(int_array, &mut code);
    new_object(object_class, object_init, &mut code);
    invoke(0xb6, is_instance, &mut code);
    field(0xb3, is_instance_obj, &mut code);
    // Object.class.isInstance(new int[0]) — a loaded class target sees the array.
    ldc_w(object_class, &mut code);
    iconst(0, &mut code);
    newarray(10, &mut code);
    invoke(0xb6, is_instance, &mut code);
    field(0xb3, is_instance_loaded, &mut code);

    // Object.class.isAssignableFrom(String[].class)
    ldc_w(object_class, &mut code);
    ldc_w(string_array, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    field(0xb3, assignable_object, &mut code);
    // Object[].class.isAssignableFrom(String[].class) — covariance.
    ldc_w(object_array, &mut code);
    ldc_w(string_array, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    field(0xb3, assignable_covariant, &mut code);
    // int[].class.isAssignableFrom(int[].class) — exact.
    ldc_w(int_array, &mut code);
    ldc_w(int_array, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    field(0xb3, assignable_exact, &mut code);
    // int[].class.isAssignableFrom(Object[].class) — neither direction of covariance.
    ldc_w(int_array, &mut code);
    ldc_w(object_array, &mut code);
    invoke(0xb6, is_assignable_from, &mut code);
    field(0xb3, assignable_not, &mut code);

    // MODIFIERS = int[].class.getModifiers(); SUPER_OBJECT = int[].class.getSuperclass() == Object.class
    ldc_w(int_array, &mut code);
    invoke(0xb6, get_modifiers, &mut code);
    field(0xb3, modifiers, &mut code);
    ldc_w(int_array, &mut code);
    invoke(0xb6, get_superclass, &mut code);
    ldc_w(object_class, &mut code);
    store_boolean(super_object, &mut code);

    op(&mut code, 0xb1); // return

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((4, 1, code)));
    std::fs::write(dir.join("ArrayClasses.class"), builder.build()).expect("fixture class");
}

fn run(dir: &Path) -> Shared {
    let path = class_path(dir);
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "ArrayClasses", &[])
        .unwrap_or_else(|error| panic!("ArrayClasses failed: {error}"));
    shared
}

fn int(statics: &std::collections::HashMap<String, Value>, name: &str) -> i32 {
    match statics.get(name) {
        Some(Value::Int(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

fn reference(statics: &std::collections::HashMap<String, Value>, name: &str) -> rava_vm::ObjRef {
    match statics.get(name) {
        Some(Value::Ref(value)) => *value,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn java_reads_array_class_objects() {
    let dir = fixture_dir("classes");
    build_fixture(&dir);
    let shared = run(&dir);
    let id = *shared
        .class_ids
        .get("ArrayClasses")
        .expect("ArrayClasses loaded");
    let statics = &shared.classes[id.index()].statics;

    // `new int[3].getClass()` is the same cached object as `int[].class`.
    let ints = reference(statics, "INTS_CLASS");
    let int_array = reference(statics, "INT_ARRAY_CLASS");
    assert_eq!(ints, int_array, "new int[3].getClass() != int[].class");
    assert_eq!(
        int(statics, "SAME"),
        1,
        "the fixture's if_acmpeq saw different objects"
    );
    assert_eq!(shared.type_class_ref(ints), Some("[I"));
    assert_eq!(shared.type_class_ref(int_array), Some("[I"));
    let class_class = shared
        .class_of(ints)
        .expect("an array class object has a runtime class");
    assert_eq!(
        shared.class(class_class).name,
        "java/lang/Class",
        "an array class object is an instance of java.lang.Class"
    );

    // Class.getName maps the descriptor: [I stays [I, [Ljava/lang/String; becomes
    // [Ljava.lang.String;.
    let name_int = reference(statics, "NAME_INT");
    assert_eq!(
        shared.string_text(name_int).expect("NAME_INT"),
        "[I",
        "int[].class.getName()"
    );
    let name_string = reference(statics, "NAME_STRING");
    assert_eq!(
        shared.string_text(name_string).expect("NAME_STRING"),
        "[Ljava.lang.String;",
        "String[].class.getName()"
    );
    let multi_name = reference(statics, "MULTI_NAME");
    assert_eq!(
        shared.string_text(multi_name).expect("MULTI_NAME"),
        "[[I",
        "multianewarray's class name"
    );
    assert_eq!(
        int(statics, "MULTI_DEEP_LEN"),
        3,
        "new int[2][3]: row 1 has 3 columns"
    );
    assert_eq!(
        int(statics, "MULTI_PARTIAL_LEN"),
        3,
        "((int[][]) new int[1][])[0] = new int[3]"
    );

    // instanceof
    assert_eq!(int(statics, "IS_INT"), 1, "int[] instanceof int[]");
    assert_eq!(
        int(statics, "IS_STR_EXACT"),
        1,
        "String[] instanceof String[]"
    );
    assert_eq!(
        int(statics, "IS_STR_OBJ"),
        1,
        "String[] instanceof Object[] (covariance)"
    );
    assert_eq!(int(statics, "IS_INT_OBJ"), 0, "int[] is not an Object[]");
    assert_eq!(
        int(statics, "IS_OBJ_STRING"),
        0,
        "Object[] is not a String[]"
    );
    assert_eq!(
        int(statics, "IS_NULL_INT"),
        0,
        "null is an instance of no type"
    );
    assert_eq!(int(statics, "IS_OBJECT"), 1, "int[] instanceof Object");

    // A successful `checkcast [B`: the value survives, and its length is readable.
    assert_eq!(
        int(statics, "CHECKCAST_LEN"),
        4,
        "checkcast [B / arraylength"
    );

    // getComponentType answers with the cached class object of the component.
    assert_eq!(
        int(statics, "COMPONENT_INT"),
        1,
        "int[].class.getComponentType() != int.class"
    );
    assert_eq!(
        int(statics, "COMPONENT_STRING"),
        1,
        "String[].class.getComponentType() != String.class"
    );

    // Class.isInstance and isAssignableFrom answer for array targets.
    assert_eq!(
        int(statics, "IS_INSTANCE_INT"),
        1,
        "int[].class.isInstance(new int[0])"
    );
    assert_eq!(
        int(statics, "IS_INSTANCE_OBJ"),
        0,
        "int[].class.isInstance(new Object())"
    );
    assert_eq!(
        int(statics, "IS_INSTANCE_LOADED"),
        1,
        "Object.class.isInstance(new int[0])"
    );
    assert_eq!(
        int(statics, "ASSIGNABLE_OBJECT"),
        1,
        "Object.class.isAssignableFrom(String[].class)"
    );
    assert_eq!(
        int(statics, "ASSIGNABLE_COVARIANT"),
        1,
        "Object[].class.isAssignableFrom(String[].class)"
    );
    assert_eq!(
        int(statics, "ASSIGNABLE_EXACT"),
        1,
        "int[].class.isAssignableFrom(int[].class)"
    );
    assert_eq!(
        int(statics, "ASSIGNABLE_NOT"),
        0,
        "int[].class.isAssignableFrom(Object[].class)"
    );

    // The remaining Class queries agree with the host JDK's array answers.
    assert_eq!(
        int(statics, "MODIFIERS"),
        0x0411,
        "int[].class.getModifiers() is PUBLIC|FINAL|ABSTRACT"
    );
    assert_eq!(
        int(statics, "SUPER_OBJECT"),
        1,
        "int[].class.getSuperclass() is Object.class"
    );
}

#[test]
fn vm_created_arrays_name_their_class() {
    let path = host_path();
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);

    // `Shared::new_string` creates the host class's `byte[] value`.
    let text = shared.new_string(&path, "hi".to_owned()).expect("String");
    let class = shared.class_of(text).expect("String's class");
    let value_slot = shared
        .class(class)
        .field_slot("value", "[B")
        .expect("String.value");
    let Value::Ref(value) = shared.instance_field(text, value_slot).expect("value") else {
        panic!("value is not a reference");
    };
    assert_eq!(shared.array_class(value), Some("[B"));
    assert_eq!(
        shared.array_class(text),
        None,
        "a String instance is not an array"
    );

    // `Shared::string_array` creates `main`'s `String[]`.
    let args = shared
        .string_array(&path, &["a".to_owned()])
        .expect("String[]");
    assert_eq!(shared.array_class(args), Some("[Ljava/lang/String;"));
}

#[test]
fn array_type_classes_carry_their_component() {
    let path = host_path();
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);

    let int_class = shared.type_class(&path, "I").expect("int.class");
    let int_array = shared.type_class(&path, "[I").expect("int[].class");
    assert_eq!(
        shared.type_class(&path, "[I").expect("cached"),
        int_array,
        "the type-class cache returns one object per descriptor"
    );
    assert_eq!(shared.type_class_ref(int_array), Some("[I"));

    // JDK 21's `Class.getComponentType()` is a Java method reading this final field, so the
    // model has to populate it when the array class object is created.
    let class_class = shared.class_of(int_array).expect("Class");
    let component_slot = shared
        .class(class_class)
        .field_slot("componentType", "Ljava/lang/Class;")
        .expect("Class.componentType");
    assert_eq!(
        shared
            .instance_field(int_array, component_slot)
            .expect("slot"),
        Value::Ref(int_class)
    );

    // Nested arrays recurse: [[I's component is [I.
    let nested = shared.type_class(&path, "[[I").expect("[[I");
    assert_eq!(
        shared.instance_field(nested, component_slot).expect("slot"),
        Value::Ref(int_array)
    );

    // A reference array's component is the loaded class object, not a type class.
    let string_array = shared
        .type_class(&path, "[Ljava/lang/String;")
        .expect("String[].class");
    let string_class = shared
        .load_class(&path, "java/lang/String")
        .expect("String");
    let string_class_object = shared
        .class_object(&path, string_class)
        .expect("String.class");
    assert_eq!(
        shared
            .instance_field(string_array, component_slot)
            .expect("slot"),
        Value::Ref(string_class_object)
    );
}
