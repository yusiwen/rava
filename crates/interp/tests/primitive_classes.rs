//! M2.15 acceptance: primitive `Class` identity and primitive member types.
//!
//! `Bean` is an ordinary class with a private `int value`, a private `String label`, a
//! `()V` and an `(I)V` constructor, int/reference accessors, a `void reset()` and a
//! static `sum(II)I`. `PrimitiveProbe` is a bytecode client, so the interpreter's
//! `getstatic`/`invokevirtual` path reaches the natives: `Class.forName` of every
//! primitive name, the primitive `Class` queries, each wrapper's `TYPE` field, a named
//! lookup whose `Class[]` holds a primitive class object, and a full reflective
//! set/get round trip through `int.class`.
//!
//! The documented boundaries are asserted too: an array class name is still a typed
//! error, a `void` class object never matches a parameter, and a static field other than
//! the nine `TYPE` fields is still absent. `Object.toString` on a primitive class object
//! keeps the M2.9 `java.lang.Class@hash` shape rather than the JDK's `class int`.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, reflect_invoke, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassRef, LoaderId, MemberKind, ObjRef, PrimitiveKind, Shared, ThreadState, Value,
    boxed, instance, object, reflection,
};

const ACC_PRIVATE: u16 = 0x0002;
const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;

/// `Modifier.PUBLIC | Modifier.ABSTRACT | Modifier.FINAL`, the JDK's primitive modifiers.
const PRIMITIVE_MODIFIERS: i32 = 0x0411;

const FIELD_GET_TYPE: &str = "()Ljava/lang/Class;";
const METHOD_GET_RETURN_TYPE: &str = "()Ljava/lang/Class;";
const PARAMETER_TYPES: &str = "()[Ljava/lang/Class;";
const MEMBER_GET_PARAMETER_COUNT: &str = "()I";
const FIELD_GET: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";
const METHOD_INVOKE: &str = "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;";
const CONSTRUCTOR_NEW_INSTANCE: &str = "([Ljava/lang/Object;)Ljava/lang/Object;";
const DECLARED_METHOD: &str = "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;";
const DECLARED_FIELD: &str = "(Ljava/lang/String;)Ljava/lang/reflect/Field;";
const DECLARED_CONSTRUCTOR: &str = "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;";
const DECLARED_FIELDS: &str = "()[Ljava/lang/reflect/Field;";

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

/// `Bean`: the fixture class whose primitive members the tests reflect over.
fn bean_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Bean", "java/lang/Object", 61);
    builder.field(ACC_PRIVATE, "value", "I");
    builder.field(ACC_PRIVATE, "label", "Ljava/lang/String;");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let value_ref = builder.field_ref("Bean", "value", "I");
    let label_ref = builder.field_ref("Bean", "label", "Ljava/lang/String;");

    // Bean() { super(); }
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "()V",
        Some((1, 1, init.finish().unwrap())),
    );

    // Bean(int value) { super(); this.value = value; }
    let mut init_int = Assembler::new();
    init_int.aload(0);
    init_int.invokespecial(object_init);
    init_int.aload(0);
    init_int.iload(1);
    init_int.putfield(value_ref);
    init_int.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "(I)V",
        Some((2, 2, init_int.finish().unwrap())),
    );

    // int getValue() { return this.value; }
    let mut get_value = Assembler::new();
    get_value.aload(0);
    get_value.getfield(value_ref);
    get_value.ireturn();
    builder.method(
        ACC_PUBLIC,
        "getValue",
        "()I",
        Some((1, 1, get_value.finish().unwrap())),
    );

    // void setValue(int value) { this.value = value; }
    let mut set_value = Assembler::new();
    set_value.aload(0);
    set_value.iload(1);
    set_value.putfield(value_ref);
    set_value.return_();
    builder.method(
        ACC_PUBLIC,
        "setValue",
        "(I)V",
        Some((2, 2, set_value.finish().unwrap())),
    );

    // String getLabel() { return this.label; }
    let mut get_label = Assembler::new();
    get_label.aload(0);
    get_label.getfield(label_ref);
    get_label.areturn();
    builder.method(
        ACC_PUBLIC,
        "getLabel",
        "()Ljava/lang/String;",
        Some((1, 1, get_label.finish().unwrap())),
    );

    // void setLabel(String label) { this.label = label; }
    let mut set_label = Assembler::new();
    set_label.aload(0);
    set_label.aload(1);
    set_label.putfield(label_ref);
    set_label.return_();
    builder.method(
        ACC_PUBLIC,
        "setLabel",
        "(Ljava/lang/String;)V",
        Some((2, 2, set_label.finish().unwrap())),
    );

    // void mixed(int value, String label) { this.value = value; this.label = label; }
    // It proves a primitive and a reference parameter build one exact descriptor in order.
    let mut mixed = Assembler::new();
    mixed.aload(0);
    mixed.iload(1);
    mixed.putfield(value_ref);
    mixed.aload(0);
    mixed.aload(2);
    mixed.putfield(label_ref);
    mixed.return_();
    builder.method(
        ACC_PUBLIC,
        "mixed",
        "(ILjava/lang/String;)V",
        Some((2, 3, mixed.finish().unwrap())),
    );

    // void reset() { super(); this.value = 0; } is needlessly indirect; a plain void body
    // is enough for `getReturnType` to see V.
    let mut reset = Assembler::new();
    reset.return_();
    builder.method(
        ACC_PUBLIC,
        "reset",
        "()V",
        Some((0, 1, reset.finish().unwrap())),
    );

    // static int sum(int left, int right) { return left + right; }
    let mut sum = Assembler::new();
    sum.iload(0);
    sum.iload(1);
    sum.iadd();
    sum.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "sum",
        "(II)I",
        Some((2, 2, sum.finish().unwrap())),
    );

    builder.build()
}

