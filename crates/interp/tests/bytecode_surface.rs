//! M2.21 acceptance: static fields, casts, switches and numeric constants.
//!
//! `BytecodeProbe` is assembled in Rust so every instruction this slice claims is
//! exercised through the interpreter's decode-and-dispatch path:
//!
//! - `putstatic`/`getstatic` for `int`, `boolean`, `String`, `long`, `float` and
//!   `double` fields, including a field declared on a superclass;
//! - `instanceof`/`checkcast` for a named class, an interface, a covariant array, a
//!   primitive array, `Object`, `Cloneable`/`Serializable`, a primitive operand and
//!   `null`;
//! - dense `tableswitch` and sparse `lookupswitch` methods with a default;
//! - `ldc`/`ldc_w` of `int`/`float` and `ldc2_w` of `long`/`double`, plus the
//!   `lreturn`/`freturn`/`dreturn` and `lcmp`/`fcmp<op>`/`dcmp<op>` forms that return and
//!   compare them.
//!
//! The same probe carries deliberately malformed methods (an unsorted/duplicate
//! `lookupswitch`, a non-boundary switch target, non-zero padding, a truncated switch, an
//! inverted `tableswitch` range, a negative `lookupswitch` pair count and an `ldc2_w` of
//! an `int`), so the typed decode errors are callable rather than asserted on a
//! hand-built `DecodedMethod`.
//!
//! One child-loader probe proves `putstatic` resolves its owner through the executing
//! class's loader: the class it writes exists only in that child, so a bootstrap
//! resolution would be a class-not-found error.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder, ClassFile};
use rava_interp::{Instr, InterpError, decode, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassSource, FieldLayout, LoaderId, ObjRef, PrimitiveKind, Shared, ThreadState, Value,
    VmError,
};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;
/// `ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT`.
const ACC_INTERFACE_ABSTRACT: u16 = 0x0601;

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

fn instance_method(
    builder: &mut ClassBuilder,
    name: &str,
    descriptor: &str,
    max_stack: u16,
    max_locals: u16,
    body: Assembler,
) {
    builder.method(
        ACC_PUBLIC,
        name,
        descriptor,
        Some((max_stack, max_locals, body.finish().unwrap())),
    );
}

/// `static boolean name(Object value) { return value instanceof <class>; }`.
fn instanceof_method(builder: &mut ClassBuilder, name: &str, class: u16) {
    let mut body = Assembler::new();
    body.aload(0);
    body.instanceof(class);
    body.ireturn();
    static_method(builder, name, "(Ljava/lang/Object;)Z", 1, 1, body);
}

/// `static Object name(Object value) { return (Class) value; }`.
fn checkcast_method(builder: &mut ClassBuilder, name: &str, class: u16) {
    let mut body = Assembler::new();
    body.aload(0);
    body.checkcast(class);
    body.areturn();
    static_method(
        builder,
        name,
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        1,
        1,
        body,
    );
}

/// A static setter over one static field, given the argument load instruction.
fn setter(
    builder: &mut ClassBuilder,
    name: &str,
    descriptor: &str,
    max_locals: u16,
    field: u16,
    load: impl FnOnce(&mut Assembler),
) {
    let mut body = Assembler::new();
    load(&mut body);
    body.putstatic(field);
    body.return_();
    static_method(builder, name, descriptor, 1, max_locals, body);
}

/// A static getter over one static field, given the return instruction.
fn getter(
    builder: &mut ClassBuilder,
    name: &str,
    descriptor: &str,
    field: u16,
    ret: fn(&mut Assembler),
) {
    let mut body = Assembler::new();
    body.getstatic(field);
    ret(&mut body);
    static_method(builder, name, descriptor, 1, 0, body);
}

fn animal_bytes() -> Vec<u8> {
    ClassBuilder::new("Animal", "java/lang/Object", 61).build()
}

fn cat_bytes() -> Vec<u8> {
    ClassBuilder::new("Cat", "Animal", 61).build()
}

fn pet_bytes() -> Vec<u8> {
    let mut builder =
        ClassBuilder::new("Pet", "java/lang/Object", 61).with_access(ACC_INTERFACE_ABSTRACT);
    builder.build()
}

fn dog_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Dog", "Animal", 61);
    builder.interface("Pet");
    builder.build()
}

fn base_statics_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("BaseStatics", "java/lang/Object", 61);
    builder.field(ACC_PUBLIC_STATIC, "shared", "I");
    builder.build()
}

fn sub_statics_bytes() -> Vec<u8> {
    ClassBuilder::new("SubStatics", "BaseStatics", 61).build()
}

/// The operand block of a `tableswitch` whose opcode is the first instruction (three
/// padding bytes), so a fixture can build malformed tables that the safe assembler
/// helpers would refuse to emit.
fn tableswitch_operand(default: i32, low: i32, high: i32, offsets: &[i32]) -> Vec<u8> {
    let mut bytes = vec![0u8; 3];
    bytes.extend_from_slice(&default.to_be_bytes());
    bytes.extend_from_slice(&low.to_be_bytes());
    bytes.extend_from_slice(&high.to_be_bytes());
    for offset in offsets {
        bytes.extend_from_slice(&offset.to_be_bytes());
    }
    bytes
}

/// The operand block of a `lookupswitch` at offset 0 (three padding bytes).
fn lookupswitch_operand(default: i32, pairs: &[(i32, i32)]) -> Vec<u8> {
    let mut bytes = vec![0u8; 3];
    bytes.extend_from_slice(&default.to_be_bytes());
    bytes.extend_from_slice(&(pairs.len() as i32).to_be_bytes());
    for (key, offset) in pairs {
        bytes.extend_from_slice(&key.to_be_bytes());
        bytes.extend_from_slice(&offset.to_be_bytes());
    }
    bytes
}

