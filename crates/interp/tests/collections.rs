//! M2.12 acceptance: `StringBuilder` and core `ArrayList` behavior.
//!
//! `CollectionsProbe` is assembled against the committed C1 bootstrap library. Its methods
//! call the real `java.lang.StringBuilder` and `java.util.ArrayList` stub methods through
//! `invokevirtual`/`invokespecial`, so the tests exercise the native lookup and the
//! receiver dispatch the interpreter actually performs rather than a Rust function
//! directly. The two primitives the decoded instruction subset cannot load (`long`,
//! `float`, `double`) are passed to their registered natives from Rust; the tests say so
//! where it happens.
//!
//! Supplementary characters are written as surrogate pairs in the class files and in heap
//! strings, so a UTF-16 length and index is asserted against units, not Rust `char`s.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ClassId, HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, collections,
    instance, object, reflection, string,
};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;

/// `U+1F600 GRINNING FACE`: one Unicode character, two UTF-16 code units.
const GRINNING_HIGH: i32 = 0xD83D;
const GRINNING_LOW: i32 = 0xDE00;

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

/// A fresh `StringBuilder` in `local`, pushed by `new`/`dup`/`<init>()V`.
fn new_builder(asm: &mut Assembler, class: u16, init: u16, local: u8) {
    asm.new_instance(class);
    asm.dup();
    asm.invokespecial(init);
    asm.astore(local);
}

/// Append `value` to the builder in `local`, keeping the returned receiver.
fn append_kept(asm: &mut Assembler, local: u8, value: impl FnOnce(&mut Assembler), append: u16) {
    asm.aload(local);
    value(asm);
    asm.invokevirtual(append);
    asm.astore(local);
}

fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("CollectionsProbe", "java/lang/Object", 61);

    // String constants first, so every `ldc` index stays inside its one-byte form.
    let missing = builder.string("missing");
    let abc = builder.string("abc");
    let grinning = builder.string("😀");
    let a_grinning_b = builder.string("a😀b");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let object_get_class =
        builder.method_ref("java/lang/Object", "getClass", "()Ljava/lang/Class;");
    let object_equals = builder.method_ref("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
    let class_get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");

    let sb_class = builder.class("java/lang/StringBuilder");
    let sb_init = builder.method_ref("java/lang/StringBuilder", "<init>", "()V");
    let sb_init_int = builder.method_ref("java/lang/StringBuilder", "<init>", "(I)V");
    let sb_init_string =
        builder.method_ref("java/lang/StringBuilder", "<init>", "(Ljava/lang/String;)V");
    let sb_append_string = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
    );
    let sb_append_object = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
    );
    let sb_append_int = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(I)Ljava/lang/StringBuilder;",
    );
    let sb_append_char = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(C)Ljava/lang/StringBuilder;",
    );
    let sb_append_boolean = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(Z)Ljava/lang/StringBuilder;",
    );
    let sb_append_char_sequence = builder.method_ref(
        "java/lang/StringBuilder",
        "append",
        "(Ljava/lang/CharSequence;)Ljava/lang/StringBuilder;",
    );
    let sb_to_string = builder.method_ref(
        "java/lang/StringBuilder",
        "toString",
        "()Ljava/lang/String;",
    );
    let sb_length = builder.method_ref("java/lang/StringBuilder", "length", "()I");
    let sb_char_at = builder.method_ref("java/lang/StringBuilder", "charAt", "(I)C");
    let sb_set_length = builder.method_ref("java/lang/StringBuilder", "setLength", "(I)V");
    let sb_reverse = builder.method_ref(
        "java/lang/StringBuilder",
        "reverse",
        "()Ljava/lang/StringBuilder;",
    );
    let sb_delete_char_at = builder.method_ref(
        "java/lang/StringBuilder",
        "deleteCharAt",
        "(I)Ljava/lang/StringBuilder;",
    );
    let sb_insert_string = builder.method_ref(
        "java/lang/StringBuilder",
        "insert",
        "(ILjava/lang/String;)Ljava/lang/StringBuilder;",
    );

    let al_class = builder.class("java/util/ArrayList");
    let al_init = builder.method_ref("java/util/ArrayList", "<init>", "()V");
    let al_init_int = builder.method_ref("java/util/ArrayList", "<init>", "(I)V");
    let al_add = builder.method_ref("java/util/ArrayList", "add", "(Ljava/lang/Object;)Z");
    let al_add_at = builder.method_ref("java/util/ArrayList", "add", "(ILjava/lang/Object;)V");
    let al_get = builder.method_ref("java/util/ArrayList", "get", "(I)Ljava/lang/Object;");
    let al_set = builder.method_ref(
        "java/util/ArrayList",
        "set",
        "(ILjava/lang/Object;)Ljava/lang/Object;",
    );
    let al_size = builder.method_ref("java/util/ArrayList", "size", "()I");
    let al_is_empty = builder.method_ref("java/util/ArrayList", "isEmpty", "()Z");
    let al_contains =
        builder.method_ref("java/util/ArrayList", "contains", "(Ljava/lang/Object;)Z");
    let al_index_of = builder.method_ref("java/util/ArrayList", "indexOf", "(Ljava/lang/Object;)I");
    let al_clear = builder.method_ref("java/util/ArrayList", "clear", "()V");
    let al_remove_at = builder.method_ref("java/util/ArrayList", "remove", "(I)Ljava/lang/Object;");
    let al_remove_object =
        builder.method_ref("java/util/ArrayList", "remove", "(Ljava/lang/Object;)Z");

    // CollectionsProbe() { super(); }
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

    // static String build(String a, int b) {
    //     StringBuilder sb = new StringBuilder();
    //     sb = sb.append(a);
    //     sb = sb.append(b);
    //     return sb.toString();
    // }
    let mut build = Assembler::new();
    new_builder(&mut build, sb_class, sb_init, 2);
    append_kept(&mut build, 2, |asm| asm.aload(0), sb_append_string);
    append_kept(&mut build, 2, |asm| asm.iload(1), sb_append_int);
    build.aload(2);
    build.invokevirtual(sb_to_string);
    build.areturn();
    method(
        &mut builder,
        "build",
        "(Ljava/lang/String;I)Ljava/lang/String;",
        2,
        3,
        build,
    );

    // static String builderClass() { return new StringBuilder().getClass().getName(); }
    let mut builder_class = Assembler::new();
    builder_class.new_instance(sb_class);
    builder_class.dup();
    builder_class.invokespecial(sb_init);
    builder_class.invokevirtual(object_get_class);
    builder_class.invokevirtual(class_get_name);
    builder_class.areturn();
    method(
        &mut builder,
        "builderClass",
        "()Ljava/lang/String;",
        2,
        0,
        builder_class,
    );

    // static String listClass() { return new ArrayList().getClass().getName(); }
    let mut list_class = Assembler::new();
    list_class.new_instance(al_class);
    list_class.dup();
    list_class.invokespecial(al_init);
    list_class.invokevirtual(object_get_class);
    list_class.invokevirtual(class_get_name);
    list_class.areturn();
    method(
        &mut builder,
        "listClass",
        "()Ljava/lang/String;",
        2,
        0,
        list_class,
    );

    // static String reverseOf(String a) {
    //     StringBuilder sb = new StringBuilder().append(a);
    //     return sb.reverse().toString();
    // }
    let mut reverse_of = Assembler::new();
    new_builder(&mut reverse_of, sb_class, sb_init, 1);
    append_kept(&mut reverse_of, 1, |asm| asm.aload(0), sb_append_string);
    reverse_of.aload(1);
    reverse_of.invokevirtual(sb_reverse);
    reverse_of.astore(1);
    reverse_of.aload(1);
    reverse_of.invokevirtual(sb_to_string);
    reverse_of.areturn();
    method(
        &mut builder,
        "reverseOf",
        "(Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        reverse_of,
    );

    // static String appendObject(Object o) { ... sb.append(o).toString(); }
    let mut append_object = Assembler::new();
    new_builder(&mut append_object, sb_class, sb_init, 1);
    append_kept(&mut append_object, 1, |asm| asm.aload(0), sb_append_object);
    append_object.aload(1);
    append_object.invokevirtual(sb_to_string);
    append_object.areturn();
    method(
        &mut builder,
        "appendObject",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        2,
        2,
        append_object,
    );

    // static String appendChar(int c) { ... sb.append((char) c).toString(); }
    let mut append_char = Assembler::new();
    new_builder(&mut append_char, sb_class, sb_init, 1);
    append_kept(&mut append_char, 1, |asm| asm.iload(0), sb_append_char);
    append_char.aload(1);
    append_char.invokevirtual(sb_to_string);
    append_char.areturn();
    method(
        &mut builder,
        "appendChar",
        "(I)Ljava/lang/String;",
        2,
        2,
        append_char,
    );

    // static String appendBoolean() { ... sb.append(true).toString(); }
    let mut append_boolean = Assembler::new();
    new_builder(&mut append_boolean, sb_class, sb_init, 0);
    append_kept(
        &mut append_boolean,
        0,
        |asm| asm.iconst(1),
        sb_append_boolean,
    );
    append_boolean.aload(0);
    append_boolean.invokevirtual(sb_to_string);
    append_boolean.areturn();
    method(
        &mut builder,
        "appendBoolean",
        "()Ljava/lang/String;",
        2,
        1,
        append_boolean,
    );

    // static String appendCharSequence(String s) { ... sb.append((CharSequence) s) ... }
    let mut append_char_sequence = Assembler::new();
    new_builder(&mut append_char_sequence, sb_class, sb_init, 1);
    append_kept(
        &mut append_char_sequence,
        1,
        |asm| asm.aload(0),
        sb_append_char_sequence,
    );
    append_char_sequence.aload(1);
    append_char_sequence.invokevirtual(sb_to_string);
    append_char_sequence.areturn();
    method(
        &mut builder,
        "appendCharSequence",
        "(Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        append_char_sequence,
    );

    // static int builderLength(String s) { return new StringBuilder().append(s).length(); }
    let mut builder_length = Assembler::new();
    new_builder(&mut builder_length, sb_class, sb_init, 1);
    append_kept(&mut builder_length, 1, |asm| asm.aload(0), sb_append_string);
    builder_length.aload(1);
    builder_length.invokevirtual(sb_length);
    builder_length.ireturn();
    method(
        &mut builder,
        "builderLength",
        "(Ljava/lang/String;)I",
        2,
        2,
        builder_length,
    );

    // static char builderCharAt(String s, int index) { ... sb.charAt(index); }
    let mut builder_char_at = Assembler::new();
    new_builder(&mut builder_char_at, sb_class, sb_init, 2);
    append_kept(
        &mut builder_char_at,
        2,
        |asm| asm.aload(0),
        sb_append_string,
    );
    builder_char_at.aload(2);
    builder_char_at.iload(1);
    builder_char_at.invokevirtual(sb_char_at);
    builder_char_at.ireturn();
    method(
        &mut builder,
        "builderCharAt",
        "(Ljava/lang/String;I)C",
        2,
        3,
        builder_char_at,
    );

    // static String builderSetLength(String s, int n) {
    //     StringBuilder sb = new StringBuilder().append(s);
    //     sb.setLength(n);
    //     return sb.toString();
    // }
    let mut builder_set_length = Assembler::new();
    new_builder(&mut builder_set_length, sb_class, sb_init, 2);
    append_kept(
        &mut builder_set_length,
        2,
        |asm| asm.aload(0),
        sb_append_string,
    );
    builder_set_length.aload(2);
    builder_set_length.iload(1);
    builder_set_length.invokevirtual(sb_set_length);
    builder_set_length.aload(2);
    builder_set_length.invokevirtual(sb_to_string);
    builder_set_length.areturn();
    method(
        &mut builder,
        "builderSetLength",
        "(Ljava/lang/String;I)Ljava/lang/String;",
        2,
        3,
        builder_set_length,
    );

    // static String builderDeleteCharAt(String s, int index) {
    //     StringBuilder sb = new StringBuilder().append(s);
    //     sb.deleteCharAt(index);
    //     return sb.toString();
    // }
    let mut builder_delete = Assembler::new();
    new_builder(&mut builder_delete, sb_class, sb_init, 2);
    append_kept(&mut builder_delete, 2, |asm| asm.aload(0), sb_append_string);
    builder_delete.aload(2);
    builder_delete.iload(1);
    builder_delete.invokevirtual(sb_delete_char_at);
    builder_delete.astore(2);
    builder_delete.aload(2);
    builder_delete.invokevirtual(sb_to_string);
    builder_delete.areturn();
    method(
        &mut builder,
        "builderDeleteCharAt",
        "(Ljava/lang/String;I)Ljava/lang/String;",
        2,
        3,
        builder_delete,
    );

    // static String builderInsert(String a, String b) {
    //     StringBuilder sb = new StringBuilder().append(a);
    //     sb.insert(1, b);
    //     return sb.toString();
    // }
    let mut builder_insert = Assembler::new();
    new_builder(&mut builder_insert, sb_class, sb_init, 2);
    append_kept(&mut builder_insert, 2, |asm| asm.aload(0), sb_append_string);
    builder_insert.aload(2);
    builder_insert.iconst(1);
    builder_insert.aload(1);
    builder_insert.invokevirtual(sb_insert_string);
    builder_insert.astore(2);
    builder_insert.aload(2);
    builder_insert.invokevirtual(sb_to_string);
    builder_insert.areturn();
    method(
        &mut builder,
        "builderInsert",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        3,
        3,
        builder_insert,
    );

    // static String builderNullConstructor() {
    //     return new StringBuilder((String) null).toString();
    // }
    let mut null_constructor = Assembler::new();
    null_constructor.new_instance(sb_class);
    null_constructor.dup();
    null_constructor.aconst_null();
    null_constructor.invokespecial(sb_init_string);
    null_constructor.invokevirtual(sb_to_string);
    null_constructor.areturn();
    method(
        &mut builder,
        "builderNullConstructor",
        "()Ljava/lang/String;",
        3,
        0,
        null_constructor,
    );

    // static String builderAppendNullString() { ... sb.append((String) null) ... }
    let mut append_null_string = Assembler::new();
    new_builder(&mut append_null_string, sb_class, sb_init, 0);
    append_kept(
        &mut append_null_string,
        0,
        |asm| asm.aconst_null(),
        sb_append_string,
    );
    append_null_string.aload(0);
    append_null_string.invokevirtual(sb_to_string);
    append_null_string.areturn();
    method(
        &mut builder,
        "builderAppendNullString",
        "()Ljava/lang/String;",
        2,
        1,
        append_null_string,
    );

    // static String builderAppendNullObject() { ... sb.append((Object) null) ... }
    let mut append_null_object = Assembler::new();
    new_builder(&mut append_null_object, sb_class, sb_init, 0);
    append_kept(
        &mut append_null_object,
        0,
        |asm| asm.aconst_null(),
        sb_append_object,
    );
    append_null_object.aload(0);
    append_null_object.invokevirtual(sb_to_string);
    append_null_object.areturn();
    method(
        &mut builder,
        "builderAppendNullObject",
        "()Ljava/lang/String;",
        2,
        1,
        append_null_object,
    );

    // static int builderCapacityLength() { return new StringBuilder(16).length(); }
    let mut capacity_length = Assembler::new();
    capacity_length.new_instance(sb_class);
    capacity_length.dup();
    capacity_length.iconst(16);
    capacity_length.invokespecial(sb_init_int);
    capacity_length.invokevirtual(sb_length);
    capacity_length.ireturn();
    method(
        &mut builder,
        "builderCapacityLength",
        "()I",
        3,
        0,
        capacity_length,
    );

    // static int listOps(String a, String b) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     list.add(b);
    //     if (!list.contains(a)) return -1;
    //     if (!list.contains(b)) return -2;
    //     if (list.contains("missing")) return -3;
    //     return list.size();
    // }
    let mut list_ops = Assembler::new();
    list_ops.new_instance(al_class);
    list_ops.dup();
    list_ops.invokespecial(al_init);
    list_ops.astore(2);
    list_ops.aload(2);
    list_ops.aload(0);
    list_ops.invokevirtual(al_add);
    list_ops.istore(3);
    list_ops.aload(2);
    list_ops.aload(1);
    list_ops.invokevirtual(al_add);
    list_ops.istore(3);
    let has_a = list_ops.label();
    let has_b = list_ops.label();
    let no_missing = list_ops.label();
    list_ops.aload(2);
    list_ops.aload(0);
    list_ops.invokevirtual(al_contains);
    list_ops.if_cond(0x9a, has_a); // ifne
    list_ops.iconst(-1);
    list_ops.ireturn();
    list_ops.place(has_a);
    list_ops.aload(2);
    list_ops.aload(1);
    list_ops.invokevirtual(al_contains);
    list_ops.if_cond(0x9a, has_b); // ifne
    list_ops.iconst(-2);
    list_ops.ireturn();
    list_ops.place(has_b);
    list_ops.aload(2);
    list_ops.ldc(missing);
    list_ops.invokevirtual(al_contains);
    list_ops.if_cond(0x99, no_missing); // ifeq
    list_ops.iconst(-3);
    list_ops.ireturn();
    list_ops.place(no_missing);
    list_ops.aload(2);
    list_ops.invokevirtual(al_size);
    list_ops.ireturn();
    method(
        &mut builder,
        "listOps",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        2,
        4,
        list_ops,
    );

    // static String listGet(String a, String b, int index) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     list.add(b);
    //     return (String) list.get(index);
    // }
    let mut list_get = Assembler::new();
    list_get.new_instance(al_class);
    list_get.dup();
    list_get.invokespecial(al_init);
    list_get.astore(3);
    list_get.aload(3);
    list_get.aload(0);
    list_get.invokevirtual(al_add);
    list_get.istore(4);
    list_get.aload(3);
    list_get.aload(1);
    list_get.invokevirtual(al_add);
    list_get.istore(4);
    list_get.aload(3);
    list_get.iload(2);
    list_get.invokevirtual(al_get);
    list_get.areturn();
    method(
        &mut builder,
        "listGet",
        "(Ljava/lang/String;Ljava/lang/String;I)Ljava/lang/String;",
        2,
        5,
        list_get,
    );

    // static String listSet(String a, String b) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     list.add(b);
    //     list.set(0, b);
    //     return (String) list.get(0);
    // }
    let mut list_set = Assembler::new();
    list_set.new_instance(al_class);
    list_set.dup();
    list_set.invokespecial(al_init);
    list_set.astore(2);
    list_set.aload(2);
    list_set.aload(0);
    list_set.invokevirtual(al_add);
    list_set.istore(3);
    list_set.aload(2);
    list_set.aload(1);
    list_set.invokevirtual(al_add);
    list_set.istore(3);
    list_set.aload(2);
    list_set.iconst(0);
    list_set.aload(1);
    list_set.invokevirtual(al_set);
    list_set.astore(3);
    list_set.aload(2);
    list_set.iconst(0);
    list_set.invokevirtual(al_get);
    list_set.areturn();
    method(
        &mut builder,
        "listSet",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        3,
        4,
        list_set,
    );

    // static String listAddAt(String a, String b) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     list.add(0, b);
    //     return (String) list.get(0);
    // }
    let mut list_add_at = Assembler::new();
    list_add_at.new_instance(al_class);
    list_add_at.dup();
    list_add_at.invokespecial(al_init);
    list_add_at.astore(2);
    list_add_at.aload(2);
    list_add_at.aload(0);
    list_add_at.invokevirtual(al_add);
    list_add_at.istore(3);
    list_add_at.aload(2);
    list_add_at.iconst(0);
    list_add_at.aload(1);
    list_add_at.invokevirtual(al_add_at);
    list_add_at.aload(2);
    list_add_at.iconst(0);
    list_add_at.invokevirtual(al_get);
    list_add_at.areturn();
    method(
        &mut builder,
        "listAddAt",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        3,
        4,
        list_add_at,
    );

    // static String listRemoveAt(String a, String b) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     list.add(b);
    //     list.remove(0);
    //     return (String) list.get(0);
    // }
    let mut list_remove_at = Assembler::new();
    list_remove_at.new_instance(al_class);
    list_remove_at.dup();
    list_remove_at.invokespecial(al_init);
    list_remove_at.astore(2);
    list_remove_at.aload(2);
    list_remove_at.aload(0);
    list_remove_at.invokevirtual(al_add);
    list_remove_at.istore(3);
    list_remove_at.aload(2);
    list_remove_at.aload(1);
    list_remove_at.invokevirtual(al_add);
    list_remove_at.istore(3);
    list_remove_at.aload(2);
    list_remove_at.iconst(0);
    list_remove_at.invokevirtual(al_remove_at);
    list_remove_at.astore(3);
    list_remove_at.aload(2);
    list_remove_at.iconst(0);
    list_remove_at.invokevirtual(al_get);
    list_remove_at.areturn();
    method(
        &mut builder,
        "listRemoveAt",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        2,
        4,
        list_remove_at,
    );

    // static boolean listRemoveObject(String a) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     return list.remove(a);
    // }
    let mut list_remove_object = Assembler::new();
    list_remove_object.new_instance(al_class);
    list_remove_object.dup();
    list_remove_object.invokespecial(al_init);
    list_remove_object.astore(1);
    list_remove_object.aload(1);
    list_remove_object.aload(0);
    list_remove_object.invokevirtual(al_add);
    list_remove_object.istore(2);
    list_remove_object.aload(1);
    list_remove_object.aload(0);
    list_remove_object.invokevirtual(al_remove_object);
    list_remove_object.ireturn();
    method(
        &mut builder,
        "listRemoveObject",
        "(Ljava/lang/String;)Z",
        2,
        3,
        list_remove_object,
    );

    // static boolean listContainsEqual(String a, String b) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     return list.contains(b);
    // }
    let mut list_contains_equal = Assembler::new();
    list_contains_equal.new_instance(al_class);
    list_contains_equal.dup();
    list_contains_equal.invokespecial(al_init);
    list_contains_equal.astore(2);
    list_contains_equal.aload(2);
    list_contains_equal.aload(0);
    list_contains_equal.invokevirtual(al_add);
    list_contains_equal.istore(3);
    list_contains_equal.aload(2);
    list_contains_equal.aload(1);
    list_contains_equal.invokevirtual(al_contains);
    list_contains_equal.ireturn();
    method(
        &mut builder,
        "listContainsEqual",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        2,
        4,
        list_contains_equal,
    );

    // static int listIndexOf(String a, String b) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     list.add(b);
    //     return list.indexOf(b);
    // }
    let mut list_index_of = Assembler::new();
    list_index_of.new_instance(al_class);
    list_index_of.dup();
    list_index_of.invokespecial(al_init);
    list_index_of.astore(2);
    list_index_of.aload(2);
    list_index_of.aload(0);
    list_index_of.invokevirtual(al_add);
    list_index_of.istore(3);
    list_index_of.aload(2);
    list_index_of.aload(1);
    list_index_of.invokevirtual(al_add);
    list_index_of.istore(3);
    list_index_of.aload(2);
    list_index_of.aload(1);
    list_index_of.invokevirtual(al_index_of);
    list_index_of.ireturn();
    method(
        &mut builder,
        "listIndexOf",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        2,
        4,
        list_index_of,
    );

    // static boolean listClearIsEmpty(String a) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     list.clear();
    //     return list.isEmpty();
    // }
    let mut list_clear = Assembler::new();
    list_clear.new_instance(al_class);
    list_clear.dup();
    list_clear.invokespecial(al_init);
    list_clear.astore(1);
    list_clear.aload(1);
    list_clear.aload(0);
    list_clear.invokevirtual(al_add);
    list_clear.istore(2);
    list_clear.aload(1);
    list_clear.invokevirtual(al_clear);
    list_clear.aload(1);
    list_clear.invokevirtual(al_is_empty);
    list_clear.ireturn();
    method(
        &mut builder,
        "listClearIsEmpty",
        "(Ljava/lang/String;)Z",
        2,
        3,
        list_clear,
    );

    // static boolean listIsEmpty() { return new ArrayList().isEmpty(); }
    let mut list_is_empty = Assembler::new();
    list_is_empty.new_instance(al_class);
    list_is_empty.dup();
    list_is_empty.invokespecial(al_init);
    list_is_empty.invokevirtual(al_is_empty);
    list_is_empty.ireturn();
    method(&mut builder, "listIsEmpty", "()Z", 2, 0, list_is_empty);

    // static int listCapacitySize() { return new ArrayList(4).size(); }
    let mut list_capacity = Assembler::new();
    list_capacity.new_instance(al_class);
    list_capacity.dup();
    list_capacity.iconst(4);
    list_capacity.invokespecial(al_init_int);
    list_capacity.invokevirtual(al_size);
    list_capacity.ireturn();
    method(&mut builder, "listCapacitySize", "()I", 3, 0, list_capacity);

    // static int listNullOps() {
    //     ArrayList list = new ArrayList();
    //     list.add(null);
    //     if (!list.contains(null)) return 0;
    //     if (!list.remove(null)) return 0;
    //     return list.isEmpty() ? 1 : 0;
    // }
    let mut list_null_ops = Assembler::new();
    list_null_ops.new_instance(al_class);
    list_null_ops.dup();
    list_null_ops.invokespecial(al_init);
    list_null_ops.astore(0);
    list_null_ops.aload(0);
    list_null_ops.aconst_null();
    list_null_ops.invokevirtual(al_add);
    list_null_ops.istore(1);
    let contains_null = list_null_ops.label();
    let remove_null = list_null_ops.label();
    list_null_ops.aload(0);
    list_null_ops.aconst_null();
    list_null_ops.invokevirtual(al_contains);
    list_null_ops.if_cond(0x9a, contains_null); // ifne
    list_null_ops.iconst(0);
    list_null_ops.ireturn();
    list_null_ops.place(contains_null);
    list_null_ops.aload(0);
    list_null_ops.aconst_null();
    list_null_ops.invokevirtual(al_remove_object);
    list_null_ops.if_cond(0x9a, remove_null); // ifne
    list_null_ops.iconst(0);
    list_null_ops.ireturn();
    list_null_ops.place(remove_null);
    list_null_ops.aload(0);
    list_null_ops.invokevirtual(al_is_empty);
    list_null_ops.ireturn();
    method(&mut builder, "listNullOps", "()I", 2, 2, list_null_ops);

    // static Object listGetNull() {
    //     ArrayList list = new ArrayList();
    //     list.add(null);
    //     return list.get(0);
    // }
    let mut list_get_null = Assembler::new();
    list_get_null.new_instance(al_class);
    list_get_null.dup();
    list_get_null.invokespecial(al_init);
    list_get_null.astore(0);
    list_get_null.aload(0);
    list_get_null.aconst_null();
    list_get_null.invokevirtual(al_add);
    list_get_null.istore(1);
    list_get_null.aload(0);
    list_get_null.iconst(0);
    list_get_null.invokevirtual(al_get);
    list_get_null.areturn();
    method(
        &mut builder,
        "listGetNull",
        "()Ljava/lang/Object;",
        2,
        2,
        list_get_null,
    );

    // static Object listGetOutOfBounds() { return new ArrayList().get(0); }
    let mut list_get_oob = Assembler::new();
    list_get_oob.new_instance(al_class);
    list_get_oob.dup();
    list_get_oob.invokespecial(al_init);
    list_get_oob.iconst(0);
    list_get_oob.invokevirtual(al_get);
    list_get_oob.areturn();
    method(
        &mut builder,
        "listGetOutOfBounds",
        "()Ljava/lang/Object;",
        2,
        0,
        list_get_oob,
    );

    // static Object listSetOutOfBounds() { return new ArrayList().set(0, null); }
    let mut list_set_oob = Assembler::new();
    list_set_oob.new_instance(al_class);
    list_set_oob.dup();
    list_set_oob.invokespecial(al_init);
    list_set_oob.iconst(0);
    list_set_oob.aconst_null();
    list_set_oob.invokevirtual(al_set);
    list_set_oob.areturn();
    method(
        &mut builder,
        "listSetOutOfBounds",
        "()Ljava/lang/Object;",
        3,
        0,
        list_set_oob,
    );

    // static Object listRemoveAtOutOfBounds() { return new ArrayList().remove(0); }
    let mut list_remove_oob = Assembler::new();
    list_remove_oob.new_instance(al_class);
    list_remove_oob.dup();
    list_remove_oob.invokespecial(al_init);
    list_remove_oob.iconst(0);
    list_remove_oob.invokevirtual(al_remove_at);
    list_remove_oob.areturn();
    method(
        &mut builder,
        "listRemoveAtOutOfBounds",
        "()Ljava/lang/Object;",
        2,
        0,
        list_remove_oob,
    );

    // static Object listAddAtOutOfBounds() { new ArrayList().add(5, null); return null; }
    let mut list_add_oob = Assembler::new();
    list_add_oob.new_instance(al_class);
    list_add_oob.dup();
    list_add_oob.invokespecial(al_init);
    list_add_oob.iconst(5);
    list_add_oob.aconst_null();
    list_add_oob.invokevirtual(al_add_at);
    list_add_oob.aconst_null();
    list_add_oob.areturn();
    method(
        &mut builder,
        "listAddAtOutOfBounds",
        "()Ljava/lang/Object;",
        3,
        0,
        list_add_oob,
    );

    // static char builderCharAtPastEnd() {
    //     StringBuilder sb = new StringBuilder().append("abc");
    //     return sb.charAt(5);
    // }
    let mut char_at_past_end = Assembler::new();
    new_builder(&mut char_at_past_end, sb_class, sb_init, 0);
    append_kept(
        &mut char_at_past_end,
        0,
        |asm| asm.ldc(abc),
        sb_append_string,
    );
    char_at_past_end.aload(0);
    char_at_past_end.iconst(5);
    char_at_past_end.invokevirtual(sb_char_at);
    char_at_past_end.ireturn();
    method(
        &mut builder,
        "builderCharAtPastEnd",
        "()C",
        2,
        1,
        char_at_past_end,
    );

    // static Object builderSetLengthNegative() {
    //     StringBuilder sb = new StringBuilder().append("abc");
    //     sb.setLength(-1);
    //     return null;
    // }
    let mut set_length_negative = Assembler::new();
    new_builder(&mut set_length_negative, sb_class, sb_init, 0);
    append_kept(
        &mut set_length_negative,
        0,
        |asm| asm.ldc(abc),
        sb_append_string,
    );
    set_length_negative.aload(0);
    set_length_negative.iconst(-1);
    set_length_negative.invokevirtual(sb_set_length);
    set_length_negative.aconst_null();
    set_length_negative.areturn();
    method(
        &mut builder,
        "builderSetLengthNegative",
        "()Ljava/lang/Object;",
        2,
        1,
        set_length_negative,
    );

    // static String builderSurrogateSetLength() {
    //     StringBuilder sb = new StringBuilder().append("😀");
    //     sb.setLength(1);
    //     return sb.toString();
    // }
    let mut surrogate_set_length = Assembler::new();
    new_builder(&mut surrogate_set_length, sb_class, sb_init, 0);
    append_kept(
        &mut surrogate_set_length,
        0,
        |asm| asm.ldc(grinning),
        sb_append_string,
    );
    surrogate_set_length.aload(0);
    surrogate_set_length.iconst(1);
    surrogate_set_length.invokevirtual(sb_set_length);
    surrogate_set_length.aload(0);
    surrogate_set_length.invokevirtual(sb_to_string);
    surrogate_set_length.areturn();
    method(
        &mut builder,
        "builderSurrogateSetLength",
        "()Ljava/lang/String;",
        2,
        1,
        surrogate_set_length,
    );

    // static String builderSurrogateDelete() {
    //     StringBuilder sb = new StringBuilder().append("😀");
    //     sb.deleteCharAt(0);
    //     return sb.toString();
    // }
    let mut surrogate_delete = Assembler::new();
    new_builder(&mut surrogate_delete, sb_class, sb_init, 0);
    append_kept(
        &mut surrogate_delete,
        0,
        |asm| asm.ldc(grinning),
        sb_append_string,
    );
    surrogate_delete.aload(0);
    surrogate_delete.iconst(0);
    surrogate_delete.invokevirtual(sb_delete_char_at);
    surrogate_delete.astore(0);
    surrogate_delete.aload(0);
    surrogate_delete.invokevirtual(sb_to_string);
    surrogate_delete.areturn();
    method(
        &mut builder,
        "builderSurrogateDelete",
        "()Ljava/lang/String;",
        2,
        1,
        surrogate_delete,
    );

    // static String reverseSurrogate() { return new StringBuilder().append("a😀b")
    //     .reverse().toString(); }
    let mut reverse_surrogate = Assembler::new();
    new_builder(&mut reverse_surrogate, sb_class, sb_init, 1);
    append_kept(
        &mut reverse_surrogate,
        1,
        |asm| asm.ldc(a_grinning_b),
        sb_append_string,
    );
    reverse_surrogate.aload(1);
    reverse_surrogate.invokevirtual(sb_reverse);
    reverse_surrogate.astore(1);
    reverse_surrogate.aload(1);
    reverse_surrogate.invokevirtual(sb_to_string);
    reverse_surrogate.areturn();
    method(
        &mut builder,
        "reverseSurrogate",
        "()Ljava/lang/String;",
        2,
        2,
        reverse_surrogate,
    );

    // static boolean appendReturnsReceiver() {
    //     StringBuilder sb = new StringBuilder();
    //     return Object.equals(sb.append("missing"), sb);
    // }
    let mut append_returns_receiver = Assembler::new();
    new_builder(&mut append_returns_receiver, sb_class, sb_init, 0);
    append_returns_receiver.aload(0);
    append_returns_receiver.ldc(missing);
    append_returns_receiver.invokevirtual(sb_append_string);
    append_returns_receiver.aload(0);
    append_returns_receiver.invokespecial(object_equals);
    append_returns_receiver.ireturn();
    method(
        &mut builder,
        "appendReturnsReceiver",
        "()Z",
        2,
        1,
        append_returns_receiver,
    );

    // static Object makeList(String a) {
    //     ArrayList list = new ArrayList();
    //     list.add(a);
    //     return list;
    // }
    let mut make_list = Assembler::new();
    make_list.new_instance(al_class);
    make_list.dup();
    make_list.invokespecial(al_init);
    make_list.astore(1);
    make_list.aload(1);
    make_list.aload(0);
    make_list.invokevirtual(al_add);
    make_list.istore(2);
    make_list.aload(1);
    make_list.areturn();
    method(
        &mut builder,
        "makeList",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        2,
        3,
        make_list,
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
    instance::register_natives(&shared);
    object::register_natives(&shared);
    string::register_natives(&shared);
    reflection::register_natives(&shared);
    collections::register_natives(&shared);
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "CollectionsProbe", &probe_bytes())
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

