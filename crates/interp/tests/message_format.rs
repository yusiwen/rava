//! M2.29 acceptance: the minimal `java.text.MessageFormat` Spring's message path needs.
//!
//! `MessageFormatProbe` is assembled against the committed C1 bootstrap library and runs
//! through `new`, `invokespecial`, `invokevirtual` and `invokestatic`, so the tests
//! exercise the registry lookup the interpreter performs (including the inherited
//! `java.text.Format.format(Object)` declaration Spring's `MessageSourceSupport` calls)
//! rather than a Rust helper.
//!
//! Covered: literal text, `{index}` substitution, repeated indices, an absent argument (the
//! JDK keeps `{index}`), a null argument (`"null"`), the JDK's `''` quoting and
//! quote-toggling rules, `applyPattern`/`toPattern`, the static `format(String, ...)`
//! convenience method and the `StringBuffer` form. A `number`/`date`/`choice` subformat and
//! a null pattern are typed errors, which the slice documents.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_static_method};
use rava_vm::{ClassId, LoaderId, Shared, ThreadState, Value, VmError};

const ACC_PUBLIC_STATIC: u16 = 0x0009;
/// `invokestatic` has no named assembler helper.
const INVOKESTATIC: u8 = 0xb8;
/// `pop` has no named assembler helper.
const POP: u8 = 0x57;

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

/// `new MessageFormat(pattern)` into `local 0`.
fn new_message_format(asm: &mut Assembler, class: u16, init: u16, pattern: u16) {
    asm.new_instance(class);
    asm.dup();
    asm.ldc(pattern);
    asm.invokespecial(init);
    asm.astore(0);
}

/// Build the `Object[]` of arguments into `local 1`; `u16::MAX` is a null element.
fn argument_array(asm: &mut Assembler, object_class: u16, values: &[u16]) {
    argument_array_at(asm, object_class, values, 1);
}

/// Build the `Object[]` of arguments into `slot`; `u16::MAX` is a null element.
fn argument_array_at(asm: &mut Assembler, object_class: u16, values: &[u16], slot: u8) {
    asm.iconst(values.len() as i32);
    asm.anewarray(object_class);
    for (position, value) in values.iter().enumerate() {
        asm.dup();
        asm.iconst(position as i32);
        if *value == u16::MAX {
            asm.aconst_null();
        } else {
            asm.ldc(*value);
        }
        asm.aastore();
    }
    asm.astore(slot);
}

