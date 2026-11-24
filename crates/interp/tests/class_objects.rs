//! M2.4 acceptance: class objects and basic `Object`/`Class` reflection.
//!
//! `ReflectProbe` is assembled from the committed C1 bootstrap library: it calls
//! `Object.getClass` on its argument and the `Class` methods on the result. A `String`
//! reaches the real `java.lang.String` class object, an allocated instance reaches its
//! defining class, and `Class.forName` supplies an interface class object.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ArrayKind, ClassId, FieldLayout, LoaderId, ObjRef, Shared, ThreadState, Value, VmError,
    reflection,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

/// The fixture class. Every method is a static helper the test invokes directly, so the
/// assertions exercise the interpreter's `invokevirtual`/`invokestatic` path and the
/// native registry rather than calling a native function by hand.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ReflectProbe", "java/lang/Object", 61);

    let object_get_class =
        builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let class_get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");
    let class_get_modifiers = builder.method_ref("java/lang/Class", "getModifiers", "()I");
    let class_is_interface = builder.method_ref("java/lang/Class", "isInterface", "()Z");
    let class_is_array = builder.method_ref("java/lang/Class", "isArray", "()Z");
    let class_get_superclass =
        builder.method_ref("java/lang/Class", "getSuperclass", "()Ljava/lang/Class;");
    let class_for_name = builder.method_ref(
        "java/lang/Class",
        "forName",
        "(Ljava/lang/String;)Ljava/lang/Class;",
    );

    // Object className(Object o) { return o.getClass().getName(); }
    let mut class_name_from_object = Assembler::new();
    class_name_from_object.aload(0);
    class_name_from_object.invokevirtual(object_get_class);
    class_name_from_object.invokevirtual(class_get_name);
    class_name_from_object.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "className",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        Some((2, 1, class_name_from_object.finish().unwrap())),
    );

    // int modifiers(Object o) { return o.getClass().getModifiers(); }
    let mut modifiers = Assembler::new();
    modifiers.aload(0);
    modifiers.invokevirtual(object_get_class);
    modifiers.invokevirtual(class_get_modifiers);
    modifiers.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "modifiers",
        "(Ljava/lang/Object;)I",
        Some((2, 1, modifiers.finish().unwrap())),
    );

    // boolean interfaceFlag(Object o) { return o.getClass().isInterface(); }
    let mut interface_flag = Assembler::new();
    interface_flag.aload(0);
    interface_flag.invokevirtual(object_get_class);
    interface_flag.invokevirtual(class_is_interface);
    interface_flag.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "interfaceFlag",
        "(Ljava/lang/Object;)Z",
        Some((2, 1, interface_flag.finish().unwrap())),
    );

    // String superName(Object o) { return o.getClass().getSuperclass().getName(); }
    let mut super_name = Assembler::new();
    super_name.aload(0);
    super_name.invokevirtual(object_get_class);
    super_name.invokevirtual(class_get_superclass);
    super_name.invokevirtual(class_get_name);
    super_name.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "superName",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        Some((2, 1, super_name.finish().unwrap())),
    );

    // String classNameOfClass(Class c) { return c.getName(); }
    let mut class_name = Assembler::new();
    class_name.aload(0);
    class_name.invokevirtual(class_get_name);
    class_name.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "classNameOfClass",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        Some((1, 1, class_name.finish().unwrap())),
    );

    // int classModifiers(Class c) { return c.getModifiers(); }
    let mut class_modifiers = Assembler::new();
    class_modifiers.aload(0);
    class_modifiers.invokevirtual(class_get_modifiers);
    class_modifiers.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "classModifiers",
        "(Ljava/lang/Class;)I",
        Some((1, 1, class_modifiers.finish().unwrap())),
    );

    // boolean classInterface(Class c) { return c.isInterface(); }
    let mut class_interface = Assembler::new();
    class_interface.aload(0);
    class_interface.invokevirtual(class_is_interface);
    class_interface.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "classInterface",
        "(Ljava/lang/Class;)Z",
        Some((1, 1, class_interface.finish().unwrap())),
    );

    // boolean classIsArray(Class c) { return c.isArray(); }
    let mut class_is_array_body = Assembler::new();
    class_is_array_body.aload(0);
    class_is_array_body.invokevirtual(class_is_array);
    class_is_array_body.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "classIsArray",
        "(Ljava/lang/Class;)Z",
        Some((1, 1, class_is_array_body.finish().unwrap())),
    );

    // Class superclassOf(Class c) { return c.getSuperclass(); }
    let mut superclass_of = Assembler::new();
    superclass_of.aload(0);
    superclass_of.invokevirtual(class_get_superclass);
    superclass_of.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "superclassOf",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        Some((1, 1, superclass_of.finish().unwrap())),
    );

    // Class forNameClass(String name) { return Class.forName(name); }
    let mut for_name = Assembler::new();
    for_name.aload(0);
    for_name.op(0xb8, &class_for_name.to_be_bytes());
    for_name.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "forNameClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        Some((1, 1, for_name.finish().unwrap())),
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    probe: ClassId,
    thread: ThreadState,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    reflection::register_natives(&shared);
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "ReflectProbe", &probe_bytes())
        .expect("the fixture class defines");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        probe,
        thread,
    }
}

