//! M2.25 acceptance: the full primitive-box family and the `Number` accessors.
//!
//! `BoxProbe` is an ordinary class with one field per primitive kind, a constructor over
//! `byte`/`short`/`int`/`long`/`float`/`double` and static identity methods (`echoB`,
//! `echoS`, ...) so `Method.invoke` and `Constructor.newInstance` can be driven for every
//! descriptor. The tests call the real wrapper natives through `run_static_method` and
//! `run_instance_method`; no test reads a box's value without going through the registry
//! or the heap.
//!
//! Expected values are exact. A conversion test asserts the Java result of the narrowing
//! (a `long` keeps its low bits, a `float` narrows through `int`, so a huge value
//! saturates) rather than the Rust cast. Error tests assert the typed native error and a
//! fragment of its message.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, reflect_invoke, run_instance_method, run_static_method};
use rava_vm::{
    BoxedPrimitive, ClassId, ClassRef, LoaderId, MemberKind, ObjRef, Shared, ThreadState, Value,
    VmError, instance, object, reflection, string,
};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;

const FIELD_GET: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";
const FIELD_SET: &str = "(Ljava/lang/Object;Ljava/lang/Object;)V";
const METHOD_INVOKE: &str = "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;";
const CONSTRUCTOR_NEW_INSTANCE: &str = "([Ljava/lang/Object;)Ljava/lang/Object;";

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

/// `BoxProbe`: one field per primitive kind, a six-argument constructor and static
/// identity methods for every primitive descriptor.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("BoxProbe", "java/lang/Object", 61);
    let fields = [
        ("b", "B"),
        ("s", "S"),
        ("i", "I"),
        ("j", "J"),
        ("f", "F"),
        ("d", "D"),
        ("c", "C"),
        ("z", "Z"),
    ];
    for (name, descriptor) in fields {
        builder.field(ACC_PUBLIC, name, descriptor);
    }

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let field_refs: Vec<u16> = fields
        .iter()
        .map(|(name, descriptor)| builder.field_ref("BoxProbe", name, descriptor))
        .collect();

    // BoxProbe(byte b, short s, int i, long j, float f, double d) { super(); ... }
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.aload(0);
    init.iload(1);
    init.putfield(field_refs[0]);
    init.aload(0);
    init.iload(2);
    init.putfield(field_refs[1]);
    init.aload(0);
    init.iload(3);
    init.putfield(field_refs[2]);
    init.aload(0);
    init.lload(4);
    init.putfield(field_refs[3]);
    // JVM slots after `this`: B at 1, S at 2, I at 3, J at 4..=5, F at 6, D at 7..=8.
    init.aload(0);
    init.fload(6);
    init.putfield(field_refs[4]);
    init.aload(0);
    init.dload(7);
    init.putfield(field_refs[5]);
    init.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "(BSIJFD)V",
        // `this` plus B/S/I/J(2)/F/D(2) is nine JVM local slots.
        Some((4, 9, init.finish().unwrap())),
    );

    // BoxProbe(char c, boolean z) { super(); this.c = c; this.z = z; }
    let mut init_char = Assembler::new();
    init_char.aload(0);
    init_char.invokespecial(object_init);
    init_char.aload(0);
    init_char.iload(1);
    init_char.putfield(field_refs[6]);
    init_char.aload(0);
    init_char.iload(2);
    init_char.putfield(field_refs[7]);
    init_char.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "(CZ)V",
        Some((2, 3, init_char.finish().unwrap())),
    );

    // One static identity method per primitive descriptor.
    type Load = fn(&mut Assembler, u8);
    type Return = fn(&mut Assembler);
    let echoes: [(&str, &str, Load, Return, u16, u16); 8] = [
        ("echoB", "(B)B", Assembler::iload, Assembler::ireturn, 1, 1),
        ("echoS", "(S)S", Assembler::iload, Assembler::ireturn, 1, 1),
        ("echoI", "(I)I", Assembler::iload, Assembler::ireturn, 1, 1),
        ("echoJ", "(J)J", Assembler::lload, Assembler::lreturn, 2, 2),
        ("echoF", "(F)F", Assembler::fload, Assembler::freturn, 1, 1),
        ("echoD", "(D)D", Assembler::dload, Assembler::dreturn, 2, 2),
        ("echoC", "(C)C", Assembler::iload, Assembler::ireturn, 1, 1),
        ("echoZ", "(Z)Z", Assembler::iload, Assembler::ireturn, 1, 1),
    ];
    for (name, descriptor, load, ret, local_slots, stack_slots) in echoes {
        let mut code = Assembler::new();
        load(&mut code, 0);
        ret(&mut code);
        builder.method(
            ACC_PUBLIC_STATIC,
            name,
            descriptor,
            Some((stack_slots, local_slots, code.finish().unwrap())),
        );
    }

    builder.build()
}

struct Fixture {
    shared: Shared,
    probe: ClassId,
    thread: ThreadState,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    string::register_natives(&shared);
    rava_vm::boxed::register_natives(&shared);
    reflection::register_natives(&shared);
    reflect_invoke::register_natives(&shared);
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "BoxProbe", &probe_bytes())
        .expect("BoxProbe defines");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        probe,
        thread,
    }
}