/// `BytecodeProbe`: every instruction shape this slice claims, plus the malformed switch
/// methods the decode-error tests call.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("BytecodeProbe", "java/lang/Object", 61);

    builder.field(ACC_PUBLIC, "instanceValue", "I");
    builder.field(ACC_PUBLIC_STATIC, "counter", "I");
    builder.field(ACC_PUBLIC_STATIC, "label", "Ljava/lang/String;");
    builder.field(ACC_PUBLIC_STATIC, "flag", "Z");
    builder.field(ACC_PUBLIC_STATIC, "total", "J");
    builder.field(ACC_PUBLIC_STATIC, "score", "F");
    builder.field(ACC_PUBLIC_STATIC, "ratio", "D");

    let counter = builder.field_ref("BytecodeProbe", "counter", "I");
    let label = builder.field_ref("BytecodeProbe", "label", "Ljava/lang/String;");
    let flag = builder.field_ref("BytecodeProbe", "flag", "Z");
    let total = builder.field_ref("BytecodeProbe", "total", "J");
    let score = builder.field_ref("BytecodeProbe", "score", "F");
    let ratio = builder.field_ref("BytecodeProbe", "ratio", "D");
    let instance_value = builder.field_ref("BytecodeProbe", "instanceValue", "I");
    let counter_as_long = builder.field_ref("BytecodeProbe", "counter", "J");
    let absent = builder.field_ref("BytecodeProbe", "absent", "I");
    let inherited = builder.field_ref("SubStatics", "shared", "I");
    let integer_type = builder.field_ref("java/lang/Integer", "TYPE", "Ljava/lang/Class;");
    let system_out = builder.field_ref("java/lang/System", "out", "Ljava/io/PrintStream;");

    let animal = builder.class("Animal");
    let dog = builder.class("Dog");
    let pet = builder.class("Pet");
    let dog_array = builder.class("[LDog;");
    let animal_array = builder.class("[LAnimal;");
    let cat_array = builder.class("[LCat;");
    let object_array = builder.class("[Ljava/lang/Object;");
    let int_array = builder.class("[I");
    let long_array = builder.class("[J");
    let object = builder.class("java/lang/Object");
    let cloneable = builder.class("java/lang/Cloneable");
    let serializable = builder.class("java/io/Serializable");
    let class = builder.class("java/lang/Class");
    let string = builder.class("java/lang/String");
    let int_primitive = builder.class("int");

    // static int counter() { return counter; }
    getter(&mut builder, "counter", "()I", counter, Assembler::ireturn);
    // static void setCounter(int value) { counter = value; }
    setter(&mut builder, "setCounter", "(I)V", 1, counter, |body| {
        body.iload(0);
    });
    // static String label() { return label; }
    getter(
        &mut builder,
        "label",
        "()Ljava/lang/String;",
        label,
        Assembler::areturn,
    );
    // static void setLabel(String value) { label = value; }
    setter(
        &mut builder,
        "setLabel",
        "(Ljava/lang/String;)V",
        1,
        label,
        |body| {
            body.aload(0);
        },
    );
    // static boolean flag() { return flag; }
    getter(&mut builder, "flag", "()Z", flag, Assembler::ireturn);
    // static void setFlag(boolean value) { flag = value; }
    setter(&mut builder, "setFlag", "(Z)V", 1, flag, |body| {
        body.iload(0);
    });
    // static long total() { return total; }
    getter(&mut builder, "total", "()J", total, Assembler::lreturn);
    // static void setTotal(long value) { total = value; }
    setter(&mut builder, "setTotal", "(J)V", 2, total, |body| {
        body.lload(0);
    });
    // static float score() { return score; }
    getter(&mut builder, "score", "()F", score, Assembler::freturn);
    // static void setScore(float value) { score = value; }
    setter(&mut builder, "setScore", "(F)V", 1, score, |body| {
        body.fload(0);
    });
    // static double ratio() { return ratio; }
    getter(&mut builder, "ratio", "()D", ratio, Assembler::dreturn);
    // static void setRatio(double value) { ratio = value; }
    setter(&mut builder, "setRatio", "(D)V", 2, ratio, |body| {
        body.dload(0);
    });
    // static void clearLabel() { label = null; }
    let mut clear_label = Assembler::new();
    clear_label.aconst_null();
    clear_label.putstatic(label);
    clear_label.return_();
    static_method(&mut builder, "clearLabel", "()V", 1, 0, clear_label);
    // static Class intType() { return Integer.TYPE; }
    getter(
        &mut builder,
        "intType",
        "()Ljava/lang/Class;",
        integer_type,
        Assembler::areturn,
    );
    // static Object systemOut() { return System.out; }
    getter(
        &mut builder,
        "systemOut",
        "()Ljava/lang/Object;",
        system_out,
        Assembler::areturn,
    );
    // static int inherited() { return SubStatics.shared; }
    getter(
        &mut builder,
        "inherited",
        "()I",
        inherited,
        Assembler::ireturn,
    );
    // static void setInherited(int value) { SubStatics.shared = value; }
    setter(&mut builder, "setInherited", "(I)V", 1, inherited, |body| {
        body.iload(0);
    });

    // The failure paths: static/instance mismatches, a missing field, a wrong
    // descriptor, a wrong value kind and instance-field reads through a static opcode.
    let mut putfield_counter = Assembler::new();
    putfield_counter.aload(0);
    putfield_counter.iconst(1);
    putfield_counter.putfield(counter);
    putfield_counter.return_();
    instance_method(
        &mut builder,
        "counterThroughPutfield",
        "()V",
        2,
        1,
        putfield_counter,
    );

    let mut getfield_counter = Assembler::new();
    getfield_counter.aload(0);
    getfield_counter.getfield(counter);
    getfield_counter.ireturn();
    instance_method(
        &mut builder,
        "counterThroughGetfield",
        "()I",
        1,
        1,
        getfield_counter,
    );

    let mut putstatic_instance = Assembler::new();
    putstatic_instance.iconst(1);
    putstatic_instance.putstatic(instance_value);
    putstatic_instance.return_();
    static_method(
        &mut builder,
        "instanceThroughPutstatic",
        "()V",
        1,
        0,
        putstatic_instance,
    );

    let mut getstatic_instance = Assembler::new();
    getstatic_instance.getstatic(instance_value);
    getstatic_instance.ireturn();
    static_method(
        &mut builder,
        "instanceThroughGetstatic",
        "()I",
        1,
        0,
        getstatic_instance,
    );

    let mut missing = Assembler::new();
    missing.iconst(1);
    missing.putstatic(absent);
    missing.return_();
    static_method(&mut builder, "missingField", "()V", 1, 0, missing);

    let mut wrong_descriptor = Assembler::new();
    wrong_descriptor.iconst(1);
    wrong_descriptor.putstatic(counter_as_long);
    wrong_descriptor.return_();
    static_method(
        &mut builder,
        "wrongDescriptor",
        "()V",
        1,
        0,
        wrong_descriptor,
    );

    let bad_value = builder.string("not an int");
    let mut wrong_value = Assembler::new();
    wrong_value.ldc(bad_value);
    wrong_value.putstatic(counter);
    wrong_value.return_();
    static_method(&mut builder, "wrongValue", "()V", 1, 0, wrong_value);

    // instanceof over the named, interface, array and primitive shapes.
    instanceof_method(&mut builder, "isAnimal", animal);
    instanceof_method(&mut builder, "isDog", dog);
    instanceof_method(&mut builder, "isPet", pet);
    instanceof_method(&mut builder, "isDogArray", dog_array);
    instanceof_method(&mut builder, "isAnimalArray", animal_array);
    instanceof_method(&mut builder, "isCatArray", cat_array);
    instanceof_method(&mut builder, "isObjectArray", object_array);
    instanceof_method(&mut builder, "isIntArray", int_array);
    instanceof_method(&mut builder, "isLongArray", long_array);
    instanceof_method(&mut builder, "isObject", object);
    instanceof_method(&mut builder, "isCloneable", cloneable);
    instanceof_method(&mut builder, "isSerializable", serializable);
    instanceof_method(&mut builder, "isClass", class);
    instanceof_method(&mut builder, "isIntPrimitive", int_primitive);

    // checkcast over the same shapes.
    checkcast_method(&mut builder, "castDog", dog);
    checkcast_method(&mut builder, "castAnimal", animal);
    checkcast_method(&mut builder, "castPet", pet);
    checkcast_method(&mut builder, "castDogArray", dog_array);
    checkcast_method(&mut builder, "castAnimalArray", animal_array);
    checkcast_method(&mut builder, "castObjectArray", object_array);
    checkcast_method(&mut builder, "castCatArray", cat_array);
    checkcast_method(&mut builder, "castIntArray", int_array);
    checkcast_method(&mut builder, "castLongArray", long_array);
    checkcast_method(&mut builder, "castObject", object);
    checkcast_method(&mut builder, "castCloneable", cloneable);
    checkcast_method(&mut builder, "castString", string);
    checkcast_method(&mut builder, "castIntPrimitive", int_primitive);

    // static Dog[] newDogArray() { return new Dog[2]; }
    let mut new_dog_array = Assembler::new();
    new_dog_array.iconst(2);
    new_dog_array.anewarray(dog);
    new_dog_array.areturn();
    static_method(
        &mut builder,
        "newDogArray",
        "()[Ljava/lang/Object;",
        1,
        0,
        new_dog_array,
    );
    // static Animal[] newAnimalArray() { return new Animal[2]; }
    let mut new_animal_array = Assembler::new();
    new_animal_array.iconst(2);
    new_animal_array.anewarray(animal);
    new_animal_array.areturn();
    static_method(
        &mut builder,
        "newAnimalArray",
        "()[Ljava/lang/Object;",
        1,
        0,
        new_animal_array,
    );
    // static int[] newIntArray() { return new int[2]; }
    let mut new_int_array = Assembler::new();
    new_int_array.iconst(2);
    new_int_array.newarray(10); // T_INT
    new_int_array.areturn();
    static_method(&mut builder, "newIntArray", "()[I", 1, 0, new_int_array);
    // static long[] newLongArray() { return new long[2]; }
    let mut new_long_array = Assembler::new();
    new_long_array.iconst(2);
    new_long_array.newarray(11); // T_LONG
    new_long_array.areturn();
    static_method(&mut builder, "newLongArray", "()[J", 1, 0, new_long_array);

    // static int table(int key) { switch (key) { case -1: 100; case 0: 101; ... } }
    let mut table = Assembler::new();
    table.iload(0);
    let table_default = table.label();
    let case_m1 = table.label();
    let case_0 = table.label();
    let case_1 = table.label();
    let case_2 = table.label();
    table.tableswitch(table_default, -1, &[case_m1, case_0, case_1, case_2]);
    table.place(case_m1);
    table.iconst(100);
    table.ireturn();
    table.place(case_0);
    table.iconst(101);
    table.ireturn();
    table.place(case_1);
    table.iconst(102);
    table.ireturn();
    table.place(case_2);
    table.iconst(103);
    table.ireturn();
    table.place(table_default);
    table.iconst(-1);
    table.ireturn();
    static_method(&mut builder, "table", "(I)I", 1, 1, table);

    // static int lookup(int key) { switch (key) { case 10: 1; case 20: 2; ... } }
    let mut lookup = Assembler::new();
    lookup.iload(0);
    let lookup_default = lookup.label();
    let lookup_10 = lookup.label();
    let lookup_20 = lookup.label();
    let lookup_40 = lookup.label();
    let lookup_100 = lookup.label();
    lookup.lookupswitch(
        lookup_default,
        &[
            (10, lookup_10),
            (20, lookup_20),
            (40, lookup_40),
            (100, lookup_100),
        ],
    );
    lookup.place(lookup_10);
    lookup.iconst(1);
    lookup.ireturn();
    lookup.place(lookup_20);
    lookup.iconst(2);
    lookup.ireturn();
    lookup.place(lookup_40);
    lookup.iconst(4);
    lookup.ireturn();
    lookup.place(lookup_100);
    lookup.iconst(10);
    lookup.ireturn();
    lookup.place(lookup_default);
    lookup.iconst(0);
    lookup.ireturn();
    static_method(&mut builder, "lookup", "(I)I", 1, 1, lookup);

    // Numeric constants and the returns/comparisons that observe them.
    let int_wide = builder.integer(123_456_789);
    let mut int_constant = Assembler::new();
    int_constant.ldc_w(int_wide);
    int_constant.ireturn();
    static_method(&mut builder, "intConstant", "()I", 1, 0, int_constant);

    let int_short = builder.integer(-7);
    let mut int_short_body = Assembler::new();
    int_short_body.ldc(int_short);
    int_short_body.ireturn();
    static_method(
        &mut builder,
        "intShortConstant",
        "()I",
        1,
        0,
        int_short_body,
    );

    let float_constant = builder.float(1.5);
    let mut float_body = Assembler::new();
    float_body.ldc(float_constant);
    float_body.freturn();
    static_method(&mut builder, "floatConstant", "()F", 1, 0, float_body);

    let long_constant = builder.long(9_000_000_000);
    let mut long_body = Assembler::new();
    long_body.ldc2_w(long_constant);
    long_body.lreturn();
    static_method(&mut builder, "longConstant", "()J", 1, 0, long_body);

    let double_constant = builder.double(2.25);
    let mut double_body = Assembler::new();
    double_body.ldc2_w(double_constant);
    double_body.dreturn();
    static_method(&mut builder, "doubleConstant", "()D", 1, 0, double_body);

    // static long echoLong(long value) { return value; } -- one local slot, see the
    // boundary in PROJECT.md.
    let mut echo_long = Assembler::new();
    echo_long.lload(0);
    echo_long.lreturn();
    static_method(&mut builder, "echoLong", "(J)J", 1, 2, echo_long);

    let mut echo_float = Assembler::new();
    echo_float.fload(0);
    echo_float.freturn();
    static_method(&mut builder, "echoFloat", "(F)F", 1, 1, echo_float);

    let mut echo_double = Assembler::new();
    echo_double.dload(0);
    echo_double.dreturn();
    static_method(&mut builder, "echoDouble", "(D)D", 1, 2, echo_double);

    let long_seven = builder.long(7);
    let long_nine = builder.long(9);
    let mut long_compare = Assembler::new();
    long_compare.ldc2_w(long_seven);
    long_compare.ldc2_w(long_nine);
    long_compare.lcmp();
    long_compare.ireturn();
    static_method(&mut builder, "longCompare", "()I", 2, 0, long_compare);

    let float_one = builder.float(1.5);
    let float_two = builder.float(2.5);
    let mut float_compare = Assembler::new();
    float_compare.ldc(float_one);
    float_compare.ldc(float_two);
    float_compare.fcmpl();
    float_compare.ireturn();
    static_method(&mut builder, "floatCompare", "()I", 2, 0, float_compare);

    let float_nan = builder.float(f32::NAN);
    let mut float_nan_l = Assembler::new();
    float_nan_l.ldc(float_nan);
    float_nan_l.ldc(float_one);
    float_nan_l.fcmpl();
    float_nan_l.ireturn();
    static_method(&mut builder, "floatNaNL", "()I", 2, 0, float_nan_l);

    let mut float_nan_g = Assembler::new();
    float_nan_g.ldc(float_nan);
    float_nan_g.ldc(float_one);
    float_nan_g.fcmpg();
    float_nan_g.ireturn();
    static_method(&mut builder, "floatNaNG", "()I", 2, 0, float_nan_g);

    let double_one = builder.double(2.5);
    let double_two = builder.double(1.5);
    let mut double_compare = Assembler::new();
    double_compare.ldc2_w(double_one);
    double_compare.ldc2_w(double_two);
    double_compare.dcmpl();
    double_compare.ireturn();
    static_method(&mut builder, "doubleCompare", "()I", 2, 0, double_compare);

    let double_nan = builder.double(f64::NAN);
    let mut double_nan_l = Assembler::new();
    double_nan_l.ldc2_w(double_nan);
    double_nan_l.ldc2_w(double_one);
    double_nan_l.dcmpl();
    double_nan_l.ireturn();
    static_method(&mut builder, "doubleNaNL", "()I", 2, 0, double_nan_l);

    let mut double_nan_g = Assembler::new();
    double_nan_g.ldc2_w(double_nan);
    double_nan_g.ldc2_w(double_one);
    double_nan_g.dcmpg();
    double_nan_g.ireturn();
    static_method(&mut builder, "doubleNaNG", "()I", 2, 0, double_nan_g);

    // Malformed switches, each at code offset 0 so the padding is exactly three bytes.
    let mut bad_unsorted = Assembler::new();
    bad_unsorted.op(0xab, &lookupswitch_operand(0, &[(20, 0), (10, 0)]));
    bad_unsorted.return_();
    static_method(&mut builder, "badUnsorted", "()V", 1, 0, bad_unsorted);

    let mut bad_duplicate = Assembler::new();
    bad_duplicate.op(0xab, &lookupswitch_operand(0, &[(10, 0), (10, 0)]));
    bad_duplicate.return_();
    static_method(&mut builder, "badDuplicate", "()V", 1, 0, bad_duplicate);

    let mut bad_padding = Assembler::new();
    bad_padding.op(0xaa, &[1, 0, 0]);
    bad_padding.return_();
    static_method(&mut builder, "badPadding", "()V", 1, 0, bad_padding);

    let mut bad_truncated = Assembler::new();
    bad_truncated.op(0xaa, &[0, 0]);
    static_method(&mut builder, "badTruncated", "()V", 1, 0, bad_truncated);

    let mut bad_range = Assembler::new();
    bad_range.op(0xaa, &tableswitch_operand(0, 2, 1, &[]));
    bad_range.return_();
    static_method(&mut builder, "badRange", "()V", 1, 0, bad_range);

    // The pair count is a signed int, so a negative value is malformed. The operand
    // block is padding(3) + default(4) + pair count(4), so the count starts at index 7.
    let mut bad_pairs_operand = lookupswitch_operand(0, &[]);
    bad_pairs_operand[7..11].copy_from_slice(&(-1i32).to_be_bytes());
    let mut negative_pairs = Assembler::new();
    negative_pairs.op(0xab, &bad_pairs_operand);
    negative_pairs.return_();
    static_method(
        &mut builder,
        "badNegativePairs",
        "()V",
        1,
        0,
        negative_pairs,
    );

    // A valid one-entry table whose default target is the byte just before the next
    // instruction: offset 19 is inside the last jump offset, not a boundary.
    let mut bad_target = Assembler::new();
    bad_target.op(0xaa, &tableswitch_operand(19, 0, 0, &[20]));
    bad_target.return_();
    static_method(&mut builder, "badTarget", "()V", 1, 0, bad_target);

    // `ldc2_w` may not carry an `int` constant.
    let bad_ldc_index = builder.integer(1);
    let mut bad_ldc = Assembler::new();
    bad_ldc.op(0x14, &bad_ldc_index.to_be_bytes());
    bad_ldc.return_();
    static_method(&mut builder, "badLdc2Int", "()V", 1, 0, bad_ldc);

    builder.build()
}

