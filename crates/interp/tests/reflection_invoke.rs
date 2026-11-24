//! M2.10 acceptance: boxed values and reflective invocation.
//!
//! `Bean` is an ordinary class with a private `int value`, a private `String label`, a
//! static `counter`, two constructors (`()V` and `(I)V`), int/reference accessors, a
//! static `sum(II)I` and a static `boom()` that throws. The tests drive it through the
//! real reflection member natives: `Constructor.newInstance`, `Field.get`/`set` and
//! `Method.invoke`, with `Integer.valueOf`/`intValue` supplying the boxes. No test bypasses
//! the native registry: every invocation enters `run_static_method` or
//! `run_instance_method` on the `java.lang.reflect` class.
//!
//! Expected values are exact. Error tests assert the typed native error and a fragment of
//! its message, so a silent wrong answer cannot pass as success.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, reflect_invoke, run_instance_method, run_static_method};
use rava_vm::{
    ArrayKind, BoxedPrimitive, ClassId, HeapObject, LoaderId, MemberKind, ObjRef, Shared,
    ThreadState, Value, VmError, boxed, instance, object, reflection, string,
};

const ACC_PRIVATE: u16 = 0x0002;
const ACC_PRIVATE_STATIC: u16 = 0x000A;
const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;

const FIELD_GET: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";
const FIELD_SET: &str = "(Ljava/lang/Object;Ljava/lang/Object;)V";
const METHOD_INVOKE: &str = "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;";
const CONSTRUCTOR_NEW_INSTANCE: &str = "([Ljava/lang/Object;)Ljava/lang/Object;";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

/// `Bean`: the fixture class, built with the same class-file writer the other
/// interpreter tests use. Its fields are private and its constructors/methods public; the
/// slice has no access checks, which is exactly what lets `Field.get` read `value`.
fn bean_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Bean", "java/lang/Object", 61);
    builder.field(ACC_PRIVATE, "value", "I");
    builder.field(ACC_PRIVATE, "label", "Ljava/lang/String;");
    builder.field(ACC_PRIVATE_STATIC, "counter", "I");

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

    // static int boom() { throw "boom"; }
    let boom_text = builder.string("boom");
    let mut boom = Assembler::new();
    boom.ldc(boom_text);
    boom.athrow();
    builder.method(
        ACC_PUBLIC_STATIC,
        "boom",
        "()I",
        Some((1, 0, boom.finish().unwrap())),
    );

    builder.build()
}

/// `BoxProbe`: a bytecode client that calls methods on a boxed receiver, so the
/// interpreter's virtual dispatch (not just the `Integer` natives directly) is exercised.
fn box_probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("BoxProbe", "java/lang/Object", 61);
    let value_of = builder.method_ref("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;");
    let int_to_string = builder.method_ref("java/lang/Integer", "toString", "()Ljava/lang/String;");
    let get_class = builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let integer_equals = builder.method_ref("java/lang/Integer", "equals", "(Ljava/lang/Object;)Z");
    let integer_hash = builder.method_ref("java/lang/Integer", "hashCode", "()I");

    // static String toText(int value) { return Integer.valueOf(value).toString(); }
    let mut to_text = Assembler::new();
    to_text.iload(0);
    to_text.op(0xb8, &value_of.to_be_bytes()); // invokestatic
    to_text.invokevirtual(int_to_string);
    to_text.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "toText",
        "(I)Ljava/lang/String;",
        Some((2, 1, to_text.finish().unwrap())),
    );

    // static Class runtimeClass(int value) { return Integer.valueOf(value).getClass(); }
    let mut runtime_class = Assembler::new();
    runtime_class.iload(0);
    runtime_class.op(0xb8, &value_of.to_be_bytes()); // invokestatic
    runtime_class.invokevirtual(get_class);
    runtime_class.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "runtimeClass",
        "(I)Ljava/lang/Class;",
        Some((2, 1, runtime_class.finish().unwrap())),
    );

    // static boolean sameValue(int left, int right) {
    //     return Integer.valueOf(left).equals(Integer.valueOf(right));
    // }
    let mut same_value = Assembler::new();
    same_value.iload(0);
    same_value.op(0xb8, &value_of.to_be_bytes()); // invokestatic
    same_value.iload(1);
    same_value.op(0xb8, &value_of.to_be_bytes()); // invokestatic
    same_value.invokevirtual(integer_equals);
    same_value.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "sameValue",
        "(II)Z",
        Some((2, 2, same_value.finish().unwrap())),
    );

    // static int hashOf(int value) { return Integer.valueOf(value).hashCode(); }
    let mut hash_of = Assembler::new();
    hash_of.iload(0);
    hash_of.op(0xb8, &value_of.to_be_bytes()); // invokestatic
    hash_of.invokevirtual(integer_hash);
    hash_of.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "hashOf",
        "(I)I",
        Some((2, 1, hash_of.finish().unwrap())),
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    bean: ClassId,
    bean_object: ObjRef,
    box_probe: ClassId,
    thread: ThreadState,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    string::register_natives(&shared);
    boxed::register_natives(&shared);
    reflection::register_natives(&shared);
    reflect_invoke::register_natives(&shared);
    let bean = shared
        .define_class(LoaderId::BOOTSTRAP, "Bean", &bean_bytes())
        .expect("Bean defines");
    let bean_object = shared.class_object(bean).expect("Bean gets a class object");
    let box_probe = shared
        .define_class(LoaderId::BOOTSTRAP, "BoxProbe", &box_probe_bytes())
        .expect("BoxProbe defines");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        bean,
        bean_object,
        box_probe,
        thread,
    }
}

