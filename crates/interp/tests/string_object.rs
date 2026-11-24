//! M2.9 acceptance: core `Object` and `String` behavior on the C1 library.
//!
//! `StringProbe` is assembled against the committed C1 bootstrap library. Its `ldc`
//! helpers and argument-taking helpers call the real `java.lang.String` stub methods
//! through `invokevirtual`, and its object helpers call `Object.equals`, `hashCode` and
//! `toString`. Because a string dispatches from `java/lang/String` and an ordinary
//! instance dispatches from its own class, the tests exercise the native lookup for both
//! receiver shapes rather than calling a Rust function directly.
//!
//! Supplementary characters are written as surrogate pairs in the class-file constants:
//! UTF-16 lengths, indices and `charAt` values are asserted against those units, not
//! against Rust `char` counts.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ArrayKind, ClassId, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, instance, object,
    string,
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

fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("StringProbe", "java/lang/Object", 61);

    // String constants first, so every `ldc` index stays inside its one-byte form and is
    // independent of how many member references the rest of the class adds.
    let hello = builder.string("hello");
    let empty = builder.string("");
    let abc = builder.string("abc");
    let same = builder.string("same");
    let abd = builder.string("abd");
    let ab = builder.string("ab");
    let cd = builder.string("cd");
    let abcdef = builder.string("abcdef");
    let he = builder.string("he");
    let llo = builder.string("llo");
    let abcabc = builder.string("abcabc");
    let bc = builder.string("bc");
    let banana = builder.string("banana");
    let grinning = builder.string("😀");
    let grinning_x = builder.string("😀x");
    let a_grinning_b = builder.string("a😀b");
    let b = builder.string("b");
    let x = builder.string("x");
    let interned = builder.string("interned");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let object_equals = builder.method_ref("java/lang/Object", "equals", "(Ljava/lang/Object;)Z");
    let object_hash_code = builder.method_ref("java/lang/Object", "hashCode", "()I");
    let object_to_string =
        builder.method_ref("java/lang/Object", "toString", "()Ljava/lang/String;");
    let probe_class = builder.class("StringProbe");
    let probe_init = builder.method_ref("StringProbe", "<init>", "()V");

    let length = builder.method_ref("java/lang/String", "length", "()I");
    let is_empty = builder.method_ref("java/lang/String", "isEmpty", "()Z");
    let char_at = builder.method_ref("java/lang/String", "charAt", "(I)C");
    let equals = builder.method_ref("java/lang/String", "equals", "(Ljava/lang/Object;)Z");
    let hash_code = builder.method_ref("java/lang/String", "hashCode", "()I");
    let to_string = builder.method_ref("java/lang/String", "toString", "()Ljava/lang/String;");
    let compare_to = builder.method_ref("java/lang/String", "compareTo", "(Ljava/lang/String;)I");
    let concat = builder.method_ref(
        "java/lang/String",
        "concat",
        "(Ljava/lang/String;)Ljava/lang/String;",
    );
    let substring_from =
        builder.method_ref("java/lang/String", "substring", "(I)Ljava/lang/String;");
    let substring_range =
        builder.method_ref("java/lang/String", "substring", "(II)Ljava/lang/String;");
    let starts_with = builder.method_ref("java/lang/String", "startsWith", "(Ljava/lang/String;)Z");
    let ends_with = builder.method_ref("java/lang/String", "endsWith", "(Ljava/lang/String;)Z");
    let index_of = builder.method_ref("java/lang/String", "indexOf", "(Ljava/lang/String;)I");
    let replace = builder.method_ref("java/lang/String", "replace", "(CC)Ljava/lang/String;");
    let to_char_array = builder.method_ref("java/lang/String", "toCharArray", "()[C");
    let intern = builder.method_ref("java/lang/String", "intern", "()Ljava/lang/String;");

    // StringProbe() { super(); }
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

    // static int length(String s) { return s.length(); }
    let mut length_body = Assembler::new();
    length_body.aload(0);
    length_body.invokevirtual(length);
    length_body.ireturn();
    method(
        &mut builder,
        "length",
        "(Ljava/lang/String;)I",
        1,
        1,
        length_body,
    );

    // static boolean isEmpty(String s) { return s.isEmpty(); }
    let mut empty_body = Assembler::new();
    empty_body.aload(0);
    empty_body.invokevirtual(is_empty);
    empty_body.ireturn();
    method(
        &mut builder,
        "isEmpty",
        "(Ljava/lang/String;)Z",
        1,
        1,
        empty_body,
    );

    // static char charAt(String s, int index) { return s.charAt(index); }
    let mut char_at_body = Assembler::new();
    char_at_body.aload(0);
    char_at_body.iload(1);
    char_at_body.invokevirtual(char_at);
    char_at_body.ireturn();
    method(
        &mut builder,
        "charAt",
        "(Ljava/lang/String;I)C",
        2,
        2,
        char_at_body,
    );

    // static boolean equals(String a, String b) { return a.equals(b); }
    let mut equals_body = Assembler::new();
    equals_body.aload(0);
    equals_body.aload(1);
    equals_body.invokevirtual(equals);
    equals_body.ireturn();
    method(
        &mut builder,
        "equals",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        2,
        2,
        equals_body,
    );

    // static boolean equalsObject(String a, Object b) { return a.equals(b); }
    let mut equals_object = Assembler::new();
    equals_object.aload(0);
    equals_object.aload(1);
    equals_object.invokevirtual(equals);
    equals_object.ireturn();
    method(
        &mut builder,
        "equalsObject",
        "(Ljava/lang/String;Ljava/lang/Object;)Z",
        2,
        2,
        equals_object,
    );

    // static int hash(String s) { return s.hashCode(); }
    let mut hash_body = Assembler::new();
    hash_body.aload(0);
    hash_body.invokevirtual(hash_code);
    hash_body.ireturn();
    method(
        &mut builder,
        "hash",
        "(Ljava/lang/String;)I",
        1,
        1,
        hash_body,
    );

    // static String toStringOf(String s) { return s.toString(); }
    let mut to_string_body = Assembler::new();
    to_string_body.aload(0);
    to_string_body.invokevirtual(to_string);
    to_string_body.areturn();
    method(
        &mut builder,
        "toStringOf",
        "(Ljava/lang/String;)Ljava/lang/String;",
        1,
        1,
        to_string_body,
    );

    // static int compare(String a, String b) { return a.compareTo(b); }
    let mut compare_body = Assembler::new();
    compare_body.aload(0);
    compare_body.aload(1);
    compare_body.invokevirtual(compare_to);
    compare_body.ireturn();
    method(
        &mut builder,
        "compare",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        2,
        2,
        compare_body,
    );

    // static String concat(String a, String b) { return a.concat(b); }
    let mut concat_body = Assembler::new();
    concat_body.aload(0);
    concat_body.aload(1);
    concat_body.invokevirtual(concat);
    concat_body.areturn();
    method(
        &mut builder,
        "concat",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        2,
        2,
        concat_body,
    );

    // static String substringFrom(String s, int begin) { return s.substring(begin); }
    let mut substring_from_body = Assembler::new();
    substring_from_body.aload(0);
    substring_from_body.iload(1);
    substring_from_body.invokevirtual(substring_from);
    substring_from_body.areturn();
    method(
        &mut builder,
        "substringFrom",
        "(Ljava/lang/String;I)Ljava/lang/String;",
        2,
        2,
        substring_from_body,
    );

    // static String substringRange(String s, int begin, int end) { return s.substring(begin, end); }
    let mut substring_range_body = Assembler::new();
    substring_range_body.aload(0);
    substring_range_body.iload(1);
    substring_range_body.iload(2);
    substring_range_body.invokevirtual(substring_range);
    substring_range_body.areturn();
    method(
        &mut builder,
        "substringRange",
        "(Ljava/lang/String;II)Ljava/lang/String;",
        3,
        3,
        substring_range_body,
    );

    // static boolean startsWith(String s, String prefix) { return s.startsWith(prefix); }
    let mut starts_with_body = Assembler::new();
    starts_with_body.aload(0);
    starts_with_body.aload(1);
    starts_with_body.invokevirtual(starts_with);
    starts_with_body.ireturn();
    method(
        &mut builder,
        "startsWith",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        2,
        2,
        starts_with_body,
    );

    // static boolean endsWith(String s, String suffix) { return s.endsWith(suffix); }
    let mut ends_with_body = Assembler::new();
    ends_with_body.aload(0);
    ends_with_body.aload(1);
    ends_with_body.invokevirtual(ends_with);
    ends_with_body.ireturn();
    method(
        &mut builder,
        "endsWith",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        2,
        2,
        ends_with_body,
    );

    // static int indexOf(String s, String needle) { return s.indexOf(needle); }
    let mut index_of_body = Assembler::new();
    index_of_body.aload(0);
    index_of_body.aload(1);
    index_of_body.invokevirtual(index_of);
    index_of_body.ireturn();
    method(
        &mut builder,
        "indexOf",
        "(Ljava/lang/String;Ljava/lang/String;)I",
        2,
        2,
        index_of_body,
    );

    // static String replace(String s, char old, char new) { return s.replace(old, new); }
    let mut replace_body = Assembler::new();
    replace_body.aload(0);
    replace_body.iload(1);
    replace_body.iload(2);
    replace_body.invokevirtual(replace);
    replace_body.areturn();
    method(
        &mut builder,
        "replace",
        "(Ljava/lang/String;CC)Ljava/lang/String;",
        3,
        3,
        replace_body,
    );

    // static char[] toCharArrayOf(String s) { return s.toCharArray(); }
    let mut to_char_array_body = Assembler::new();
    to_char_array_body.aload(0);
    to_char_array_body.invokevirtual(to_char_array);
    to_char_array_body.areturn();
    method(
        &mut builder,
        "toCharArrayOf",
        "(Ljava/lang/String;)[C",
        1,
        1,
        to_char_array_body,
    );

    // static String internOf(String s) { return s.intern(); }
    let mut intern_body = Assembler::new();
    intern_body.aload(0);
    intern_body.invokevirtual(intern);
    intern_body.areturn();
    method(
        &mut builder,
        "internOf",
        "(Ljava/lang/String;)Ljava/lang/String;",
        1,
        1,
        intern_body,
    );

    // static boolean sameReference(Object a, Object b) { ... Object.equals(a, b) ... }
    //
    // `invokespecial` with owner `java/lang/Object` reaches the Object native even when
    // the argument is a string, which is the only way a test can ask for reference
    // identity on a String receiver: `invokevirtual` would pick `String.equals`.
    let mut same_reference = Assembler::new();
    same_reference.aload(0);
    same_reference.aload(1);
    same_reference.invokespecial(object_equals);
    same_reference.ireturn();
    method(
        &mut builder,
        "sameReference",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        2,
        2,
        same_reference,
    );

    // static int identityHash(Object o) { return o.hashCode(); }
    let mut identity_hash = Assembler::new();
    identity_hash.aload(0);
    identity_hash.invokevirtual(object_hash_code);
    identity_hash.ireturn();
    method(
        &mut builder,
        "identityHash",
        "(Ljava/lang/Object;)I",
        1,
        1,
        identity_hash,
    );

    // static String objectToString(Object o) { return o.toString(); }
    let mut object_to_string_body = Assembler::new();
    object_to_string_body.aload(0);
    object_to_string_body.invokevirtual(object_to_string);
    object_to_string_body.areturn();
    method(
        &mut builder,
        "objectToString",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        1,
        1,
        object_to_string_body,
    );

    // static boolean identityEquals(Object a, Object b) { return a.equals(b); }
    let mut identity_equals = Assembler::new();
    identity_equals.aload(0);
    identity_equals.aload(1);
    identity_equals.invokevirtual(object_equals);
    identity_equals.ireturn();
    method(
        &mut builder,
        "identityEquals",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        2,
        2,
        identity_equals,
    );

    // static Object makeObject() { return new Object(); }
    let mut make_object = Assembler::new();
    make_object.new_instance(builder.class("java/lang/Object"));
    make_object.dup();
    make_object.invokespecial(object_init);
    make_object.areturn();
    method(
        &mut builder,
        "makeObject",
        "()Ljava/lang/Object;",
        2,
        0,
        make_object,
    );

    // static StringProbe makeProbe() { return new StringProbe(); }
    let mut make_probe = Assembler::new();
    make_probe.new_instance(probe_class);
    make_probe.dup();
    make_probe.invokespecial(probe_init);
    make_probe.areturn();
    method(
        &mut builder,
        "makeProbe",
        "()LStringProbe;",
        2,
        0,
        make_probe,
    );

    // Literal helpers: each one `ldc`s its own string constant. String constants are
    // interned, so two `ldc`s of the same text are the same object; content equality over
    // distinct objects is exercised with separately allocated heap strings in the tests.

    // static int literalLength() { return "hello".length(); }
    let mut literal_length = Assembler::new();
    literal_length.ldc(hello);
    literal_length.invokevirtual(length);
    literal_length.ireturn();
    method(&mut builder, "literalLength", "()I", 1, 0, literal_length);

    // static boolean literalEmpty() { return "".isEmpty(); }
    let mut literal_empty = Assembler::new();
    literal_empty.ldc(empty);
    literal_empty.invokevirtual(is_empty);
    literal_empty.ireturn();
    method(&mut builder, "literalEmpty", "()Z", 1, 0, literal_empty);

    // static char literalCharAt(int index) { return "abc".charAt(index); }
    let mut literal_char_at = Assembler::new();
    literal_char_at.ldc(abc);
    literal_char_at.iload(0);
    literal_char_at.invokevirtual(char_at);
    literal_char_at.ireturn();
    method(&mut builder, "literalCharAt", "(I)C", 2, 1, literal_char_at);

    // static boolean literalEquals() { return "same".equals("same"); }
    let mut literal_equals = Assembler::new();
    literal_equals.ldc(same);
    literal_equals.ldc(same);
    literal_equals.invokevirtual(equals);
    literal_equals.ireturn();
    method(&mut builder, "literalEquals", "()Z", 2, 0, literal_equals);

    // static int literalHash() { return "hello".hashCode(); }
    let mut literal_hash = Assembler::new();
    literal_hash.ldc(hello);
    literal_hash.invokevirtual(hash_code);
    literal_hash.ireturn();
    method(&mut builder, "literalHash", "()I", 1, 0, literal_hash);

    // static int literalCompare() { return "abc".compareTo("abd"); }
    let mut literal_compare = Assembler::new();
    literal_compare.ldc(abc);
    literal_compare.ldc(abd);
    literal_compare.invokevirtual(compare_to);
    literal_compare.ireturn();
    method(&mut builder, "literalCompare", "()I", 2, 0, literal_compare);

    // static String literalConcat() { return "ab".concat("cd"); }
    let mut literal_concat = Assembler::new();
    literal_concat.ldc(ab);
    literal_concat.ldc(cd);
    literal_concat.invokevirtual(concat);
    literal_concat.areturn();
    method(
        &mut builder,
        "literalConcat",
        "()Ljava/lang/String;",
        2,
        0,
        literal_concat,
    );

    // static String literalSubstring() { return "abcdef".substring(2, 4); }
    let mut literal_substring = Assembler::new();
    literal_substring.ldc(abcdef);
    literal_substring.iconst(2);
    literal_substring.iconst(4);
    literal_substring.invokevirtual(substring_range);
    literal_substring.areturn();
    method(
        &mut builder,
        "literalSubstring",
        "()Ljava/lang/String;",
        3,
        0,
        literal_substring,
    );

    // static boolean literalStartsWith() { return "hello".startsWith("he"); }
    let mut literal_starts = Assembler::new();
    literal_starts.ldc(hello);
    literal_starts.ldc(he);
    literal_starts.invokevirtual(starts_with);
    literal_starts.ireturn();
    method(
        &mut builder,
        "literalStartsWith",
        "()Z",
        2,
        0,
        literal_starts,
    );

    // static boolean literalEndsWith() { return "hello".endsWith("llo"); }
    let mut literal_ends = Assembler::new();
    literal_ends.ldc(hello);
    literal_ends.ldc(llo);
    literal_ends.invokevirtual(ends_with);
    literal_ends.ireturn();
    method(&mut builder, "literalEndsWith", "()Z", 2, 0, literal_ends);

    // static int literalIndexOf() { return "abcabc".indexOf("bc"); }
    let mut literal_index_of = Assembler::new();
    literal_index_of.ldc(abcabc);
    literal_index_of.ldc(bc);
    literal_index_of.invokevirtual(index_of);
    literal_index_of.ireturn();
    method(
        &mut builder,
        "literalIndexOf",
        "()I",
        2,
        0,
        literal_index_of,
    );

    // static String literalReplace() { return "banana".replace('a', 'o'); }
    let mut literal_replace = Assembler::new();
    literal_replace.ldc(banana);
    literal_replace.iconst(97);
    literal_replace.iconst(111);
    literal_replace.invokevirtual(replace);
    literal_replace.areturn();
    method(
        &mut builder,
        "literalReplace",
        "()Ljava/lang/String;",
        3,
        0,
        literal_replace,
    );

    // static char[] literalChars() { return "😀".toCharArray(); }
    let mut literal_chars = Assembler::new();
    literal_chars.ldc(grinning);
    literal_chars.invokevirtual(to_char_array);
    literal_chars.areturn();
    method(&mut builder, "literalChars", "()[C", 1, 0, literal_chars);

    // static String literalToString() { return "hello".toString(); }
    let mut literal_to_string = Assembler::new();
    literal_to_string.ldc(hello);
    literal_to_string.invokevirtual(to_string);
    literal_to_string.areturn();
    method(
        &mut builder,
        "literalToString",
        "()Ljava/lang/String;",
        1,
        0,
        literal_to_string,
    );

    // static int surrogateLength() { return "😀".length(); }
    let mut surrogate_length = Assembler::new();
    surrogate_length.ldc(grinning);
    surrogate_length.invokevirtual(length);
    surrogate_length.ireturn();
    method(
        &mut builder,
        "surrogateLength",
        "()I",
        1,
        0,
        surrogate_length,
    );

    // static char surrogateCharAt(int index) { return "😀".charAt(index); }
    let mut surrogate_char_at = Assembler::new();
    surrogate_char_at.ldc(grinning);
    surrogate_char_at.iload(0);
    surrogate_char_at.invokevirtual(char_at);
    surrogate_char_at.ireturn();
    method(
        &mut builder,
        "surrogateCharAt",
        "(I)C",
        2,
        1,
        surrogate_char_at,
    );

    // static String surrogateSubstring() { return "😀x".substring(0, 2); }
    let mut surrogate_substring = Assembler::new();
    surrogate_substring.ldc(grinning_x);
    surrogate_substring.iconst(0);
    surrogate_substring.iconst(2);
    surrogate_substring.invokevirtual(substring_range);
    surrogate_substring.areturn();
    method(
        &mut builder,
        "surrogateSubstring",
        "()Ljava/lang/String;",
        3,
        0,
        surrogate_substring,
    );

    // static int surrogateIndexOf() { return "a😀b".indexOf("b"); }
    let mut surrogate_index_of = Assembler::new();
    surrogate_index_of.ldc(a_grinning_b);
    surrogate_index_of.ldc(b);
    surrogate_index_of.invokevirtual(index_of);
    surrogate_index_of.ireturn();
    method(
        &mut builder,
        "surrogateIndexOf",
        "()I",
        2,
        0,
        surrogate_index_of,
    );

    // static String surrogateSplit() { return "😀".substring(0, 1); }
    let mut surrogate_split = Assembler::new();
    surrogate_split.ldc(grinning);
    surrogate_split.iconst(0);
    surrogate_split.iconst(1);
    surrogate_split.invokevirtual(substring_range);
    surrogate_split.areturn();
    method(
        &mut builder,
        "surrogateSplit",
        "()Ljava/lang/String;",
        3,
        0,
        surrogate_split,
    );

    // static String concatNull() { return "x".concat(null); }
    let mut concat_null = Assembler::new();
    concat_null.ldc(x);
    concat_null.aconst_null();
    concat_null.invokevirtual(concat);
    concat_null.areturn();
    method(
        &mut builder,
        "concatNull",
        "()Ljava/lang/String;",
        2,
        0,
        concat_null,
    );

    // static boolean startsWithNull() { return "x".startsWith(null); }
    let mut starts_with_null = Assembler::new();
    starts_with_null.ldc(x);
    starts_with_null.aconst_null();
    starts_with_null.invokevirtual(starts_with);
    starts_with_null.ireturn();
    method(
        &mut builder,
        "startsWithNull",
        "()Z",
        2,
        0,
        starts_with_null,
    );

    // static boolean equalsNull() { return "x".equals(null); }
    let mut equals_null = Assembler::new();
    equals_null.ldc(x);
    equals_null.aconst_null();
    equals_null.invokevirtual(equals);
    equals_null.ireturn();
    method(&mut builder, "equalsNull", "()Z", 2, 0, equals_null);

    // static char charAtPastEnd() { return "abc".charAt(5); }
    let mut char_at_past_end = Assembler::new();
    char_at_past_end.ldc(abc);
    char_at_past_end.iconst(5);
    char_at_past_end.invokevirtual(char_at);
    char_at_past_end.ireturn();
    method(&mut builder, "charAtPastEnd", "()C", 2, 0, char_at_past_end);

    // static char charAtNegative() { return "abc".charAt(-1); }
    let mut char_at_negative = Assembler::new();
    char_at_negative.ldc(abc);
    char_at_negative.iconst(-1);
    char_at_negative.invokevirtual(char_at);
    char_at_negative.ireturn();
    method(
        &mut builder,
        "charAtNegative",
        "()C",
        2,
        0,
        char_at_negative,
    );

    // static String substringPastEnd() { return "abc".substring(1, 5); }
    let mut substring_past_end = Assembler::new();
    substring_past_end.ldc(abc);
    substring_past_end.iconst(1);
    substring_past_end.iconst(5);
    substring_past_end.invokevirtual(substring_range);
    substring_past_end.areturn();
    method(
        &mut builder,
        "substringPastEnd",
        "()Ljava/lang/String;",
        3,
        0,
        substring_past_end,
    );

    // static String substringReversed() { return "abc".substring(2, 1); }
    let mut substring_reversed = Assembler::new();
    substring_reversed.ldc(abc);
    substring_reversed.iconst(2);
    substring_reversed.iconst(1);
    substring_reversed.invokevirtual(substring_range);
    substring_reversed.areturn();
    method(
        &mut builder,
        "substringReversed",
        "()Ljava/lang/String;",
        3,
        0,
        substring_reversed,
    );

    // static boolean internSame() {
    //     String a = "interned".intern();
    //     String b = "interned".intern();
    //     return sameReference(a, b);
    // }
    let mut intern_same = Assembler::new();
    intern_same.ldc(interned);
    intern_same.invokevirtual(intern);
    intern_same.astore(0);
    intern_same.ldc(interned);
    intern_same.invokevirtual(intern);
    intern_same.astore(1);
    intern_same.aload(0);
    intern_same.aload(1);
    intern_same.invokespecial(object_equals);
    intern_same.ireturn();
    method(&mut builder, "internSame", "()Z", 2, 2, intern_same);

    // static boolean fullRangeIdentity() {
    //     String s = "abc";
    //     return sameReference(s.substring(0, 3), s);
    // }
    let mut full_range = Assembler::new();
    full_range.ldc(abc);
    full_range.astore(0);
    full_range.aload(0);
    full_range.iconst(0);
    full_range.iconst(3);
    full_range.invokevirtual(substring_range);
    full_range.aload(0);
    full_range.invokespecial(object_equals);
    full_range.ireturn();
    method(&mut builder, "fullRangeIdentity", "()Z", 3, 1, full_range);

    // static boolean replaceIdentity() {
    //     String s = "abc";
    //     return sameReference(s.replace('z', 'z'), s);
    // }
    let mut replace_identity = Assembler::new();
    replace_identity.ldc(abc);
    replace_identity.astore(0);
    replace_identity.aload(0);
    replace_identity.iconst(122);
    replace_identity.iconst(122);
    replace_identity.invokevirtual(replace);
    replace_identity.aload(0);
    replace_identity.invokespecial(object_equals);
    replace_identity.ireturn();
    method(
        &mut builder,
        "replaceIdentity",
        "()Z",
        3,
        1,
        replace_identity,
    );

    // static boolean toStringIdentity() {
    //     String s = "abc";
    //     return sameReference(s.toString(), s);
    // }
    let mut to_string_identity = Assembler::new();
    to_string_identity.ldc(abc);
    to_string_identity.astore(0);
    to_string_identity.aload(0);
    to_string_identity.invokevirtual(to_string);
    to_string_identity.aload(0);
    to_string_identity.invokespecial(object_equals);
    to_string_identity.ireturn();
    method(
        &mut builder,
        "toStringIdentity",
        "()Z",
        2,
        1,
        to_string_identity,
    );

    // static boolean literalSameReference() { return sameReference("same", "same"); }
    //
    // A string constant is interned, so two `ldc`s of the same text are reference-equal.
    let mut literal_same_reference = Assembler::new();
    literal_same_reference.ldc(same);
    literal_same_reference.ldc(same);
    literal_same_reference.invokespecial(object_equals);
    literal_same_reference.ireturn();
    method(
        &mut builder,
        "literalSameReference",
        "()Z",
        2,
        0,
        literal_same_reference,
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
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "StringProbe", &probe_bytes())
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