fn library_class(fixture: &Fixture, name: &str) -> ClassId {
    fixture
        .shared
        .load_library_class(name)
        .unwrap_or_else(|error| panic!("{name} loads: {error}"))
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

fn boxed_ref(fixture: &Fixture, boxed: BoxedPrimitive) -> ObjRef {
    fixture.shared.boxed_value(boxed)
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

fn long_value(value: Option<Value>) -> i64 {
    match value {
        Some(Value::Long(value)) => value,
        other => panic!("expected a long result, found {other:?}"),
    }
}

fn float_value(value: Option<Value>) -> f32 {
    match value {
        Some(Value::Float(value)) => value,
        other => panic!("expected a float result, found {other:?}"),
    }
}

fn double_value(value: Option<Value>) -> f64 {
    match value {
        Some(Value::Double(value)) => value,
        other => panic!("expected a double result, found {other:?}"),
    }
}

fn boxed_value(fixture: &Fixture, value: Option<Value>) -> BoxedPrimitive {
    let reference = ref_value(value);
    fixture
        .shared
        .boxed(reference)
        .expect("the result is a boxed primitive")
}

fn text_value(shared: &Shared, value: Option<Value>) -> String {
    shared
        .heap()
        .string(ref_value(value))
        .expect("the result is a string")
        .to_owned()
}

fn static_call(
    fixture: &Fixture,
    class_name: &str,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    let class = library_class(fixture, class_name);
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        class,
        name,
        descriptor,
        args,
    )
}

fn instance_call(
    fixture: &Fixture,
    class_name: &str,
    name: &str,
    descriptor: &str,
    receiver: ObjRef,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    let class = library_class(fixture, class_name);
    run_instance_method(
        &fixture.shared,
        &fixture.thread,
        class,
        name,
        descriptor,
        receiver,
        args,
    )
}

fn wrapper_class_name(boxed: BoxedPrimitive) -> &'static str {
    boxed.binary_name()
}

fn class_name_of(fixture: &Fixture, reference: ObjRef) -> String {
    let class = fixture
        .shared
        .get_class(reference)
        .expect("the box has a class");
    text_value(
        &fixture.shared,
        instance_call(
            fixture,
            "java/lang/Class",
            "getName",
            "()Ljava/lang/String;",
            class,
            Vec::new(),
        )
        .expect("Class.getName runs"),
    )
}

fn field_get(
    fixture: &Fixture,
    field: ObjRef,
    receiver: ObjRef,
) -> Result<Option<Value>, InterpError> {
    instance_call(
        fixture,
        "java/lang/reflect/Field",
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
    instance_call(
        fixture,
        "java/lang/reflect/Field",
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
    instance_call(
        fixture,
        "java/lang/reflect/Method",
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
    instance_call(
        fixture,
        "java/lang/reflect/Constructor",
        "newInstance",
        CONSTRUCTOR_NEW_INSTANCE,
        constructor,
        vec![arguments],
    )
}

fn member(fixture: &Fixture, kind: MemberKind, name: &str, descriptor: &str) -> ObjRef {
    fixture
        .shared
        .reflect_member(fixture.probe, kind, name, descriptor)
        .expect("the member object allocates")
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

/// The raw slot value of a `BoxProbe` field, without the reflection boxing.
fn probe_field(fixture: &Fixture, probe: ObjRef, name: &str, descriptor: &str) -> Value {
    let layout = fixture
        .shared
        .field_layout(fixture.probe)
        .expect("the layout builds");
    let slot = layout
        .slot(name, descriptor)
        .expect("the field is laid out");
    fixture.shared.heap().instance(probe).unwrap().fields[slot]
}

fn new_probe(fixture: &Fixture) -> ObjRef {
    let constructor = member(fixture, MemberKind::Constructor, "<init>", "(BSIJFD)V");
    let arguments = reference_array(
        &fixture.shared,
        &[
            boxed_ref(fixture, BoxedPrimitive::Byte(-5)),
            boxed_ref(fixture, BoxedPrimitive::Short(300)),
            boxed_ref(fixture, BoxedPrimitive::Integer(70_000)),
            boxed_ref(fixture, BoxedPrimitive::Long(5_000_000_000)),
            boxed_ref(fixture, BoxedPrimitive::Float(1.5)),
            boxed_ref(fixture, BoxedPrimitive::Double(2.25)),
        ],
    );
    ref_value(
        constructor_new_instance(fixture, constructor, Value::Ref(arguments))
            .expect("Constructor.newInstance succeeds"),
    )
}

#[test]
fn every_box_reports_its_wrapper_class() {
    let fixture = fixture();
    for boxed in [
        BoxedPrimitive::Byte(1),
        BoxedPrimitive::Short(1),
        BoxedPrimitive::Integer(1),
        BoxedPrimitive::Long(1),
        BoxedPrimitive::Float(1.0),
        BoxedPrimitive::Double(1.0),
        BoxedPrimitive::Character('A'),
        BoxedPrimitive::Boolean(true),
    ] {
        let reference = boxed_ref(&fixture, boxed);
        let name = class_name_of(&fixture, reference);
        assert_eq!(name, wrapper_class_name(boxed));
        // The dotted and the internal name agree on the simple name.
        assert_eq!(
            name.rsplit('.').next().unwrap(),
            boxed.binary_name().rsplit('.').next().unwrap()
        );
    }
}

#[test]
fn byte_and_short_boxes_apply_number_conversions() {
    let fixture = fixture();

    // Byte.valueOf(byte) and every Number accessor on a positive and a negative value.
    let byte_127 = ref_value(
        static_call(
            &fixture,
            "java/lang/Byte",
            "valueOf",
            "(B)Ljava/lang/Byte;",
            vec![Value::Int(127)],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(byte_127))),
        BoxedPrimitive::Byte(127)
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "byteValue",
                "()B",
                byte_127,
                vec![]
            )
            .unwrap()
        ),
        127
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "shortValue",
                "()S",
                byte_127,
                vec![]
            )
            .unwrap()
        ),
        127
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "intValue",
                "()I",
                byte_127,
                vec![]
            )
            .unwrap()
        ),
        127
    );
    assert_eq!(
        long_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "longValue",
                "()J",
                byte_127,
                vec![]
            )
            .unwrap()
        ),
        127
    );
    assert_eq!(
        float_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "floatValue",
                "()F",
                byte_127,
                vec![]
            )
            .unwrap()
        ),
        127.0
    );
    assert_eq!(
        double_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "doubleValue",
                "()D",
                byte_127,
                vec![]
            )
            .unwrap()
        ),
        127.0
    );

    let byte_minus_five = boxed_ref(&fixture, BoxedPrimitive::Byte(-5));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "byteValue",
                "()B",
                byte_minus_five,
                vec![]
            )
            .unwrap()
        ),
        -5
    );
    assert_eq!(
        long_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "longValue",
                "()J",
                byte_minus_five,
                vec![]
            )
            .unwrap()
        ),
        -5
    );
    assert_eq!(
        double_value(
            instance_call(
                &fixture,
                "java/lang/Byte",
                "doubleValue",
                "()D",
                byte_minus_five,
                vec![]
            )
            .unwrap()
        ),
        -5.0
    );

    // Short widens to int; narrowing past the sign bit is the Java `(byte)` result.
    let short_30000 = boxed_ref(&fixture, BoxedPrimitive::Short(30_000));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Short",
                "intValue",
                "()I",
                short_30000,
                vec![]
            )
            .unwrap()
        ),
        30_000
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Short",
                "byteValue",
                "()B",
                short_30000,
                vec![]
            )
            .unwrap()
        ),
        30_000_i16 as i8 as i32
    );
    assert_eq!(
        long_value(
            instance_call(
                &fixture,
                "java/lang/Short",
                "longValue",
                "()J",
                short_30000,
                vec![]
            )
            .unwrap()
        ),
        30_000
    );

    // Integer: the byte/short narrowing keeps the low bits, float rounds the magnitude.
    let int_max = boxed_ref(&fixture, BoxedPrimitive::Integer(i32::MAX));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Integer",
                "byteValue",
                "()B",
                int_max,
                vec![]
            )
            .unwrap()
        ),
        i32::MAX as i8 as i32
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Integer",
                "shortValue",
                "()S",
                int_max,
                vec![]
            )
            .unwrap()
        ),
        i32::MAX as i16 as i32
    );
    assert_eq!(
        long_value(
            instance_call(
                &fixture,
                "java/lang/Integer",
                "longValue",
                "()J",
                int_max,
                vec![]
            )
            .unwrap()
        ),
        i64::from(i32::MAX)
    );
    assert_eq!(
        float_value(
            instance_call(
                &fixture,
                "java/lang/Integer",
                "floatValue",
                "()F",
                int_max,
                vec![]
            )
            .unwrap()
        ),
        i32::MAX as f32
    );

    // Wrapper.toString and Integer.toString(int).
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Byte",
                "toString",
                "()Ljava/lang/String;",
                byte_minus_five,
                vec![]
            )
            .unwrap()
        ),
        "-5"
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            static_call(
                &fixture,
                "java/lang/Integer",
                "toString",
                "(I)Ljava/lang/String;",
                vec![Value::Int(-7)]
            )
            .unwrap()
        ),
        "-7"
    );
}

