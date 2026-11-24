//! M2.13 acceptance: `java.util.HashMap` and `String.valueOf`/value conversion.
//!
//! `MapProbe` is assembled against the committed C1 bootstrap library. Every map operation
//! and every `String.valueOf` overload the decoded instruction subset can express runs
//! through `invokevirtual`/`invokespecial`/`invokestatic`, so the tests exercise receiver
//! dispatch and registry lookup, not a Rust function directly. The `long`, `float` and
//! `double` overloads are called through the registry because the decoded instruction
//! subset has no `lload`/`fload`/`dload` or `ldc2_w`; the tests say so where it happens.
//!
//! A string key is compared by content and a boxed `Integer` key by value. The tests build
//! two distinct heap strings and two distinct boxes to prove that, rather than relying on
//! the same handle.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ClassId, HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, boxed, instance,
    maps, object, reflection, string,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

/// `invokestatic` is not one of the assembler's named helpers.
const INVOKESTATIC: u8 = 0xb8;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn method(
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

/// A fresh `HashMap` in `local`, pushed by `new`/`dup`/`<init>()V`.
fn new_map(asm: &mut Assembler, class: u16, init: u16, local: u8) {
    asm.new_instance(class);
    asm.dup();
    asm.invokespecial(init);
    asm.astore(local);
}

fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("MapProbe", "java/lang/Object", 61);

    // String constants first, so every `ldc` index stays inside its one-byte form.
    let value_text = builder.string("v");
    let null_key_text = builder.string("n");
    let seven_text = builder.string("seven");
    let key_text = builder.string("k");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let object_get_class =
        builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let object_to_string =
        builder.method_ref("java/lang/Object", "toString", "()Ljava/lang/String;");
    let class_get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");
    let string_equals = builder.method_ref("java/lang/String", "equals", "(Ljava/lang/Object;)Z");

    let string_value_of_object = builder.method_ref(
        "java/lang/String",
        "valueOf",
        "(Ljava/lang/Object;)Ljava/lang/String;",
    );
    let string_value_of_int =
        builder.method_ref("java/lang/String", "valueOf", "(I)Ljava/lang/String;");
    let string_value_of_boolean =
        builder.method_ref("java/lang/String", "valueOf", "(Z)Ljava/lang/String;");
    let string_value_of_char =
        builder.method_ref("java/lang/String", "valueOf", "(C)Ljava/lang/String;");
    let integer_value_of =
        builder.method_ref("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;");
    let integer_to_string =
        builder.method_ref("java/lang/Integer", "toString", "(I)Ljava/lang/String;");

    let map_class = builder.class("java/util/HashMap");
    let map_init = builder.method_ref("java/util/HashMap", "<init>", "()V");
    let map_init_capacity = builder.method_ref("java/util/HashMap", "<init>", "(I)V");
    let map_put = builder.method_ref(
        "java/util/HashMap",
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let map_get = builder.method_ref(
        "java/util/HashMap",
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let map_size = builder.method_ref("java/util/HashMap", "size", "()I");
    let map_is_empty = builder.method_ref("java/util/HashMap", "isEmpty", "()Z");
    let map_contains_key =
        builder.method_ref("java/util/HashMap", "containsKey", "(Ljava/lang/Object;)Z");
    let map_contains_value = builder.method_ref(
        "java/util/HashMap",
        "containsValue",
        "(Ljava/lang/Object;)Z",
    );
    let map_remove = builder.method_ref(
        "java/util/HashMap",
        "remove",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let map_clear = builder.method_ref("java/util/HashMap", "clear", "()V");
    let map_key_set = builder.method_ref("java/util/HashMap", "keySet", "()Ljava/util/Set;");

    // static String putGet(String k, String v) {
    //     HashMap map = new HashMap();
    //     map.put(k, v);
    //     return (String) map.get(k);
    // }
    let mut put_get = Assembler::new();
    new_map(&mut put_get, map_class, map_init, 2);
    put_get.aload(2);
    put_get.aload(0);
    put_get.aload(1);
    put_get.invokevirtual(map_put);
    put_get.astore(3); // discard the previous value
    put_get.aload(2);
    put_get.aload(0);
    put_get.invokevirtual(map_get);
    put_get.areturn();
    method(
        &mut builder,
        "putGet",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        3,
        4,
        put_get,
    );

    // static int sizeAfter(String k1, String k2) {
    //     HashMap map = new HashMap();
    //     map.put(k1, "v");
    //     map.put(k2, "v");
    //     return map.size();
    // }
    let mut size_after = Assembler::new();
    new_map(&mut size_after, map_class, map_init, 3);
    size_after.aload(3);
    size_after.aload(0);
    size_after.ldc(value_text);
    size_after.invokevirtual(map_put);
    size_after.astore(4);
    size_after.aload(3);
    size_after.aload(1);
    size_after.ldc(value_text);
    size_after.invokevirtual(map_put);
    size_after.astore(4);
    size_after.aload(3);
    size_after.invokevirtual(map_size);
    size_after.ireturn();
    method(
        &mut builder,
        "sizeAfter",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        3,
        5,
        size_after,
    );

    // static boolean contains(String k) {
    //     HashMap map = new HashMap();
    //     map.put(k, "v");
    //     return map.containsKey(k);
    // }
    let mut contains = Assembler::new();
    new_map(&mut contains, map_class, map_init, 2);
    contains.aload(2);
    contains.aload(0);
    contains.ldc(value_text);
    contains.invokevirtual(map_put);
    contains.astore(3);
    contains.aload(2);
    contains.aload(0);
    contains.invokevirtual(map_contains_key);
    contains.ireturn();
    method(
        &mut builder,
        "contains",
        "(Ljava/lang/String;)Z",
        3,
        4,
        contains,
    );

    // static boolean containsOther(String stored, String probe) {
    //     HashMap map = new HashMap();
    //     map.put(stored, "v");
    //     return map.containsKey(probe);
    // }
    let mut contains_other = Assembler::new();
    new_map(&mut contains_other, map_class, map_init, 2);
    contains_other.aload(2);
    contains_other.aload(0);
    contains_other.ldc(value_text);
    contains_other.invokevirtual(map_put);
    contains_other.astore(3);
    contains_other.aload(2);
    contains_other.aload(1);
    contains_other.invokevirtual(map_contains_key);
    contains_other.ireturn();
    method(
        &mut builder,
        "containsOther",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        3,
        4,
        contains_other,
    );

    // static String valueOfInt(int v) {
    //     return String.valueOf(v);
    // }
    let mut value_of_int = Assembler::new();
    value_of_int.iload(0);
    value_of_int.op(INVOKESTATIC, &string_value_of_int.to_be_bytes());
    value_of_int.areturn();
    method(
        &mut builder,
        "valueOfInt",
        "(I)Ljava/lang/String;",
        1,
        1,
        value_of_int,
    );

    // static String valueOfNull() {
    //     return String.valueOf((Object) null);
    // }
    let mut value_of_null = Assembler::new();
    value_of_null.aconst_null();
    value_of_null.op(INVOKESTATIC, &string_value_of_object.to_be_bytes());
    value_of_null.areturn();
    method(
        &mut builder,
        "valueOfNull",
        "()Ljava/lang/String;",
        1,
        0,
        value_of_null,
    );

    // static String valueOfBoolean(int v) {
    //     return String.valueOf(v != 0);
    // }
    let mut value_of_boolean = Assembler::new();
    value_of_boolean.iload(0);
    value_of_boolean.op(INVOKESTATIC, &string_value_of_boolean.to_be_bytes());
    value_of_boolean.areturn();
    method(
        &mut builder,
        "valueOfBoolean",
        "(I)Ljava/lang/String;",
        1,
        1,
        value_of_boolean,
    );

    // static String valueOfChar(int v) {
    //     return String.valueOf((char) v);
    // }
    let mut value_of_char = Assembler::new();
    value_of_char.iload(0);
    value_of_char.op(INVOKESTATIC, &string_value_of_char.to_be_bytes());
    value_of_char.areturn();
    method(
        &mut builder,
        "valueOfChar",
        "(I)Ljava/lang/String;",
        1,
        1,
        value_of_char,
    );

    // static String valueOfObject() {
    //     return String.valueOf(new Object());
    // }
    let mut value_of_object = Assembler::new();
    value_of_object.new_instance(builder.class("java/lang/Object"));
    value_of_object.dup();
    value_of_object.invokespecial(object_init);
    value_of_object.op(INVOKESTATIC, &string_value_of_object.to_be_bytes());
    value_of_object.areturn();
    method(
        &mut builder,
        "valueOfObject",
        "()Ljava/lang/String;",
        2,
        0,
        value_of_object,
    );

    // static String valueOfObjectArg(Object o) {
    //     return String.valueOf(o);
    // }
    let mut value_of_object_arg = Assembler::new();
    value_of_object_arg.aload(0);
    value_of_object_arg.op(INVOKESTATIC, &string_value_of_object.to_be_bytes());
    value_of_object_arg.areturn();
    method(
        &mut builder,
        "valueOfObjectArg",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        1,
        1,
        value_of_object_arg,
    );

    // static boolean valueOfMatchesToString() {
    //     Object o = new Object();
    //     return o.toString().equals(String.valueOf(o));
    // }
    let mut matches = Assembler::new();
    matches.new_instance(builder.class("java/lang/Object"));
    matches.dup();
    matches.invokespecial(object_init);
    matches.astore(0);
    matches.aload(0);
    matches.invokevirtual(object_to_string);
    matches.astore(1);
    matches.aload(0);
    matches.op(INVOKESTATIC, &string_value_of_object.to_be_bytes());
    matches.astore(2);
    matches.aload(1);
    matches.aload(2);
    matches.invokevirtual(string_equals);
    matches.ireturn();
    method(&mut builder, "valueOfMatchesToString", "()Z", 2, 3, matches);

    // static boolean intFormatMatches(int v) {
    //     return Integer.toString(v).equals(String.valueOf(v));
    // }
    let mut int_format = Assembler::new();
    int_format.iload(0);
    int_format.op(INVOKESTATIC, &integer_to_string.to_be_bytes());
    int_format.astore(1);
    int_format.iload(0);
    int_format.op(INVOKESTATIC, &string_value_of_int.to_be_bytes());
    int_format.astore(2);
    int_format.aload(1);
    int_format.aload(2);
    int_format.invokevirtual(string_equals);
    int_format.ireturn();
    method(&mut builder, "intFormatMatches", "(I)Z", 2, 3, int_format);

    // static String previousValue(String k, String first, String second) {
    //     HashMap map = new HashMap();
    //     map.put(k, first);
    //     return (String) map.put(k, second);
    // }
    let mut previous_value = Assembler::new();
    new_map(&mut previous_value, map_class, map_init, 3);
    previous_value.aload(3);
    previous_value.aload(0);
    previous_value.aload(1);
    previous_value.invokevirtual(map_put);
    previous_value.astore(4);
    previous_value.aload(3);
    previous_value.aload(0);
    previous_value.aload(2);
    previous_value.invokevirtual(map_put);
    previous_value.areturn();
    method(
        &mut builder,
        "previousValue",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        3,
        5,
        previous_value,
    );

    // static Object firstPutPrevious(String k) {
    //     HashMap map = new HashMap();
    //     return map.put(k, "v");
    // }
    let mut first_put = Assembler::new();
    new_map(&mut first_put, map_class, map_init, 1);
    first_put.aload(1);
    first_put.aload(0);
    first_put.ldc(value_text);
    first_put.invokevirtual(map_put);
    first_put.astore(2);
    first_put.aload(2);
    first_put.areturn();
    method(
        &mut builder,
        "firstPutPrevious",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        3,
        3,
        first_put,
    );

    // static String removeReturns(String k, String v) {
    //     HashMap map = new HashMap();
    //     map.put(k, v);
    //     return (String) map.remove(k);
    // }
    let mut remove_returns = Assembler::new();
    new_map(&mut remove_returns, map_class, map_init, 2);
    remove_returns.aload(2);
    remove_returns.aload(0);
    remove_returns.aload(1);
    remove_returns.invokevirtual(map_put);
    remove_returns.astore(3);
    remove_returns.aload(2);
    remove_returns.aload(0);
    remove_returns.invokevirtual(map_remove);
    remove_returns.areturn();
    method(
        &mut builder,
        "removeReturns",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        3,
        4,
        remove_returns,
    );

    // static int sizeAfterRemove(String k) {
    //     HashMap map = new HashMap();
    //     map.put(k, "v");
    //     map.remove(k);
    //     return map.size();
    // }
    let mut size_after_remove = Assembler::new();
    new_map(&mut size_after_remove, map_class, map_init, 1);
    size_after_remove.aload(1);
    size_after_remove.aload(0);
    size_after_remove.ldc(value_text);
    size_after_remove.invokevirtual(map_put);
    size_after_remove.astore(2);
    size_after_remove.aload(1);
    size_after_remove.aload(0);
    size_after_remove.invokevirtual(map_remove);
    size_after_remove.astore(3);
    size_after_remove.aload(1);
    size_after_remove.invokevirtual(map_size);
    size_after_remove.ireturn();
    method(
        &mut builder,
        "sizeAfterRemove",
        "(Ljava/lang/String;)I",
        3,
        4,
        size_after_remove,
    );

    // static int sizeAfterClear(String k1, String k2) {
    //     HashMap map = new HashMap();
    //     map.put(k1, "v");
    //     map.put(k2, "v");
    //     map.clear();
    //     return map.size();
    // }
    let mut size_after_clear = Assembler::new();
    new_map(&mut size_after_clear, map_class, map_init, 3);
    size_after_clear.aload(3);
    size_after_clear.aload(0);
    size_after_clear.ldc(value_text);
    size_after_clear.invokevirtual(map_put);
    size_after_clear.astore(4);
    size_after_clear.aload(3);
    size_after_clear.aload(1);
    size_after_clear.ldc(value_text);
    size_after_clear.invokevirtual(map_put);
    size_after_clear.astore(4);
    size_after_clear.aload(3);
    size_after_clear.invokevirtual(map_clear);
    size_after_clear.aload(3);
    size_after_clear.invokevirtual(map_size);
    size_after_clear.ireturn();
    method(
        &mut builder,
        "sizeAfterClear",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        3,
        5,
        size_after_clear,
    );

    // static boolean containsValue(String probe) {
    //     HashMap map = new HashMap();
    //     map.put("k", "v");
    //     return map.containsValue(probe);
    // }
    let mut contains_value = Assembler::new();
    new_map(&mut contains_value, map_class, map_init, 1);
    contains_value.aload(1);
    contains_value.ldc(key_text);
    contains_value.ldc(value_text);
    contains_value.invokevirtual(map_put);
    contains_value.astore(2);
    contains_value.aload(1);
    contains_value.aload(0);
    contains_value.invokevirtual(map_contains_value);
    contains_value.ireturn();
    method(
        &mut builder,
        "containsValue",
        "(Ljava/lang/String;)Z",
        3,
        3,
        contains_value,
    );

    // static String getWithOtherKey(String k1, String k2) {
    //     HashMap map = new HashMap();
    //     map.put(k1, "v");
    //     return (String) map.get(k2);
    // }
    let mut get_other = Assembler::new();
    new_map(&mut get_other, map_class, map_init, 2);
    get_other.aload(2);
    get_other.aload(0);
    get_other.ldc(value_text);
    get_other.invokevirtual(map_put);
    get_other.astore(3);
    get_other.aload(2);
    get_other.aload(1);
    get_other.invokevirtual(map_get);
    get_other.areturn();
    method(
        &mut builder,
        "getWithOtherKey",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        3,
        4,
        get_other,
    );

    // static String getAbsent(String k) {
    //     HashMap map = new HashMap();
    //     return (String) map.get(k);
    // }
    let mut get_absent = Assembler::new();
    new_map(&mut get_absent, map_class, map_init, 1);
    get_absent.aload(1);
    get_absent.aload(0);
    get_absent.invokevirtual(map_get);
    get_absent.areturn();
    method(
        &mut builder,
        "getAbsent",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        2,
        2,
        get_absent,
    );

    // static String nullKeyLookup() {
    //     HashMap map = new HashMap();
    //     map.put(null, "n");
    //     return (String) map.get(null);
    // }
    let mut null_key = Assembler::new();
    new_map(&mut null_key, map_class, map_init, 0);
    null_key.aload(0);
    null_key.aconst_null();
    null_key.ldc(null_key_text);
    null_key.invokevirtual(map_put);
    null_key.astore(1);
    null_key.aload(0);
    null_key.aconst_null();
    null_key.invokevirtual(map_get);
    null_key.areturn();
    method(
        &mut builder,
        "nullKeyLookup",
        "()Ljava/lang/String;",
        3,
        2,
        null_key,
    );

    // static int nullValueSize(String k) {
    //     HashMap map = new HashMap();
    //     map.put(k, null);
    //     return map.size();
    // }
    let mut null_value_size = Assembler::new();
    new_map(&mut null_value_size, map_class, map_init, 1);
    null_value_size.aload(1);
    null_value_size.aload(0);
    null_value_size.aconst_null();
    null_value_size.invokevirtual(map_put);
    null_value_size.astore(2);
    null_value_size.aload(1);
    null_value_size.invokevirtual(map_size);
    null_value_size.ireturn();
    method(
        &mut builder,
        "nullValueSize",
        "(Ljava/lang/String;)I",
        3,
        3,
        null_value_size,
    );

    // static Object getNullValue(String k) {
    //     HashMap map = new HashMap();
    //     map.put(k, null);
    //     return map.get(k);
    // }
    let mut get_null = Assembler::new();
    new_map(&mut get_null, map_class, map_init, 1);
    get_null.aload(1);
    get_null.aload(0);
    get_null.aconst_null();
    get_null.invokevirtual(map_put);
    get_null.astore(2);
    get_null.aload(1);
    get_null.aload(0);
    get_null.invokevirtual(map_get);
    get_null.areturn();
    method(
        &mut builder,
        "getNullValue",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        3,
        3,
        get_null,
    );

    // static String boxedKeyLookup() {
    //     HashMap map = new HashMap();
    //     Integer first = Integer.valueOf(7);
    //     Integer second = Integer.valueOf(7);
    //     map.put(first, "seven");
    //     return (String) map.get(second);
    // }
    let mut boxed_key = Assembler::new();
    new_map(&mut boxed_key, map_class, map_init, 0);
    boxed_key.iconst(7);
    boxed_key.op(INVOKESTATIC, &integer_value_of.to_be_bytes());
    boxed_key.astore(1);
    boxed_key.iconst(7);
    boxed_key.op(INVOKESTATIC, &integer_value_of.to_be_bytes());
    boxed_key.astore(2);
    boxed_key.aload(0);
    boxed_key.aload(1);
    boxed_key.ldc(seven_text);
    boxed_key.invokevirtual(map_put);
    boxed_key.astore(3);
    boxed_key.aload(0);
    boxed_key.aload(2);
    boxed_key.invokevirtual(map_get);
    boxed_key.areturn();
    method(
        &mut builder,
        "boxedKeyLookup",
        "()Ljava/lang/String;",
        3,
        4,
        boxed_key,
    );

    // static String mapClassName() {
    //     return new HashMap().getClass().getName();
    // }
    let mut map_class_name = Assembler::new();
    new_map(&mut map_class_name, map_class, map_init, 0);
    map_class_name.aload(0);
    map_class_name.invokevirtual(object_get_class);
    map_class_name.invokevirtual(class_get_name);
    map_class_name.areturn();
    method(
        &mut builder,
        "mapClassName",
        "()Ljava/lang/String;",
        2,
        1,
        map_class_name,
    );

    // static boolean emptyAfterNew() {
    //     return new HashMap().isEmpty();
    // }
    let mut empty_after_new = Assembler::new();
    new_map(&mut empty_after_new, map_class, map_init, 0);
    empty_after_new.aload(0);
    empty_after_new.invokevirtual(map_is_empty);
    empty_after_new.ireturn();
    method(&mut builder, "emptyAfterNew", "()Z", 2, 1, empty_after_new);

    // static boolean newWithCapacity(int capacity) {
    //     return new HashMap(capacity).isEmpty();
    // }
    let mut with_capacity = Assembler::new();
    with_capacity.new_instance(map_class);
    with_capacity.dup();
    with_capacity.iload(0);
    with_capacity.invokespecial(map_init_capacity);
    with_capacity.invokevirtual(map_is_empty);
    with_capacity.ireturn();
    method(&mut builder, "newWithCapacity", "(I)Z", 3, 1, with_capacity);

    // static Object unsupportedKeySet(String k) {
    //     HashMap map = new HashMap();
    //     map.put(k, "v");
    //     return map.keySet();
    // }
    let mut key_set = Assembler::new();
    new_map(&mut key_set, map_class, map_init, 1);
    key_set.aload(1);
    key_set.aload(0);
    key_set.ldc(value_text);
    key_set.invokevirtual(map_put);
    key_set.astore(2);
    key_set.aload(1);
    key_set.invokevirtual(map_key_set);
    key_set.areturn();
    method(
        &mut builder,
        "unsupportedKeySet",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        3,
        3,
        key_set,
    );

    builder.build()
}

fn hash_map_class(fixture: &Fixture) -> ClassId {
    fixture
        .shared
        .load_library_class("java/util/HashMap")
        .expect("the C1 HashMap stub loads")
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
    boxed::register_natives(&shared);
    reflection::register_natives(&shared);
    maps::register_natives(&shared);
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "MapProbe", &probe_bytes())
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

fn ok(fixture: &Fixture, method: &str, descriptor: &str, args: Vec<Value>) -> Option<Value> {
    call(fixture, method, descriptor, args)
        .unwrap_or_else(|error| panic!("{method}{descriptor} failed: {error}"))
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn bool_value(value: Option<Value>) -> bool {
    int_value(value) != 0
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
}

fn string_value(shared: &Shared, value: Option<Value>) -> String {
    let reference = ref_value(value);
    shared
        .heap()
        .string(reference)
        .expect("the result is a string")
        .to_owned()
}

fn heap_string(fixture: &Fixture, text: &str) -> Value {
    Value::Ref(fixture.shared.heap().allocate_string(text))
}

fn heap_string_ref(fixture: &Fixture, text: &str) -> ObjRef {
    ref_value(Some(heap_string(fixture, text)))
}

/// Call a registered native directly, with no bytecode in between.
fn native(
    fixture: &Fixture,
    owner: &str,
    name: &str,
    descriptor: &str,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let function = fixture
        .shared
        .native(owner, name, descriptor)
        .unwrap_or_else(|| panic!("{owner}.{name}{descriptor} is registered"));
    function(
        &fixture.shared,
        &fixture.thread,
        LoaderId::BOOTSTRAP,
        receiver,
        args,
    )
}

/// Call one `String.valueOf` overload directly; the primitive `J`/`F`/`D` forms have no
/// load instruction in the decoded subset.
fn string_value_of_native(
    fixture: &Fixture,
    descriptor: &str,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    native(
        fixture,
        "java/lang/String",
        "valueOf",
        descriptor,
        ObjRef::NULL,
        args,
    )
}

const PUT_DESCRIPTOR: &str = "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;";
const GET_DESCRIPTOR: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";
const REMOVE_DESCRIPTOR: &str = "(Ljava/lang/Object;)Ljava/lang/Object;";
const CONTAINS_KEY_DESCRIPTOR: &str = "(Ljava/lang/Object;)Z";

#[test]
fn string_keys_compare_by_content() {
    let fixture = fixture();
    let first = heap_string_ref(&fixture, "alpha");
    let second = heap_string_ref(&fixture, "alpha");
    let other = heap_string(&fixture, "beta");
    assert_ne!(first, second, "the two equal strings are distinct objects");

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "getWithOtherKey",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![Value::Ref(first), Value::Ref(second),]
            )
        ),
        "v"
    );

    assert!(bool_value(ok(
        &fixture,
        "containsOther",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![Value::Ref(first), Value::Ref(second)]
    )));
    assert!(!bool_value(ok(
        &fixture,
        "containsOther",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![Value::Ref(first), other]
    )));

    // Two equal-content keys are one entry, not two.
    assert_eq!(
        int_value(ok(
            &fixture,
            "sizeAfter",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![Value::Ref(first), Value::Ref(second)]
        )),
        1
    );

    assert!(bool_value(ok(
        &fixture,
        "contains",
        "(Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "k")]
    )));
}

#[test]
fn size_and_is_empty_follow_insertions() {
    let fixture = fixture();
    assert!(bool_value(ok(&fixture, "emptyAfterNew", "()Z", vec![])));
    assert_eq!(
        int_value(ok(
            &fixture,
            "sizeAfter",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "a"), heap_string(&fixture, "b"),]
        )),
        2
    );
}

