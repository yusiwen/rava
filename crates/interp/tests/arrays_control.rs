//! M2.6 acceptance: arrays and integer control flow.
//!
//! `ArrayProbe` is assembled with the real `newarray`/`anewarray`/`multianewarray`,
//! `arraylength`, array load/store and integer branch opcodes. Every method is a static
//! helper the test calls directly, so the assertions run through the interpreter's
//! decode-and-dispatch path rather than a hand-built frame.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder, ClassFile};
use rava_interp::{Instr, IntCondition, InterpError, decode, run_static_method};
use rava_vm::{
    ArrayKind, ClassId, ClassRef, HeapError, LoaderId, ObjRef, PrimitiveKind, Shared, ThreadState,
    Value,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

/// A factory method `static <descriptor> name(int n) { return new <atype>[n]; }`.
fn array_factory(builder: &mut ClassBuilder, name: &str, descriptor: &str, atype: u8) {
    let mut body = Assembler::new();
    body.iload(0);
    body.newarray(atype);
    body.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        name,
        descriptor,
        Some((1, 1, body.finish().unwrap())),
    );
}

/// A comparison method `static boolean name(int a, int b)` over one `if_icmp` opcode.
fn comparison_method(builder: &mut ClassBuilder, name: &str, opcode: u8) {
    let mut body = Assembler::new();
    let true_label = body.label();
    body.iload(0);
    body.iload(1);
    body.if_icmp(opcode, true_label);
    body.iconst(0);
    body.ireturn();
    body.place(true_label);
    body.iconst(1);
    body.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        name,
        "(II)Z",
        Some((2, 2, body.finish().unwrap())),
    );
}

/// A predicate `static boolean name(int value)` over one `if<cond>` opcode.
fn condition_method(builder: &mut ClassBuilder, name: &str, opcode: u8) {
    let mut body = Assembler::new();
    let true_label = body.label();
    body.iload(0);
    body.if_cond(opcode, true_label);
    body.iconst(0);
    body.ireturn();
    body.place(true_label);
    body.iconst(1);
    body.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        name,
        "(I)Z",
        Some((1, 1, body.finish().unwrap())),
    );
}

fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ArrayProbe", "java/lang/Object", 61);

    let object_class = builder.class("java/lang/Object");
    let grid_class = builder.class("[[I");
    let item = builder.string("item");

    // static int sum(int[] values) {
    //     int i = 0;
    //     int total = 0;
    //     while (i < values.length) { total += values[i]; i++; }
    //     return total;
    // }
    let mut sum = Assembler::new();
    let sum_loop = sum.label();
    let sum_done = sum.label();
    sum.iconst(0);
    sum.istore(1);
    sum.iconst(0);
    sum.istore(2);
    sum.place(sum_loop);
    sum.iload(1);
    sum.iload(0);
    sum.arraylength();
    sum.if_icmp(0xa2, sum_done); // if_icmpge
    sum.iload(2);
    sum.iload(0);
    sum.iload(1);
    sum.iaload();
    sum.iadd();
    sum.istore(2);
    sum.iinc(1, 1);
    sum.goto(sum_loop);
    sum.place(sum_done);
    sum.iload(2);
    sum.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "sum",
        "([I)I",
        Some((3, 3, sum.finish().unwrap())),
    );

    // static int[] make(int n) {
    //     int[] values = new int[n];
    //     int i = 0;
    //     while (i < n) { values[i] = i * 10; i++; }
    //     return values;
    // }
    let mut make = Assembler::new();
    let make_loop = make.label();
    let make_done = make.label();
    make.iload(0);
    make.newarray(10); // T_INT
    make.astore(1);
    make.iconst(0);
    make.istore(2);
    make.place(make_loop);
    make.iload(2);
    make.iload(0);
    make.if_icmp(0xa2, make_done); // if_icmpge
    make.aload(1);
    make.iload(2);
    make.iload(2);
    make.iconst(10);
    make.imul();
    make.iastore();
    make.iinc(2, 1);
    make.goto(make_loop);
    make.place(make_done);
    make.aload(1);
    make.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "make",
        "(I)[I",
        Some((4, 3, make.finish().unwrap())),
    );

    // static int get(int[] values, int index) { return values[index]; }
    let mut get = Assembler::new();
    get.aload(0);
    get.iload(1);
    get.iaload();
    get.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "get",
        "([II)I",
        Some((2, 2, get.finish().unwrap())),
    );

    // static void set(int[] values, int index, int value) { values[index] = value; }
    let mut set = Assembler::new();
    set.aload(0);
    set.iload(1);
    set.iload(2);
    set.iastore();
    set.return_();
    builder.method(
        ACC_PUBLIC_STATIC,
        "set",
        "([III)V",
        Some((3, 3, set.finish().unwrap())),
    );

    // static Object[] refs(int n) { ... a[i] = "item"; ... }
    let mut refs = Assembler::new();
    let refs_loop = refs.label();
    let refs_done = refs.label();
    refs.iload(0);
    refs.anewarray(object_class);
    refs.astore(1);
    refs.iconst(0);
    refs.istore(2);
    refs.place(refs_loop);
    refs.iload(2);
    refs.iload(0);
    refs.if_icmp(0xa2, refs_done); // if_icmpge
    refs.aload(1);
    refs.iload(2);
    refs.ldc(item);
    refs.aastore();
    refs.iinc(2, 1);
    refs.goto(refs_loop);
    refs.place(refs_done);
    refs.aload(1);
    refs.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "refs",
        "(I)[Ljava/lang/Object;",
        Some((3, 3, refs.finish().unwrap())),
    );

    // static String element(Object[] values, int index) { return (String) values[index]; }
    // The cast is absent: the interpreter returns the reference the array holds.
    let mut element = Assembler::new();
    element.aload(0);
    element.iload(1);
    element.aaload();
    element.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "element",
        "([Ljava/lang/Object;I)Ljava/lang/String;",
        Some((2, 2, element.finish().unwrap())),
    );

    // static void badRefStore(int[] values) { values[0] = null; }
    let mut bad_ref_store = Assembler::new();
    bad_ref_store.aload(0);
    bad_ref_store.iconst(0);
    bad_ref_store.aconst_null();
    bad_ref_store.aastore();
    bad_ref_store.return_();
    builder.method(
        ACC_PUBLIC_STATIC,
        "badRefStore",
        "([I)V",
        Some((3, 1, bad_ref_store.finish().unwrap())),
    );

    // static int badIntLoad(long[] values, int index) { return values[index]; }
    let mut bad_int_load = Assembler::new();
    bad_int_load.aload(0);
    bad_int_load.iload(1);
    bad_int_load.iaload();
    bad_int_load.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "badIntLoad",
        "([JI)I",
        Some((2, 2, bad_int_load.finish().unwrap())),
    );

    // static void copyLong(long[] values, int from, int to) { values[to] = values[from]; }
    // A wide element (category 2 `Value`) round-trips through `laload` and `lastore`.
    let mut copy_long = Assembler::new();
    copy_long.aload(0);
    copy_long.iload(2);
    copy_long.aload(0);
    copy_long.iload(1);
    copy_long.op(0x2f, &[]); // laload
    copy_long.op(0x50, &[]); // lastore
    copy_long.return_();
    builder.method(
        ACC_PUBLIC_STATIC,
        "copyLong",
        "([JII)V",
        Some((4, 3, copy_long.finish().unwrap())),
    );

    // `baload` accepts a `boolean[]` as well as a `byte[]` (JVMS §6.5.baload).
    let mut boolean_at = Assembler::new();
    boolean_at.aload(0);
    boolean_at.iload(1);
    boolean_at.op(0x33, &[]); // baload
    boolean_at.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "booleanAt",
        "([ZI)Z",
        Some((2, 2, boolean_at.finish().unwrap())),
    );

    let mut byte_at = Assembler::new();
    byte_at.aload(0);
    byte_at.iload(1);
    byte_at.op(0x33, &[]); // baload
    byte_at.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "byteAt",
        "([BI)B",
        Some((2, 2, byte_at.finish().unwrap())),
    );

    // Every primitive `atype` value, so the decoder's whole table is exercised.
    array_factory(&mut builder, "booleans", "(I)[Z", 4);
    array_factory(&mut builder, "chars", "(I)[C", 5);
    array_factory(&mut builder, "floats", "(I)[F", 6);
    array_factory(&mut builder, "doubles", "(I)[D", 7);
    array_factory(&mut builder, "bytes", "(I)[B", 8);
    array_factory(&mut builder, "shorts", "(I)[S", 9);
    array_factory(&mut builder, "ints", "(I)[I", 10);
    array_factory(&mut builder, "longs", "(I)[J", 11);

    // static int sign(int value) { if (value > 0) return 1; if (value < 0) return -1; return 0; }
    let mut sign = Assembler::new();
    let positive = sign.label();
    let negative = sign.label();
    sign.iload(0);
    sign.if_cond(0x9d, positive); // ifgt
    sign.iload(0);
    sign.if_cond(0x9b, negative); // iflt
    sign.iconst(0);
    sign.ireturn();
    sign.place(positive);
    sign.iconst(1);
    sign.ireturn();
    sign.place(negative);
    sign.iconst(-1);
    sign.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "sign",
        "(I)I",
        Some((1, 1, sign.finish().unwrap())),
    );

    // static boolean isNull(Object o) { return o == null; }
    let mut is_null = Assembler::new();
    let is_null_label = is_null.label();
    is_null.aload(0);
    is_null.if_cond(0xc6, is_null_label); // ifnull
    is_null.iconst(0);
    is_null.ireturn();
    is_null.place(is_null_label);
    is_null.iconst(1);
    is_null.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "isNull",
        "(Ljava/lang/Object;)Z",
        Some((1, 1, is_null.finish().unwrap())),
    );

    // static boolean isNotNull(Object o) { return o != null; }
    let mut is_not_null = Assembler::new();
    let is_not_null_label = is_not_null.label();
    is_not_null.aload(0);
    is_not_null.if_cond(0xc7, is_not_null_label); // ifnonnull
    is_not_null.iconst(0);
    is_not_null.ireturn();
    is_not_null.place(is_not_null_label);
    is_not_null.iconst(1);
    is_not_null.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "isNotNull",
        "(Ljava/lang/Object;)Z",
        Some((1, 1, is_not_null.finish().unwrap())),
    );

    // static int[][] grid(int rows, int cols) { return new int[rows][cols]; }
    let mut grid = Assembler::new();
    grid.iload(0);
    grid.iload(1);
    grid.multianewarray(grid_class, 2);
    grid.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "grid",
        "(II)[[I",
        Some((2, 2, grid.finish().unwrap())),
    );

    // static int[][] rows(int n) { return new int[n][]; }
    let mut rows = Assembler::new();
    rows.iload(0);
    rows.multianewarray(grid_class, 1);
    rows.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "rows",
        "(I)[[I",
        Some((1, 1, rows.finish().unwrap())),
    );

    condition_method(&mut builder, "isZero", 0x99); // ifeq
    condition_method(&mut builder, "isNonZero", 0x9a); // ifne
    comparison_method(&mut builder, "eq", 0x9f); // if_icmpeq
    comparison_method(&mut builder, "ne", 0xa0); // if_icmpne
    comparison_method(&mut builder, "lt", 0xa1); // if_icmplt
    comparison_method(&mut builder, "ge", 0xa2); // if_icmpge
    comparison_method(&mut builder, "gt", 0xa3); // if_icmpgt
    comparison_method(&mut builder, "le", 0xa4); // if_icmple

    builder.build()
}