/// The probe class. Constants are declared first so every `ldc` index stays one byte.
fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("MessageFormatProbe", "java/lang/Object", 61);

    let patterns = [
        "hello {0}",
        "{0} {1} {0}",
        "a{2}b",
        "x{0}y",
        "don''t {0}",
        "'{0}'",
        "x",
        "y {0}",
        "a{0}b",
        "v={0}",
        "{0}!",
        "{0,number}",
        "",
    ];
    let arguments = ["world", "a", "b", "x", "1", "z", "pre"];
    let mut constants = Vec::new();
    for text in patterns.iter().chain(arguments.iter()) {
        constants.push(builder.string(text));
    }
    let (
        hello_world,
        repeated_pattern,
        missing_pattern,
        null_argument_pattern,
        quoted_pattern,
        quoted_placeholder_pattern,
        initial_pattern,
        applied_pattern,
        to_pattern_pattern,
        static_pattern,
        buffer_pattern,
        number_pattern,
        empty_pattern,
        world,
        a,
        b,
        x,
        one,
        z,
        pre,
    ) = (
        constants[0],
        constants[1],
        constants[2],
        constants[3],
        constants[4],
        constants[5],
        constants[6],
        constants[7],
        constants[8],
        constants[9],
        constants[10],
        constants[11],
        constants[12],
        constants[13],
        constants[14],
        constants[15],
        constants[16],
        constants[17],
        constants[18],
        constants[19],
    );

    let message_format = builder.class("java/text/MessageFormat");
    let message_format_init =
        builder.method_ref("java/text/MessageFormat", "<init>", "(Ljava/lang/String;)V");
    let message_format_init_locale = builder.method_ref(
        "java/text/MessageFormat",
        "<init>",
        "(Ljava/lang/String;Ljava/util/Locale;)V",
    );
    let apply_pattern = builder.method_ref(
        "java/text/MessageFormat",
        "applyPattern",
        "(Ljava/lang/String;)V",
    );
    let to_pattern = builder.method_ref(
        "java/text/MessageFormat",
        "toPattern",
        "()Ljava/lang/String;",
    );
    let format_object = builder.method_ref(
        "java/text/Format",
        "format",
        "(Ljava/lang/Object;)Ljava/lang/String;",
    );
    let format_buffer = builder.method_ref(
        "java/text/MessageFormat",
        "format",
        "([Ljava/lang/Object;Ljava/lang/StringBuffer;Ljava/text/FieldPosition;)Ljava/lang/StringBuffer;",
    );
    let static_format = builder.method_ref(
        "java/text/MessageFormat",
        "format",
        "(Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/String;",
    );
    let object_class = builder.class("java/lang/Object");
    let string_buffer = builder.class("java/lang/StringBuffer");
    let string_buffer_init = builder.method_ref("java/lang/StringBuffer", "<init>", "()V");
    let string_buffer_init_string =
        builder.method_ref("java/lang/StringBuffer", "<init>", "(Ljava/lang/String;)V");
    let string_buffer_append = builder.method_ref(
        "java/lang/StringBuffer",
        "append",
        "(Ljava/lang/String;)Ljava/lang/StringBuffer;",
    );
    let string_buffer_to_string =
        builder.method_ref("java/lang/StringBuffer", "toString", "()Ljava/lang/String;");

    // static String simple() { return new MessageFormat("hello {0}").format(new Object[]{"world"}); }
    let mut body = Assembler::new();
    new_message_format(&mut body, message_format, message_format_init, hello_world);
    argument_array(&mut body, object_class, &[world]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(&mut builder, "simple", "()Ljava/lang/String;", 4, 2, body);

    // static String repeated() { ..."{0} {1} {0}" with {"a","b"}... }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        repeated_pattern,
    );
    argument_array(&mut body, object_class, &[a, b]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(&mut builder, "repeated", "()Ljava/lang/String;", 4, 2, body);

    // static String missing() { ..."a{2}b" with {"x"}... }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        missing_pattern,
    );
    argument_array(&mut body, object_class, &[x]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(&mut builder, "missing", "()Ljava/lang/String;", 4, 2, body);

    // static String nullArgument() { ..."x{0}y" with {null}... }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        null_argument_pattern,
    );
    argument_array(&mut body, object_class, &[u16::MAX]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(
        &mut builder,
        "nullArgument",
        "()Ljava/lang/String;",
        4,
        2,
        body,
    );

    // static String nullArguments() { ..."x{0}y" with a null Object[]... }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        null_argument_pattern,
    );
    body.aconst_null();
    body.astore(1);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(
        &mut builder,
        "nullArguments",
        "()Ljava/lang/String;",
        4,
        2,
        body,
    );

    // static String quoted() { ..."don''t {0}" with {"x"}... }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        quoted_pattern,
    );
    argument_array(&mut body, object_class, &[x]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(&mut builder, "quoted", "()Ljava/lang/String;", 4, 2, body);

    // static String quotedPlaceholder() { ..."'{0}'" with {"x"}... }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        quoted_placeholder_pattern,
    );
    argument_array(&mut body, object_class, &[x]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(
        &mut builder,
        "quotedPlaceholder",
        "()Ljava/lang/String;",
        4,
        2,
        body,
    );

    // static String applied() { MessageFormat mf = new MessageFormat("x");
    //   mf.applyPattern("y {0}"); return mf.format(new Object[]{"z"}); }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        initial_pattern,
    );
    body.aload(0);
    body.ldc(applied_pattern);
    body.invokevirtual(apply_pattern);
    argument_array(&mut body, object_class, &[z]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(&mut builder, "applied", "()Ljava/lang/String;", 4, 2, body);

    // static String pattern() { return new MessageFormat("a{0}b").toPattern(); }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        to_pattern_pattern,
    );
    body.aload(0);
    body.invokevirtual(to_pattern);
    body.areturn();
    static_method(&mut builder, "pattern", "()Ljava/lang/String;", 3, 1, body);

    // static String staticFormat() { return MessageFormat.format("v={0}", new Object[]{"1"}); }
    let mut body = Assembler::new();
    body.ldc(static_pattern);
    body.iconst(1);
    body.anewarray(object_class);
    body.dup();
    body.iconst(0);
    body.ldc(one);
    body.aastore();
    body.op(INVOKESTATIC, &static_format.to_be_bytes());
    body.areturn();
    static_method(
        &mut builder,
        "staticFormat",
        "()Ljava/lang/String;",
        6,
        0,
        body,
    );

    // static String bufferForm() {
    //   MessageFormat mf = new MessageFormat("{0}!");
    //   StringBuffer buffer = new StringBuffer("pre");
    //   mf.format(new Object[]{"a"}, buffer, null);
    //   return buffer.toString();
    // }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        buffer_pattern,
    );
    body.new_instance(string_buffer);
    body.dup();
    body.ldc(pre);
    body.invokespecial(string_buffer_init_string);
    body.astore(1);
    argument_array_at(&mut body, object_class, &[a], 2);
    body.aload(0);
    body.aload(2);
    body.aload(1);
    body.aconst_null();
    body.invokevirtual(format_buffer);
    body.op(POP, &[]);
    body.aload(1);
    body.invokevirtual(string_buffer_to_string);
    body.areturn();
    static_method(
        &mut builder,
        "bufferForm",
        "()Ljava/lang/String;",
        4,
        3,
        body,
    );

    // static StringBuffer appendTwice() { StringBuffer b = new StringBuffer();
    //   return b.append("a").append("b"); }
    let mut body = Assembler::new();
    body.new_instance(string_buffer);
    body.dup();
    body.invokespecial(string_buffer_init);
    body.astore(0);
    body.aload(0);
    body.ldc(a);
    body.invokevirtual(string_buffer_append);
    body.ldc(b);
    body.invokevirtual(string_buffer_append);
    body.areturn();
    static_method(
        &mut builder,
        "appendTwice",
        "()Ljava/lang/StringBuffer;",
        2,
        1,
        body,
    );

    // static String numberSubformat() { ..."{0,number}" with {"x"}... }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        number_pattern,
    );
    argument_array(&mut body, object_class, &[x]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(
        &mut builder,
        "numberSubformat",
        "()Ljava/lang/String;",
        4,
        2,
        body,
    );

    // static void nullPattern() { new MessageFormat(null); }
    let mut body = Assembler::new();
    body.new_instance(message_format);
    body.dup();
    body.aconst_null();
    body.invokespecial(message_format_init);
    body.op(POP, &[]);
    body.return_();
    static_method(&mut builder, "nullPattern", "()V", 3, 0, body);

    // static String localeConstructor() {
    //   return new MessageFormat("hello {0}", null).format(new Object[]{"world"}); }
    let mut body = Assembler::new();
    body.new_instance(message_format);
    body.dup();
    body.ldc(hello_world);
    body.aconst_null();
    body.invokespecial(message_format_init_locale);
    body.astore(0);
    argument_array(&mut body, object_class, &[world]);
    body.aload(0);
    body.aload(1);
    body.invokevirtual(format_object);
    body.areturn();
    static_method(
        &mut builder,
        "localeConstructor",
        "()Ljava/lang/String;",
        4,
        2,
        body,
    );

    // static String emptyPattern() { return new MessageFormat("").toPattern(); }
    let mut body = Assembler::new();
    new_message_format(
        &mut body,
        message_format,
        message_format_init,
        empty_pattern,
    );
    body.aload(0);
    body.invokevirtual(to_pattern);
    body.areturn();
    static_method(
        &mut builder,
        "emptyPattern",
        "()Ljava/lang/String;",
        3,
        1,
        body,
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    rava_vm::library::register_all(&shared).expect("the runtime library registers");
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "MessageFormatProbe", &probe_bytes())
        .expect("the probe defines");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        probe,
    }
}