fn string_bounds(error: Result<Option<Value>, InterpError>) -> (i32, usize) {
    match error {
        Err(InterpError::Vm(VmError::StringIndexOutOfBounds { index, length })) => (index, length),
        other => panic!("expected StringIndexOutOfBounds, found {other:?}"),
    }
}

fn list_bounds(error: Result<Option<Value>, InterpError>) -> (&'static str, i32, usize) {
    match error {
        Err(InterpError::Vm(VmError::CollectionIndexOutOfBounds {
            operation,
            index,
            length,
        })) => (operation, index, length),
        other => panic!("expected CollectionIndexOutOfBounds, found {other:?}"),
    }
}

#[test]
fn string_builder_appends_and_renders_text() {
    let fixture = fixture();

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "build",
                "(Ljava/lang/String;I)Ljava/lang/String;",
                vec![heap_string(&fixture, "ab"), Value::Int(7)]
            )
        ),
        "ab7"
    );

    // A null String and a null Object append the JDK's "null"; the String constructor does
    // the same because it appends its argument.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "builderAppendNullString",
                "()Ljava/lang/String;",
                vec![]
            )
        ),
        "null"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "builderAppendNullObject",
                "()Ljava/lang/String;",
                vec![]
            )
        ),
        "null"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "builderNullConstructor",
                "()Ljava/lang/String;",
                vec![]
            )
        ),
        "null"
    );

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "appendChar",
                "(I)Ljava/lang/String;",
                vec![Value::Int(65)]
            )
        ),
        "A"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "appendBoolean", "()Ljava/lang/String;", vec![])
        ),
        "true"
    );

    // append(Object) uses the VM's String.valueOf-shaped conversion: a String contributes
    // its content and a boxed Integer its decimal value.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "appendObject",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![heap_string(&fixture, "obj")]
            )
        ),
        "obj"
    );
    let boxed = Value::Ref(fixture.shared.box_int(42));
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "appendObject",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![boxed]
            )
        ),
        "42"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "appendObject",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![Value::Ref(ObjRef::NULL)]
            )
        ),
        "null"
    );

    // append(CharSequence) accepts the two character sequences the library models.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "appendCharSequence",
                "(Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "cs")]
            )
        ),
        "cs"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "appendCharSequence",
                "(Ljava/lang/String;)Ljava/lang/String;",
                vec![Value::Ref(ObjRef::NULL)]
            )
        ),
        "null"
    );

    assert_eq!(
        int_value(ok(&fixture, "builderCapacityLength", "()I", vec![])),
        0
    );

    // Every append returns the same builder object, not a copy.
    assert!(bool_value(ok(
        &fixture,
        "appendReturnsReceiver",
        "()Z",
        vec![]
    )));
}