fn child_statics_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ChildStatics", "java/lang/Object", 61);
    builder.field(ACC_PUBLIC_STATIC, "value", "I");
    builder.build()
}

fn child_probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ChildStaticProbe", "java/lang/Object", 61);
    let value = builder.field_ref("ChildStatics", "value", "I");

    let mut set = Assembler::new();
    set.iload(0);
    set.putstatic(value);
    set.return_();
    static_method(&mut builder, "set", "(I)V", 1, 1, set);

    let mut get = Assembler::new();
    get.getstatic(value);
    get.ireturn();
    static_method(&mut builder, "get", "()I", 1, 0, get);

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
    dog: ClassId,
    cat: ClassId,
    base_statics: ClassId,
    sub_statics: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());

    let define = |name: &str, bytes: Vec<u8>| {
        shared
            .define_class(LoaderId::BOOTSTRAP, name, &bytes)
            .unwrap_or_else(|error| panic!("{name} defines: {error}"))
    };
    // `Animal` and `Pet` are defined up front so `Dog`'s superclass and interface are
    // already loaded when the cast tests walk them.
    define("Animal", animal_bytes());
    define("Pet", pet_bytes());
    let dog = define("Dog", dog_bytes());
    let cat = define("Cat", cat_bytes());
    let base_statics = define("BaseStatics", base_statics_bytes());
    let sub_statics = define("SubStatics", sub_statics_bytes());
    let probe = define("BytecodeProbe", probe_bytes());

    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        probe,
        dog,
        cat,
        base_statics,
        sub_statics,
    }
}