#[test]
fn put_returns_the_previous_value() {
    let fixture = fixture();
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "previousValue",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![
                    heap_string(&fixture, "k"),
                    heap_string(&fixture, "first"),
                    heap_string(&fixture, "second"),
                ]
            )
        ),
        "first"
    );
    let first_put = ref_value(ok(
        &fixture,
        "firstPutPrevious",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        vec![heap_string(&fixture, "k")],
    ));
    assert!(first_put.is_null(), "a first put has no previous value");
}

#[test]
fn null_key_and_null_value_are_stored() {
    let fixture = fixture();

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "nullKeyLookup", "()Ljava/lang/String;", vec![])
        ),
        "n"
    );

    let stored = heap_string(&fixture, "k");
    let null_value = ref_value(ok(
        &fixture,
        "getNullValue",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        vec![stored],
    ));
    assert!(null_value.is_null());
    assert_eq!(
        int_value(ok(
            &fixture,
            "nullValueSize",
            "(Ljava/lang/String;)I",
            vec![stored]
        )),
        1,
        "a null value is an entry, not an absent key"
    );

    // An absent key also returns null, but containsKey tells it apart, and a stored null
    // value is found by containsValue(null).
    let absent = ref_value(ok(
        &fixture,
        "getAbsent",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        vec![heap_string(&fixture, "missing")],
    ));
    assert!(absent.is_null());

    let map = fixture.shared.heap().allocate_map(hash_map_class(&fixture));
    fixture
        .shared
        .heap()
        .map_mut(map)
        .unwrap()
        .push((Value::Ref(ObjRef::NULL), Value::Ref(ObjRef::NULL)));
    let contains_null = native(
        &fixture,
        "java/util/HashMap",
        "containsValue",
        CONTAINS_KEY_DESCRIPTOR,
        map,
        &[Value::Ref(ObjRef::NULL)],
    )
    .expect("containsValue runs")
    .expect("containsValue returns a value");
    assert!(bool_value(Some(contains_null)));
}