#[test]
fn long_box_keeps_the_low_bits_of_a_narrowing_conversion() {
    let fixture = fixture();
    let value: i64 = 0x0000_0001_2345_6789;
    let long = boxed_ref(&fixture, BoxedPrimitive::Long(value));

    assert_eq!(
        long_value(
            instance_call(&fixture, "java/lang/Long", "longValue", "()J", long, vec![]).unwrap()
        ),
        value
    );
    assert_eq!(
        int_value(
            instance_call(&fixture, "java/lang/Long", "intValue", "()I", long, vec![]).unwrap()
        ),
        value as i32
    );
    assert_eq!(
        int_value(
            instance_call(&fixture, "java/lang/Long", "byteValue", "()B", long, vec![]).unwrap()
        ),
        value as i8 as i32
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Long",
                "shortValue",
                "()S",
                long,
                vec![]
            )
            .unwrap()
        ),
        value as i16 as i32
    );
    assert_eq!(
        float_value(
            instance_call(
                &fixture,
                "java/lang/Long",
                "floatValue",
                "()F",
                long,
                vec![]
            )
            .unwrap()
        ),
        value as f32
    );
    assert_eq!(
        double_value(
            instance_call(
                &fixture,
                "java/lang/Long",
                "doubleValue",
                "()D",
                long,
                vec![]
            )
            .unwrap()
        ),
        value as f64
    );

    // Long.hashCode is the JDK's folded 64-bit value.
    let expected_hash = (value ^ ((value as u64 >> 32) as i64)) as i32;
    assert_eq!(
        int_value(
            instance_call(&fixture, "java/lang/Long", "hashCode", "()I", long, vec![]).unwrap()
        ),
        expected_hash
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Long",
                "hashCode",
                "(J)I",
                vec![Value::Long(value)]
            )
            .unwrap()
        ),
        expected_hash
    );

    // Long.valueOf(long) and toString(long) share the decimal text.
    let boxed = ref_value(
        static_call(
            &fixture,
            "java/lang/Long",
            "valueOf",
            "(J)Ljava/lang/Long;",
            vec![Value::Long(-9)],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(boxed))),
        BoxedPrimitive::Long(-9)
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Long",
                "toString",
                "()Ljava/lang/String;",
                boxed,
                vec![]
            )
            .unwrap()
        ),
        "-9"
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            static_call(
                &fixture,
                "java/lang/Long",
                "toString",
                "(J)Ljava/lang/String;",
                vec![Value::Long(value)]
            )
            .unwrap()
        ),
        value.to_string()
    );
}