fn call(fixture: &Fixture, name: &str, descriptor: &str) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        name,
        descriptor,
        Vec::new(),
    )
}

fn text(fixture: &Fixture, name: &str) -> String {
    let value = call(fixture, name, "()Ljava/lang/String;")
        .unwrap_or_else(|error| panic!("{name} runs: {error}"));
    match value {
        Some(Value::Ref(reference)) => fixture
            .shared
            .heap()
            .string(reference)
            .expect("a String result")
            .to_owned(),
        other => panic!("expected a String, found {other:?}"),
    }
}

#[test]
fn formats_a_simple_argument() {
    assert_eq!(text(&fixture(), "simple"), "hello world");
}

#[test]
fn formats_repeated_indices() {
    assert_eq!(text(&fixture(), "repeated"), "a b a");
}

#[test]
fn keeps_the_placeholder_for_a_missing_argument() {
    assert_eq!(text(&fixture(), "missing"), "a{2}b");
    assert_eq!(text(&fixture(), "nullArguments"), "x{0}y");
}

#[test]
fn formats_a_null_argument_as_null() {
    assert_eq!(text(&fixture(), "nullArgument"), "xnully");
}

#[test]
fn honours_quoting() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "quoted"), "don't x");
    assert_eq!(text(&fixture, "quotedPlaceholder"), "{0}");
}