/// A `char[]` result as its UTF-16 units, asserting the element kind.
fn char_units(shared: &Shared, value: Option<Value>) -> Vec<i32> {
    let reference = ref_value(value);
    let heap = shared.heap();
    let array = heap.array(reference).expect("the result is an array");
    assert_eq!(array.kind, ArrayKind::Char);
    array
        .elements
        .iter()
        .map(|element| element.as_int().expect("a char element is an int"))
        .collect()
}

fn heap_string(fixture: &Fixture, text: &str) -> Value {
    Value::Ref(fixture.shared.heap().allocate_string(text))
}

fn bounds(error: Result<Option<Value>, InterpError>) -> (i32, usize) {
    match error {
        Err(InterpError::Vm(VmError::StringIndexOutOfBounds { index, length })) => (index, length),
        other => panic!("expected a StringIndexOutOfBounds error, found {other:?}"),
    }
}

#[test]
fn strings_measure_and_index_utf16_code_units() {
    let fixture = fixture();
    assert_eq!(int_value(ok(&fixture, "literalLength", "()I", vec![])), 5);
    assert!(bool_value(ok(&fixture, "literalEmpty", "()Z", vec![])));
    assert_eq!(
        int_value(ok(&fixture, "literalCharAt", "(I)C", vec![Value::Int(1)])),
        i32::from(b'b')
    );

    // The empty string has UTF-16 length 0.
    assert_eq!(
        int_value(ok(
            &fixture,
            "length",
            "(Ljava/lang/String;)I",
            vec![heap_string(&fixture, "")]
        )),
        0
    );

    // "😀" is one Rust char but two UTF-16 code units.
    assert_eq!(int_value(ok(&fixture, "surrogateLength", "()I", vec![])), 2);
    assert_eq!(
        int_value(ok(&fixture, "surrogateCharAt", "(I)C", vec![Value::Int(0)])),
        GRINNING_HIGH
    );
    assert_eq!(
        int_value(ok(&fixture, "surrogateCharAt", "(I)C", vec![Value::Int(1)])),
        GRINNING_LOW
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "surrogateSubstring",
                "()Ljava/lang/String;",
                vec![]
            )
        ),
        "😀"
    );
    // "a😀b": the 'b' is at UTF-16 index 3, not Rust char index 2.
    assert_eq!(
        int_value(ok(&fixture, "surrogateIndexOf", "()I", vec![])),
        3
    );

    // toCharArray exposes the surrogate pair as two char units.
    assert_eq!(
        char_units(
            &fixture.shared,
            ok(&fixture, "literalChars", "()[C", vec![])
        ),
        vec![GRINNING_HIGH, GRINNING_LOW]
    );
}