fn call(
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

fn ok_call(fixture: &Fixture, name: &str, descriptor: &str, args: Vec<Value>) -> Option<Value> {
    call(fixture, name, descriptor, args)
        .unwrap_or_else(|error| panic!("{name}{descriptor} runs: {error}"))
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int, found {other:?}"),
    }
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference, found {other:?}"),
    }
}

fn allocate(fixture: &Fixture, class: ClassId) -> ObjRef {
    fixture
        .shared
        .allocate_instance(class)
        .unwrap_or_else(|error| panic!("class {} allocates: {error}", class.0))
}

fn statics(fixture: &Fixture, class: ClassId) -> HashMap<(String, String), Value> {
    fixture
        .shared
        .entry(class)
        .expect("the class is loaded")
        .statics
        .lock()
        .expect("static field lock")
        .clone()
}

fn assert_decode_error(result: Result<Option<Value>, InterpError>, fragment: &str) {
    match result {
        Err(InterpError::Decode(detail)) => assert!(
            detail.contains(fragment),
            "expected a decode error containing {fragment:?}, found {detail:?}"
        ),
        other => panic!("expected a decode error containing {fragment:?}, found {other:?}"),
    }
}

#[test]
fn static_fields_round_trip_through_the_statics_table() {
    let fixture = fixture();

    ok_call(&fixture, "setCounter", "(I)V", vec![Value::Int(7)]);
    assert_eq!(
        int_value(ok_call(&fixture, "counter", "()I", Vec::new())),
        7
    );

    ok_call(
        &fixture,
        "setLabel",
        "(Ljava/lang/String;)V",
        vec![Value::Ref(fixture.shared.intern_string("probe"))],
    );
    let stored_label = ref_value(ok_call(
        &fixture,
        "label",
        "()Ljava/lang/String;",
        Vec::new(),
    ));
    assert_eq!(
        fixture.shared.heap().string(stored_label).unwrap(),
        "probe",
        "putstatic stores the reference and getstatic reads it back"
    );
    assert!(
        fixture
            .shared
            .roots(&[&fixture.thread])
            .contains(&stored_label),
        "a reference in the statics table is a VM root"
    );

    ok_call(&fixture, "setFlag", "(Z)V", vec![Value::Int(1)]);
    assert_eq!(int_value(ok_call(&fixture, "flag", "()Z", Vec::new())), 1);
    ok_call(&fixture, "setFlag", "(Z)V", vec![Value::Int(0)]);
    assert_eq!(int_value(ok_call(&fixture, "flag", "()Z", Vec::new())), 0);

    ok_call(
        &fixture,
        "setTotal",
        "(J)V",
        vec![Value::Long(9_000_000_000)],
    );
    assert_eq!(
        ok_call(&fixture, "total", "()J", Vec::new()),
        Some(Value::Long(9_000_000_000))
    );

    ok_call(&fixture, "setScore", "(F)V", vec![Value::Float(2.5)]);
    assert_eq!(
        ok_call(&fixture, "score", "()F", Vec::new()),
        Some(Value::Float(2.5))
    );

    ok_call(&fixture, "setRatio", "(D)V", vec![Value::Double(0.25)]);
    assert_eq!(
        ok_call(&fixture, "ratio", "()D", Vec::new()),
        Some(Value::Double(0.25))
    );

    // A null reference is a legal value for a reference-typed static field.
    ok_call(&fixture, "clearLabel", "()V", Vec::new());
    assert_eq!(
        ref_value(ok_call(
            &fixture,
            "label",
            "()Ljava/lang/String;",
            Vec::new()
        )),
        ObjRef::NULL
    );

    let fields = statics(&fixture, fixture.probe);
    assert_eq!(
        fields.get(&("counter".to_owned(), "I".to_owned())),
        Some(&Value::Int(7))
    );
    assert_eq!(
        fields.get(&("flag".to_owned(), "Z".to_owned())),
        Some(&Value::Int(0))
    );
    assert_eq!(
        fields.get(&("total".to_owned(), "J".to_owned())),
        Some(&Value::Long(9_000_000_000))
    );
    assert_eq!(
        fields.get(&("score".to_owned(), "F".to_owned())),
        Some(&Value::Float(2.5))
    );
    assert_eq!(
        fields.get(&("ratio".to_owned(), "D".to_owned())),
        Some(&Value::Double(0.25))
    );
    assert_eq!(
        fields.get(&("label".to_owned(), "Ljava/lang/String;".to_owned())),
        Some(&Value::Ref(ObjRef::NULL))
    );
    assert!(
        !fields.contains_key(&("instanceValue".to_owned(), "I".to_owned())),
        "an instance field never appears in the statics table"
    );
}