struct Fixture {
    shared: Shared,
    probe: ClassId,
    thread: ThreadState,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "ArrayProbe", &probe_bytes())
        .expect("ArrayProbe defines");
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
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        method,
        descriptor,
        args,
    )
}

fn ok_call(fixture: &Fixture, method: &str, descriptor: &str, args: Vec<Value>) -> Option<Value> {
    call(fixture, method, descriptor, args).unwrap_or_else(|error| {
        panic!("{method}{descriptor} failed: {error}");
    })
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

fn make_array(fixture: &Fixture, length: i32) -> ObjRef {
    ref_value(ok_call(fixture, "make", "(I)[I", vec![Value::Int(length)]))
}

#[test]
fn sum_walks_an_array_with_if_icmpge_goto_and_iaload() {
    let fixture = fixture();
    let array = make_array(&fixture, 5);
    assert_eq!(
        int_value(ok_call(&fixture, "sum", "([I)I", vec![Value::Ref(array)])),
        100,
        "0 + 10 + 20 + 30 + 40"
    );

    let empty = make_array(&fixture, 0);
    assert_eq!(
        int_value(ok_call(&fixture, "sum", "([I)I", vec![Value::Ref(empty)])),
        0
    );
    assert_eq!(fixture.thread.frame_depth(), 0);
    assert!(fixture.thread.frame_roots().is_empty());
}

#[test]
fn make_allocates_the_requested_length_and_stores_each_element() {
    let fixture = fixture();
    let array = make_array(&fixture, 4);
    {
        let heap = fixture.shared.heap();
        let array = heap.array(array).expect("make returns an array");
        assert_eq!(array.kind, ArrayKind::Int);
        assert_eq!(
            array.elements,
            vec![
                Value::Int(0),
                Value::Int(10),
                Value::Int(20),
                Value::Int(30)
            ]
        );
    }

    // `ints` allocates through `newarray` without storing, so the JVM defaults show.
    let ints = ref_value(ok_call(&fixture, "ints", "(I)[I", vec![Value::Int(3)]));
    let heap = fixture.shared.heap();
    assert_eq!(heap.array(ints).unwrap().elements, vec![Value::Int(0); 3]);
    drop(heap);

    let empty = ref_value(ok_call(&fixture, "ints", "(I)[I", vec![Value::Int(0)]));
    assert!(
        fixture
            .shared
            .heap()
            .array(empty)
            .unwrap()
            .elements
            .is_empty()
    );
}

#[test]
fn every_primitive_atype_allocates_its_array_kind_with_defaults() {
    let fixture = fixture();
    let cases = [
        ("booleans", "(I)[Z", ArrayKind::Boolean),
        ("chars", "(I)[C", ArrayKind::Char),
        ("floats", "(I)[F", ArrayKind::Float),
        ("doubles", "(I)[D", ArrayKind::Double),
        ("bytes", "(I)[B", ArrayKind::Byte),
        ("shorts", "(I)[S", ArrayKind::Short),
        ("ints", "(I)[I", ArrayKind::Int),
        ("longs", "(I)[J", ArrayKind::Long),
    ];
    for (method, descriptor, kind) in cases {
        let array = ref_value(ok_call(&fixture, method, descriptor, vec![Value::Int(2)]));
        let heap = fixture.shared.heap();
        let array = heap.array(array).expect("a factory returns an array");
        assert_eq!(array.kind, kind, "{method}");
        assert_eq!(array.elements, vec![kind.default_value(); 2], "{method}");
    }
}

#[test]
fn reference_arrays_round_trip_aastore_and_aaload() {
    let fixture = fixture();
    let array = ref_value(ok_call(
        &fixture,
        "refs",
        "(I)[Ljava/lang/Object;",
        vec![Value::Int(3)],
    ));
    {
        let heap = fixture.shared.heap();
        let array = heap.array(array).expect("refs returns an array");
        assert_eq!(array.kind, ArrayKind::Reference);
        assert_eq!(array.elements.len(), 3);
        for element in &array.elements {
            let Value::Ref(reference) = element else {
                panic!("a reference array holds references, found {element:?}");
            };
            assert_eq!(heap.string(*reference).unwrap(), "item");
        }
    }

    let element = ok_call(
        &fixture,
        "element",
        "([Ljava/lang/Object;I)Ljava/lang/String;",
        vec![Value::Ref(array), Value::Int(1)],
    );
    let Some(Value::Ref(element)) = element else {
        panic!("element returned {element:?}");
    };
    assert_eq!(fixture.shared.heap().string(element).unwrap(), "item");
}

#[test]
fn baload_accepts_both_boolean_and_byte_arrays() {
    let fixture = fixture();

    let booleans = ref_value(ok_call(&fixture, "booleans", "(I)[Z", vec![Value::Int(2)]));
    assert_eq!(
        int_value(ok_call(
            &fixture,
            "booleanAt",
            "([ZI)Z",
            vec![Value::Ref(booleans), Value::Int(0)]
        )),
        0
    );

    let bytes = ref_value(ok_call(&fixture, "bytes", "(I)[B", vec![Value::Int(2)]));
    assert_eq!(
        int_value(ok_call(
            &fixture,
            "byteAt",
            "([BI)B",
            vec![Value::Ref(bytes), Value::Int(1)]
        )),
        0
    );
}

#[test]
fn a_wide_array_element_round_trips_through_laload_and_lastore() {
    let fixture = fixture();
    let longs = ref_value(ok_call(&fixture, "longs", "(I)[J", vec![Value::Int(3)]));
    fixture.shared.heap().array_mut(longs).unwrap().elements[0] = Value::Long(9);

    ok_call(
        &fixture,
        "copyLong",
        "([JII)V",
        vec![Value::Ref(longs), Value::Int(0), Value::Int(2)],
    );
    let heap = fixture.shared.heap();
    let longs = heap.array(longs).expect("longs returns an array");
    assert_eq!(longs.kind, ArrayKind::Long);
    assert_eq!(
        longs.elements,
        vec![Value::Long(9), Value::Long(0), Value::Long(9)]
    );
}

#[test]
fn multianewarray_allocates_each_requested_dimension() {
    let fixture = fixture();
    let grid = ref_value(ok_call(
        &fixture,
        "grid",
        "(II)[[I",
        vec![Value::Int(2), Value::Int(3)],
    ));
    {
        let heap = fixture.shared.heap();
        let outer = heap.array(grid).expect("grid returns an array");
        assert_eq!(outer.kind, ArrayKind::Reference);
        assert_eq!(outer.elements.len(), 2);
        for row in &outer.elements {
            let Value::Ref(row) = row else {
                panic!("a row is a reference, found {row:?}");
            };
            let row = heap.array(*row).expect("a row is an array");
            assert_eq!(row.kind, ArrayKind::Int);
            assert_eq!(row.elements, vec![Value::Int(0); 3]);
        }
    }

    // One dimension of two: the inner references stay null, as in `new int[3][]`.
    let rows = ref_value(ok_call(&fixture, "rows", "(I)[[I", vec![Value::Int(3)]));
    let heap = fixture.shared.heap();
    let rows = heap.array(rows).expect("rows returns an array");
    assert_eq!(rows.kind, ArrayKind::Reference);
    assert_eq!(rows.elements, vec![Value::Ref(ObjRef::NULL); 3]);
}

#[test]
fn bounds_null_and_negative_sizes_are_typed_errors() {
    let fixture = fixture();

    let error = call(&fixture, "sum", "([I)I", vec![Value::Ref(ObjRef::NULL)]);
    assert!(
        matches!(error, Err(InterpError::Heap(HeapError::NullReference))),
        "found {error:?}"
    );

    let array = make_array(&fixture, 3);
    match call(
        &fixture,
        "get",
        "([II)I",
        vec![Value::Ref(array), Value::Int(3)],
    ) {
        Err(InterpError::ArrayIndexOutOfBounds {
            index: 3,
            length: 3,
        }) => {}
        other => panic!("expected a bounds error at 3, found {other:?}"),
    }
    match call(
        &fixture,
        "get",
        "([II)I",
        vec![Value::Ref(array), Value::Int(-1)],
    ) {
        Err(InterpError::ArrayIndexOutOfBounds {
            index: -1,
            length: 3,
        }) => {}
        other => panic!("expected a bounds error at -1, found {other:?}"),
    }
    match call(
        &fixture,
        "set",
        "([III)V",
        vec![Value::Ref(array), Value::Int(5), Value::Int(7)],
    ) {
        Err(InterpError::ArrayIndexOutOfBounds {
            index: 5,
            length: 3,
        }) => {}
        other => panic!("expected a bounds error on a store, found {other:?}"),
    }
    match call(&fixture, "make", "(I)[I", vec![Value::Int(-1)]) {
        Err(InterpError::NegativeArraySize(-1)) => {}
        other => panic!("expected a negative-size error, found {other:?}"),
    }

    // A non-array reference keeps the heap's typed kind error.
    let string = fixture.shared.heap().allocate_string("not an array");
    match call(&fixture, "sum", "([I)I", vec![Value::Ref(string)]) {
        Err(InterpError::Heap(HeapError::WrongObjectKind {
            expected: "an array",
            ..
        })) => {}
        other => panic!("expected an object-kind error, found {other:?}"),
    }
}

#[test]
fn array_element_kinds_are_checked() {
    let fixture = fixture();

    let longs = fixture
        .shared
        .heap()
        .allocate_array(ClassRef::Primitive(PrimitiveKind::Long), 2);
    match call(
        &fixture,
        "badIntLoad",
        "([JI)I",
        vec![Value::Ref(longs), Value::Int(0)],
    ) {
        Err(InterpError::ArrayKindMismatch {
            expected: "int",
            found: ArrayKind::Long,
        }) => {}
        other => panic!("expected an int/long mismatch, found {other:?}"),
    }

    let ints = make_array(&fixture, 2);
    match call(&fixture, "badRefStore", "([I)V", vec![Value::Ref(ints)]) {
        Err(InterpError::ArrayKindMismatch {
            expected: "reference",
            found: ArrayKind::Int,
        }) => {}
        other => panic!("expected a reference/int mismatch, found {other:?}"),
    }
    match call(
        &fixture,
        "element",
        "([Ljava/lang/Object;I)Ljava/lang/String;",
        vec![Value::Ref(ints), Value::Int(0)],
    ) {
        Err(InterpError::ArrayKindMismatch {
            expected: "reference",
            found: ArrayKind::Int,
        }) => {}
        other => panic!("expected a reference/int mismatch, found {other:?}"),
    }
    match call(
        &fixture,
        "booleanAt",
        "([ZI)Z",
        vec![Value::Ref(ints), Value::Int(0)],
    ) {
        Err(InterpError::ArrayKindMismatch {
            expected: "boolean or byte",
            found: ArrayKind::Int,
        }) => {}
        other => panic!("expected a boolean-or-byte mismatch, found {other:?}"),
    }

    // A store whose value does not fit the element slot is rejected, not coerced.
    match call(
        &fixture,
        "set",
        "([III)V",
        vec![Value::Ref(ints), Value::Int(0), Value::Long(1)],
    ) {
        Err(InterpError::WrongValue) => {}
        other => panic!("expected a wrong-value error, found {other:?}"),
    }
}

#[test]
fn every_integer_comparison_branches_like_java() {
    let fixture = fixture();
    let cases = [
        (0, 0),
        (1, 0),
        (0, 1),
        (-1, 1),
        (7, 7),
        (i32::MIN, i32::MAX),
        (i32::MAX, i32::MIN),
    ];
    for (left, right) in cases {
        let expected = [
            ("eq", left == right),
            ("ne", left != right),
            ("lt", left < right),
            ("ge", left >= right),
            ("gt", left > right),
            ("le", left <= right),
        ];
        for (method, expected) in expected {
            let actual = int_value(ok_call(
                &fixture,
                method,
                "(II)Z",
                vec![Value::Int(left), Value::Int(right)],
            )) != 0;
            assert_eq!(actual, expected, "{method}({left}, {right})");
        }
    }
}

#[test]
fn ifeq_ifne_and_null_branches_take_the_right_side() {
    let fixture = fixture();

    assert_eq!(
        int_value(ok_call(&fixture, "isZero", "(I)Z", vec![Value::Int(0)])),
        1
    );
    assert_eq!(
        int_value(ok_call(&fixture, "isZero", "(I)Z", vec![Value::Int(5)])),
        0
    );
    assert_eq!(
        int_value(ok_call(&fixture, "isNonZero", "(I)Z", vec![Value::Int(0)])),
        0
    );
    assert_eq!(
        int_value(ok_call(&fixture, "isNonZero", "(I)Z", vec![Value::Int(5)])),
        1
    );

    assert_eq!(
        int_value(ok_call(&fixture, "sign", "(I)I", vec![Value::Int(-5)])),
        -1
    );
    assert_eq!(
        int_value(ok_call(&fixture, "sign", "(I)I", vec![Value::Int(0)])),
        0
    );
    assert_eq!(
        int_value(ok_call(&fixture, "sign", "(I)I", vec![Value::Int(9)])),
        1
    );

    let string = fixture.shared.heap().allocate_string("present");
    assert_eq!(
        int_value(ok_call(
            &fixture,
            "isNull",
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(ObjRef::NULL)]
        )),
        1
    );
    assert_eq!(
        int_value(ok_call(
            &fixture,
            "isNull",
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(string)]
        )),
        0
    );
    assert_eq!(
        int_value(ok_call(
            &fixture,
            "isNotNull",
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(ObjRef::NULL)]
        )),
        0
    );
    assert_eq!(
        int_value(ok_call(
            &fixture,
            "isNotNull",
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(string)]
        )),
        1
    );
}