/// `PrimitiveProbe`: a bytecode client for the primitive `Class` surface.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("PrimitiveProbe", "java/lang/Object", 61);
    let class_class = builder.class("java/lang/Class");
    let object_class = builder.class("java/lang/Object");
    let object_get_class =
        builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let class_get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");
    let class_for_name = builder.method_ref(
        "java/lang/Class",
        "forName",
        "(Ljava/lang/String;)Ljava/lang/Class;",
    );
    let class_get_modifiers = builder.method_ref("java/lang/Class", "getModifiers", "()I");
    let class_is_primitive = builder.method_ref("java/lang/Class", "isPrimitive", "()Z");
    let class_is_array = builder.method_ref("java/lang/Class", "isArray", "()Z");
    let class_get_superclass =
        builder.method_ref("java/lang/Class", "getSuperclass", "()Ljava/lang/Class;");
    let class_get_component_type =
        builder.method_ref("java/lang/Class", "getComponentType", "()Ljava/lang/Class;");
    let get_declared_fields =
        builder.method_ref("java/lang/Class", "getDeclaredFields", DECLARED_FIELDS);
    let get_declared_method =
        builder.method_ref("java/lang/Class", "getDeclaredMethod", DECLARED_METHOD);
    let method_invoke = builder.method_ref("java/lang/reflect/Method", "invoke", METHOD_INVOKE);
    let integer_value_of =
        builder.method_ref("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;");
    let integer_int_value = builder.method_ref("java/lang/Integer", "intValue", "()I");

    // static Class forNameClass(String name) { return Class.forName(name); }
    let mut for_name = Assembler::new();
    for_name.aload(0);
    for_name.op(0xb8, &class_for_name.to_be_bytes()); // invokestatic
    for_name.areturn();
    static_method(
        &mut builder,
        "forNameClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        1,
        1,
        for_name,
    );

    // static String name(Class c) { return c.getName(); }
    let mut name = Assembler::new();
    name.aload(0);
    name.invokevirtual(class_get_name);
    name.areturn();
    static_method(
        &mut builder,
        "name",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        1,
        1,
        name,
    );

    // static int modifiers(Class c) { return c.getModifiers(); }
    let mut modifiers = Assembler::new();
    modifiers.aload(0);
    modifiers.invokevirtual(class_get_modifiers);
    modifiers.ireturn();
    static_method(
        &mut builder,
        "modifiers",
        "(Ljava/lang/Class;)I",
        1,
        1,
        modifiers,
    );

    // static boolean isPrimitive(Class c) { return c.isPrimitive(); }
    let mut is_primitive = Assembler::new();
    is_primitive.aload(0);
    is_primitive.invokevirtual(class_is_primitive);
    is_primitive.ireturn();
    static_method(
        &mut builder,
        "isPrimitive",
        "(Ljava/lang/Class;)Z",
        1,
        1,
        is_primitive,
    );

    // static boolean isArray(Class c) { return c.isArray(); }
    let mut is_array = Assembler::new();
    is_array.aload(0);
    is_array.invokevirtual(class_is_array);
    is_array.ireturn();
    static_method(
        &mut builder,
        "isArray",
        "(Ljava/lang/Class;)Z",
        1,
        1,
        is_array,
    );

    // static Class superclass(Class c) { return c.getSuperclass(); }
    let mut superclass = Assembler::new();
    superclass.aload(0);
    superclass.invokevirtual(class_get_superclass);
    superclass.areturn();
    static_method(
        &mut builder,
        "superclass",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        1,
        1,
        superclass,
    );

    // static Class componentType(Class c) { return c.getComponentType(); }
    let mut component_type = Assembler::new();
    component_type.aload(0);
    component_type.invokevirtual(class_get_component_type);
    component_type.areturn();
    static_method(
        &mut builder,
        "componentType",
        "(Ljava/lang/Class;)Ljava/lang/Class;",
        1,
        1,
        component_type,
    );

    // static String runtimeName(Class c) { return c.getClass().getName(); }
    let mut runtime_name = Assembler::new();
    runtime_name.aload(0);
    runtime_name.invokevirtual(object_get_class);
    runtime_name.invokevirtual(class_get_name);
    runtime_name.areturn();
    static_method(
        &mut builder,
        "runtimeName",
        "(Ljava/lang/Class;)Ljava/lang/String;",
        2,
        1,
        runtime_name,
    );

    // static int declaredFieldCount(Class c) { return c.getDeclaredFields().length; }
    let mut field_count = Assembler::new();
    field_count.aload(0);
    field_count.invokevirtual(get_declared_fields);
    field_count.arraylength();
    field_count.ireturn();
    static_method(
        &mut builder,
        "declaredFieldCount",
        "(Ljava/lang/Class;)I",
        1,
        1,
        field_count,
    );

    // static Method setValueMethod(Class c) {
    //     return c.getDeclaredMethod("setValue", new Class[] { Class.forName("int") });
    // }
    let mut set_value_method = Assembler::new();
    set_value_method.aload(0);
    set_value_method.ldc(builder.string("setValue"));
    set_value_method.iconst(1);
    set_value_method.anewarray(class_class);
    set_value_method.dup();
    set_value_method.iconst(0);
    set_value_method.ldc(builder.string("int"));
    set_value_method.op(0xb8, &class_for_name.to_be_bytes()); // invokestatic
    set_value_method.aastore();
    set_value_method.invokevirtual(get_declared_method);
    set_value_method.areturn();
    static_method(
        &mut builder,
        "setValueMethod",
        "(Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        7,
        1,
        set_value_method,
    );

    // static int setAndRead(Object bean, int value) reflectively sets then reads `value`.
    let mut set_and_read = Assembler::new();
    // Integer box = Integer.valueOf(value);
    set_and_read.aload(1);
    set_and_read.op(0xb8, &integer_value_of.to_be_bytes()); // invokestatic
    set_and_read.astore(2);
    // Object[] arguments = new Object[] { box };
    set_and_read.iconst(1);
    set_and_read.anewarray(object_class);
    set_and_read.dup();
    set_and_read.iconst(0);
    set_and_read.aload(2);
    set_and_read.aastore();
    set_and_read.astore(3);
    // bean.getClass().getDeclaredMethod("setValue", new Class[] { int.class })
    //     .invoke(bean, arguments);
    set_and_read.aload(0);
    set_and_read.aload(0);
    set_and_read.invokevirtual(object_get_class);
    set_and_read.ldc(builder.string("setValue"));
    set_and_read.iconst(1);
    set_and_read.anewarray(class_class);
    set_and_read.dup();
    set_and_read.iconst(0);
    set_and_read.ldc(builder.string("int"));
    set_and_read.op(0xb8, &class_for_name.to_be_bytes()); // invokestatic
    set_and_read.aastore();
    set_and_read.invokevirtual(get_declared_method);
    set_and_read.aload(0);
    set_and_read.aload(3);
    set_and_read.invokevirtual(method_invoke);
    set_and_read.astore(4);
    // Integer result = (Integer) bean.getClass()
    //     .getDeclaredMethod("getValue", new Class[0]).invoke(bean, null);
    set_and_read.aload(0);
    set_and_read.invokevirtual(object_get_class);
    set_and_read.ldc(builder.string("getValue"));
    set_and_read.iconst(0);
    set_and_read.anewarray(class_class);
    set_and_read.invokevirtual(get_declared_method);
    set_and_read.aload(0);
    set_and_read.aconst_null();
    set_and_read.invokevirtual(method_invoke);
    // return result.intValue();
    set_and_read.invokevirtual(integer_int_value);
    set_and_read.ireturn();
    static_method(
        &mut builder,
        "setAndRead",
        "(Ljava/lang/Object;I)I",
        8,
        5,
        set_and_read,
    );

    // static Class <kind>Type() { return <Wrapper>.TYPE; }
    for (name, owner) in [
        ("booleanType", "java/lang/Boolean"),
        ("byteType", "java/lang/Byte"),
        ("charType", "java/lang/Character"),
        ("shortType", "java/lang/Short"),
        ("intType", "java/lang/Integer"),
        ("longType", "java/lang/Long"),
        ("floatType", "java/lang/Float"),
        ("doubleType", "java/lang/Double"),
        ("voidType", "java/lang/Void"),
    ] {
        let field = builder.field_ref(owner, "TYPE", "Ljava/lang/Class;");
        let mut body = Assembler::new();
        body.getstatic(field);
        body.areturn();
        static_method(&mut builder, name, "()Ljava/lang/Class;", 1, 0, body);
    }

    // static String intTypeName() { return Integer.TYPE.getName(); }
    let integer_type = builder.field_ref("java/lang/Integer", "TYPE", "Ljava/lang/Class;");
    let mut int_type_name = Assembler::new();
    int_type_name.getstatic(integer_type);
    int_type_name.invokevirtual(class_get_name);
    int_type_name.areturn();
    static_method(
        &mut builder,
        "intTypeName",
        "()Ljava/lang/String;",
        1,
        0,
        int_type_name,
    );

    // static String voidTypeName() { return Void.TYPE.getName(); }
    let void_type = builder.field_ref("java/lang/Void", "TYPE", "Ljava/lang/Class;");
    let mut void_type_name = Assembler::new();
    void_type_name.getstatic(void_type);
    void_type_name.invokevirtual(class_get_name);
    void_type_name.areturn();
    static_method(
        &mut builder,
        "voidTypeName",
        "()Ljava/lang/String;",
        1,
        0,
        void_type_name,
    );

    // static int maxValue() { return Integer.MAX_VALUE; } -- a declared static final whose
    // value comes from the ConstantValue attribute, not from an initializer Code.
    let max_value = builder.field_ref("java/lang/Integer", "MAX_VALUE", "I");
    let mut max_value_body = Assembler::new();
    max_value_body.getstatic(max_value);
    max_value_body.ireturn();
    static_method(&mut builder, "maxValue", "()I", 1, 0, max_value_body);

    builder.build()
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    bean: ClassId,
    bean_object: ObjRef,
    probe: ClassId,
    class_class: ClassId,
    class_object: ObjRef,
    object_class: ClassId,
    field_class: ClassId,
    method_class: ClassId,
    constructor_class: ClassId,
    string_object: ObjRef,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    boxed::register_natives(&shared);
    reflection::register_natives(&shared);
    reflect_invoke::register_natives(&shared);

    let bean = shared
        .define_class(LoaderId::BOOTSTRAP, "Bean", &bean_bytes())
        .expect("Bean defines");
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "PrimitiveProbe", &probe_bytes())
        .expect("PrimitiveProbe defines");

    let load = |name: &str| {
        shared
            .load_library_class(name)
            .unwrap_or_else(|error| panic!("{name} loads: {error}"))
    };
    let class_class = load("java/lang/Class");
    let class_object = shared
        .class_object(class_class)
        .expect("Class class object");
    let object_class = load("java/lang/Object");
    let field_class = load("java/lang/reflect/Field");
    let method_class = load("java/lang/reflect/Method");
    let constructor_class = load("java/lang/reflect/Constructor");
    let string_object = shared
        .class_object(load("java/lang/String"))
        .expect("String class object");
    let bean_object = shared.class_object(bean).expect("Bean class object");

    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        bean,
        bean_object,
        probe,
        class_class,
        class_object,
        object_class,
        field_class,
        method_class,
        constructor_class,
        string_object,
    }
}