#[test]
fn string_builder_primitive_overloads_use_the_registry() {
    let fixture = fixture();

    // The decoded instruction subset has no `lload`/`fload`/`dload` and no `ldc2_w`, so
    // these three overloads are exercised by calling the registered natives with the
    // `Value` the interpreter would have carried.
    let cases: [(&str, Value, &str); 5] = [
        (
            "(J)Ljava/lang/StringBuilder;",
            Value::Long(9_000_000_000),
            "9000000000",
        ),
        ("(F)Ljava/lang/StringBuilder;", Value::Float(1.5), "1.5"),
        ("(F)Ljava/lang/StringBuilder;", Value::Float(1.0), "1.0"),
        ("(D)Ljava/lang/StringBuilder;", Value::Double(2.25), "2.25"),
        ("(D)Ljava/lang/StringBuilder;", Value::Double(42.0), "42.0"),
    ];
    for (descriptor, value, expected) in cases {
        let builder = fixture.shared.heap().allocate_string_builder();
        native(
            &fixture,
            "java/lang/StringBuilder",
            "append",
            descriptor,
            builder,
            &[value],
        )
        .unwrap_or_else(|error| panic!("append{descriptor} failed: {error}"));
        let result = native(
            &fixture,
            "java/lang/StringBuilder",
            "toString",
            "()Ljava/lang/String;",
            builder,
            &[],
        )
        .unwrap()
        .expect("toString returns a value");
        assert_eq!(string_value(&fixture.shared, Some(result)), expected);
    }
}