#[test]
fn putstatic_resolves_the_declaring_class() {
    let fixture = fixture();

    // The Fieldref names `SubStatics`, but `shared` is declared on `BaseStatics`: the
    // value belongs to the declaring class, not to the class named by the instruction.
    ok_call(&fixture, "setInherited", "(I)V", vec![Value::Int(31)]);
    assert_eq!(
        int_value(ok_call(&fixture, "inherited", "()I", Vec::new())),
        31
    );
    assert_eq!(
        statics(&fixture, fixture.base_statics).get(&("shared".to_owned(), "I".to_owned())),
        Some(&Value::Int(31)),
        "putstatic stores on the declaring class"
    );
    assert!(
        statics(&fixture, fixture.sub_statics).is_empty(),
        "the subclass's own statics table stays empty"
    );
}

#[test]
fn putstatic_resolves_the_owner_through_the_executing_loader() {
    let fixture = fixture();
    let child = fixture
        .shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(MapSource::new(vec![
                ("ChildStatics", child_statics_bytes()),
                ("ChildStaticProbe", child_probe_bytes()),
            ])),
        )
        .expect("the child loader is created");
    let probe = fixture
        .shared
        .load_class(child, "ChildStaticProbe")
        .expect("the child defines its probe");
    let child_statics = fixture
        .shared
        .load_class(child, "ChildStatics")
        .expect("the child defines its statics holder");

    run_static_method(
        &fixture.shared,
        &fixture.thread,
        probe,
        "set",
        "(I)V",
        vec![Value::Int(5)],
    )
    .expect("the child probe writes its own class");

    let value = run_static_method(
        &fixture.shared,
        &fixture.thread,
        probe,
        "get",
        "()I",
        Vec::new(),
    )
    .expect("the child probe reads its own class");
    assert_eq!(value, Some(Value::Int(5)));
    assert_eq!(
        fixture
            .shared
            .entry(child_statics)
            .expect("the child statics class is loaded")
            .statics
            .lock()
            .unwrap()
            .get(&("value".to_owned(), "I".to_owned())),
        Some(&Value::Int(5)),
        "the bootstrap does not define ChildStatics, so this could only resolve through the child loader"
    );
}