#[test]
fn equality_hash_and_to_string_follow_string_content() {
    let fixture = fixture();
    // Two separately allocated heap strings with the same content: content equality
    // cannot be reference identity, because only `ldc` constants are interned.
    assert!(bool_value(ok(
        &fixture,
        "equals",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "same"), heap_string(&fixture, "same")]
    )));
    assert!(bool_value(ok(&fixture, "literalEquals", "()Z", vec![])));
    assert!(
        bool_value(ok(&fixture, "literalSameReference", "()Z", vec![])),
        "two ldcs of the same constant are interned to one object"
    );
    assert!(!bool_value(ok(&fixture, "equalsNull", "()Z", vec![])));
    assert!(!bool_value(ok(
        &fixture,
        "equals",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "abc"), heap_string(&fixture, "abd")]
    )));

    let object = ref_value(ok(&fixture, "makeObject", "()Ljava/lang/Object;", vec![]));
    assert!(!bool_value(ok(
        &fixture,
        "equalsObject",
        "(Ljava/lang/String;Ljava/lang/Object;)Z",
        vec![heap_string(&fixture, "abc"), Value::Ref(object)]
    )));

    // Java's 31*h + c fold: "hello" is 99162322, "" is 0 and "😀" is 1772899.
    assert_eq!(
        int_value(ok(&fixture, "literalHash", "()I", vec![])),
        99_162_322
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "hash",
            "(Ljava/lang/String;)I",
            vec![heap_string(&fixture, "")]
        )),
        0
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "hash",
            "(Ljava/lang/String;)I",
            vec![heap_string(&fixture, "😀")]
        )),
        1_772_899
    );

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "literalToString", "()Ljava/lang/String;", vec![])
        ),
        "hello"
    );
    assert!(
        bool_value(ok(&fixture, "toStringIdentity", "()Z", vec![])),
        "String.toString returns the receiver"
    );

    // UTF-16 lexicographic order: first differing unit, then length difference.
    assert_eq!(int_value(ok(&fixture, "literalCompare", "()I", vec![])), -1);
    assert_eq!(
        int_value(ok(
            &fixture,
            "compare",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "abc"), heap_string(&fixture, "ab")]
        )),
        1
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "compare",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "abc"), heap_string(&fixture, "abc")]
        )),
        0
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "compare",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "😀"), heap_string(&fixture, "😀x")]
        )),
        -1
    );
}