#[test]
fn boxed_integer_keys_compare_by_value() {
    let fixture = fixture();
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "boxedKeyLookup", "()Ljava/lang/String;", vec![])
        ),
        "seven"
    );
}

#[test]
fn remove_and_clear_empty_the_map() {
    let fixture = fixture();
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "removeReturns",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "k"), heap_string(&fixture, "value")]
            )
        ),
        "value"
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "sizeAfterRemove",
            "(Ljava/lang/String;)I",
            vec![heap_string(&fixture, "k")]
        )),
        0
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "sizeAfterClear",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "a"), heap_string(&fixture, "b"),]
        )),
        0
    );
}

#[test]
fn contains_value_uses_content_equality_for_strings() {
    let fixture = fixture();
    assert!(bool_value(ok(
        &fixture,
        "containsValue",
        "(Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "v")]
    )));
    assert!(!bool_value(ok(
        &fixture,
        "containsValue",
        "(Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "missing")]
    )));
}

#[test]
fn map_class_and_dedicated_representation() {
    let fixture = fixture();
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "mapClassName", "()Ljava/lang/String;", vec![])
        ),
        "java.util.HashMap"
    );

    let map = fixture.shared.heap().allocate_map(hash_map_class(&fixture));
    assert!(matches!(
        fixture.shared.heap().get(map).unwrap(),
        HeapObject::Map(map) if map.entries.is_empty()
    ));
    assert!(fixture.shared.heap().list(map).is_err());
    assert!(fixture.shared.heap().map(map).is_ok());

    let class = fixture.shared.get_class(map).expect("a map has a class");
    let id = fixture
        .shared
        .heap()
        .class(class)
        .expect("it is a class object");
    assert_eq!(fixture.shared.entry(id).unwrap().name, "java/util/HashMap");

    // `new` and reflective construction select the representation from the class identity.
    let map_class = fixture
        .shared
        .load_library_class("java/util/HashMap")
        .unwrap();
    let allocated = fixture.shared.allocate_instance(map_class).unwrap();
    assert!(matches!(
        fixture.shared.heap().get(allocated).unwrap(),
        HeapObject::Map(_)
    ));
}

