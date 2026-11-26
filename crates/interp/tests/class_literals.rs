//! M2.17 acceptance: `ldc`/`ldc_w` class constants and Java class literals.
//!
//! `ClassLiteralProbe` is assembled in Rust rather than compiled by a JDK: javac spells
//! `int.class` as `getstatic Integer.TYPE`, so a committed Java fixture would exercise
//! the M2.15 wrapper path instead of the class constant this slice implements. The probe
//! therefore hand-assembles every class-constant shape (a named class, a primitive name,
//! the two array descriptors) plus one `ldc_w` whose constant-pool index is above 255.
//!
//! The assertions cover the JDK shape: `String.class` and `int.class` are the cached class
//! objects the VM already owns, `int[].class` and `String[].class` are the cached array
//! class objects with their component chains, `getName` reports the dotted name or the
//! descriptor form, `isPrimitive`/`isArray` agree with the kind, and two executions of one
//! constant — and `Class.forName` of the same name — return the same `ObjRef`. A child
//! loader probe then shows resolution runs through the *current* class's loader: its own
//! `ChildOwner` literal is the child identity even though a bootstrap class of the same
//! name exists, while a library class still delegates to the bootstrap identity. The
//! documented errors are asserted too: a missing class, a malformed array descriptor and
//! an unsupported constant kind.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassRef, ClassSource, LoaderId, ObjRef, PrimitiveKind, Shared, ThreadState, Value,
    VmError, instance, object, reflection,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

const CLASS_NAME: &str = "()Ljava/lang/String;";
const CLASS_IS_PRIMITIVE: &str = "()Z";
const CLASS_IS_ARRAY: &str = "()Z";
const CLASS_COMPONENT_TYPE: &str = "()Ljava/lang/Class;";
const CLASS_SUPERCLASS: &str = "()Ljava/lang/Class;";
const CLASS_FOR_NAME: &str = "(Ljava/lang/String;)Ljava/lang/Class;";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn static_method(
    builder: &mut ClassBuilder,
    name: &str,
    descriptor: &str,
    max_stack: u16,
    max_locals: u16,
    body: Assembler,
) {
    builder.method(
        ACC_PUBLIC_STATIC,
        name,
        descriptor,
        Some((max_stack, max_locals, body.finish().unwrap())),
    );
}