#[test]
fn concat_substring_and_search_work_on_utf16_units() {
    let fixture = fixture();
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "literalConcat", "()Ljava/lang/String;", vec![])
        ),
        "abcd"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "concat",
                "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
                vec![heap_string(&fixture, "a😀"), heap_string(&fixture, "b")]
            )
        ),
        "a😀b"
    );

    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "literalSubstring", "()Ljava/lang/String;", vec![])
        ),
        "cd"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "substringFrom",
                "(Ljava/lang/String;I)Ljava/lang/String;",
                vec![heap_string(&fixture, "abcdef"), Value::Int(2)]
            )
        ),
        "cdef"
    );
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "substringRange",
                "(Ljava/lang/String;II)Ljava/lang/String;",
                vec![
                    heap_string(&fixture, "abcdef"),
                    Value::Int(1),
                    Value::Int(3)
                ]
            )
        ),
        "bc"
    );
    assert!(
        bool_value(ok(&fixture, "fullRangeIdentity", "()Z", vec![])),
        "substring(0, length) returns the receiver"
    );

    assert!(bool_value(ok(&fixture, "literalStartsWith", "()Z", vec![])));
    assert!(bool_value(ok(&fixture, "literalEndsWith", "()Z", vec![])));
    assert!(bool_value(ok(
        &fixture,
        "startsWith",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "abc"), heap_string(&fixture, "")]
    )));
    assert!(bool_value(ok(
        &fixture,
        "endsWith",
        "(Ljava/lang/String;Ljava/lang/String;)Z",
        vec![heap_string(&fixture, "abc"), heap_string(&fixture, "")]
    )));

    assert_eq!(int_value(ok(&fixture, "literalIndexOf", "()I", vec![])), 1);
    assert_eq!(
        int_value(ok(
            &fixture,
            "indexOf",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "abcabc"), heap_string(&fixture, "")]
        )),
        0
    );
    assert_eq!(
        int_value(ok(
            &fixture,
            "indexOf",
            "(Ljava/lang/String;Ljava/lang/String;)I",
            vec![heap_string(&fixture, "abc"), heap_string(&fixture, "z")]
        )),
        -1
    );
}