#[test]
fn float_and_double_boxes_follow_the_ieee_comparison_rules() {
    let fixture = fixture();

    let float_one_and_a_half = boxed_ref(&fixture, BoxedPrimitive::Float(1.5));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "intValue",
                "()I",
                float_one_and_a_half,
                vec![]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        long_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "longValue",
                "()J",
                float_one_and_a_half,
                vec![]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "byteValue",
                "()B",
                float_one_and_a_half,
                vec![]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        double_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "doubleValue",
                "()D",
                float_one_and_a_half,
                vec![]
            )
            .unwrap()
        ),
        1.5
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Float",
                "toString",
                "()Ljava/lang/String;",
                float_one_and_a_half,
                vec![]
            )
            .unwrap()
        ),
        "1.5"
    );

    // A float that overflows the int range saturates, exactly as `(int)` does; the byte
    // narrowing then keeps the low 8 bits of that saturated int.
    let huge = boxed_ref(&fixture, BoxedPrimitive::Float(1.0e10));
    assert_eq!(
        int_value(
            instance_call(&fixture, "java/lang/Float", "intValue", "()I", huge, vec![]).unwrap()
        ),
        i32::MAX
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "byteValue",
                "()B",
                huge,
                vec![]
            )
            .unwrap()
        ),
        i32::MAX as i8 as i32
    );

    // NaN narrows to zero and is its own equal; -0.0 and 0.0 are not equal.
    let nan = boxed_ref(&fixture, BoxedPrimitive::Float(f32::NAN));
    let other_nan = boxed_ref(&fixture, BoxedPrimitive::Float(f32::NAN));
    assert_eq!(
        int_value(
            instance_call(&fixture, "java/lang/Float", "intValue", "()I", nan, vec![]).unwrap()
        ),
        0
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "equals",
                "(Ljava/lang/Object;)Z",
                nan,
                vec![Value::Ref(other_nan)]
            )
            .unwrap()
        ),
        1
    );
    let positive_zero = boxed_ref(&fixture, BoxedPrimitive::Float(0.0));
    let negative_zero = boxed_ref(&fixture, BoxedPrimitive::Float(-0.0));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "equals",
                "(Ljava/lang/Object;)Z",
                positive_zero,
                vec![Value::Ref(negative_zero)]
            )
            .unwrap()
        ),
        0
    );
    // Float.compare orders `0.0` above `-0.0` and `NaN` above everything.
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "compareTo",
                "(Ljava/lang/Float;)I",
                positive_zero,
                vec![Value::Ref(negative_zero)]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "compareTo",
                "(Ljava/lang/Float;)I",
                nan,
                vec![Value::Ref(float_one_and_a_half)]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Float",
                "compareTo",
                "(Ljava/lang/Float;)I",
                nan,
                vec![Value::Ref(other_nan)]
            )
            .unwrap()
        ),
        0
    );
    // The hash of NaN is the canonical floatToIntBits pattern.
    assert_eq!(
        int_value(
            instance_call(&fixture, "java/lang/Float", "hashCode", "()I", nan, vec![]).unwrap()
        ),
        0x7fc0_0000
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Float",
                "floatToIntBits",
                "(F)I",
                vec![Value::Float(f32::NAN)]
            )
            .unwrap()
        ),
        0x7fc0_0000
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Float",
                "toString",
                "()Ljava/lang/String;",
                nan,
                vec![]
            )
            .unwrap()
        ),
        "NaN"
    );

    // Double: the same rules at 64 bits, and `Double.toString` keeps the `.0`.
    let double_two_and_a_quarter = boxed_ref(&fixture, BoxedPrimitive::Double(2.25));
    assert_eq!(
        double_value(
            instance_call(
                &fixture,
                "java/lang/Double",
                "doubleValue",
                "()D",
                double_two_and_a_quarter,
                vec![]
            )
            .unwrap()
        ),
        2.25
    );
    assert_eq!(
        float_value(
            instance_call(
                &fixture,
                "java/lang/Double",
                "floatValue",
                "()F",
                double_two_and_a_quarter,
                vec![]
            )
            .unwrap()
        ),
        2.25
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Double",
                "intValue",
                "()I",
                double_two_and_a_quarter,
                vec![]
            )
            .unwrap()
        ),
        2
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Double",
                "byteValue",
                "()B",
                double_two_and_a_quarter,
                vec![]
            )
            .unwrap()
        ),
        2
    );
    let double_nan = boxed_ref(&fixture, BoxedPrimitive::Double(f64::NAN));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Double",
                "equals",
                "(Ljava/lang/Object;)Z",
                double_nan,
                vec![Value::Ref(boxed_ref(
                    &fixture,
                    BoxedPrimitive::Double(f64::NAN)
                ))]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Double",
                "hashCode",
                "()I",
                double_nan,
                vec![]
            )
            .unwrap()
        ),
        0x7ff8_0000_i32
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Double",
                "toString",
                "()Ljava/lang/String;",
                boxed_ref(&fixture, BoxedPrimitive::Double(1.0)),
                vec![]
            )
            .unwrap()
        ),
        "1.0"
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Double",
                "toString",
                "()Ljava/lang/String;",
                boxed_ref(&fixture, BoxedPrimitive::Double(f64::INFINITY)),
                vec![]
            )
            .unwrap()
        ),
        "Infinity"
    );
}