#[test]
fn string_builder_measures_and_mutates_utf16_units() {
    let fixture = fixture();

    // "a😀b" is four UTF-16 units; 'b' is at index 3.
    assert_eq!(
        int_value(ok(
            &fixture,
            "builderLength",
            "(Ljava/lang/String;)I",
            vec![heap_string(&fixture, "a😀b")]
        )),
        4
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "builderCharAt",
            "(Ljava/lang/String;I)C",
            vec![heap_string(&fixture, "a😀b"), Value::Int(0)]
        )),
        i32::from(b'a')
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "builderCharAt",
            "(Ljava/lang/String;I)C",
            vec![heap_string(&fixture, "a😀b"), Value::Int(1)]
        )),
        GRINNING_HIGH
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "builderCharAt",
            "(Ljava/lang/String;I)C",
            vec![heap_string(&fixture, "a😀b"), Value::Int(2)]
        )),
        GRINNING_LOW
    );

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "reverseOf",
                "(Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "abc")]
            )
        ),
        "cba"
    );
    // JDK reverse keeps a supplementary character intact instead of reversing its pair.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "reverseSurrogate", "()Ljava/lang/String;", vec![])
        ),
        "b😀a"
    );

    // setLength grows with NUL code units and shrinks in UTF-16 units.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "builderSetLength",
                "(Ljava/lang/String;I)Ljava/lang/String;",
                vec![heap_string(&fixture, "ab"), Value::Int(4)]
            )
        ),
        "ab\0\0"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "builderSetLength",
                "(Ljava/lang/String;I)Ljava/lang/String;",
                vec![heap_string(&fixture, "abcd"), Value::Int(2)]
            )
        ),
        "ab"
    );

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "builderDeleteCharAt",
                "(Ljava/lang/String;I)Ljava/lang/String;",
                vec![heap_string(&fixture, "abc"), Value::Int(1)]
            )
        ),
        "ac"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "builderInsert",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "ac"), heap_string(&fixture, "x")]
            )
        ),
        "axc"
    );
}