fn library_class(fixture: &Fixture, name: &str) -> ClassId {
    fixture
        .shared
        .load_library_class(name)
        .unwrap_or_else(|error| panic!("{name} loads: {error}"))
}

fn member(fixture: &Fixture, kind: MemberKind, name: &str, descriptor: &str) -> ObjRef {
    fixture
        .shared
        .reflect_member(fixture.bean, kind, name, descriptor)
        .expect("the member object allocates")
}

fn reference_array(shared: &Shared, values: &[ObjRef]) -> ObjRef {
    let mut heap = shared.heap();
    let array = heap.allocate_array(ArrayKind::Reference, values.len());
    {
        let array = heap.array_mut(array).expect("a reference array");
        for (slot, value) in array.elements.iter_mut().zip(values) {
            *slot = Value::Ref(*value);
        }
    }
    array
}

fn box_int(fixture: &Fixture, value: i32) -> ObjRef {
    fixture.shared.box_int(value)
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn boxed_int(fixture: &Fixture, value: Option<Value>) -> i32 {
    fixture.shared.unbox_int(ref_value(value)).unwrap()
}

fn string_value(shared: &Shared, value: Option<Value>) -> String {
    shared
        .heap()
        .string(ref_value(value))
        .expect("the result is a string")
        .to_owned()
}

fn new_bean(fixture: &Fixture, value: i32) -> ObjRef {
    let constructor = member(fixture, MemberKind::Constructor, "<init>", "(I)V");
    let arguments = reference_array(&fixture.shared, &[box_int(fixture, value)]);
    let constructor_class = library_class(fixture, "java/lang/reflect/Constructor");
    let result = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        constructor_class,
        "newInstance",
        CONSTRUCTOR_NEW_INSTANCE,
        constructor,
        vec![Value::Ref(arguments)],
    )
    .expect("Constructor.newInstance succeeds");
    ref_value(result)
}

fn field_get(
    fixture: &Fixture,
    field: ObjRef,
    receiver: ObjRef,
) -> Result<Option<Value>, InterpError> {
    let field_class = library_class(fixture, "java/lang/reflect/Field");
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        field_class,
        "get",
        FIELD_GET,
        field,
        vec![Value::Ref(receiver)],
    )
}

fn field_set(
    fixture: &Fixture,
    field: ObjRef,
    receiver: ObjRef,
    value: Value,
) -> Result<Option<Value>, InterpError> {
    let field_class = library_class(fixture, "java/lang/reflect/Field");
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        field_class,
        "set",
        FIELD_SET,
        field,
        vec![Value::Ref(receiver), value],
    )
}

fn method_invoke(
    fixture: &Fixture,
    method: ObjRef,
    receiver: Value,
    arguments: ObjRef,
) -> Result<Option<Value>, InterpError> {
    let method_class = library_class(fixture, "java/lang/reflect/Method");
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        method_class,
        "invoke",
        METHOD_INVOKE,
        method,
        vec![receiver, Value::Ref(arguments)],
    )
}