#[test]
fn getstatic_keeps_the_type_and_system_out_paths() {
    let fixture = fixture();

    let int_type = ref_value(ok_call(
        &fixture,
        "intType",
        "()Ljava/lang/Class;",
        Vec::new(),
    ));
    assert_eq!(
        int_type,
        fixture.shared.primitive_class(PrimitiveKind::Int),
        "getstatic Integer.TYPE still resolves virtually to the primitive class object"
    );

    let print_stream = fixture
        .shared
        .load_library_class("java/io/PrintStream")
        .expect("PrintStream loads from the C1 tree");
    let stream = fixture
        .shared
        .heap()
        .allocate_instance(print_stream, &FieldLayout::new());
    let system = fixture
        .shared
        .load_library_class("java/lang/System")
        .expect("System loads from the C1 tree");
    fixture
        .shared
        .entry(system)
        .expect("System is loaded")
        .statics
        .lock()
        .expect("static field lock")
        .insert(
            ("out".to_owned(), "Ljava/io/PrintStream;".to_owned()),
            Value::Ref(stream),
        );

    assert_eq!(
        ref_value(ok_call(
            &fixture,
            "systemOut",
            "()Ljava/lang/Object;",
            Vec::new()
        )),
        stream,
        "getstatic System.out still reads the System statics table"
    );
}

#[test]
fn putstatic_rejects_instance_missing_and_mismatched_fields() {
    let fixture = fixture();
    let probe_instance = allocate(&fixture, fixture.probe);

    match run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        "counterThroughPutfield",
        "()V",
        probe_instance,
        Vec::new(),
    ) {
        Err(InterpError::StaticFieldAccess {
            class,
            name,
            descriptor,
        }) => {
            assert_eq!(class, "BytecodeProbe");
            assert_eq!(name, "counter");
            assert_eq!(descriptor, "I");
        }
        other => panic!("expected a static-field access error, found {other:?}"),
    }

    match run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        "counterThroughGetfield",
        "()I",
        probe_instance,
        Vec::new(),
    ) {
        Err(InterpError::StaticFieldAccess { name, .. }) => assert_eq!(name, "counter"),
        other => panic!("expected a static-field access error, found {other:?}"),
    }

    match call(&fixture, "instanceThroughPutstatic", "()V", Vec::new()) {
        Err(InterpError::InstanceFieldAccess {
            class,
            name,
            descriptor,
        }) => {
            assert_eq!(class, "BytecodeProbe");
            assert_eq!(name, "instanceValue");
            assert_eq!(descriptor, "I");
        }
        other => panic!("expected an instance-field access error, found {other:?}"),
    }

    match call(&fixture, "instanceThroughGetstatic", "()I", Vec::new()) {
        Err(InterpError::InstanceFieldAccess { name, .. }) => assert_eq!(name, "instanceValue"),
        other => panic!("expected an instance-field access error, found {other:?}"),
    }

    match call(&fixture, "missingField", "()V", Vec::new()) {
        Err(InterpError::FieldNotFound {
            class,
            name,
            descriptor,
        }) => {
            assert_eq!(class, "BytecodeProbe");
            assert_eq!(name, "absent");
            assert_eq!(descriptor, "I");
        }
        other => panic!("expected a missing-field error, found {other:?}"),
    }

    match call(&fixture, "wrongDescriptor", "()V", Vec::new()) {
        Err(InterpError::FieldDescriptorMismatch { class, name, found }) => {
            assert_eq!(class, "BytecodeProbe");
            assert_eq!(name, "counter");
            assert_eq!(found, "I");
        }
        other => panic!("expected a descriptor mismatch, found {other:?}"),
    }

    match call(&fixture, "wrongValue", "()V", Vec::new()) {
        Err(InterpError::FieldValueMismatch {
            class,
            name,
            descriptor,
        }) => {
            assert_eq!(class, "BytecodeProbe");
            assert_eq!(name, "counter");
            assert_eq!(descriptor, "I");
        }
        other => panic!("expected a field-value mismatch, found {other:?}"),
    }
}

#[test]
fn instanceof_covers_classes_interfaces_and_arrays() {
    let fixture = fixture();
    let dog_instance = allocate(&fixture, fixture.dog);
    let cat_instance = allocate(&fixture, fixture.cat);

    let dog_array = ref_value(ok_call(
        &fixture,
        "newDogArray",
        "()[Ljava/lang/Object;",
        Vec::new(),
    ));
    let animal_array = ref_value(ok_call(
        &fixture,
        "newAnimalArray",
        "()[Ljava/lang/Object;",
        Vec::new(),
    ));
    let int_array = ref_value(ok_call(&fixture, "newIntArray", "()[I", Vec::new()));

    let is = |method: &str, value: ObjRef| {
        int_value(ok_call(
            &fixture,
            method,
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(value)],
        ))
    };

    assert_eq!(is("isAnimal", dog_instance), 1);
    assert_eq!(is("isDog", dog_instance), 1);
    assert_eq!(is("isDog", cat_instance), 0);
    assert_eq!(is("isPet", dog_instance), 1);
    assert_eq!(is("isPet", cat_instance), 0);
    assert_eq!(is("isObject", dog_instance), 1);

    assert_eq!(is("isDogArray", dog_array), 1);
    assert_eq!(is("isDogArray", animal_array), 0);
    assert_eq!(is("isAnimalArray", dog_array), 1);
    assert_eq!(is("isObjectArray", dog_array), 1);
    assert_eq!(is("isCatArray", dog_array), 0);
    assert_eq!(is("isIntArray", dog_array), 0);

    assert_eq!(is("isIntArray", int_array), 1);
    assert_eq!(is("isLongArray", int_array), 0);
    assert_eq!(is("isObjectArray", int_array), 0);
    assert_eq!(is("isObject", int_array), 1);
    assert_eq!(is("isCloneable", int_array), 1);
    assert_eq!(is("isSerializable", int_array), 1);

    // Boxing a primitive does not make it a `Dog`, and a class object is a `Class` and an
    // `Object`.
    let boxed = fixture.shared.box_int(3);
    assert_eq!(is("isDog", boxed), 0);
    let dog_class_object = fixture
        .shared
        .class_object(fixture.dog)
        .expect("Dog class object");
    assert_eq!(is("isClass", dog_class_object), 1);
    assert_eq!(is("isObject", dog_class_object), 1);
}