#[test]
fn string_builder_bounds_and_surrogate_splits_are_typed_errors() {
    let fixture = fixture();

    assert_eq!(
        string_bounds(call(&fixture, "builderCharAtPastEnd", "()C", vec![])),
        (5, 3)
    );
    assert_eq!(
        string_bounds(call(
            &fixture,
            "builderSetLengthNegative",
            "()Ljava/lang/Object;",
            vec![]
        )),
        (-1, 3)
    );

    // Splitting the surrogate pair with setLength or deleteCharAt would need a lone
    // surrogate, which the UTF-8 text cannot represent.
    assert!(matches!(
        call(
            &fixture,
            "builderSurrogateSetLength",
            "()Ljava/lang/String;",
            vec![]
        ),
        Err(InterpError::Vm(VmError::UnpairedSurrogate {
            method: "StringBuilder.setLength"
        }))
    ));
    assert!(matches!(
        call(
            &fixture,
            "builderSurrogateDelete",
            "()Ljava/lang/String;",
            vec![]
        ),
        Err(InterpError::Vm(VmError::UnpairedSurrogate {
            method: "StringBuilder.deleteCharAt"
        }))
    ));

    // A lone surrogate passed as a char is the same typed error.
    assert!(matches!(
        call(
            &fixture,
            "appendChar",
            "(I)Ljava/lang/String;",
            vec![Value::Int(GRINNING_HIGH)]
        ),
        Err(InterpError::Vm(VmError::UnpairedSurrogate {
            method: "StringBuilder.append"
        }))
    ));

    // append(Object) does not dispatch an overridden toString; a list gets the VM's
    // Object.toString shape. The slice boundary documents this limitation.
    let list = ref_value(ok(
        &fixture,
        "makeList",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        vec![heap_string(&fixture, "x")],
    ));
    let text = string_value(
        &fixture.shared,
        ok(
            &fixture,
            "appendObject",
            "(Ljava/lang/Object;)Ljava/lang/String;",
            vec![Value::Ref(list)],
        ),
    );
    assert_eq!(
        text,
        format!("java.util.ArrayList@{:x}", list.identity_hash() as u32)
    );
}