#[test]
fn map_keys_and_values_are_enumerated_as_roots() {
    let fixture = fixture();

    let key = heap_string_ref(&fixture, "root-key");
    let value = heap_string_ref(&fixture, "root-value");
    let map = fixture.shared.heap().allocate_map(hash_map_class(&fixture));
    fixture
        .shared
        .heap()
        .map_mut(map)
        .unwrap()
        .push((Value::Ref(key), Value::Ref(value)));
    let roots = fixture.shared.roots(&[]);
    assert!(roots.contains(&key), "a map key is a root: {roots:?}");
    assert!(roots.contains(&value), "a map value is a root: {roots:?}");

    // The map the bytecode path allocated is unreachable after the call returns, but the
    // heap still holds it, so its key must remain in the root set.
    let built_key = heap_string_ref(&fixture, "built");
    let _ = ok(
        &fixture,
        "putGet",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        vec![Value::Ref(built_key), heap_string(&fixture, "built-value")],
    );
    let roots = fixture.shared.roots(&[]);
    assert!(
        roots.contains(&built_key),
        "a reference stored in a heap map is a root: {roots:?}"
    );

    // A null value is stored, but null is never a root.
    let null_map = fixture.shared.heap().allocate_map(hash_map_class(&fixture));
    fixture
        .shared
        .heap()
        .map_mut(null_map)
        .unwrap()
        .push((Value::Ref(key), Value::Ref(ObjRef::NULL)));
    let roots = fixture.shared.roots(&[]);
    assert!(roots.contains(&key));
    assert!(!roots.contains(&ObjRef::NULL));
}