#[test]
fn instanceof_handles_null_and_primitive_operands() {
    let fixture = fixture();
    let null = ObjRef::NULL;

    let is = |method: &str, value: ObjRef| {
        int_value(ok_call(
            &fixture,
            method,
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(value)],
        ))
    };

    // JVMS §6.5: `instanceof` of a null reference pushes 0, for a class, an interface
    // and an array alike.
    assert_eq!(is("isDog", null), 0);
    assert_eq!(is("isPet", null), 0);
    assert_eq!(is("isDogArray", null), 0);
    assert_eq!(is("isIntArray", null), 0);
    assert_eq!(is("isCloneable", null), 0);

    // A primitive operand names no reference type, so nothing is its instance, null
    // included.
    let dog_instance = allocate(&fixture, fixture.dog);
    assert_eq!(is("isIntPrimitive", dog_instance), 0);
    assert_eq!(is("isIntPrimitive", null), 0);
}

#[test]
fn checkcast_passes_assignable_values_and_null() {
    let fixture = fixture();
    let dog_instance = allocate(&fixture, fixture.dog);
    let dog_array = ref_value(ok_call(
        &fixture,
        "newDogArray",
        "()[Ljava/lang/Object;",
        Vec::new(),
    ));
    let int_array = ref_value(ok_call(&fixture, "newIntArray", "()[I", Vec::new()));

    let cast = |method: &str, value: ObjRef| {
        ref_value(ok_call(
            &fixture,
            method,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Ref(value)],
        ))
    };

    assert_eq!(cast("castDog", dog_instance), dog_instance);
    assert_eq!(cast("castAnimal", dog_instance), dog_instance);
    assert_eq!(cast("castPet", dog_instance), dog_instance);
    assert_eq!(cast("castObject", dog_instance), dog_instance);
    assert_eq!(cast("castDogArray", dog_array), dog_array);
    assert_eq!(cast("castAnimalArray", dog_array), dog_array);
    assert_eq!(cast("castObjectArray", dog_array), dog_array);
    assert_eq!(cast("castIntArray", int_array), int_array);
    assert_eq!(cast("castObject", int_array), int_array);
    assert_eq!(cast("castCloneable", int_array), int_array);
    assert_eq!(cast("castDog", ObjRef::NULL), ObjRef::NULL);

    let string = fixture.shared.intern_string("text");
    assert_eq!(cast("castString", string), string);
}

#[test]
fn checkcast_reports_a_typed_class_cast() {
    let fixture = fixture();
    let cat_instance = allocate(&fixture, fixture.cat);
    let string = fixture.shared.intern_string("text");
    let boxed = fixture.shared.box_int(1);
    let dog_array = ref_value(ok_call(
        &fixture,
        "newDogArray",
        "()[Ljava/lang/Object;",
        Vec::new(),
    ));
    let int_array = ref_value(ok_call(&fixture, "newIntArray", "()[I", Vec::new()));

    let cast = |method: &str, value: ObjRef| {
        call(
            &fixture,
            method,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Ref(value)],
        )
    };
    let assert_cast =
        |result: Result<Option<Value>, InterpError>, expected: &str, found: &str| match result {
            Err(InterpError::ClassCast {
                expected: actual_expected,
                found: actual_found,
            }) => {
                assert_eq!(actual_expected, expected);
                assert_eq!(actual_found, found);
            }
            other => panic!("expected a class-cast error, found {other:?}"),
        };

    assert_cast(cast("castDog", cat_instance), "Dog", "Cat");
    assert_cast(cast("castDog", string), "Dog", "java.lang.String");
    assert_cast(cast("castDog", boxed), "Dog", "java.lang.Integer");
    assert_cast(cast("castCatArray", dog_array), "[LCat;", "[LDog;");
    assert_cast(cast("castLongArray", int_array), "[J", "[I");
    assert_cast(cast("castIntPrimitive", boxed), "int", "java.lang.Integer");
    assert_cast(cast("castIntPrimitive", ObjRef::NULL), "int", "null");
}

#[test]
fn tableswitch_and_lookupswitch_choose_case_or_default() {
    let fixture = fixture();

    let table = |key: i32| int_value(ok_call(&fixture, "table", "(I)I", vec![Value::Int(key)]));
    assert_eq!(table(-2), -1, "below the dense range takes the default");
    assert_eq!(table(-1), 100);
    assert_eq!(table(0), 101);
    assert_eq!(table(1), 102);
    assert_eq!(table(2), 103);
    assert_eq!(table(3), -1, "above the dense range takes the default");

    let lookup = |key: i32| int_value(ok_call(&fixture, "lookup", "(I)I", vec![Value::Int(key)]));
    assert_eq!(lookup(9), 0, "a gap takes the default");
    assert_eq!(lookup(10), 1);
    assert_eq!(lookup(20), 2);
    assert_eq!(lookup(30), 0, "a gap takes the default");
    assert_eq!(lookup(40), 4);
    assert_eq!(lookup(100), 10);
    assert_eq!(lookup(101), 0);
}