#[test]
fn array_list_adds_gets_and_reports_size() {
    let fixture = fixture();

    assert_eq!(
        int_value(ok(
            &fixture,
            "listOps",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "a"), heap_string(&fixture, "b")]
        )),
        2
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "listGet",
                "(Ljava/lang/String;Ljava/lang/String;I)Ljava/lang/String;",
                vec![
                    heap_string(&fixture, "a"),
                    heap_string(&fixture, "b"),
                    Value::Int(0)
                ]
            )
        ),
        "a"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "listGet",
                "(Ljava/lang/String;Ljava/lang/String;I)Ljava/lang/String;",
                vec![
                    heap_string(&fixture, "a"),
                    heap_string(&fixture, "b"),
                    Value::Int(1)
                ]
            )
        ),
        "b"
    );

    // set returns the previous element; add(int, E) inserts at the index.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "listSet",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "a"), heap_string(&fixture, "b")]
            )
        ),
        "b"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "listAddAt",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "a"), heap_string(&fixture, "b")]
            )
        ),
        "b"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "listRemoveAt",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "a"), heap_string(&fixture, "b")]
            )
        ),
        "b"
    );

    assert_eq!(
        int_value(ok(&fixture, "listCapacitySize", "()I", vec![])),
        0
    );
    assert!(bool_value(ok(&fixture, "listIsEmpty", "()Z", vec![])));
    assert!(bool_value(ok(
        &fixture,
        "listClearIsEmpty",
        "(Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "a")]
    )));
}

#[test]
fn array_list_null_elements_are_values() {
    let fixture = fixture();

    assert_eq!(int_value(ok(&fixture, "listNullOps", "()I", vec![])), 1);
    assert_eq!(
        ref_value(ok(&fixture, "listGetNull", "()Ljava/lang/Object;", vec![])),
        ObjRef::NULL
    );
}

#[test]
fn array_list_contains_and_remove_use_reference_identity() {
    let fixture = fixture();

    // The same handle is found.
    let same = heap_string(&fixture, "x");
    assert!(bool_value(ok(
        &fixture,
        "listContainsEqual",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![same, same]
    )));

    // Two distinct strings with equal content are not found: reference equality, not
    // Object.equals. This is the documented M2.12 limitation.
    assert!(!bool_value(ok(
        &fixture,
        "listContainsEqual",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "x"), heap_string(&fixture, "x")]
    )));

    assert!(bool_value(ok(
        &fixture,
        "listRemoveObject",
        "(Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "a")]
    )));
    assert_eq!(
        int_value(ok(
            &fixture,
            "listIndexOf",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "a"), heap_string(&fixture, "b")]
        )),
        1
    );
}

#[test]
fn array_list_bounds_are_typed_errors() {
    let fixture = fixture();

    assert_eq!(
        list_bounds(call(
            &fixture,
            "listGetOutOfBounds",
            "()Ljava/lang/Object;",
            vec![]
        )),
        ("ArrayList.get", 0, 0)
    );
    assert_eq!(
        list_bounds(call(
            &fixture,
            "listSetOutOfBounds",
            "()Ljava/lang/Object;",
            vec![]
        )),
        ("ArrayList.set", 0, 0)
    );
    assert_eq!(
        list_bounds(call(
            &fixture,
            "listRemoveAtOutOfBounds",
            "()Ljava/lang/Object;",
            vec![]
        )),
        ("ArrayList.remove", 0, 0)
    );
    assert_eq!(
        list_bounds(call(
            &fixture,
            "listAddAtOutOfBounds",
            "()Ljava/lang/Object;",
            vec![]
        )),
        ("ArrayList.add", 5, 0)
    );
}