fn primitive(fixture: &Fixture, kind: PrimitiveKind) -> ObjRef {
    fixture.shared.primitive_class(kind)
}

fn reference_array(shared: &Shared, values: &[ObjRef]) -> ObjRef {
    let component = ClassRef::Named(
        shared
            .load_library_class("java/lang/Object")
            .expect("Object loads"),
    );
    let mut heap = shared.heap();
    let array = heap.allocate_array(component, values.len());
    {
        let array = heap.array_mut(array).expect("a reference array");
        for (slot, value) in array.elements.iter_mut().zip(values) {
            *slot = Value::Ref(*value);
        }
    }
    array
}

fn array_elements(shared: &Shared, reference: ObjRef) -> Vec<Value> {
    shared
        .heap()
        .array(reference)
        .expect("a heap array")
        .elements
        .clone()
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

fn call_member(
    fixture: &Fixture,
    owner: ClassId,
    receiver: ObjRef,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        owner,
        name,
        descriptor,
        receiver,
        args,
    )
}

fn member(fixture: &Fixture, kind: MemberKind, name: &str, descriptor: &str) -> ObjRef {
    fixture
        .shared
        .reflect_member(fixture.bean, kind, name, descriptor)
        .expect("the member object allocates")
}

fn name_value(fixture: &Fixture, name: &str) -> Value {
    Value::Ref(fixture.shared.intern_string(name))
}