fn constructor_new_instance(
    fixture: &Fixture,
    constructor: ObjRef,
    arguments: Value,
) -> Result<Option<Value>, InterpError> {
    let constructor_class = library_class(fixture, "java/lang/reflect/Constructor");
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        constructor_class,
        "newInstance",
        CONSTRUCTOR_NEW_INSTANCE,
        constructor,
        vec![arguments],
    )
}

/// Assert a typed native error that mentions `fragment`.
fn assert_native_error(result: Result<Option<Value>, InterpError>, fragment: &str) {
    match result {
        Err(InterpError::Vm(VmError::Native(ref detail))) => assert!(
            detail.contains(fragment),
            "expected a native error containing {fragment:?}, found {detail:?}"
        ),
        other => panic!("expected a native error containing {fragment:?}, found {other:?}"),
    }
}

fn bean_field(fixture: &Fixture, bean: ObjRef, name: &str, descriptor: &str) -> Value {
    let class = fixture
        .shared
        .heap()
        .instance(bean)
        .expect("a Bean instance")
        .class;
    let layout = fixture
        .shared
        .field_layout(class)
        .expect("the layout builds");
    let slot = layout
        .slot(name, descriptor)
        .expect("the field is laid out");
    fixture.shared.heap().instance(bean).unwrap().fields[slot]
}

#[test]
fn boxed_integer_value_of_int_value_and_object_methods() {
    let fixture = fixture();
    let integer = library_class(&fixture, "java/lang/Integer");
    let object = library_class(&fixture, "java/lang/Object");

    // Integer.valueOf(int) allocates a box, and the heap representation is the box - not
    // a second kind of `Value`.
    let boxed = ref_value(
        run_static_method(
            &fixture.shared,
            &fixture.thread,
            integer,
            "valueOf",
            "(I)Ljava/lang/Integer;",
            vec![Value::Int(42)],
        )
        .unwrap(),
    );
    assert!(matches!(
        fixture.shared.heap().get(boxed).unwrap(),
        HeapObject::Boxed(BoxedPrimitive::Integer(42))
    ));
    assert_eq!(fixture.shared.unbox_int(boxed).unwrap(), 42);

    // Object.getClass reports the boxed class.
    let runtime_class = ref_value(
        run_instance_method(
            &fixture.shared,
            &fixture.thread,
            object,
            "getClass",
            "()Ljava/lang/Class;",
            boxed,
            Vec::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        runtime_class,
        fixture.shared.class_object(integer).unwrap(),
        "getClass of a box is java.lang.Integer"
    );
    assert_eq!(
        fixture.shared.get_class(boxed).unwrap(),
        runtime_class,
        "Shared::get_class agrees with Object.getClass"
    );

    // Integer's own primitive methods.
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "intValue",
                "()I",
                boxed,
                Vec::new(),
            )
            .unwrap()
        ),
        42
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "toString",
                "()Ljava/lang/String;",
                boxed,
                Vec::new(),
            )
            .unwrap()
        ),
        "42"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            run_static_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "toString",
                "(I)Ljava/lang/String;",
                vec![Value::Int(-7)],
            )
            .unwrap()
        ),
        "-7"
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "hashCode",
                "()I",
                boxed,
                Vec::new(),
            )
            .unwrap()
        ),
        42,
        "Integer.hashCode is the value"
    );

    // equals compares the boxed values; null and other object kinds are false.
    let same = ref_value(
        run_static_method(
            &fixture.shared,
            &fixture.thread,
            integer,
            "valueOf",
            "(I)Ljava/lang/Integer;",
            vec![Value::Int(42)],
        )
        .unwrap(),
    );
    let other = box_int(&fixture, 43);
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "equals",
                "(Ljava/lang/Object;)Z",
                boxed,
                vec![Value::Ref(same)],
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "equals",
                "(Ljava/lang/Object;)Z",
                boxed,
                vec![Value::Ref(other)],
            )
            .unwrap()
        ),
        0
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "equals",
                "(Ljava/lang/Object;)Z",
                boxed,
                vec![Value::Ref(ObjRef::NULL)],
            )
            .unwrap()
        ),
        0
    );
    let text = fixture.shared.intern_string("not an Integer");
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                integer,
                "equals",
                "(Ljava/lang/Object;)Z",
                boxed,
                vec![Value::Ref(text)],
            )
            .unwrap()
        ),
        0
    );

    // compareTo is -1, 0 or 1.
    for (right, expected) in [(41, 1), (42, 0), (43, -1)] {
        let right = box_int(&fixture, right);
        assert_eq!(
            int_value(
                run_instance_method(
                    &fixture.shared,
                    &fixture.thread,
                    integer,
                    "compareTo",
                    "(Ljava/lang/Integer;)I",
                    boxed,
                    vec![Value::Ref(right)],
                )
                .unwrap()
            ),
            expected
        );
    }

    // parseInt parses the JDK's decimal syntax and rejects a malformed value.
    for (text, expected) in [("123", 123), ("-7", -7), ("+8", 8)] {
        let argument = fixture.shared.intern_string(text);
        assert_eq!(
            int_value(
                run_static_method(
                    &fixture.shared,
                    &fixture.thread,
                    integer,
                    "parseInt",
                    "(Ljava/lang/String;)I",
                    vec![Value::Ref(argument)],
                )
                .unwrap()
            ),
            expected
        );
    }
    let malformed = fixture.shared.intern_string("12x");
    assert_native_error(
        run_static_method(
            &fixture.shared,
            &fixture.thread,
            integer,
            "parseInt",
            "(Ljava/lang/String;)I",
            vec![Value::Ref(malformed)],
        ),
        "parseInt",
    );

    // Object's own methods still run on a box when they are resolved directly, and the
    // runtime name is the boxed class.
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                object,
                "equals",
                "(Ljava/lang/Object;)Z",
                boxed,
                vec![Value::Ref(boxed)],
            )
            .unwrap()
        ),
        1,
        "Object.equals on one box is reference identity"
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &fixture.shared,
                &fixture.thread,
                object,
                "hashCode",
                "()I",
                boxed,
                Vec::new(),
            )
            .unwrap()
        ),
        boxed.identity_hash()
    );
    let printed = string_value(
        &fixture.shared,
        run_instance_method(
            &fixture.shared,
            &fixture.thread,
            object,
            "toString",
            "()Ljava/lang/String;",
            boxed,
            Vec::new(),
        )
        .unwrap(),
    );
    assert!(
        printed.starts_with("java.lang.Integer@"),
        "Object.toString names the boxed class, found {printed:?}"
    );
}