fn call(
    fixture: &Fixture,
    method: &str,
    descriptor: &str,
    argument: Value,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        method,
        descriptor,
        vec![argument],
    )
}

fn string_value(shared: &Shared, value: Option<Value>) -> String {
    let Some(Value::Ref(reference)) = value else {
        panic!("expected a string result, found {value:?}");
    };
    shared
        .heap()
        .string(reference)
        .expect("the result is a string")
        .to_owned()
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
}

#[test]
fn a_string_reaches_the_java_lang_string_class_object() {
    let fixture = fixture();
    let string = fixture.shared.heap().allocate_string("hello");

    let name = call(
        &fixture,
        "className",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        Value::Ref(string),
    )
    .unwrap();
    assert_eq!(string_value(&fixture.shared, name), "java.lang.String");

    let modifiers = call(
        &fixture,
        "modifiers",
        "(Ljava/lang/Object;)I",
        Value::Ref(string),
    )
    .unwrap();
    // `String` is public final: `0x0011`. The class-file `ACC_SUPER` bit is masked out.
    assert_eq!(int_value(modifiers), 0x0011);

    let flag = call(
        &fixture,
        "interfaceFlag",
        "(Ljava/lang/Object;)Z",
        Value::Ref(string),
    )
    .unwrap();
    assert_eq!(int_value(flag), 0);

    let super_name = call(
        &fixture,
        "superName",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        Value::Ref(string),
    )
    .unwrap();
    assert_eq!(
        string_value(&fixture.shared, super_name),
        "java.lang.Object"
    );
}

#[test]
fn an_instance_reaches_its_defining_class() {
    let fixture = fixture();
    let instance = fixture
        .shared
        .heap()
        .allocate_instance(fixture.probe, &FieldLayout::new());

    let name = call(
        &fixture,
        "className",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        Value::Ref(instance),
    )
    .unwrap();
    assert_eq!(string_value(&fixture.shared, name), "ReflectProbe");

    let modifiers = call(
        &fixture,
        "modifiers",
        "(Ljava/lang/Object;)I",
        Value::Ref(instance),
    )
    .unwrap();
    assert_eq!(int_value(modifiers), 0x0001, "ReflectProbe is public");

    let flag = call(
        &fixture,
        "interfaceFlag",
        "(Ljava/lang/Object;)Z",
        Value::Ref(instance),
    )
    .unwrap();
    assert_eq!(int_value(flag), 0);
}

#[test]
fn class_objects_are_cached_by_class_identity_and_are_roots() {
    let fixture = fixture();
    let string_class = fixture
        .shared
        .load_library_class("java/lang/String")
        .unwrap();

    let first = fixture.shared.class_object(string_class).unwrap();
    let second = fixture.shared.class_object(string_class).unwrap();
    assert_eq!(first, second, "one class object per class identity");

    let string = fixture.shared.heap().allocate_string("hello");
    assert_eq!(
        fixture.shared.get_class(string).unwrap(),
        first,
        "getClass on a String reports the cached String class object"
    );

    let class_class = fixture
        .shared
        .load_library_class("java/lang/Class")
        .unwrap();
    let class_object = fixture.shared.class_object(class_class).unwrap();
    assert_eq!(
        fixture.shared.get_class(class_object).unwrap(),
        class_object,
        "getClass on a Class object reports java.lang.Class itself"
    );

    let roots = fixture.shared.roots(&[]);
    assert!(roots.contains(&first));
    assert!(roots.contains(&class_object));

    // The class-file facts come from the class table, not a second copy.
    let entry = fixture.shared.entry(string_class).unwrap();
    assert_eq!(entry.name, "java/lang/String");
    assert_ne!(entry.file.access_flags & 0x7E1F, 0);
}