#[test]
fn parse_and_value_of_string_forms_are_typed() {
    let fixture = fixture();

    // Decimal and radix parse for the integer wrappers.
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Integer",
                "parseInt",
                "(Ljava/lang/String;)I",
                vec![Value::Ref(fixture.shared.intern_string("-42"))]
            )
            .unwrap()
        ),
        -42
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Integer",
                "parseInt",
                "(Ljava/lang/String;I)I",
                vec![
                    Value::Ref(fixture.shared.intern_string("ff")),
                    Value::Int(16)
                ]
            )
            .unwrap()
        ),
        255
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Byte",
                "parseByte",
                "(Ljava/lang/String;)B",
                vec![Value::Ref(fixture.shared.intern_string("-128"))]
            )
            .unwrap()
        ),
        -128
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Short",
                "parseShort",
                "(Ljava/lang/String;)S",
                vec![Value::Ref(fixture.shared.intern_string("30000"))]
            )
            .unwrap()
        ),
        30_000
    );
    assert_eq!(
        long_value(
            static_call(
                &fixture,
                "java/lang/Long",
                "parseLong",
                "(Ljava/lang/String;)J",
                vec![Value::Ref(
                    fixture.shared.intern_string("9223372036854775807")
                )]
            )
            .unwrap()
        ),
        i64::MAX
    );
    assert_eq!(
        long_value(
            static_call(
                &fixture,
                "java/lang/Long",
                "parseLong",
                "(Ljava/lang/String;I)J",
                vec![
                    Value::Ref(fixture.shared.intern_string("-ff")),
                    Value::Int(16)
                ]
            )
            .unwrap()
        ),
        -255
    );

    // Out-of-range and malformed values are typed errors, not wrapped exceptions.
    assert_native_error(
        static_call(
            &fixture,
            "java/lang/Byte",
            "parseByte",
            "(Ljava/lang/String;)B",
            vec![Value::Ref(fixture.shared.intern_string("128"))],
        ),
        "out of range",
    );
    assert_native_error(
        static_call(
            &fixture,
            "java/lang/Short",
            "parseShort",
            "(Ljava/lang/String;)S",
            vec![Value::Ref(fixture.shared.intern_string("40000"))],
        ),
        "out of range",
    );
    assert_native_error(
        static_call(
            &fixture,
            "java/lang/Integer",
            "parseInt",
            "(Ljava/lang/String;)I",
            vec![Value::Ref(fixture.shared.intern_string("2147483648"))],
        ),
        "out of range",
    );
    assert_native_error(
        static_call(
            &fixture,
            "java/lang/Long",
            "parseLong",
            "(Ljava/lang/String;)J",
            vec![Value::Ref(
                fixture.shared.intern_string("9223372036854775808"),
            )],
        ),
        "out of range",
    );
    assert_native_error(
        static_call(
            &fixture,
            "java/lang/Integer",
            "parseInt",
            "(Ljava/lang/String;)I",
            vec![Value::Ref(fixture.shared.intern_string("12x"))],
        ),
        "not a radix 10 integer",
    );
    assert_native_error(
        static_call(
            &fixture,
            "java/lang/Integer",
            "parseInt",
            "(Ljava/lang/String;I)I",
            vec![
                Value::Ref(fixture.shared.intern_string("10")),
                Value::Int(1),
            ],
        ),
        "radix 1",
    );

    // The String-taking valueOf overloads box the parsed value.
    let byte_box = ref_value(
        static_call(
            &fixture,
            "java/lang/Byte",
            "valueOf",
            "(Ljava/lang/String;)Ljava/lang/Byte;",
            vec![Value::Ref(fixture.shared.intern_string("7"))],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(byte_box))),
        BoxedPrimitive::Byte(7)
    );
    let short_box = ref_value(
        static_call(
            &fixture,
            "java/lang/Short",
            "valueOf",
            "(Ljava/lang/String;I)Ljava/lang/Short;",
            vec![
                Value::Ref(fixture.shared.intern_string("10")),
                Value::Int(16),
            ],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(short_box))),
        BoxedPrimitive::Short(16)
    );
    let long_box = ref_value(
        static_call(
            &fixture,
            "java/lang/Long",
            "valueOf",
            "(Ljava/lang/String;)Ljava/lang/Long;",
            vec![Value::Ref(fixture.shared.intern_string("5"))],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(long_box))),
        BoxedPrimitive::Long(5)
    );

    // Floating parse accepts the suffix and the Java spellings, and errors on text.
    assert_eq!(
        float_value(
            static_call(
                &fixture,
                "java/lang/Float",
                "parseFloat",
                "(Ljava/lang/String;)F",
                vec![Value::Ref(fixture.shared.intern_string("1.5f"))]
            )
            .unwrap()
        ),
        1.5
    );
    assert!(
        float_value(
            static_call(
                &fixture,
                "java/lang/Float",
                "parseFloat",
                "(Ljava/lang/String;)F",
                vec![Value::Ref(fixture.shared.intern_string("NaN"))]
            )
            .unwrap()
        )
        .is_nan()
    );
    assert_eq!(
        double_value(
            static_call(
                &fixture,
                "java/lang/Double",
                "parseDouble",
                "(Ljava/lang/String;)D",
                vec![Value::Ref(fixture.shared.intern_string(" -Infinity "))]
            )
            .unwrap()
        ),
        f64::NEG_INFINITY
    );
    assert_eq!(
        double_value(
            static_call(
                &fixture,
                "java/lang/Double",
                "parseDouble",
                "(Ljava/lang/String;)D",
                vec![Value::Ref(fixture.shared.intern_string("2.25d"))]
            )
            .unwrap()
        ),
        2.25
    );
    assert_native_error(
        static_call(
            &fixture,
            "java/lang/Double",
            "parseDouble",
            "(Ljava/lang/String;)D",
            vec![Value::Ref(fixture.shared.intern_string("abc"))],
        ),
        "not a double literal",
    );
    let float_box = ref_value(
        static_call(
            &fixture,
            "java/lang/Float",
            "valueOf",
            "(Ljava/lang/String;)Ljava/lang/Float;",
            vec![Value::Ref(fixture.shared.intern_string("0.5"))],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(float_box))),
        BoxedPrimitive::Float(0.5)
    );
    let double_box = ref_value(
        static_call(
            &fixture,
            "java/lang/Double",
            "valueOf",
            "(Ljava/lang/String;)Ljava/lang/Double;",
            vec![Value::Ref(fixture.shared.intern_string("3.5"))],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(double_box))),
        BoxedPrimitive::Double(3.5)
    );
}