#[test]
fn string_value_of_renders_null_and_primitives() {
    let fixture = fixture();

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "valueOfNull", "()Ljava/lang/String;", vec![])
        ),
        "null"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "valueOfInt",
                "(I)Ljava/lang/String;",
                vec![Value::Int(42)]
            )
        ),
        "42"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "valueOfInt",
                "(I)Ljava/lang/String;",
                vec![Value::Int(-7)]
            )
        ),
        "-7"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "valueOfBoolean",
                "(I)Ljava/lang/String;",
                vec![Value::Int(1)]
            )
        ),
        "true"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "valueOfBoolean",
                "(I)Ljava/lang/String;",
                vec![Value::Int(0)]
            )
        ),
        "false"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "valueOfChar",
                "(I)Ljava/lang/String;",
                vec![Value::Int(65)]
            )
        ),
        "A"
    );
}

#[test]
fn string_value_of_object_uses_the_object_to_string_shape() {
    let fixture = fixture();

    let rendered = string_value(
        &fixture.shared,
        ok(&fixture, "valueOfObject", "()Ljava/lang/String;", vec![]),
    );
    let suffix = rendered
        .strip_prefix("java.lang.Object@")
        .unwrap_or_else(|| panic!("unexpected String.valueOf(Object) text {rendered:?}"));
    assert!(!suffix.is_empty());
    assert!(
        suffix
            .chars()
            .all(|character| character.is_ascii_hexdigit()),
        "the identity hash is hexadecimal: {rendered:?}"
    );
    assert!(bool_value(ok(
        &fixture,
        "valueOfMatchesToString",
        "()Z",
        vec![]
    )));

    // A String argument is returned itself, preserving reference identity.
    let hello = heap_string_ref(&fixture, "hello");
    let rendered = ref_value(ok(
        &fixture,
        "valueOfObjectArg",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        vec![Value::Ref(hello)],
    ));
    assert_eq!(rendered, hello);

    // A boxed Integer contributes its decimal value.
    let boxed = Value::Ref(fixture.shared.box_int(42));
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "valueOfObjectArg",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![boxed]
            )
        ),
        "42"
    );
}