#[test]
fn branch_targets_are_instruction_indices_resolved_at_decode_time() {
    let bytes = probe_bytes();
    let file = ClassFile::parse(&bytes).expect("the fixture parses");

    // `eq` is `iload_0; iload_1; if_icmpeq; iconst_0; ireturn; iconst_1; ireturn`.
    // The branch's byte target is offset 7 but the decoded target is instruction 5.
    let method = file.find_method("eq", "(II)Z").unwrap().expect("eq exists");
    let code = file.code(method).unwrap().expect("eq has Code");
    let decoded = decode(&file, &code.data).expect("eq decodes");
    assert_eq!(
        decoded.instructions[2],
        Instr::IfICmp(IntCondition::Eq, 5),
        "the branch names an instruction index, not its byte offset"
    );

    // `sum`'s `goto` jumps back to the loop head, and its fall-through is the
    // `iload_2` that follows the loop.
    let method = file
        .find_method("sum", "([I)I")
        .unwrap()
        .expect("sum exists");
    let code = file.code(method).unwrap().expect("sum has Code");
    let decoded = decode(&file, &code.data).expect("sum decodes");
    let goto = decoded
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instr::Goto(_)))
        .expect("sum has a goto");
    let Instr::Goto(target) = decoded.instructions[goto] else {
        panic!("instruction {goto} is not a goto");
    };
    assert!(
        matches!(decoded.instructions[target], Instr::ILoad(1)),
        "the goto returns to the loop head, target {target}"
    );
    assert!(
        matches!(decoded.instructions[goto + 1], Instr::ILoad(2)),
        "the branch falls through to `iload_2` after the jump"
    );
}