#[test]
fn replace_and_to_char_array_rewrite_units() {
    let fixture = fixture();
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(&fixture, "literalReplace", "()Ljava/lang/String;", vec![])
        ),
        "bonono"
    );
    assert!(
        bool_value(ok(&fixture, "replaceIdentity", "()Z", vec![])),
        "replace with an absent character returns the receiver"
    );

    assert_eq!(
        char_units(
            &fixture.shared,
            ok(
                &fixture,
                "toCharArrayOf",
                "(Ljava/lang/String;)[C",
                vec![heap_string(&fixture, "abc")]
            )
        ),
        vec![97, 98, 99]
    );
}

#[test]
fn bounds_and_null_arguments_are_typed_errors() {
    let fixture = fixture();

    assert_eq!(
        bounds(call(&fixture, "charAtPastEnd", "()C", vec![])),
        (5, 3)
    );
    assert_eq!(
        bounds(call(&fixture, "charAtNegative", "()C", vec![])),
        (-1, 3)
    );
    assert_eq!(
        bounds(call(
            &fixture,
            "substringPastEnd",
            "()Ljava/lang/String;",
            vec![]
        )),
        (5, 3)
    );
    // A reversed range reports the offending begin index.
    assert_eq!(
        bounds(call(
            &fixture,
            "substringReversed",
            "()Ljava/lang/String;",
            vec![]
        )),
        (2, 3)
    );

    assert!(matches!(
        call(&fixture, "concatNull", "()Ljava/lang/String;", vec![]),
        Err(InterpError::Vm(VmError::NullStringArgument {
            method: "String.concat"
        }))
    ));
    assert!(matches!(
        call(&fixture, "startsWithNull", "()Z", vec![]),
        Err(InterpError::Vm(VmError::NullStringArgument {
            method: "String.startsWith"
        }))
    ));

    // Splitting a surrogate pair would need a lone surrogate, which the heap's String
    // storage (valid UTF-8) cannot represent, so the operation is a typed error.
    assert!(matches!(
        call(&fixture, "surrogateSplit", "()Ljava/lang/String;", vec![]),
        Err(InterpError::Vm(VmError::UnpairedSurrogate {
            method: "String.substring"
        }))
    ));
}