#[test]
fn string_value_of_int_matches_integer_to_string() {
    let fixture = fixture();
    for value in [0, 1, -1, 42, -2147483648, 2147483647] {
        assert!(
            bool_value(ok(
                &fixture,
                "intFormatMatches",
                "(I)Z",
                vec![Value::Int(value)]
            )),
            "String.valueOf({value}) matches Integer.toString({value})"
        );
    }
}

#[test]
fn string_value_of_long_float_and_double_use_one_formatting() {
    let fixture = fixture();

    let long = string_value(
        &fixture.shared,
        string_value_of_native(
            &fixture,
            "(J)Ljava/lang/String;",
            &[Value::Long(1234567890123)],
        )
        .expect("String.valueOf(long) runs"),
    );
    assert_eq!(long, "1234567890123");

    let float = string_value(
        &fixture.shared,
        string_value_of_native(&fixture, "(F)Ljava/lang/String;", &[Value::Float(1.5)])
            .expect("String.valueOf(float) runs"),
    );
    assert_eq!(float, "1.5");
    let whole = string_value(
        &fixture.shared,
        string_value_of_native(&fixture, "(F)Ljava/lang/String;", &[Value::Float(1.0)])
            .expect("String.valueOf(float) runs"),
    );
    assert_eq!(whole, "1.0", "a whole float keeps the mandatory .0");

    let double = string_value(
        &fixture.shared,
        string_value_of_native(&fixture, "(D)Ljava/lang/String;", &[Value::Double(2.25)])
            .expect("String.valueOf(double) runs"),
    );
    assert_eq!(double, "2.25");
    let infinite = string_value(
        &fixture.shared,
        string_value_of_native(
            &fixture,
            "(D)Ljava/lang/String;",
            &[Value::Double(f64::NEG_INFINITY)],
        )
        .expect("String.valueOf(double) runs"),
    );
    assert_eq!(infinite, "-Infinity");
}