/// `ClassLiteralProbe`: one static method per class-constant shape the slice claims.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ClassLiteralProbe", "java/lang/Object", 61);
    let class_get_name = builder.method_ref("java/lang/Class", "getName", CLASS_NAME);

    // The class constants the one-byte `ldc` addresses. Every one is added before the
    // padding below, so its pool index stays below 256 and the assembler picks `ldc`.
    let string_class = builder.class("java/lang/String");
    let int_class = builder.class("int");
    let int_array_class = builder.class("[I");
    let string_array_class = builder.class("[Ljava/lang/String;");
    let missing_class = builder.class("MissingClass");
    let malformed_unknown_element = builder.class("[Q");
    let malformed_void_element = builder.class("[V");
    let malformed_empty_component = builder.class("[L;");
    let hello = builder.string("hello");

    // Pad the constant pool so the next class constant needs the two-byte `ldc_w` index.
    for index in 0..300 {
        builder.utf8(&format!("padding{index}"));
    }
    let wide_class = builder.class("java/lang/Integer");
    assert!(
        wide_class >= 256,
        "the ldc_w fixture needs a constant-pool index above 255, found {wide_class}"
    );
    // A `CONSTANT_Long` is the unsupported kind the decode test uses. It is never pushed:
    // the instruction is rejected while decoding.
    let long_constant = builder.long(1);

    // static Class<?> stringClass() { return String.class; }
    let mut string_literal = Assembler::new();
    string_literal.ldc(string_class);
    string_literal.areturn();
    static_method(
        &mut builder,
        "stringClass",
        "()Ljava/lang/Class;",
        1,
        0,
        string_literal,
    );

    // static Class<?> intClass() { return int.class; }
    let mut int_literal = Assembler::new();
    int_literal.ldc(int_class);
    int_literal.areturn();
    static_method(
        &mut builder,
        "intClass",
        "()Ljava/lang/Class;",
        1,
        0,
        int_literal,
    );

    // static Class<?> intArrayClass() { return int[].class; }
    let mut int_array_literal = Assembler::new();
    int_array_literal.ldc(int_array_class);
    int_array_literal.areturn();
    static_method(
        &mut builder,
        "intArrayClass",
        "()Ljava/lang/Class;",
        1,
        0,
        int_array_literal,
    );

    // static Class<?> stringArrayClass() { return String[].class; }
    let mut string_array_literal = Assembler::new();
    string_array_literal.ldc(string_array_class);
    string_array_literal.areturn();
    static_method(
        &mut builder,
        "stringArrayClass",
        "()Ljava/lang/Class;",
        1,
        0,
        string_array_literal,
    );

    // static String className() { return int[].class.getName(); }
    let mut class_name = Assembler::new();
    class_name.ldc(int_array_class);
    class_name.invokevirtual(class_get_name);
    class_name.areturn();
    static_method(
        &mut builder,
        "className",
        "()Ljava/lang/String;",
        1,
        0,
        class_name,
    );

    // static boolean same() { return int.class == int.class; }
    let mut same = Assembler::new();
    let mismatch = same.label();
    same.ldc(int_class);
    same.ldc(int_class);
    same.if_acmp(0xa6, mismatch); // if_acmpne
    same.iconst(1);
    same.ireturn();
    same.place(mismatch);
    same.iconst(0);
    same.ireturn();
    static_method(&mut builder, "same", "()Z", 2, 0, same);

    // static Class<?> wideIntegerClass() { return Integer.class; }
    let mut wide_integer = Assembler::new();
    wide_integer.ldc_w(wide_class);
    wide_integer.areturn();
    static_method(
        &mut builder,
        "wideIntegerClass",
        "()Ljava/lang/Class;",
        1,
        0,
        wide_integer,
    );

    // static Class<?> missingClass() { return ldc MissingClass; }
    let mut missing = Assembler::new();
    missing.ldc(missing_class);
    missing.areturn();
    static_method(
        &mut builder,
        "missingClass",
        "()Ljava/lang/Class;",
        1,
        0,
        missing,
    );

    // static Class<?> malformedArray() { return ldc [Q; }
    let mut malformed = Assembler::new();
    malformed.ldc(malformed_unknown_element);
    malformed.areturn();
    static_method(
        &mut builder,
        "malformedArray",
        "()Ljava/lang/Class;",
        1,
        0,
        malformed,
    );

    // static Class<?> malformedVoidArray() { return ldc [V; }
    let mut malformed_void = Assembler::new();
    malformed_void.ldc(malformed_void_element);
    malformed_void.areturn();
    static_method(
        &mut builder,
        "malformedVoidArray",
        "()Ljava/lang/Class;",
        1,
        0,
        malformed_void,
    );

    // static Class<?> malformedEmptyArray() { return ldc [L;; }
    let mut malformed_empty = Assembler::new();
    malformed_empty.ldc(malformed_empty_component);
    malformed_empty.areturn();
    static_method(
        &mut builder,
        "malformedEmptyArray",
        "()Ljava/lang/Class;",
        1,
        0,
        malformed_empty,
    );

    // static void unsupportedLong() { ldc_w <CONSTANT_Long>; }
    let mut unsupported_long = Assembler::new();
    unsupported_long.ldc_w(long_constant);
    unsupported_long.return_();
    static_method(
        &mut builder,
        "unsupportedLong",
        "()V",
        2,
        0,
        unsupported_long,
    );

    // static String stringValue() { return "hello"; }
    let mut string_value = Assembler::new();
    string_value.ldc(hello);
    string_value.areturn();
    static_method(
        &mut builder,
        "stringValue",
        "()Ljava/lang/String;",
        1,
        0,
        string_value,
    );

    builder.build()
}

/// `ChildOwner`: the class the child-loader probe names by class constant.
fn child_owner_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ChildOwner", "java/lang/Object", 61);
    builder.build()
}