#[test]
fn boxed_receivers_dispatch_virtually_to_integer() {
    let fixture = fixture();
    let integer = library_class(&fixture, "java/lang/Integer");

    let text = run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.box_probe,
        "toText",
        "(I)Ljava/lang/String;",
        vec![Value::Int(42)],
    )
    .unwrap();
    assert_eq!(string_value(&fixture.shared, text), "42");

    let runtime_class = run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.box_probe,
        "runtimeClass",
        "(I)Ljava/lang/Class;",
        vec![Value::Int(42)],
    )
    .unwrap();
    assert_eq!(
        ref_value(runtime_class),
        fixture.shared.class_object(integer).unwrap()
    );

    assert_eq!(
        int_value(
            run_static_method(
                &fixture.shared,
                &fixture.thread,
                fixture.box_probe,
                "sameValue",
                "(II)Z",
                vec![Value::Int(42), Value::Int(42)],
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            run_static_method(
                &fixture.shared,
                &fixture.thread,
                fixture.box_probe,
                "sameValue",
                "(II)Z",
                vec![Value::Int(42), Value::Int(43)],
            )
            .unwrap()
        ),
        0
    );
    assert_eq!(
        int_value(
            run_static_method(
                &fixture.shared,
                &fixture.thread,
                fixture.box_probe,
                "hashOf",
                "(I)I",
                vec![Value::Int(42)],
            )
            .unwrap()
        ),
        42
    );
}

#[test]
fn constructor_new_instance_unboxes_an_int_and_allocates_the_declaring_class() {
    let fixture = fixture();
    let bean = new_bean(&fixture, 42);

    let class = fixture.shared.heap().instance(bean).unwrap().class;
    assert_eq!(class, fixture.bean);
    assert_eq!(bean_field(&fixture, bean, "value", "I"), Value::Int(42));
    assert_eq!(
        bean_field(&fixture, bean, "label", "Ljava/lang/String;"),
        Value::Ref(ObjRef::NULL),
        "an unset reference field keeps its default"
    );

    // The no-arg constructor runs through the same machinery. A zero-parameter call
    // accepts an empty array and, like the JDK, a null array.
    let constructor = member(&fixture, MemberKind::Constructor, "<init>", "()V");
    let empty = reference_array(&fixture.shared, &[]);
    let bean =
        ref_value(constructor_new_instance(&fixture, constructor, Value::Ref(empty)).unwrap());
    assert_eq!(bean_field(&fixture, bean, "value", "I"), Value::Int(0));

    let bean = ref_value(
        constructor_new_instance(&fixture, constructor, Value::Ref(ObjRef::NULL)).unwrap(),
    );
    assert_eq!(bean_field(&fixture, bean, "value", "I"), Value::Int(0));

    assert_eq!(fixture.thread.frame_depth(), 0);
}