#[test]
fn class_for_name_supplies_an_interface_class_object() {
    let fixture = fixture();
    let argument = fixture.shared.heap().allocate_string("java.util.List");

    let class = ref_value(
        call(
            &fixture,
            "forNameClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            Value::Ref(argument),
        )
        .unwrap(),
    );

    let name = call(
        &fixture,
        "classNameOfClass",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        Value::Ref(class),
    )
    .unwrap();
    assert_eq!(string_value(&fixture.shared, name), "java.util.List");

    let flag = call(
        &fixture,
        "classInterface",
        "(Ljava/lang/Class;)Z",
        Value::Ref(class),
    )
    .unwrap();
    assert_eq!(int_value(flag), 1, "java.util.List is an interface");

    let modifiers = call(
        &fixture,
        "classModifiers",
        "(Ljava/lang/Class;)I",
        Value::Ref(class),
    )
    .unwrap();
    assert_eq!(int_value(modifiers), 0x0601, "public interface abstract");

    let is_array = call(
        &fixture,
        "classIsArray",
        "(Ljava/lang/Class;)Z",
        Value::Ref(class),
    )
    .unwrap();
    assert_eq!(int_value(is_array), 0);

    // `Class.getName` is a C1 stub: no `Code` and not `ACC_NATIVE`. The registry lookup
    // is what made the calls above work.
    let class_id = fixture
        .shared
        .load_library_class("java/lang/Class")
        .unwrap();
    let entry = fixture.shared.entry(class_id).unwrap();
    let method = entry
        .file
        .find_method("getName", "()Ljava/lang/String;")
        .unwrap()
        .expect("java.lang.Class declares getName");
    assert!(!method.is_native());
    assert!(entry.file.code(method).unwrap().is_none());

    // An interface reports no superclass, exactly as on the JDK.
    let superclass = call(
        &fixture,
        "superclassOf",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        Value::Ref(class),
    )
    .unwrap();
    assert_eq!(superclass, Some(Value::Ref(ObjRef::NULL)));
}

#[test]
fn object_has_no_superclass_and_strings_report_object() {
    let fixture = fixture();
    let object_class = fixture
        .shared
        .load_library_class("java/lang/Object")
        .unwrap();
    let object_object = fixture.shared.class_object(object_class).unwrap();
    let superclass = call(
        &fixture,
        "superclassOf",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        Value::Ref(object_object),
    )
    .unwrap();
    assert_eq!(superclass, Some(Value::Ref(ObjRef::NULL)));

    let string_class = fixture
        .shared
        .load_library_class("java/lang/String")
        .unwrap();
    let string_object = fixture.shared.class_object(string_class).unwrap();
    let superclass = call(
        &fixture,
        "superclassOf",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        Value::Ref(string_object),
    )
    .unwrap();
    assert_eq!(
        superclass,
        Some(Value::Ref(object_object)),
        "String's superclass object is the cached Object class object"
    );
}

#[test]
fn an_array_has_no_class_object_and_the_error_is_typed() {
    let fixture = fixture();
    let array = fixture.shared.heap().allocate_array(ArrayKind::Int, 2);

    match fixture.shared.get_class(array) {
        Err(VmError::Native(detail)) => {
            assert!(detail.contains("array class objects"), "{detail}")
        }
        other => panic!("expected a typed native error, found {other:?}"),
    }

    let error = call(
        &fixture,
        "className",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        Value::Ref(array),
    );
    assert!(matches!(
        error,
        Err(InterpError::Vm(VmError::Native(ref detail)))
            if detail.contains("array class objects")
    ));
}

#[test]
fn class_for_name_of_a_missing_class_is_a_typed_error() {
    let fixture = fixture();
    let argument = fixture
        .shared
        .heap()
        .allocate_string("no.such.ReflectProbe");

    let error = call(
        &fixture,
        "forNameClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        Value::Ref(argument),
    );
    assert!(matches!(
        error,
        Err(InterpError::Vm(VmError::ClassNotFound { .. }))
    ));
}

#[test]
fn string_modifiers_match_the_class_file_flags_with_super_removed() {
    let fixture = fixture();
    let string_class = fixture
        .shared
        .load_library_class("java/lang/String")
        .unwrap();
    let flags = fixture
        .shared
        .entry(string_class)
        .unwrap()
        .file
        .access_flags;
    // The class file records ACC_PUBLIC | ACC_FINAL | ACC_SUPER (0x0031);
    // `getModifiers()` exposes only the Java modifiers (0x0011).
    assert_eq!(i32::from(flags), 0x0031);
    assert_eq!(i32::from(flags & 0x7E1F), 0x0011);
}