/// `ChildLiteralProbe`: the same literals, but defined in a child loader.
fn child_probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ChildLiteralProbe", "java/lang/Object", 61);
    let owner_class = builder.class("ChildOwner");
    let owner_array_class = builder.class("[LChildOwner;");
    let string_class = builder.class("java/lang/String");

    // static Class<?> ownerClass() { return ChildOwner.class; }
    let mut owner = Assembler::new();
    owner.ldc(owner_class);
    owner.areturn();
    static_method(
        &mut builder,
        "ownerClass",
        "()Ljava/lang/Class;",
        1,
        0,
        owner,
    );

    // static Class<?> ownerArrayClass() { return ChildOwner[].class; }
    let mut owner_array = Assembler::new();
    owner_array.ldc(owner_array_class);
    owner_array.areturn();
    static_method(
        &mut builder,
        "ownerArrayClass",
        "()Ljava/lang/Class;",
        1,
        0,
        owner_array,
    );

    // static Class<?> stringClass() { return String.class; }
    let mut string_literal = Assembler::new();
    string_literal.ldc(string_class);
    string_literal.areturn();
    static_method(
        &mut builder,
        "stringClass",
        "()Ljava/lang/Class;",
        1,
        0,
        string_literal,
    );

    builder.build()
}

/// A class source over in-memory bytes, so the child loader needs no fixture directory.
#[derive(Debug)]
struct MapSource {
    entries: HashMap<String, Vec<u8>>,
}

impl MapSource {
    fn new(entries: Vec<(&str, Vec<u8>)>) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(name, bytes)| (name.to_owned(), bytes))
                .collect(),
        }
    }
}

impl ClassSource for MapSource {
    fn read(&self, binary_name: &str) -> Result<Option<Vec<u8>>, VmError> {
        Ok(self.entries.get(binary_name).cloned())
    }
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
    class_class: ClassId,
    class_object: ObjRef,
    object_object: ObjRef,
    string_class_object: ObjRef,
    int_primitive: ObjRef,
    int_array_object: ObjRef,
    string_array_object: ObjRef,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    reflection::register_natives(&shared);

    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "ClassLiteralProbe", &probe_bytes())
        .expect("ClassLiteralProbe defines");

    let load = |name: &str| {
        shared
            .load_library_class(name)
            .unwrap_or_else(|error| panic!("{name} loads: {error}"))
    };
    let class_class = load("java/lang/Class");
    let class_object = shared
        .class_object(class_class)
        .expect("Class class object");
    let object_object = shared
        .class_object(load("java/lang/Object"))
        .expect("Object class object");
    let string_class_object = shared
        .class_object(load("java/lang/String"))
        .expect("String class object");
    let int_primitive = shared.primitive_class(PrimitiveKind::Int);
    let int_array_object = shared
        .class_object_for_ref(&ClassRef::array(ClassRef::Primitive(PrimitiveKind::Int)))
        .expect("int[] class object");
    let string_array_object = shared
        .class_object_for_ref(&ClassRef::array(ClassRef::Named(load("java/lang/String"))))
        .expect("String[] class object");

    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        probe,
        class_class,
        class_object,
        object_object,
        string_class_object,
        int_primitive,
        int_array_object,
        string_array_object,
    }
}

fn call_static(
    fixture: &Fixture,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        args,
    )
}

fn call_class(
    fixture: &Fixture,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.class_class,
        name,
        descriptor,
        receiver,
        args,
    )
}

fn literal(fixture: &Fixture, method: &str) -> ObjRef {
    ref_value(
        call_static(fixture, method, "()Ljava/lang/Class;", Vec::new())
            .unwrap_or_else(|error| panic!("{method}: {error}")),
    )
}

fn name_of(fixture: &Fixture, class: ObjRef) -> String {
    string_value(
        &fixture.shared,
        call_class(fixture, class, "getName", CLASS_NAME, Vec::new())
            .expect("Class.getName succeeds"),
    )
}

fn component_of(fixture: &Fixture, class: ObjRef) -> ObjRef {
    ref_value(
        call_class(
            fixture,
            class,
            "getComponentType",
            CLASS_COMPONENT_TYPE,
            Vec::new(),
        )
        .expect("Class.getComponentType succeeds"),
    )
}