#[test]
fn character_and_boolean_surfaces_are_complete() {
    let fixture = fixture();

    let a = ref_value(
        static_call(
            &fixture,
            "java/lang/Character",
            "valueOf",
            "(C)Ljava/lang/Character;",
            vec![Value::Int(i32::from('A' as u16))],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(a))),
        BoxedPrimitive::Character('A')
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Character",
                "charValue",
                "()C",
                a,
                vec![]
            )
            .unwrap()
        ),
        i32::from('A' as u16)
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Character",
                "toString",
                "()Ljava/lang/String;",
                a,
                vec![]
            )
            .unwrap()
        ),
        "A"
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Character",
                "hashCode",
                "()I",
                a,
                vec![]
            )
            .unwrap()
        ),
        i32::from('A' as u16)
    );
    let other_a = boxed_ref(&fixture, BoxedPrimitive::Character('A'));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Character",
                "equals",
                "(Ljava/lang/Object;)Z",
                a,
                vec![Value::Ref(other_a)]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Character",
                "equals",
                "(Ljava/lang/Object;)Z",
                a,
                vec![Value::Ref(fixture.shared.intern_string("A"))]
            )
            .unwrap()
        ),
        0
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Character",
                "compareTo",
                "(Ljava/lang/Character;)I",
                boxed_ref(&fixture, BoxedPrimitive::Character('B')),
                vec![Value::Ref(a)]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Character",
                "compare",
                "(CC)I",
                vec![
                    Value::Int(i32::from('a' as u16)),
                    Value::Int(i32::from('b' as u16))
                ]
            )
            .unwrap()
        ),
        -1
    );

    let yes = ref_value(
        static_call(
            &fixture,
            "java/lang/Boolean",
            "valueOf",
            "(Z)Ljava/lang/Boolean;",
            vec![Value::Int(1)],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(yes))),
        BoxedPrimitive::Boolean(true)
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Boolean",
                "booleanValue",
                "()Z",
                yes,
                vec![]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            instance_call(
                &fixture,
                "java/lang/Boolean",
                "toString",
                "()Ljava/lang/String;",
                yes,
                vec![]
            )
            .unwrap()
        ),
        "true"
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Boolean",
                "hashCode",
                "()I",
                yes,
                vec![]
            )
            .unwrap()
        ),
        1231
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Boolean",
                "hashCode",
                "()I",
                boxed_ref(&fixture, BoxedPrimitive::Boolean(false)),
                vec![]
            )
            .unwrap()
        ),
        1237
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Boolean",
                "compareTo",
                "(Ljava/lang/Boolean;)I",
                yes,
                vec![Value::Ref(boxed_ref(
                    &fixture,
                    BoxedPrimitive::Boolean(false)
                ))]
            )
            .unwrap()
        ),
        1
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Boolean",
                "compare",
                "(ZZ)I",
                vec![Value::Int(0), Value::Int(1)]
            )
            .unwrap()
        ),
        -1
    );
    let from_string = ref_value(
        static_call(
            &fixture,
            "java/lang/Boolean",
            "valueOf",
            "(Ljava/lang/String;)Ljava/lang/Boolean;",
            vec![Value::Ref(fixture.shared.intern_string("TRUE"))],
        )
        .unwrap(),
    );
    assert_eq!(
        boxed_value(&fixture, Some(Value::Ref(from_string))),
        BoxedPrimitive::Boolean(true)
    );
    assert_eq!(
        int_value(
            static_call(
                &fixture,
                "java/lang/Boolean",
                "parseBoolean",
                "(Ljava/lang/String;)Z",
                vec![Value::Ref(fixture.shared.intern_string("yes"))]
            )
            .unwrap()
        ),
        0
    );
}

#[test]
fn string_value_of_renders_every_box_through_its_to_string() {
    let fixture = fixture();
    let cases = [
        (BoxedPrimitive::Byte(-5), "-5"),
        (BoxedPrimitive::Short(300), "300"),
        (BoxedPrimitive::Integer(70_000), "70000"),
        (BoxedPrimitive::Long(5_000_000_000), "5000000000"),
        (BoxedPrimitive::Float(1.5), "1.5"),
        (BoxedPrimitive::Float(1.0), "1.0"),
        (BoxedPrimitive::Double(2.25), "2.25"),
        (BoxedPrimitive::Double(-0.5), "-0.5"),
        (BoxedPrimitive::Character('A'), "A"),
        (BoxedPrimitive::Boolean(true), "true"),
        (BoxedPrimitive::Boolean(false), "false"),
    ];
    for (boxed, expected) in cases {
        let reference = boxed_ref(&fixture, boxed);
        let text = text_value(
            &fixture.shared,
            static_call(
                &fixture,
                "java/lang/String",
                "valueOf",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![Value::Ref(reference)],
            )
            .unwrap(),
        );
        assert_eq!(text, expected, "String.valueOf({boxed:?})");
    }

    // The primitive overloads keep their ordinary decimal/Java-float spelling.
    assert_eq!(
        text_value(
            &fixture.shared,
            static_call(
                &fixture,
                "java/lang/String",
                "valueOf",
                "(J)Ljava/lang/String;",
                vec![Value::Long(-9)]
            )
            .unwrap()
        ),
        "-9"
    );
    assert_eq!(
        text_value(
            &fixture.shared,
            static_call(
                &fixture,
                "java/lang/String",
                "valueOf",
                "(D)Ljava/lang/String;",
                vec![Value::Double(1.0)]
            )
            .unwrap()
        ),
        "1.0"
    );
}

