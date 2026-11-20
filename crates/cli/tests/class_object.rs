//! S9 acceptance: class objects are instances of the host's `java.lang.Class`.
//!
//! The VM used to represent a class object as an opaque tag (`HeapObject::ClassRef`), so any
//! Java code that read a `Class` field — `desiredAssertionStatus()` reads `classLoader` — failed
//! with a type mismatch. A class object is now a real instance with the host class's layout, and
//! the VM keeps the object-to-class mapping on its side (the host `java.lang.Class` has no spare
//! field for it, unlike OpenJ9's `vmRef`).
//!
//! The structural tests pin the representation; the fixture runs Java that reads the fields and
//! calls the natives, which is the property that was broken.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{ClassPath, HeapObject, Shared, ThreadState, Value};

fn host_path() -> ClassPath {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    rava_cli::class_path(Path::new(&home), Path::new("/nonexistent")).expect("class path")
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-classobj-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

#[test]
fn a_class_object_is_an_instance_of_class() {
    let path = host_path();
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let string = shared
        .load_class(&path, "java/lang/String")
        .expect("String");
    let class_class = shared.load_class(&path, "java/lang/Class").expect("Class");

    let object = shared.class_object(&path, string).expect("String.class");
    // Identity: the cache returns the same object, which is what `Foo.class == Foo.class` means.
    assert_eq!(shared.class_object(&path, string).expect("cached"), object);

    let HeapObject::Instance(instance) = &shared.heap[object.index()] else {
        panic!("a class object is not an instance");
    };
    assert_eq!(
        instance.class, class_class,
        "its runtime class is java.lang.Class"
    );
    // That is what makes `instanceof Class` and virtual dispatch on class objects work.
    assert_eq!(shared.class_of(object), Some(class_class));

    // The VM-side mapping is the reverse of the cache; the host class has no field for it.
    assert_eq!(shared.class_ref(object), Some(string));

    // `classLoader` is null for a bootstrap class, which is what `desiredAssertionStatus` reads.
    let slot = shared
        .class(class_class)
        .field_slot("classLoader", "Ljava/lang/ClassLoader;")
        .expect("Class.classLoader");
    assert_eq!(
        shared.instance_field(object, slot).expect("field"),
        Value::NULL
    );
}

#[test]
fn type_class_objects_are_instances_too() {
    let path = host_path();
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let class_class = shared.load_class(&path, "java/lang/Class").expect("Class");

    let int_class = shared.type_class(&path, "I").expect("int.class");
    assert_eq!(shared.type_class(&path, "I").expect("cached"), int_class);
    let HeapObject::Instance(instance) = &shared.heap[int_class.index()] else {
        panic!("int.class is not an instance");
    };
    assert_eq!(instance.class, class_class);
    assert_eq!(shared.type_class_ref(int_class), Some("I"));
    assert_eq!(
        shared.class_ref(int_class),
        None,
        "a primitive names no class"
    );
}

#[test]
fn a_class_object_survives_collection() {
    let path = host_path();
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let object = shared
        .load_class(&path, "java/lang/Object")
        .and_then(|class| shared.class_object(&path, class))
        .expect("Object.class");

    shared.collect(&ThreadState::new());
    assert!(
        matches!(shared.heap[object.index()], HeapObject::Instance(_)),
        "the class-object cache must keep it alive"
    );
}

/// A fixture that reads the class object three ways: `getName` (a field read plus
/// `initClassName`), `desiredAssertionStatus` (a field read plus `desiredAssertionStatus0`), and
/// `getClass` on an ordinary object.
fn build_class_info(dir: &Path) {
    let mut builder = ClassBuilder::new("ClassInfo", "java/lang/Object", 65);
    builder.field(0x0009, "NAME", "Ljava/lang/String;");
    builder.field(0x0009, "ASSERTIONS", "I");
    builder.field(0x0009, "SELF", "Ljava/lang/Class;");

    let class_info = builder.class("ClassInfo");
    let get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");
    let name_field = builder.field_ref("ClassInfo", "NAME", "Ljava/lang/String;");
    let desired = builder.method_ref("java/lang/Class", "desiredAssertionStatus", "()Z");
    let assertions = builder.field_ref("ClassInfo", "ASSERTIONS", "I");
    let object_class = builder.class("java/lang/Object");
    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let get_class = builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let self_field = builder.field_ref("ClassInfo", "SELF", "Ljava/lang/Class;");

    let mut code = Vec::new();
    let ldc = |index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[0x13, (index >> 8) as u8, index as u8]);
    };
    let call = |opcode: u8, index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[opcode, (index >> 8) as u8, index as u8]);
    };

    // NAME = ClassInfo.class.getName();
    ldc(class_info, &mut code);
    call(0xb6, get_name, &mut code);
    call(0xb3, name_field, &mut code);
    // ASSERTIONS = ClassInfo.class.desiredAssertionStatus();
    ldc(class_info, &mut code);
    call(0xb6, desired, &mut code);
    call(0xb3, assertions, &mut code);
    // SELF = new Object().getClass();
    code.extend_from_slice(&[0xbb, (object_class >> 8) as u8, object_class as u8]);
    code.push(0x59);
    call(0xb7, object_init, &mut code);
    call(0xb6, get_class, &mut code);
    call(0xb3, self_field, &mut code);
    code.push(0xb1);

    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((2, 1, code)));
    std::fs::write(dir.join("ClassInfo.class"), builder.build()).expect("fixture class");
}

#[test]
fn java_reads_and_calls_the_class_object() {
    let dir = fixture_dir("classinfo");
    build_class_info(&dir);
    let path = {
        let home = std::env::var_os("JAVA21_HOME").expect("JAVA21_HOME");
        rava_cli::class_path(Path::new(&home), &dir).expect("class path")
    };
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "ClassInfo", &[])
        .unwrap_or_else(|error| panic!("ClassInfo failed: {error}"));

    let id = *shared.class_ids.get("ClassInfo").expect("ClassInfo loaded");
    // The fixture stores `new Object().getClass()`, which is `java.lang.Object`'s class object.
    let object = shared
        .load_class(&path, "java/lang/Object")
        .expect("Object");
    let expected_self = shared.class_object(&path, object).expect("Object.class");
    let statics = &shared.classes[id.index()].statics;

    let name = statics.get("NAME").expect("NAME set");
    let Value::Ref(name) = name else {
        panic!("NAME is {name:?}");
    };
    assert_eq!(
        shared.string_text(*name).expect("NAME is a String"),
        "ClassInfo",
        "getName returned the wrong name"
    );

    assert_eq!(statics.get("ASSERTIONS"), Some(&Value::Int(0)));

    let self_value = statics.get("SELF").expect("SELF set");
    let Value::Ref(self_value) = self_value else {
        panic!("SELF is {self_value:?}");
    };
    assert_eq!(
        expected_self, *self_value,
        "getClass must return Object's cached class object"
    );
}