#[test]
fn object_identity_hash_is_stable_and_to_string_names_the_runtime_class() {
    let fixture = fixture();
    let first = ref_value(ok(&fixture, "makeObject", "()Ljava/lang/Object;", vec![]));
    let second = ref_value(ok(&fixture, "makeObject", "()Ljava/lang/Object;", vec![]));

    assert!(bool_value(ok(
        &fixture,
        "identityEquals",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(first), Value::Ref(first)]
    )));
    assert!(!bool_value(ok(
        &fixture,
        "identityEquals",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(first), Value::Ref(second)]
    )));
    assert!(!bool_value(ok(
        &fixture,
        "identityEquals",
        "(Ljava/lang/Object;Ljava/lang/Object;)Z",
        vec![Value::Ref(first), Value::Ref(ObjRef::NULL)]
    )));

    // The identity hash is stable across calls and distinct for distinct live objects.
    let first_hash = int_value(ok(
        &fixture,
        "identityHash",
        "(Ljava/lang/Object;)I",
        vec![Value::Ref(first)],
    ));
    let first_hash_again = int_value(ok(
        &fixture,
        "identityHash",
        "(Ljava/lang/Object;)I",
        vec![Value::Ref(first)],
    ));
    let second_hash = int_value(ok(
        &fixture,
        "identityHash",
        "(Ljava/lang/Object;)I",
        vec![Value::Ref(second)],
    ));
    assert_eq!(first_hash, first_hash_again);
    assert_ne!(first_hash, second_hash);

    // Object.toString: dotted runtime binary name plus the unsigned hex identity hash.
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "objectToString",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![Value::Ref(first)]
            )
        ),
        format!("java.lang.Object@{:x}", first_hash as u32)
    );

    let probe = ref_value(ok(&fixture, "makeProbe", "()LStringProbe;", vec![]));
    let probe_hash = int_value(ok(
        &fixture,
        "identityHash",
        "(Ljava/lang/Object;)I",
        vec![Value::Ref(probe)],
    ));
    assert_eq!(
        string_value(
            &fixture.shared,
            ok(
                &fixture,
                "objectToString",
                "(Ljava/lang/Object;)Ljava/lang/String;",
                vec![Value::Ref(probe)]
            )
        ),
        format!("StringProbe@{:x}", probe_hash as u32)
    );
}