#[test]
fn string_value_of_char_rejects_a_lone_surrogate() {
    let fixture = fixture();
    match call(
        &fixture,
        "valueOfChar",
        "(I)Ljava/lang/String;",
        vec![Value::Int(0xD800)],
    ) {
        Err(InterpError::Vm(VmError::UnpairedSurrogate { method })) => {
            assert_eq!(method, "String.valueOf");
        }
        other => panic!("expected UnpairedSurrogate, found {other:?}"),
    }
}

#[test]
fn map_constructors_and_arguments_are_typed_errors() {
    let fixture = fixture();

    // A negative capacity is the JDK's IllegalArgumentException, reported as a typed VM
    // error because this slice has no exception-object synthesis.
    match call(&fixture, "newWithCapacity", "(I)Z", vec![Value::Int(-1)]) {
        Err(InterpError::Vm(VmError::Native(message))) => {
            assert!(message.contains("negative capacity"), "{message}");
        }
        other => panic!("expected a native error, found {other:?}"),
    }
    // A capacity is a hint: a non-negative one does not change the empty map.
    assert!(bool_value(ok(
        &fixture,
        "newWithCapacity",
        "(I)Z",
        vec![Value::Int(16)]
    )));

    let map = fixture.shared.heap().allocate_map(hash_map_class(&fixture));
    for (name, descriptor, args) in [
        (
            "put",
            PUT_DESCRIPTOR,
            vec![Value::Int(0), Value::Ref(ObjRef::NULL)],
        ),
        (
            "put",
            PUT_DESCRIPTOR,
            vec![Value::Ref(ObjRef::NULL), Value::Int(1)],
        ),
        ("get", GET_DESCRIPTOR, vec![Value::Int(0)]),
    ] {
        match native(&fixture, "java/util/HashMap", name, descriptor, map, &args) {
            Err(VmError::Native(message)) => {
                assert!(message.contains("reference argument"), "{message}");
            }
            other => panic!("expected a typed reference-argument error, found {other:?}"),
        }
    }

    match native(
        &fixture,
        "java/util/HashMap",
        "size",
        "()I",
        ObjRef::NULL,
        &[],
    ) {
        Err(VmError::Native(message)) => assert!(message.contains("null receiver"), "{message}"),
        other => panic!("expected a null-receiver error, found {other:?}"),
    }

    let not_a_map = fixture.shared.heap().allocate_string("not a map");
    match native(&fixture, "java/util/HashMap", "size", "()I", not_a_map, &[]) {
        Err(VmError::Native(message)) => {
            assert!(message.contains("java.util.HashMap"), "{message}")
        }
        other => panic!("expected a wrong-receiver error, found {other:?}"),
    }

    // remove and containsKey share the same reference-argument rule.
    for (name, descriptor) in [
        ("remove", REMOVE_DESCRIPTOR),
        ("containsKey", CONTAINS_KEY_DESCRIPTOR),
    ] {
        match native(
            &fixture,
            "java/util/HashMap",
            name,
            descriptor,
            map,
            &[Value::Int(0)],
        ) {
            Err(VmError::Native(message)) => {
                assert!(message.contains("reference argument"), "{message}");
            }
            other => panic!("expected a typed reference-argument error, found {other:?}"),
        }
    }
}

#[test]
fn map_key_set_is_a_set_view_of_the_keys() {
    let fixture = fixture();
    let value = ok(
        &fixture,
        "unsupportedKeySet",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        vec![heap_string(&fixture, "k")],
    )
    .expect("keySet returns a set");
    let Value::Ref(reference) = value else {
        panic!("keySet returned a non-reference value");
    };
    assert!(!reference.is_null(), "keySet returned null");
    let elements = fixture
        .shared
        .heap()
        .set(reference)
        .expect("keySet returns the set shape")
        .to_vec();
    assert_eq!(elements.len(), 1);
    let Value::Ref(key) = elements[0] else {
        panic!("keySet contains a non-reference");
    };
    assert_eq!(
        fixture
            .shared
            .heap()
            .string(key)
            .expect("the key is a String"),
        "k"
    );
}