#[test]
fn field_get_and_set_cover_every_primitive_kind() {
    let fixture = fixture();
    let probe = new_probe(&fixture);

    // Constructor.newInstance unboxed every argument into the right field kind.
    assert_eq!(probe_field(&fixture, probe, "b", "B"), Value::Int(-5));
    assert_eq!(probe_field(&fixture, probe, "s", "S"), Value::Int(300));
    assert_eq!(probe_field(&fixture, probe, "i", "I"), Value::Int(70_000));
    assert_eq!(
        probe_field(&fixture, probe, "j", "J"),
        Value::Long(5_000_000_000)
    );
    assert_eq!(probe_field(&fixture, probe, "f", "F"), Value::Float(1.5));
    assert_eq!(probe_field(&fixture, probe, "d", "D"), Value::Double(2.25));

    // Field.get boxes each one through its own wrapper.
    let expected = [
        ("b", "B", BoxedPrimitive::Byte(-5)),
        ("s", "S", BoxedPrimitive::Short(300)),
        ("i", "I", BoxedPrimitive::Integer(70_000)),
        ("j", "J", BoxedPrimitive::Long(5_000_000_000)),
        ("f", "F", BoxedPrimitive::Float(1.5)),
        ("d", "D", BoxedPrimitive::Double(2.25)),
    ];
    for (name, descriptor, boxed) in expected {
        let field = member(&fixture, MemberKind::Field, name, descriptor);
        let read = field_get(&fixture, field, probe).unwrap();
        assert_eq!(
            boxed_value(&fixture, read),
            boxed,
            "Field.get {name}:{descriptor}"
        );
    }

    // Field.set unboxes the matching wrapper and writes the primitive slot.
    for (name, descriptor, boxed, slot_value) in [
        ("b", "B", BoxedPrimitive::Byte(11), Value::Int(11)),
        ("s", "S", BoxedPrimitive::Short(-300), Value::Int(-300)),
        (
            "i",
            "I",
            BoxedPrimitive::Integer(-70_000),
            Value::Int(-70_000),
        ),
        ("j", "J", BoxedPrimitive::Long(-5), Value::Long(-5)),
        ("f", "F", BoxedPrimitive::Float(0.25), Value::Float(0.25)),
        (
            "d",
            "D",
            BoxedPrimitive::Double(-0.125),
            Value::Double(-0.125),
        ),
    ] {
        let field = member(&fixture, MemberKind::Field, name, descriptor);
        field_set(
            &fixture,
            field,
            probe,
            Value::Ref(boxed_ref(&fixture, boxed)),
        )
        .unwrap();
        assert_eq!(
            probe_field(&fixture, probe, name, descriptor),
            slot_value,
            "Field.set {name}:{descriptor}"
        );
    }

    // char and boolean round-trip through their own wrappers too.
    let char_field = member(&fixture, MemberKind::Field, "c", "C");
    let boolean_field = member(&fixture, MemberKind::Field, "z", "Z");
    field_set(
        &fixture,
        char_field,
        probe,
        Value::Ref(boxed_ref(&fixture, BoxedPrimitive::Character('Z'))),
    )
    .unwrap();
    field_set(
        &fixture,
        boolean_field,
        probe,
        Value::Ref(boxed_ref(&fixture, BoxedPrimitive::Boolean(true))),
    )
    .unwrap();
    assert_eq!(
        probe_field(&fixture, probe, "c", "C"),
        Value::Int(i32::from('Z' as u16))
    );
    assert_eq!(probe_field(&fixture, probe, "z", "Z"), Value::Int(1));
    assert_eq!(
        boxed_value(&fixture, field_get(&fixture, char_field, probe).unwrap()),
        BoxedPrimitive::Character('Z')
    );
    assert_eq!(
        boxed_value(&fixture, field_get(&fixture, boolean_field, probe).unwrap()),
        BoxedPrimitive::Boolean(true)
    );

    // A wrong wrapper for a primitive field is a typed error.
    assert_native_error(
        field_set(
            &fixture,
            member(&fixture, MemberKind::Field, "i", "I"),
            probe,
            Value::Ref(boxed_ref(&fixture, BoxedPrimitive::Long(1))),
        ),
        "does not widen",
    );
}

#[test]
fn method_invoke_boxes_every_primitive_result_and_accepts_wrappers() {
    let fixture = fixture();

    let cases = [
        (
            "echoB",
            "(B)B",
            BoxedPrimitive::Byte(-1),
            BoxedPrimitive::Byte(-1),
        ),
        (
            "echoS",
            "(S)S",
            BoxedPrimitive::Short(-2),
            BoxedPrimitive::Short(-2),
        ),
        (
            "echoI",
            "(I)I",
            BoxedPrimitive::Integer(-3),
            BoxedPrimitive::Integer(-3),
        ),
        (
            "echoJ",
            "(J)J",
            BoxedPrimitive::Long(-4),
            BoxedPrimitive::Long(-4),
        ),
        (
            "echoF",
            "(F)F",
            BoxedPrimitive::Float(0.5),
            BoxedPrimitive::Float(0.5),
        ),
        (
            "echoD",
            "(D)D",
            BoxedPrimitive::Double(0.25),
            BoxedPrimitive::Double(0.25),
        ),
        (
            "echoC",
            "(C)C",
            BoxedPrimitive::Character('Q'),
            BoxedPrimitive::Character('Q'),
        ),
        (
            "echoZ",
            "(Z)Z",
            BoxedPrimitive::Boolean(true),
            BoxedPrimitive::Boolean(true),
        ),
    ];
    for (name, descriptor, argument, expected) in cases {
        let method = member(&fixture, MemberKind::Method, name, descriptor);
        let arguments = reference_array(&fixture.shared, &[boxed_ref(&fixture, argument)]);
        let result = method_invoke(&fixture, method, Value::Ref(ObjRef::NULL), arguments).unwrap();
        assert_eq!(
            boxed_value(&fixture, result),
            expected,
            "Method.invoke {name}"
        );
    }

    // A narrower wrapper widens: an Integer argument reaches a long parameter.
    let method = member(&fixture, MemberKind::Method, "echoJ", "(J)J");
    let arguments = reference_array(
        &fixture.shared,
        &[boxed_ref(&fixture, BoxedPrimitive::Integer(7))],
    );
    let result = method_invoke(&fixture, method, Value::Ref(ObjRef::NULL), arguments).unwrap();
    assert_eq!(boxed_value(&fixture, result), BoxedPrimitive::Long(7));

    // A wider wrapper does not narrow: a Long argument cannot reach an int parameter.
    let method = member(&fixture, MemberKind::Method, "echoI", "(I)I");
    let arguments = reference_array(
        &fixture.shared,
        &[boxed_ref(&fixture, BoxedPrimitive::Long(7))],
    );
    assert_native_error(
        method_invoke(&fixture, method, Value::Ref(ObjRef::NULL), arguments),
        "does not widen",
    );
}