#[test]
fn get_class_reports_the_vm_class_of_builders_and_lists() {
    let fixture = fixture();

    // The bytecode path: `invokevirtual Object.getClass` on a builder resolves through
    // the class library to the Object native, and getClass reports the builder class.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "builderClass", "()Ljava/lang/String;", vec![])
        ),
        "java.lang.StringBuilder"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "listClass", "()Ljava/lang/String;", vec![])
        ),
        "java.util.ArrayList"
    );

    // The shared state sees the same identities for objects allocated directly.
    let builder = fixture.shared.heap().allocate_string_builder();
    let class = fixture.shared.get_class(builder).unwrap();
    let id = fixture.shared.heap().class(class).unwrap();
    assert_eq!(
        fixture.shared.entry(id).unwrap().name,
        "java/lang/StringBuilder"
    );

    let list = fixture.shared.heap().allocate_list();
    let class = fixture.shared.get_class(list).unwrap();
    let id = fixture.shared.heap().class(class).unwrap();
    assert_eq!(
        fixture.shared.entry(id).unwrap().name,
        "java/util/ArrayList"
    );
}

#[test]
fn heap_lists_and_builders_have_dedicated_representations() {
    let fixture = fixture();

    let builder = fixture.shared.heap().allocate_string_builder();
    assert!(matches!(
        fixture.shared.heap().get(builder).unwrap(),
        HeapObject::StringBuilder(text) if text.is_empty()
    ));
    fixture
        .shared
        .heap()
        .string_builder_mut(builder)
        .unwrap()
        .push_str("hi");
    assert_eq!(fixture.shared.heap().string_builder(builder).unwrap(), "hi");

    let list = fixture.shared.heap().allocate_list();
    assert!(matches!(
        fixture.shared.heap().get(list).unwrap(),
        HeapObject::List(elements) if elements.is_empty()
    ));
    assert!(fixture.shared.heap().string_builder(list).is_err());
    assert!(fixture.shared.heap().list(builder).is_err());

    // `new` and reflective construction go through `Shared::allocate_instance`, which
    // selects the representation from the class identity.
    let sb_class = fixture
        .shared
        .load_library_class("java/lang/StringBuilder")
        .unwrap();
    let allocated = fixture.shared.allocate_instance(sb_class).unwrap();
    assert!(matches!(
        fixture.shared.heap().get(allocated).unwrap(),
        HeapObject::StringBuilder(_)
    ));
    let al_class = fixture
        .shared
        .load_library_class("java/util/ArrayList")
        .unwrap();
    let allocated = fixture.shared.allocate_instance(al_class).unwrap();
    assert!(matches!(
        fixture.shared.heap().get(allocated).unwrap(),
        HeapObject::List(_)
    ));
}

#[test]
fn list_elements_are_enumerated_as_roots() {
    let fixture = fixture();

    // The fixture frame is gone once the call returns, so the element is reachable only
    // through the list object; root enumeration must still report it.
    let element = heap_string(&fixture, "rooted");
    let element_reference = ref_value(Some(element));
    let list = ref_value(ok(
        &fixture,
        "makeList",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        vec![element],
    ));
    let roots = fixture.shared.roots(&[]);
    assert!(
        roots.contains(&element_reference),
        "a reference stored in a heap list is a root: {roots:?}"
    );

    // A list allocated directly behaves the same.
    let second = fixture.shared.heap().allocate_string("second");
    let direct = fixture.shared.heap().allocate_list();
    fixture
        .shared
        .heap()
        .list_mut(direct)
        .unwrap()
        .push(Value::Ref(second));
    let roots = fixture.shared.roots(&[]);
    assert!(roots.contains(&second));

    // The list is still a live heap object; its element is what root enumeration adds.
    assert_eq!(
        fixture.shared.heap().list(list).unwrap().len(),
        1,
        "the fixture list still holds its one element"
    );
}

#[test]
fn the_c1_stubs_have_no_code_and_the_collection_natives_are_registered() {
    let fixture = fixture();

    let entries: [(&str, &str, &str); 32] = [
        ("java/lang/StringBuilder", "<init>", "()V"),
        ("java/lang/StringBuilder", "<init>", "(I)V"),
        ("java/lang/StringBuilder", "<init>", "(Ljava/lang/String;)V"),
        (
            "java/lang/StringBuilder",
            "append",
            "(Ljava/lang/String;)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(Ljava/lang/Object;)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(I)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(J)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(C)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(Z)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(F)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(D)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "append",
            "(Ljava/lang/CharSequence;)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "toString",
            "()Ljava/lang/String;",
        ),
        ("java/lang/StringBuilder", "length", "()I"),
        ("java/lang/StringBuilder", "charAt", "(I)C"),
        ("java/lang/StringBuilder", "setLength", "(I)V"),
        (
            "java/lang/StringBuilder",
            "reverse",
            "()Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "deleteCharAt",
            "(I)Ljava/lang/StringBuilder;",
        ),
        (
            "java/lang/StringBuilder",
            "insert",
            "(ILjava/lang/String;)Ljava/lang/StringBuilder;",
        ),
        ("java/util/ArrayList", "<init>", "()V"),
        ("java/util/ArrayList", "<init>", "(I)V"),
        ("java/util/ArrayList", "size", "()I"),
        ("java/util/ArrayList", "isEmpty", "()Z"),
        ("java/util/ArrayList", "add", "(Ljava/lang/Object;)Z"),
        ("java/util/ArrayList", "add", "(ILjava/lang/Object;)V"),
        ("java/util/ArrayList", "get", "(I)Ljava/lang/Object;"),
        (
            "java/util/ArrayList",
            "set",
            "(ILjava/lang/Object;)Ljava/lang/Object;",
        ),
        ("java/util/ArrayList", "clear", "()V"),
        ("java/util/ArrayList", "remove", "(I)Ljava/lang/Object;"),
        ("java/util/ArrayList", "remove", "(Ljava/lang/Object;)Z"),
        ("java/util/ArrayList", "contains", "(Ljava/lang/Object;)Z"),
        ("java/util/ArrayList", "indexOf", "(Ljava/lang/Object;)I"),
    ];
    for (class_name, name, descriptor) in entries {
        let class = fixture
            .shared
            .load_library_class(class_name)
            .expect("the stub loads");
        let entry = fixture.shared.entry(class).expect("the stub is loaded");
        let method = entry
            .file
            .find_method(name, descriptor)
            .unwrap()
            .unwrap_or_else(|| panic!("{class_name} declares {name}{descriptor}"));
        assert!(
            entry.file.code(method).unwrap().is_none(),
            "{class_name}.{name}{descriptor} has no Code"
        );
        assert!(
            fixture
                .shared
                .native(class_name, name, descriptor)
                .is_some(),
            "{class_name}.{name}{descriptor} is registered"
        );
    }
}