fn is_array(fixture: &Fixture, class: ObjRef) -> bool {
    bool_value(
        call_class(fixture, class, "isArray", CLASS_IS_ARRAY, Vec::new())
            .expect("Class.isArray succeeds"),
    )
}

fn is_primitive(fixture: &Fixture, class: ObjRef) -> bool {
    bool_value(
        call_class(
            fixture,
            class,
            "isPrimitive",
            CLASS_IS_PRIMITIVE,
            Vec::new(),
        )
        .expect("Class.isPrimitive succeeds"),
    )
}

fn superclass_of(fixture: &Fixture, class: ObjRef) -> ObjRef {
    ref_value(
        call_class(
            fixture,
            class,
            "getSuperclass",
            CLASS_SUPERCLASS,
            Vec::new(),
        )
        .expect("Class.getSuperclass succeeds"),
    )
}

fn for_name(fixture: &Fixture, name: &str) -> Result<ObjRef, InterpError> {
    let value = run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.class_class,
        "forName",
        CLASS_FOR_NAME,
        vec![Value::Ref(fixture.shared.intern_string(name))],
    )?;
    Ok(ref_value(value))
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
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

fn bool_value(value: Option<Value>) -> bool {
    match value {
        Some(Value::Int(value)) => value != 0,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn is_null(result: Result<Option<Value>, InterpError>) -> bool {
    match result.unwrap_or_else(|error| panic!("query failed: {error}")) {
        Some(Value::Ref(reference)) => reference.is_null(),
        other => panic!("expected a reference result, found {other:?}"),
    }
}

fn assert_missing_class(fixture: &Fixture, result: Result<Option<Value>, InterpError>, name: &str) {
    // A missing class literal is the JVM's `NoClassDefFoundError`, a catchable throwable;
    // `Class.forName` uses `ClassNotFoundException` instead.
    match result {
        Err(InterpError::Thrown(throwable)) => {
            let message = fixture
                .shared
                .throwable_message(throwable)
                .unwrap_or_default();
            assert!(
                message.contains(name),
                "the missing class is named, found {message:?}"
            );
        }
        other => panic!("expected a missing-class error for {name:?}, found {other:?}"),
    }
}

fn assert_native_error(result: Result<Option<Value>, InterpError>, fragment: &str) {
    match result {
        Err(InterpError::Vm(VmError::Native(ref detail))) => assert!(
            detail.contains(fragment),
            "expected a native error containing {fragment:?}, found {detail:?}"
        ),
        other => panic!("expected a native error containing {fragment:?}, found {other:?}"),
    }
}

fn assert_decode_error(result: Result<Option<Value>, InterpError>, fragment: &str) {
    match result {
        Err(InterpError::Decode(ref detail)) => assert!(
            detail.contains(fragment),
            "expected a decode error containing {fragment:?}, found {detail:?}"
        ),
        other => panic!("expected a decode error containing {fragment:?}, found {other:?}"),
    }
}

#[test]
fn class_literals_push_the_cached_class_objects() {
    let fixture = fixture();

    let string_class = literal(&fixture, "stringClass");
    assert_eq!(
        string_class, fixture.string_class_object,
        "String.class is the bootstrap java.lang.String class object"
    );
    assert_eq!(name_of(&fixture, string_class), "java.lang.String");
    assert!(!is_array(&fixture, string_class));
    assert!(!is_primitive(&fixture, string_class));
    assert_eq!(
        fixture.shared.get_class(string_class).unwrap(),
        fixture.class_object,
        "Object.getClass on a class object reports java.lang.Class"
    );

    let int_class = literal(&fixture, "intClass");
    assert_eq!(
        int_class, fixture.int_primitive,
        "int.class is the cached primitive class object"
    );
    assert_eq!(name_of(&fixture, int_class), "int");
    assert!(is_primitive(&fixture, int_class));
    assert!(!is_array(&fixture, int_class));
    assert!(
        is_null(call_class(
            &fixture,
            int_class,
            "getComponentType",
            CLASS_COMPONENT_TYPE,
            Vec::new(),
        )),
        "a primitive class has no component type"
    );
    assert!(
        is_null(call_class(
            &fixture,
            int_class,
            "getSuperclass",
            CLASS_SUPERCLASS,
            Vec::new(),
        )),
        "a primitive class has no superclass"
    );

    let int_array = literal(&fixture, "intArrayClass");
    assert_eq!(
        int_array, fixture.int_array_object,
        "int[].class is the cached array class object for the int component"
    );
    assert_eq!(name_of(&fixture, int_array), "[I");
    assert!(is_array(&fixture, int_array));
    assert!(!is_primitive(&fixture, int_array));
    assert_eq!(
        component_of(&fixture, int_array),
        fixture.int_primitive,
        "int[].class.getComponentType() is int.class"
    );
    assert_eq!(
        superclass_of(&fixture, int_array),
        fixture.object_object,
        "an array class extends java.lang.Object"
    );
    assert_eq!(
        fixture.shared.get_class(int_array).unwrap(),
        fixture.class_object,
        "Object.getClass on an array class object reports java.lang.Class"
    );

    let string_array = literal(&fixture, "stringArrayClass");
    assert_eq!(
        string_array, fixture.string_array_object,
        "String[].class is the cached array class object for the String component"
    );
    assert_eq!(name_of(&fixture, string_array), "[Ljava.lang.String;");
    assert!(is_array(&fixture, string_array));
    assert!(!is_primitive(&fixture, string_array));
    assert_eq!(
        component_of(&fixture, string_array),
        fixture.string_class_object,
        "String[].class.getComponentType() is String.class"
    );

    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(&fixture, "className", "()Ljava/lang/String;", Vec::new()).unwrap(),
        ),
        "[I",
        "int[].class.getName() from bytecode"
    );
    assert!(
        bool_value(call_static(&fixture, "same", "()Z", Vec::new()).unwrap()),
        "int.class == int.class is reference identity"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(&fixture, "stringValue", "()Ljava/lang/String;", Vec::new(),).unwrap(),
        ),
        "hello",
        "ldc of a String constant still works"
    );

    for reference in [string_class, int_class, int_array, string_array] {
        assert!(
            fixture.shared.roots(&[]).contains(&reference),
            "a cached class object is a root"
        );
    }
}