#[test]
fn constructor_new_instance_unboxes_every_primitive_kind() {
    let fixture = fixture();
    let constructor = member(&fixture, MemberKind::Constructor, "<init>", "(BSIJFD)V");
    let arguments = reference_array(
        &fixture.shared,
        &[
            boxed_ref(&fixture, BoxedPrimitive::Byte(-8)),
            boxed_ref(&fixture, BoxedPrimitive::Short(-9)),
            boxed_ref(&fixture, BoxedPrimitive::Integer(-10)),
            boxed_ref(&fixture, BoxedPrimitive::Long(i64::from(i32::MIN) - 1)),
            boxed_ref(&fixture, BoxedPrimitive::Float(0.125)),
            boxed_ref(&fixture, BoxedPrimitive::Double(0.0625)),
        ],
    );
    let probe = ref_value(
        constructor_new_instance(&fixture, constructor, Value::Ref(arguments))
            .expect("Constructor.newInstance succeeds"),
    );
    assert_eq!(probe_field(&fixture, probe, "b", "B"), Value::Int(-8));
    assert_eq!(probe_field(&fixture, probe, "s", "S"), Value::Int(-9));
    assert_eq!(probe_field(&fixture, probe, "i", "I"), Value::Int(-10));
    assert_eq!(
        probe_field(&fixture, probe, "j", "J"),
        Value::Long(i64::from(i32::MIN) - 1)
    );
    assert_eq!(probe_field(&fixture, probe, "f", "F"), Value::Float(0.125));
    assert_eq!(
        probe_field(&fixture, probe, "d", "D"),
        Value::Double(0.0625)
    );

    // The `(CZ)V` constructor completes the family.
    let constructor = member(&fixture, MemberKind::Constructor, "<init>", "(CZ)V");
    let arguments = reference_array(
        &fixture.shared,
        &[
            boxed_ref(&fixture, BoxedPrimitive::Character('W')),
            boxed_ref(&fixture, BoxedPrimitive::Boolean(true)),
        ],
    );
    let probe = ref_value(
        constructor_new_instance(&fixture, constructor, Value::Ref(arguments))
            .expect("Constructor.newInstance succeeds"),
    );
    assert_eq!(
        probe_field(&fixture, probe, "c", "C"),
        Value::Int(i32::from('W' as u16))
    );
    assert_eq!(probe_field(&fixture, probe, "z", "Z"), Value::Int(1));

    // A wrong box kind is a typed error naming the argument position.
    let arguments = reference_array(
        &fixture.shared,
        &[
            boxed_ref(&fixture, BoxedPrimitive::Boolean(true)),
            boxed_ref(&fixture, BoxedPrimitive::Boolean(true)),
        ],
    );
    let constructor = member(&fixture, MemberKind::Constructor, "<init>", "(CZ)V");
    assert_native_error(
        constructor_new_instance(&fixture, constructor, Value::Ref(arguments)),
        "argument 0",
    );
}

#[test]
fn number_base_class_accessors_dispatch_from_any_numeric_box() {
    let fixture = fixture();
    // A call resolved to `java.lang.Number` itself (not the wrapper override) still
    // converts the receiver's value.
    let byte = boxed_ref(&fixture, BoxedPrimitive::Byte(-5));
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Number",
                "intValue",
                "()I",
                byte,
                vec![]
            )
            .unwrap()
        ),
        -5
    );
    assert_eq!(
        long_value(
            instance_call(
                &fixture,
                "java/lang/Number",
                "longValue",
                "()J",
                byte,
                vec![]
            )
            .unwrap()
        ),
        -5
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Number",
                "byteValue",
                "()B",
                boxed_ref(&fixture, BoxedPrimitive::Integer(300)),
                vec![]
            )
            .unwrap()
        ),
        300_i32 as i8 as i32
    );
    assert_eq!(
        int_value(
            instance_call(
                &fixture,
                "java/lang/Number",
                "shortValue",
                "()S",
                boxed_ref(&fixture, BoxedPrimitive::Integer(70_000)),
                vec![]
            )
            .unwrap()
        ),
        70_000_i32 as i16 as i32
    );
    assert_eq!(
        double_value(
            instance_call(
                &fixture,
                "java/lang/Number",
                "doubleValue",
                "()D",
                boxed_ref(&fixture, BoxedPrimitive::Float(1.5)),
                vec![]
            )
            .unwrap()
        ),
        1.5
    );
    // A non-Number box has no accessor.
    assert_native_error(
        instance_call(
            &fixture,
            "java/lang/Number",
            "intValue",
            "()I",
            boxed_ref(&fixture, BoxedPrimitive::Character('A')),
            vec![],
        ),
        "not a java.lang.Number",
    );
}