#[test]
fn switch_targets_are_instruction_indices() {
    let bytes = probe_bytes();
    let file = ClassFile::parse(&bytes).expect("the probe parses");

    let method = file
        .find_method("table", "(I)I")
        .unwrap()
        .expect("table exists");
    let code = file.code(method).unwrap().expect("table has Code");
    let decoded = decode(&file, &code.data).expect("table decodes");
    let switch_index = decoded
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instr::TableSwitch { .. }))
        .expect("table has a tableswitch");
    let Instr::TableSwitch {
        default,
        low,
        targets,
    } = decoded.instructions[switch_index].clone()
    else {
        panic!("instruction {switch_index} is not a tableswitch");
    };
    assert_eq!(low, -1);
    assert_eq!(targets.len(), 4);
    for (case, target) in targets.iter().enumerate() {
        let expected = 100 + case as i32;
        assert!(
            matches!(decoded.instructions[*target], Instr::Iconst(value) if value == expected),
            "case {case} points at instruction {target}, expected iconst {expected}"
        );
    }
    assert!(
        matches!(decoded.instructions[default], Instr::Iconst(-1)),
        "the default points at its own iconst"
    );

    let method = file
        .find_method("lookup", "(I)I")
        .unwrap()
        .expect("lookup exists");
    let code = file.code(method).unwrap().expect("lookup has Code");
    let decoded = decode(&file, &code.data).expect("lookup decodes");
    let switch_index = decoded
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instr::LookupSwitch { .. }))
        .expect("lookup has a lookupswitch");
    let Instr::LookupSwitch {
        default,
        keys,
        targets,
    } = decoded.instructions[switch_index].clone()
    else {
        panic!("instruction {switch_index} is not a lookupswitch");
    };
    assert_eq!(keys, vec![10, 20, 40, 100]);
    let expected_results = [1, 2, 4, 10];
    for (case, target) in targets.iter().enumerate() {
        let expected = expected_results[case];
        assert!(
            matches!(decoded.instructions[*target], Instr::Iconst(value) if value == expected),
            "key {} points at instruction {target}, expected iconst {expected}",
            keys[case]
        );
    }
    assert!(matches!(decoded.instructions[default], Instr::Iconst(0)));
}

#[test]
fn numeric_constants_keep_their_value_kind() {
    let fixture = fixture();

    assert_eq!(
        ok_call(&fixture, "intConstant", "()I", Vec::new()),
        Some(Value::Int(123_456_789)),
        "ldc_w carries the full two-byte index"
    );
    assert_eq!(
        ok_call(&fixture, "intShortConstant", "()I", Vec::new()),
        Some(Value::Int(-7))
    );
    assert_eq!(
        ok_call(&fixture, "floatConstant", "()F", Vec::new()),
        Some(Value::Float(1.5))
    );
    assert_eq!(
        ok_call(&fixture, "longConstant", "()J", Vec::new()),
        Some(Value::Long(9_000_000_000))
    );
    assert_eq!(
        ok_call(&fixture, "doubleConstant", "()D", Vec::new()),
        Some(Value::Double(2.25))
    );

    // The interpreter keeps one local slot per value even for `long`/`double`, so the
    // value round-trips through local 0 without a second slot.
    assert_eq!(
        ok_call(&fixture, "echoLong", "(J)J", vec![Value::Long(42)]),
        Some(Value::Long(42))
    );
    assert_eq!(
        ok_call(&fixture, "echoFloat", "(F)F", vec![Value::Float(2.5)]),
        Some(Value::Float(2.5))
    );
    assert_eq!(
        ok_call(&fixture, "echoDouble", "(D)D", vec![Value::Double(0.5)]),
        Some(Value::Double(0.5))
    );
}

#[test]
fn numeric_constants_compare() {
    let fixture = fixture();

    assert_eq!(
        int_value(ok_call(&fixture, "longCompare", "()I", Vec::new())),
        -1,
        "7L compares below 9L"
    );
    assert_eq!(
        int_value(ok_call(&fixture, "floatCompare", "()I", Vec::new())),
        -1,
        "1.5f compares below 2.5f"
    );
    assert_eq!(
        int_value(ok_call(&fixture, "floatNaNL", "()I", Vec::new())),
        -1,
        "fcmpl reports NaN as lower"
    );
    assert_eq!(
        int_value(ok_call(&fixture, "floatNaNG", "()I", Vec::new())),
        1,
        "fcmpg reports NaN as higher"
    );
    assert_eq!(
        int_value(ok_call(&fixture, "doubleCompare", "()I", Vec::new())),
        1,
        "2.5d compares above 1.5d"
    );
    assert_eq!(
        int_value(ok_call(&fixture, "doubleNaNL", "()I", Vec::new())),
        -1,
        "dcmpl reports NaN as lower"
    );
    assert_eq!(
        int_value(ok_call(&fixture, "doubleNaNG", "()I", Vec::new())),
        1,
        "dcmpg reports NaN as higher"
    );
}

#[test]
fn malformed_switches_are_decode_errors() {
    let fixture = fixture();

    assert_decode_error(
        call(&fixture, "badUnsorted", "()V", Vec::new()),
        "keys must be strictly ascending",
    );
    assert_decode_error(
        call(&fixture, "badDuplicate", "()V", Vec::new()),
        "key 10 after 10",
    );
    assert_decode_error(
        call(&fixture, "badPadding", "()V", Vec::new()),
        "non-zero padding byte",
    );
    assert_decode_error(
        call(&fixture, "badTruncated", "()V", Vec::new()),
        "truncated operand",
    );
    assert_decode_error(
        call(&fixture, "badRange", "()V", Vec::new()),
        "high 1 below low 2",
    );
    assert_decode_error(
        call(&fixture, "badNegativePairs", "()V", Vec::new()),
        "negative pair count",
    );
    assert_decode_error(
        call(&fixture, "badTarget", "()V", Vec::new()),
        "switch target byte 19 is not an instruction boundary",
    );
}

#[test]
fn numeric_ldc_width_rules_are_typed_errors() {
    let fixture = fixture();
    assert_decode_error(
        call(&fixture, "badLdc2Int", "()V", Vec::new()),
        "ldc2_w of an unsupported Integer constant",
    );

    // The existing `ldc` of a `long` stays rejected as well.
    let mut builder = ClassBuilder::new("LdcLongProbe", "java/lang/Object", 61);
    let long_index = builder.long(1);
    let mut body = Assembler::new();
    body.ldc_w(long_index);
    body.return_();
    static_method(&mut builder, "run", "()V", 2, 0, body);
    let bytes = builder.build();
    let file = ClassFile::parse(&bytes).expect("the fixture parses");
    let method = file.find_method("run", "()V").unwrap().expect("run exists");
    let code = file.code(method).unwrap().expect("run has Code");
    match decode(&file, &code.data) {
        Err(InterpError::Decode(detail)) => assert!(
            detail.contains("unsupported Long constant"),
            "expected an unsupported-Long decode error, found {detail:?}"
        ),
        other => panic!("expected a decode error, found {other:?}"),
    }
}