fn params(fixture: &Fixture, classes: &[ObjRef]) -> Value {
    Value::Ref(reference_array(&fixture.shared, classes))
}

fn declared_method(
    fixture: &Fixture,
    class: ObjRef,
    name: &str,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    call_class(
        fixture,
        class,
        "getDeclaredMethod",
        DECLARED_METHOD,
        vec![name_value(fixture, name), params(fixture, parameters)],
    )
}

fn declared_constructor(
    fixture: &Fixture,
    class: ObjRef,
    parameters: &[ObjRef],
) -> Result<Option<Value>, InterpError> {
    call_class(
        fixture,
        class,
        "getDeclaredConstructor",
        DECLARED_CONSTRUCTOR,
        vec![params(fixture, parameters)],
    )
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

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

/// Assert that a lookup raised `exception` with exactly `message`.
fn assert_lookup_miss(
    fixture: &Fixture,
    result: Result<Option<Value>, InterpError>,
    exception: &str,
    message: &str,
) {
    match result {
        Err(InterpError::Thrown(throwable)) => {
            let class = fixture
                .shared
                .heap()
                .object_class(throwable)
                .expect("the throwable has a class");
            assert_eq!(
                fixture.shared.entry(class).expect("a loaded class").name,
                exception,
                "the lookup miss threw the wrong class"
            );
            assert_eq!(
                fixture.shared.throwable_message(throwable).as_deref(),
                Some(message),
                "the lookup miss carried the wrong message"
            );
        }
        other => panic!("expected a thrown {exception} ({message}), found {other:?}"),
    }
}

/// The member identity behind a reference returned by a lookup.
fn member_of(fixture: &Fixture, reference: ObjRef) -> rava_vm::ReflectMember {
    fixture
        .shared
        .heap()
        .reflect(reference)
        .expect("a reflected member")
        .clone()
}

#[test]
fn every_primitive_for_name_is_one_cached_class_object_with_jdk_flags() {
    let fixture = fixture();

    for kind in PrimitiveKind::ALL {
        let first = ref_value(
            call_static(
                &fixture,
                "forNameClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                vec![name_value(&fixture, kind.name())],
            )
            .unwrap(),
        );
        assert_eq!(
            first,
            primitive(&fixture, kind),
            "Class.forName({:?}) is the cached primitive class object",
            kind.name()
        );
        assert_eq!(
            fixture.shared.heap().primitive_class(first).unwrap(),
            kind,
            "the heap object holds the primitive kind"
        );
        assert!(
            fixture.shared.heap().class(first).is_err(),
            "a primitive class object is not a loaded-class identity"
        );

        let again = ref_value(
            call_static(
                &fixture,
                "forNameClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                vec![name_value(&fixture, kind.name())],
            )
            .unwrap(),
        );
        assert_eq!(again, first, "a second forName returns the same object");

        let name = string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "name",
                "(Ljava/lang/Class;)Ljava/lang/String;",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert_eq!(name, kind.name(), "Class.getName");

        let modifiers = int_value(
            call_static(
                &fixture,
                "modifiers",
                "(Ljava/lang/Class;)I",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert_eq!(
            modifiers, PRIMITIVE_MODIFIERS,
            "primitive modifiers for {kind}"
        );

        let is_primitive = int_value(
            call_static(
                &fixture,
                "isPrimitive",
                "(Ljava/lang/Class;)Z",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert_eq!(is_primitive, 1, "isPrimitive for {kind}");

        let is_array = int_value(
            call_static(
                &fixture,
                "isArray",
                "(Ljava/lang/Class;)Z",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert_eq!(is_array, 0, "isArray for {kind}");

        let superclass = ref_value(
            call_static(
                &fixture,
                "superclass",
                "(Ljava/lang/Class;)Ljava/lang/Class;",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert!(superclass.is_null(), "getSuperclass for {kind} is null");

        let component = ref_value(
            call_static(
                &fixture,
                "componentType",
                "(Ljava/lang/Class;)Ljava/lang/Class;",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert!(component.is_null(), "getComponentType for {kind} is null");

        let runtime_name = string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "runtimeName",
                "(Ljava/lang/Class;)Ljava/lang/String;",
                vec![Value::Ref(first)],
            )
            .unwrap(),
        );
        assert_eq!(
            runtime_name, "java.lang.Class",
            "getClass() on a primitive class object reports java.lang.Class"
        );

        assert_eq!(
            fixture.shared.get_class(first).unwrap(),
            fixture.class_object,
            "Shared::get_class agrees with the getClass native"
        );

        let roots = fixture.shared.roots(&[]);
        assert!(
            roots.contains(&first),
            "the primitive class object is a root"
        );
    }

    // The cache is one object per kind: two kinds never share an object.
    let int = primitive(&fixture, PrimitiveKind::Int);
    let long = primitive(&fixture, PrimitiveKind::Long);
    assert_ne!(int, long);

    // A named class still reports non-primitive and its class-file modifiers.
    let string = ref_value(
        call_static(
            &fixture,
            "forNameClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            vec![name_value(&fixture, "java.lang.String")],
        )
        .unwrap(),
    );
    assert_eq!(
        int_value(
            call_static(
                &fixture,
                "isPrimitive",
                "(Ljava/lang/Class;)Z",
                vec![Value::Ref(string)]
            )
            .unwrap()
        ),
        0
    );

    // M2.16 turns an array name into the cached array class object, not an error.
    let int_array = ref_value(
        call_static(
            &fixture,
            "forNameClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            vec![name_value(&fixture, "[I")],
        )
        .unwrap(),
    );
    assert_eq!(
        int_array,
        fixture
            .shared
            .array_class(ClassRef::Primitive(PrimitiveKind::Int))
            .unwrap(),
        "Class.forName(\"[I\") is the cached int[] class object"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(
                &fixture,
                "name",
                "(Ljava/lang/Class;)Ljava/lang/String;",
                vec![Value::Ref(int_array)],
            )
            .unwrap()
        ),
        "[I"
    );
}

#[test]
fn wrapper_type_fields_are_the_primitive_class_objects() {
    let fixture = fixture();

    for (method, descriptor, kind) in [
        ("booleanType", "()Ljava/lang/Class;", PrimitiveKind::Boolean),
        ("byteType", "()Ljava/lang/Class;", PrimitiveKind::Byte),
        ("charType", "()Ljava/lang/Class;", PrimitiveKind::Char),
        ("shortType", "()Ljava/lang/Class;", PrimitiveKind::Short),
        ("intType", "()Ljava/lang/Class;", PrimitiveKind::Int),
        ("longType", "()Ljava/lang/Class;", PrimitiveKind::Long),
        ("floatType", "()Ljava/lang/Class;", PrimitiveKind::Float),
        ("doubleType", "()Ljava/lang/Class;", PrimitiveKind::Double),
        ("voidType", "()Ljava/lang/Class;", PrimitiveKind::Void),
    ] {
        let value = ref_value(call_static(&fixture, method, descriptor, Vec::new()).unwrap());
        assert_eq!(
            value,
            primitive(&fixture, kind),
            "{method} is the cached {} class object",
            kind.name()
        );
    }

    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(&fixture, "intTypeName", "()Ljava/lang/String;", Vec::new()).unwrap()
        ),
        "int"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            call_static(&fixture, "voidTypeName", "()Ljava/lang/String;", Vec::new()).unwrap()
        ),
        "void"
    );

    // `Integer.MAX_VALUE` is a `static final int` with a `ConstantValue` attribute, so it
    // reads as the constant even though the C1 stub carries no initializer `Code`.
    assert_eq!(
        int_value(call_static(&fixture, "maxValue", "()I", Vec::new()).unwrap()),
        i32::MAX,
        "a declared static final reads its ConstantValue"
    );
}

#[test]
fn primitive_member_types_are_primitive_class_objects() {
    let fixture = fixture();
    let int = primitive(&fixture, PrimitiveKind::Int);
    let void = primitive(&fixture, PrimitiveKind::Void);

    let field = member(&fixture, MemberKind::Field, "value", "I");
    let field_type = ref_value(
        call_member(
            &fixture,
            fixture.field_class,
            field,
            "getType",
            FIELD_GET_TYPE,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(field_type, int, "Field.getType of value:I is int");

    let label = member(&fixture, MemberKind::Field, "label", "Ljava/lang/String;");
    let label_type = ref_value(
        call_member(
            &fixture,
            fixture.field_class,
            label,
            "getType",
            FIELD_GET_TYPE,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        label_type, fixture.string_object,
        "a reference field type is still the named class object"
    );

    let set_value = member(&fixture, MemberKind::Method, "setValue", "(I)V");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                set_value,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new()
            )
            .unwrap()
        ),
        void,
        "Method.getReturnType of (I)V is void"
    );
    let set_value_parameters = ref_value(
        call_member(
            &fixture,
            fixture.method_class,
            set_value,
            "getParameterTypes",
            PARAMETER_TYPES,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        array_elements(&fixture.shared, set_value_parameters),
        [Value::Ref(int)],
        "Method.getParameterTypes of (I)V is [int]"
    );

    let get_value = member(&fixture, MemberKind::Method, "getValue", "()I");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                get_value,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new()
            )
            .unwrap()
        ),
        int,
        "Method.getReturnType of ()I is int"
    );
    let get_value_parameters = ref_value(
        call_member(
            &fixture,
            fixture.method_class,
            get_value,
            "getParameterTypes",
            PARAMETER_TYPES,
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(
        array_elements(&fixture.shared, get_value_parameters).is_empty(),
        "an empty parameter list is an empty array"
    );

    let set_label = member(
        &fixture,
        MemberKind::Method,
        "setLabel",
        "(Ljava/lang/String;)V",
    );
    let set_label_parameters = ref_value(
        call_member(
            &fixture,
            fixture.method_class,
            set_label,
            "getParameterTypes",
            PARAMETER_TYPES,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        array_elements(&fixture.shared, set_label_parameters),
        [Value::Ref(fixture.string_object)],
        "a reference parameter type is the named class object"
    );
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                set_label,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new()
            )
            .unwrap()
        ),
        void
    );

    let sum = member(&fixture, MemberKind::Method, "sum", "(II)I");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                sum,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new()
            )
            .unwrap()
        ),
        int
    );
    let sum_parameters = ref_value(
        call_member(
            &fixture,
            fixture.method_class,
            sum,
            "getParameterTypes",
            PARAMETER_TYPES,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        array_elements(&fixture.shared, sum_parameters),
        [Value::Ref(int), Value::Ref(int)]
    );

    let mixed = member(
        &fixture,
        MemberKind::Method,
        "mixed",
        "(ILjava/lang/String;)V",
    );
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                mixed,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new()
            )
            .unwrap()
        ),
        void
    );
    let mixed_parameters = ref_value(
        call_member(
            &fixture,
            fixture.method_class,
            mixed,
            "getParameterTypes",
            PARAMETER_TYPES,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        array_elements(&fixture.shared, mixed_parameters),
        [Value::Ref(int), Value::Ref(fixture.string_object)],
        "primitive and reference parameters keep their order"
    );

    let reset = member(&fixture, MemberKind::Method, "reset", "()V");
    assert_eq!(
        ref_value(
            call_member(
                &fixture,
                fixture.method_class,
                reset,
                "getReturnType",
                METHOD_GET_RETURN_TYPE,
                Vec::new()
            )
            .unwrap()
        ),
        void
    );

    let int_constructor = member(&fixture, MemberKind::Constructor, "<init>", "(I)V");
    let constructor_parameters = ref_value(
        call_member(
            &fixture,
            fixture.constructor_class,
            int_constructor,
            "getParameterTypes",
            PARAMETER_TYPES,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        array_elements(&fixture.shared, constructor_parameters),
        [Value::Ref(int)],
        "Constructor.getParameterTypes of (I)V is [int]"
    );
    assert_eq!(
        int_value(
            call_member(
                &fixture,
                fixture.constructor_class,
                int_constructor,
                "getParameterCount",
                MEMBER_GET_PARAMETER_COUNT,
                Vec::new()
            )
            .unwrap()
        ),
        1
    );

    // A primitive receiver declares no members, exactly as on the JDK.
    let primitive_fields = ref_value(
        call_class(
            &fixture,
            int,
            "getDeclaredFields",
            DECLARED_FIELDS,
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(
        array_elements(&fixture.shared, primitive_fields).is_empty(),
        "int.class declares no fields"
    );
}

#[test]
fn named_lookup_matches_primitive_parameter_classes() {
    let fixture = fixture();
    let int = primitive(&fixture, PrimitiveKind::Int);
    let long = primitive(&fixture, PrimitiveKind::Long);
    let void = primitive(&fixture, PrimitiveKind::Void);

    let set_value =
        ref_value(declared_method(&fixture, fixture.bean_object, "setValue", &[int]).unwrap());
    let set_value_member = member_of(&fixture, set_value);
    assert_eq!(set_value_member.kind, MemberKind::Method);
    assert_eq!(set_value_member.name, "setValue");
    assert_eq!(
        set_value_member.descriptor, "(I)V",
        "an int.class parameter builds the I descriptor"
    );
    assert_eq!(
        set_value,
        member(&fixture, MemberKind::Method, "setValue", "(I)V"),
        "the lookup returns the cached member object"
    );

    // The bytecode client builds the same query with `Class.forName("int")`.
    let via_bytecode = ref_value(
        call_static(
            &fixture,
            "setValueMethod",
            "(Ljava/lang/Class;)Ljava/lang/reflect/Method;",
            vec![Value::Ref(fixture.bean_object)],
        )
        .unwrap(),
    );
    assert_eq!(via_bytecode, set_value);

    // A mismatched primitive descriptor raises NoSuchMethodException (M2.30), naming the
    // receiver class and the exact parameter descriptors that were asked for.
    for (parameters, message) in [
        (Vec::new(), "Bean.setValue()"),
        (
            vec![fixture.string_object],
            "Bean.setValue(Ljava/lang/String;)",
        ),
        (vec![long], "Bean.setValue(J)"),
        (vec![int, int], "Bean.setValue(I, I)"),
        (vec![void], "Bean.setValue(V)"),
    ] {
        assert_lookup_miss(
            &fixture,
            declared_method(&fixture, fixture.bean_object, "setValue", &parameters),
            "java/lang/NoSuchMethodException",
            message,
        );
    }

    let sum =
        ref_value(declared_method(&fixture, fixture.bean_object, "sum", &[int, int]).unwrap());
    assert_eq!(member_of(&fixture, sum).descriptor, "(II)I");
    assert_lookup_miss(
        &fixture,
        declared_method(&fixture, fixture.bean_object, "sum", &[int]),
        "java/lang/NoSuchMethodException",
        "Bean.sum(I)",
    );

    // A primitive and a reference parameter in one query build `(ILjava/lang/String;)V`,
    // and order matters: the reversed query is a miss.
    let mixed = ref_value(
        declared_method(
            &fixture,
            fixture.bean_object,
            "mixed",
            &[int, fixture.string_object],
        )
        .unwrap(),
    );
    assert_eq!(
        member_of(&fixture, mixed).descriptor,
        "(ILjava/lang/String;)V"
    );
    assert_lookup_miss(
        &fixture,
        declared_method(
            &fixture,
            fixture.bean_object,
            "mixed",
            &[fixture.string_object, int],
        ),
        "java/lang/NoSuchMethodException",
        "Bean.mixed(Ljava/lang/String;, I)",
    );

    // `getMethod` accepts the same primitive `Class[]`; the declaring class is the target.
    let inherited = ref_value(
        call_class(
            &fixture,
            fixture.bean_object,
            "getMethod",
            DECLARED_METHOD,
            vec![
                name_value(&fixture, "mixed"),
                params(&fixture, &[int, fixture.string_object]),
            ],
        )
        .unwrap(),
    );
    assert_eq!(inherited, mixed, "getMethod returns the same cached member");

    let int_constructor =
        ref_value(declared_constructor(&fixture, fixture.bean_object, &[int]).unwrap());
    assert_eq!(member_of(&fixture, int_constructor).descriptor, "(I)V");
    let default_constructor =
        ref_value(declared_constructor(&fixture, fixture.bean_object, &[]).unwrap());
    assert_eq!(member_of(&fixture, default_constructor).descriptor, "()V");
    assert_lookup_miss(
        &fixture,
        declared_constructor(&fixture, fixture.bean_object, &[long]),
        "java/lang/NoSuchMethodException",
        "Bean.<init>(J)",
    );
    assert_lookup_miss(
        &fixture,
        declared_constructor(&fixture, fixture.bean_object, &[int, int]),
        "java/lang/NoSuchMethodException",
        "Bean.<init>(I, I)",
    );

    // A named field query uses a name only, so it is unaffected by primitive types.
    let value = ref_value(
        call_class(
            &fixture,
            fixture.bean_object,
            "getDeclaredField",
            DECLARED_FIELD,
            vec![name_value(&fixture, "value")],
        )
        .unwrap(),
    );
    assert_eq!(member_of(&fixture, value).descriptor, "I");

    // A primitive receiver declares nothing, so its named lookups raise too.
    assert_lookup_miss(
        &fixture,
        declared_method(&fixture, int, "setValue", &[int]),
        "java/lang/NoSuchMethodException",
        "int.setValue(I)",
    );
    assert_lookup_miss(
        &fixture,
        declared_constructor(&fixture, int, &[]),
        "java/lang/NoSuchMethodException",
        "int.<init>()",
    );
    assert_lookup_miss(
        &fixture,
        declared_constructor(&fixture, void, &[]),
        "java/lang/NoSuchMethodException",
        "void.<init>()",
    );
}

#[test]
fn reflective_invocation_resolves_int_and_void_types() {
    let fixture = fixture();
    let int = primitive(&fixture, PrimitiveKind::Int);

    // `Method.invoke` on `setValue(I)V` selected through `int.class`.
    let set_value =
        ref_value(declared_method(&fixture, fixture.bean_object, "setValue", &[int]).unwrap());
    let bean = fixture
        .shared
        .allocate_instance(fixture.bean)
        .expect("Bean allocates");
    let boxed = fixture.shared.box_int(42);
    let arguments = reference_array(&fixture.shared, &[boxed]);
    call_member(
        &fixture,
        fixture.method_class,
        set_value,
        "invoke",
        METHOD_INVOKE,
        vec![Value::Ref(bean), Value::Ref(arguments)],
    )
    .unwrap();
    let value_field = member(&fixture, MemberKind::Field, "value", "I");
    let read = ref_value(
        call_member(
            &fixture,
            fixture.field_class,
            value_field,
            "get",
            FIELD_GET,
            vec![Value::Ref(bean)],
        )
        .unwrap(),
    );
    assert_eq!(fixture.shared.unbox_int(read).unwrap(), 42);

    // `Constructor.newInstance` on the `(I)V` constructor chosen by `int.class`.
    let constructor =
        ref_value(declared_constructor(&fixture, fixture.bean_object, &[int]).unwrap());
    let seven = fixture.shared.box_int(7);
    let arguments = reference_array(&fixture.shared, &[seven]);
    let instance = ref_value(
        call_member(
            &fixture,
            fixture.constructor_class,
            constructor,
            "newInstance",
            CONSTRUCTOR_NEW_INSTANCE,
            vec![Value::Ref(arguments)],
        )
        .unwrap(),
    );
    let get_value = member(&fixture, MemberKind::Method, "getValue", "()I");
    let result = ref_value(
        call_member(
            &fixture,
            fixture.method_class,
            get_value,
            "invoke",
            METHOD_INVOKE,
            vec![Value::Ref(instance), Value::Ref(ObjRef::NULL)],
        )
        .unwrap(),
    );
    assert_eq!(fixture.shared.unbox_int(result).unwrap(), 7);

    // The same round trip inside bytecode.
    let other = fixture
        .shared
        .allocate_instance(fixture.bean)
        .expect("Bean allocates");
    let result = int_value(
        call_static(
            &fixture,
            "setAndRead",
            "(Ljava/lang/Object;I)I",
            vec![Value::Ref(other), Value::Int(9)],
        )
        .unwrap(),
    );
    assert_eq!(result, 9);
}

#[test]
fn primitive_class_to_string_keeps_the_documented_class_shape() {
    let fixture = fixture();
    let int = primitive(&fixture, PrimitiveKind::Int);
    let text = string_value(
        &fixture.shared,
        call_member(
            &fixture,
            fixture.object_class,
            int,
            "toString",
            "()Ljava/lang/String;",
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(
        text.starts_with("java.lang.Class@"),
        "Object.toString on a primitive class object reports java.lang.Class: {text:?}"
    );
    // `Class` does not override `toString` in this slice, so this is the M2.9 shape rather
    // than the JDK's `class int`; the slice documents that instead of pretending.
}