#[test]
fn class_literals_and_class_for_name_share_one_cache() {
    let fixture = fixture();

    let cases = [
        (
            "stringClass",
            "java.lang.String",
            fixture.string_class_object,
        ),
        ("intClass", "int", fixture.int_primitive),
        ("intArrayClass", "[I", fixture.int_array_object),
        (
            "stringArrayClass",
            "[Ljava.lang.String;",
            fixture.string_array_object,
        ),
    ];
    for (method, query, expected) in cases {
        let first = literal(&fixture, method);
        let second = literal(&fixture, method);
        assert_eq!(first, second, "{method} returns one object every time");
        assert_eq!(first, expected, "{method} is the class cache's object");
        assert_eq!(
            for_name(&fixture, query)
                .unwrap_or_else(|error| panic!("Class.forName({query}): {error}")),
            first,
            "Class.forName({query:?}) is reference-identical to the literal"
        );
    }

    // The class object is not re-created by repeated resolution: the four queries above
    // cover the named, primitive and array caches.
}

#[test]
fn ldc_w_resolves_a_class_constant_above_index_255() {
    let fixture = fixture();
    let wide = literal(&fixture, "wideIntegerClass");
    let integer = fixture
        .shared
        .class_object(
            fixture
                .shared
                .load_library_class("java/lang/Integer")
                .expect("java.lang.Integer loads"),
        )
        .expect("Integer class object");
    assert_eq!(
        wide, integer,
        "ldc_w pushed the java.lang.Integer class object"
    );
    assert_eq!(name_of(&fixture, wide), "java.lang.Integer");
    assert!(fixture.shared.roots(&[]).contains(&wide));
}