#[test]
fn apply_pattern_and_to_pattern_round_trip() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "applied"), "y z");
    assert_eq!(text(&fixture, "pattern"), "a{0}b");
    assert_eq!(text(&fixture, "emptyPattern"), "");
}

#[test]
fn static_format_and_the_locale_constructor_work() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "staticFormat"), "v=1");
    assert_eq!(text(&fixture, "localeConstructor"), "hello world");
}

#[test]
fn formats_into_a_string_buffer() {
    let fixture = fixture();
    assert_eq!(text(&fixture, "bufferForm"), "prea!");
}

#[test]
fn unsupported_subformat_is_a_typed_error() {
    let fixture = fixture();
    match call(&fixture, "numberSubformat", "()Ljava/lang/String;") {
        Err(InterpError::Vm(VmError::Native(detail))) => {
            assert!(
                detail.contains("number") && detail.contains("not implemented"),
                "unexpected detail: {detail}"
            );
        }
        other => panic!("expected a typed native error, found {other:?}"),
    }
}

#[test]
fn null_pattern_is_a_typed_error() {
    let fixture = fixture();
    match call(&fixture, "nullPattern", "()V") {
        Err(InterpError::Vm(VmError::NullStringArgument { method })) => {
            assert_eq!(method, "MessageFormat.<init>");
        }
        other => panic!("expected a null-String error, found {other:?}"),
    }
}

#[test]
fn string_buffer_receiver_and_get_class_identity() {
    let fixture = fixture();
    // `StringBuffer.append` is a registered native and returns the receiver.
    let value = call(&fixture, "appendTwice", "()Ljava/lang/StringBuffer;")
        .unwrap_or_else(|error| panic!("appendTwice runs: {error}"));
    let Some(Value::Ref(buffer)) = value else {
        panic!("expected a StringBuffer reference, found {value:?}");
    };
    assert_eq!(
        fixture
            .shared
            .heap()
            .string_buffer(buffer)
            .expect("a StringBuffer shape"),
        "ab"
    );
    let class = fixture
        .shared
        .get_class(buffer)
        .expect("StringBuffer has a class identity");
    let name = fixture
        .shared
        .entry(fixture.shared.heap().class(class).expect("a class object"))
        .expect("the class is loaded")
        .name
        .clone();
    assert_eq!(name, "java/lang/StringBuffer");
}