#[test]
fn field_get_and_set_box_int_and_pass_references_through() {
    let fixture = fixture();
    let bean = new_bean(&fixture, 42);
    let value_field = member(&fixture, MemberKind::Field, "value", "I");

    let read = field_get(&fixture, value_field, bean).unwrap();
    assert_eq!(boxed_int(&fixture, read), 42);
    assert!(matches!(
        fixture.shared.heap().get(ref_value(read)).unwrap(),
        HeapObject::Boxed(BoxedPrimitive::Integer(42))
    ));

    assert_eq!(
        field_set(
            &fixture,
            value_field,
            bean,
            Value::Ref(box_int(&fixture, 99))
        )
        .unwrap(),
        None
    );
    assert_eq!(bean_field(&fixture, bean, "value", "I"), Value::Int(99));
    assert_eq!(
        boxed_int(&fixture, field_get(&fixture, value_field, bean).unwrap()),
        99
    );

    // A reference field is not copied or re-boxed: the same heap handle comes back.
    let label_field = member(&fixture, MemberKind::Field, "label", "Ljava/lang/String;");
    let label = fixture.shared.intern_string("hello");
    assert_eq!(
        field_set(&fixture, label_field, bean, Value::Ref(label)).unwrap(),
        None
    );
    assert_eq!(
        bean_field(&fixture, bean, "label", "Ljava/lang/String;"),
        Value::Ref(label)
    );
    let read = ref_value(field_get(&fixture, label_field, bean).unwrap());
    assert_eq!(read, label, "Field.get preserves String identity");

    // A null reference is a legal reference value.
    assert_eq!(
        field_set(&fixture, label_field, bean, Value::Ref(ObjRef::NULL)).unwrap(),
        None
    );
    assert_eq!(
        ref_value(field_get(&fixture, label_field, bean).unwrap()),
        ObjRef::NULL
    );

    assert_eq!(fixture.thread.frame_depth(), 0);
}

#[test]
fn method_invoke_runs_instance_and_static_methods() {
    let fixture = fixture();
    let bean = new_bean(&fixture, 42);
    let empty = reference_array(&fixture.shared, &[]);

    let get_value = member(&fixture, MemberKind::Method, "getValue", "()I");
    let read = method_invoke(&fixture, get_value, Value::Ref(bean), empty).unwrap();
    assert_eq!(boxed_int(&fixture, read), 42);

    let set_value = member(&fixture, MemberKind::Method, "setValue", "(I)V");
    let arguments = reference_array(&fixture.shared, &[box_int(&fixture, 7)]);
    assert_eq!(
        method_invoke(&fixture, set_value, Value::Ref(bean), arguments).unwrap(),
        Some(Value::Ref(ObjRef::NULL)),
        "a void method returns null through Method.invoke"
    );
    let read = method_invoke(&fixture, get_value, Value::Ref(bean), empty).unwrap();
    assert_eq!(boxed_int(&fixture, read), 7);

    // A reference argument and return keep their identity.
    let set_label = member(
        &fixture,
        MemberKind::Method,
        "setLabel",
        "(Ljava/lang/String;)V",
    );
    let get_label = member(
        &fixture,
        MemberKind::Method,
        "getLabel",
        "()Ljava/lang/String;",
    );
    let label = fixture.shared.intern_string("world");
    let arguments = reference_array(&fixture.shared, &[label]);
    assert_eq!(
        method_invoke(&fixture, set_label, Value::Ref(bean), arguments).unwrap(),
        Some(Value::Ref(ObjRef::NULL))
    );
    assert_eq!(
        ref_value(method_invoke(&fixture, get_label, Value::Ref(bean), empty).unwrap()),
        label
    );

    // A static method ignores the receiver argument and boxes its int result.
    let sum = member(&fixture, MemberKind::Method, "sum", "(II)I");
    let arguments = reference_array(
        &fixture.shared,
        &[box_int(&fixture, 2), box_int(&fixture, 3)],
    );
    let result = method_invoke(&fixture, sum, Value::Ref(ObjRef::NULL), arguments).unwrap();
    assert_eq!(boxed_int(&fixture, result), 5);
    assert_eq!(
        fixture.shared.unbox_int(ref_value(result)).unwrap(),
        5,
        "the result is a java.lang.Integer box"
    );

    // The same static call with an arbitrary receiver still ignores it.
    let arguments = reference_array(
        &fixture.shared,
        &[box_int(&fixture, 10), box_int(&fixture, 20)],
    );
    let result = method_invoke(&fixture, sum, Value::Ref(label), arguments).unwrap();
    assert_eq!(boxed_int(&fixture, result), 30);

    assert_eq!(fixture.thread.frame_depth(), 0);
    assert!(fixture.thread.frame_roots().is_empty());
}