#[test]
fn missing_classes_and_malformed_descriptors_are_typed_errors() {
    let fixture = fixture();

    assert_missing_class(
        &fixture,
        call_static(&fixture, "missingClass", "()Ljava/lang/Class;", Vec::new()),
        "MissingClass",
    );
    assert_native_error(
        call_static(
            &fixture,
            "malformedArray",
            "()Ljava/lang/Class;",
            Vec::new(),
        ),
        "malformed array descriptor [Q",
    );
    assert_native_error(
        call_static(
            &fixture,
            "malformedVoidArray",
            "()Ljava/lang/Class;",
            Vec::new(),
        ),
        "malformed array descriptor [V",
    );
    assert_native_error(
        call_static(
            &fixture,
            "malformedEmptyArray",
            "()Ljava/lang/Class;",
            Vec::new(),
        ),
        "malformed array descriptor [L;",
    );
    assert_decode_error(
        call_static(&fixture, "unsupportedLong", "()V", Vec::new()),
        "unsupported Long constant",
    );
}

#[test]
fn class_literals_resolve_through_the_executing_loader() {
    let fixture = fixture();
    let shared = &fixture.shared;

    // Order matters: the class table returns an already-defined class from the loader
    // chain (child first, then bootstrap), so defining the bootstrap `ChildOwner` first
    // would make `load_class(child, ...)` hand back the bootstrap identity. Loading the
    // child class first makes the contrast explicit: two distinct identities for one
    // binary name.
    let child = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(MapSource::new(vec![
                ("ChildOwner", child_owner_bytes()),
                ("ChildLiteralProbe", child_probe_bytes()),
            ])),
        )
        .expect("the child loader is created");
    let child_owner = shared
        .load_class(child, "ChildOwner")
        .expect("the child defines ChildOwner");
    let child_probe = shared
        .load_class(child, "ChildLiteralProbe")
        .expect("the child defines ChildLiteralProbe");
    let bootstrap_owner = shared
        .define_class(LoaderId::BOOTSTRAP, "ChildOwner", &child_owner_bytes())
        .expect("the bootstrap defines its own ChildOwner");

    let child_owner_object = ref_value(
        run_static_method(
            shared,
            &fixture.thread,
            child_probe,
            "ownerClass",
            "()Ljava/lang/Class;",
            Vec::new(),
        )
        .expect("the child probe runs"),
    );
    assert_eq!(
        child_owner_object,
        shared
            .class_object(child_owner)
            .expect("the child class object"),
        "ldc resolves through the executing class's loader"
    );
    let bootstrap_owner_object = shared
        .class_object(bootstrap_owner)
        .expect("the bootstrap class object");
    assert_ne!(
        child_owner_object, bootstrap_owner_object,
        "a child loader's class keeps its own identity"
    );
    assert_eq!(
        name_of(&fixture, child_owner_object),
        "ChildOwner",
        "both identities share the binary name"
    );
    assert_eq!(
        name_of(&fixture, bootstrap_owner_object),
        "ChildOwner",
        "the bootstrap identity has the same name"
    );

    let child_array = ref_value(
        run_static_method(
            shared,
            &fixture.thread,
            child_probe,
            "ownerArrayClass",
            "()Ljava/lang/Class;",
            Vec::new(),
        )
        .expect("the child probe runs"),
    );
    let stored = shared
        .heap()
        .array_class(child_array)
        .expect("an array class object")
        .clone();
    assert_eq!(
        stored.component,
        ClassRef::Named(child_owner),
        "an array descriptor's named component resolves through the same loader"
    );
    assert_ne!(
        child_array,
        shared
            .class_object_for_ref(&ClassRef::array(ClassRef::Named(bootstrap_owner)))
            .expect("the bootstrap array class object"),
        "the child array class is distinct from the bootstrap array class"
    );

    // A library class is still delegated to the bootstrap loader, so `String.class` from
    // the child probe is the one and only bootstrap java.lang.String class object.
    let delegated_string = ref_value(
        run_static_method(
            shared,
            &fixture.thread,
            child_probe,
            "stringClass",
            "()Ljava/lang/Class;",
            Vec::new(),
        )
        .expect("the child probe runs"),
    );
    assert_eq!(
        delegated_string, fixture.string_class_object,
        "a library class literal delegates to the bootstrap loader"
    );
}