#[test]
fn the_c1_stubs_have_no_code_and_resolve_through_the_registry() {
    let fixture = fixture();

    // The C1 stubs carry neither a `Code` attribute nor, for the methods below,
    // `ACC_NATIVE`; the registry is consulted before either, which is what makes these
    // methods run. `Object.hashCode` is the one exception: the C1 stub does mark it
    // native, and the registry still wins.
    let entries: [(&str, &str, &str, bool); 7] = [
        ("java/lang/String", "length", "()I", false),
        ("java/lang/String", "equals", "(Ljava/lang/Object;)Z", false),
        ("java/lang/String", "hashCode", "()I", false),
        (
            "java/lang/String",
            "toString",
            "()Ljava/lang/String;",
            false,
        ),
        ("java/lang/Object", "equals", "(Ljava/lang/Object;)Z", false),
        ("java/lang/Object", "hashCode", "()I", true),
        (
            "java/lang/Object",
            "toString",
            "()Ljava/lang/String;",
            false,
        ),
    ];
    for (class_name, name, descriptor, native) in entries {
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
        assert_eq!(
            method.is_native(),
            native,
            "{class_name}.{name} native flag"
        );
        assert!(
            entry.file.code(method).unwrap().is_none(),
            "{class_name}.{name} has no Code"
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

#[test]
fn intern_returns_one_canonical_rooted_object() {
    let fixture = fixture();
    assert!(bool_value(ok(&fixture, "internSame", "()Z", vec![])));

    // Two distinct heap strings with the same content intern to the same object.
    let first = ref_value(ok(
        &fixture,
        "internOf",
        "(Ljava/lang/String;)Ljava/lang/String;",
        vec![heap_string(&fixture, "interned")],
    ));
    let second = ref_value(ok(
        &fixture,
        "internOf",
        "(Ljava/lang/String;)Ljava/lang/String;",
        vec![heap_string(&fixture, "interned")],
    ));
    assert_eq!(first, second);
    assert_eq!(fixture.shared.heap().string(first).unwrap(), "interned");

    // The intern table owns the object and reports it as a root.
    assert!(fixture.shared.roots(&[]).contains(&first));
}