#[test]
fn wrong_receivers_static_fields_and_wrong_arguments_are_typed_errors() {
    let fixture = fixture();
    let bean = new_bean(&fixture, 42);
    let text = fixture.shared.intern_string("not a Bean");
    let object_class = library_class(&fixture, "java/lang/Object");
    let object_instance = fixture
        .shared
        .allocate_instance(object_class)
        .expect("an Object instance allocates");
    let empty = reference_array(&fixture.shared, &[]);

    // Method.invoke on a non-instance receiver.
    let get_value = member(&fixture, MemberKind::Method, "getValue", "()I");
    assert_native_error(
        method_invoke(&fixture, get_value, Value::Ref(text), empty),
        "not an instance",
    );
    // Method.invoke on an instance of an unrelated class.
    assert_native_error(
        method_invoke(&fixture, get_value, Value::Ref(object_instance), empty),
        "not assignable",
    );
    // A null instance receiver is rejected before dispatch.
    assert_native_error(
        method_invoke(&fixture, get_value, Value::Ref(ObjRef::NULL), empty),
        "null instance receiver",
    );

    // Field.get on a non-instance receiver, and on an unrelated instance.
    let value_field = member(&fixture, MemberKind::Field, "value", "I");
    assert_native_error(field_get(&fixture, value_field, text), "not an instance");
    assert_native_error(
        field_get(&fixture, value_field, object_instance),
        "not assignable",
    );
    // Field.set checks the receiver too.
    assert_native_error(
        field_set(
            &fixture,
            value_field,
            object_instance,
            Value::Ref(box_int(&fixture, 1)),
        ),
        "not assignable",
    );

    // A static field is rejected through the instance field API.
    let counter = member(&fixture, MemberKind::Field, "counter", "I");
    assert_native_error(field_get(&fixture, counter, bean), "static field");
    assert_native_error(
        field_set(&fixture, counter, bean, Value::Ref(box_int(&fixture, 1))),
        "static field",
    );

    // Wrong argument count and wrong argument type.
    let set_value = member(&fixture, MemberKind::Method, "setValue", "(I)V");
    assert_native_error(
        method_invoke(&fixture, set_value, Value::Ref(bean), empty),
        "expects 1 argument(s), found 0",
    );
    let wrong = reference_array(&fixture.shared, &[text]);
    assert_native_error(
        method_invoke(&fixture, set_value, Value::Ref(bean), wrong),
        "boxed Integer",
    );
    assert_native_error(
        field_set(&fixture, value_field, bean, Value::Ref(text)),
        "boxed Integer",
    );

    // Constructor argument errors.
    let constructor = member(&fixture, MemberKind::Constructor, "<init>", "(I)V");
    assert_native_error(
        constructor_new_instance(&fixture, constructor, Value::Ref(empty)),
        "expects 1 argument(s), found 0",
    );
    assert_native_error(
        constructor_new_instance(&fixture, constructor, Value::Ref(ObjRef::NULL)),
        "null Object[]",
    );
    let wrong = reference_array(&fixture.shared, &[text]);
    assert_native_error(
        constructor_new_instance(&fixture, constructor, Value::Ref(wrong)),
        "boxed Integer",
    );

    assert_eq!(fixture.thread.frame_depth(), 0);
}

#[test]
fn the_no_overload_and_no_lookup_boundary_is_explicit() {
    let fixture = fixture();

    // The reflective lookup API that would resolve overloads by name and parameter types
    // is still absent: it reaches the abstract C1 stub, not a native.
    let class_class = library_class(&fixture, "java/lang/Class");
    let name = fixture.shared.intern_string("setValue");
    let parameter_types = reference_array(&fixture.shared, &[]);
    let result = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        class_class,
        "getDeclaredMethod",
        "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;",
        fixture.bean_object,
        vec![Value::Ref(name), Value::Ref(parameter_types)],
    );
    assert!(
        matches!(
            result,
            Err(InterpError::NativeOrAbstract { ref name, .. }) if name == "getDeclaredMethod"
        ),
        "found {result:?}"
    );

    // A member object whose descriptor does not exist is not resolved to a same-named
    // overload: Method.invoke uses the reflected name and descriptor exactly.
    let bogus = fixture
        .shared
        .reflect_member(fixture.bean, MemberKind::Method, "missing", "(I)V")
        .unwrap();
    let arguments = reference_array(&fixture.shared, &[box_int(&fixture, 1)]);
    assert_native_error(
        method_invoke(
            &fixture,
            bogus,
            Value::Ref(new_bean(&fixture, 0)),
            arguments,
        ),
        "has no method",
    );

    // Arity is checked against the reflected descriptor, not a name-only lookup.
    let sum = member(&fixture, MemberKind::Method, "sum", "(II)I");
    let three = reference_array(
        &fixture.shared,
        &[
            box_int(&fixture, 1),
            box_int(&fixture, 2),
            box_int(&fixture, 3),
        ],
    );
    assert_native_error(
        method_invoke(&fixture, sum, Value::Ref(ObjRef::NULL), three),
        "expects 2 argument(s), found 3",
    );

    // The member kind is checked too: a Method native on a Constructor object and the
    // constructor native on a Method object both fail loudly.
    let constructor = member(&fixture, MemberKind::Constructor, "<init>", "(I)V");
    let method_class = library_class(&fixture, "java/lang/reflect/Method");
    let wrong_kind = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        method_class,
        "invoke",
        METHOD_INVOKE,
        constructor,
        vec![
            Value::Ref(ObjRef::NULL),
            Value::Ref(reference_array(&fixture.shared, &[])),
        ],
    );
    assert_native_error(wrong_kind, "requires a Method receiver");

    let get_value = member(&fixture, MemberKind::Method, "getValue", "()I");
    assert_native_error(
        constructor_new_instance(
            &fixture,
            get_value,
            Value::Ref(reference_array(&fixture.shared, &[])),
        ),
        "requires a Constructor receiver",
    );

    let field_native = library_class(&fixture, "java/lang/reflect/Field");
    let not_a_field = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        field_native,
        "get",
        FIELD_GET,
        get_value,
        vec![Value::Ref(new_bean(&fixture, 0))],
    );
    assert_native_error(not_a_field, "requires a Field receiver");
}

#[test]
fn a_thrown_invocation_is_propagated_not_swallowed() {
    let fixture = fixture();
    let boom = member(&fixture, MemberKind::Method, "boom", "()I");
    let empty = reference_array(&fixture.shared, &[]);
    let result = method_invoke(&fixture, boom, Value::Ref(ObjRef::NULL), empty);
    match result {
        Err(InterpError::Thrown(throwable)) => {
            assert_eq!(
                fixture.shared.heap().string(throwable).unwrap(),
                "boom",
                "the original throwable crosses the native boundary unchanged"
            );
        }
        other => panic!("expected the Java throwable, found {other:?}"),
    }
}

#[test]
fn boxed_helpers_reject_null_and_non_integer_references() {
    let fixture = fixture();
    assert!(matches!(
        fixture.shared.unbox_int(ObjRef::NULL),
        Err(VmError::Native(ref detail)) if detail.contains("null")
    ));
    let text = fixture.shared.intern_string("42");
    assert!(matches!(
        fixture.shared.unbox_int(text),
        Err(VmError::Native(ref detail)) if detail.contains("not a boxed Integer")
    ));

    // A primitive descriptor other than int is not guessed at.
    assert!(matches!(
        boxed::unbox_argument(&fixture.shared, "J", Value::Ref(fixture.shared.box_int(1))),
        Err(VmError::Native(ref detail)) if detail.contains("only int and reference")
    ));
    assert!(matches!(
        boxed::box_result(&fixture.shared, "V", None),
        Ok(Value::Ref(reference)) if reference == ObjRef::NULL
    ));
    assert!(matches!(
        boxed::box_result(&fixture.shared, "J", Some(Value::Long(1))),
        Err(VmError::Native(ref detail)) if detail.contains("only int")
    ));
}
