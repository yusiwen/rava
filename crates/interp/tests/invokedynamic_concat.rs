//! M2.18 acceptance: `invokedynamic` string concatenation through
//! `java.lang.invoke.StringConcatFactory`.
//!
//! `demo.ConcatProbe` is a committed class file compiled with `javac --release 17`, so
//! the call sites are exactly the ones a Java 17 compiler emits: every `+` is an
//! `invokedynamic` whose bootstrap is `makeConcatWithConstants`, including a recipe that
//! uses `\u0002` because the constant text contains `\u0001`. The tests load it with no
//! JDK at test time.
//!
//! A second class is assembled in Rust for the shapes javac does not emit here:
//! `makeConcat`, a recipe carrying every supported static constant kind, and arbitrary
//! primitive parameter descriptors (long, float, double, boolean, char, byte). Two more
//! assembled classes prove the documented failures: a bootstrap owner other than
//! `StringConcatFactory` is typed `VmError::UnsupportedBootstrapMethod`, and an
//! out-of-range `bootstrap_method_attr_index` is a typed
//! `rava_classfile::Error::BadBootstrapMethod` during decoding.
//!
//! Every result is pushed as an interned `String`, so a repeated call with equal input
//! is reference-identical as well as text-equal.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder, ReferenceKind};
use rava_interp::{InterpError, run_static_method};
use rava_vm::{
    ClassId, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, instance, object, string,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

/// The `StringConcatFactory` bootstrap descriptor for both factory methods.
const FACTORY_DESCRIPTOR: &str = "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/invoke/CallSite;";

const CONCAT_STRING_INT: &str = "(Ljava/lang/String;I)Ljava/lang/String;";
const CONCAT_STRING: &str = "(Ljava/lang/String;)Ljava/lang/String;";
const NO_ARGUMENT_STRING: &str = "()Ljava/lang/String;";

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

/// `ConcatCallSiteProbe`: the call sites javac does not emit.
fn call_site_probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("ConcatCallSiteProbe", "java/lang/Object", 61);

    let make_concat_bootstrap = builder.bootstrap_method(
        "java/lang/invoke/StringConcatFactory",
        "makeConcat",
        FACTORY_DESCRIPTOR,
        Vec::new(),
    );
    let make_concat =
        builder.invoke_dynamic(make_concat_bootstrap, "makeConcat", CONCAT_STRING_INT);

    // A `makeConcat` call site with no constant text.
    let mut concat = Assembler::new();
    concat.aload(0);
    concat.iload(1);
    concat.invokedynamic(make_concat);
    concat.areturn();
    static_method(&mut builder, "makeConcat", CONCAT_STRING_INT, 2, 2, concat);

    // `makeConcat` over every primitive descriptor the conversion supports. The body uses
    // compact local slots because this interpreter keeps one local per argument (it does
    // not widen long/double to two slots).
    let primitives_descriptor = "(JFDZCB)Ljava/lang/String;";
    let primitives_bootstrap = builder.bootstrap_method(
        "java/lang/invoke/StringConcatFactory",
        "makeConcat",
        FACTORY_DESCRIPTOR,
        Vec::new(),
    );
    let primitives =
        builder.invoke_dynamic(primitives_bootstrap, "makeConcat", primitives_descriptor);
    let mut primitive_body = Assembler::new();
    primitive_body.lload(0);
    primitive_body.fload(1);
    primitive_body.dload(2);
    primitive_body.iload(3);
    primitive_body.iload(4);
    primitive_body.iload(5);
    primitive_body.invokedynamic(primitives);
    primitive_body.areturn();
    static_method(
        &mut builder,
        "primitives",
        primitives_descriptor,
        6,
        6,
        primitive_body,
    );

    // `makeConcat` over references: this is where a null, a boxed Integer and an ordinary
    // object reach the M2.9/M2.13 value conversion directly.
    let objects = builder.invoke_dynamic(
        make_concat_bootstrap,
        "makeConcat",
        "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/String;",
    );
    let mut object_body = Assembler::new();
    object_body.aload(0);
    object_body.aload(1);
    object_body.invokedynamic(objects);
    object_body.areturn();
    static_method(
        &mut builder,
        "concatObject",
        "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/String;",
        2,
        2,
        object_body,
    );

    // A recipe whose `\u0002` markers consume every supported static constant kind.
    let recipe = builder.string("s=\u{2},i=\u{2},c=\u{2},l=\u{2},f=\u{2},d=\u{2}");
    let argument_string = builder.string("x");
    let argument_int = builder.integer(7);
    let argument_class = builder.class("java/lang/String");
    let argument_long = builder.long(9);
    let argument_float = builder.float(1.5);
    let argument_double = builder.double(2.25);
    let recipe_bootstrap = builder.bootstrap_method(
        "java/lang/invoke/StringConcatFactory",
        "makeConcatWithConstants",
        FACTORY_DESCRIPTOR,
        vec![
            recipe,
            argument_string,
            argument_int,
            argument_class,
            argument_long,
            argument_float,
            argument_double,
        ],
    );
    let recipe_call = builder.invoke_dynamic(
        recipe_bootstrap,
        "makeConcatWithConstants",
        NO_ARGUMENT_STRING,
    );
    let mut recipe_body = Assembler::new();
    recipe_body.invokedynamic(recipe_call);
    recipe_body.areturn();
    static_method(
        &mut builder,
        "recipeConstants",
        NO_ARGUMENT_STRING,
        1,
        0,
        recipe_body,
    );

    // `makeConcatWithConstants` with a recipe that consumes more than its descriptor has.
    let short_recipe = builder.string("\u{1}\u{1}");
    let mismatch_bootstrap = builder.bootstrap_method(
        "java/lang/invoke/StringConcatFactory",
        "makeConcatWithConstants",
        FACTORY_DESCRIPTOR,
        vec![short_recipe],
    );
    let mismatch_call =
        builder.invoke_dynamic(mismatch_bootstrap, "makeConcatWithConstants", CONCAT_STRING);
    let mut mismatch_body = Assembler::new();
    mismatch_body.aload(0);
    mismatch_body.invokedynamic(mismatch_call);
    mismatch_body.areturn();
    static_method(
        &mut builder,
        "recipeMismatch",
        CONCAT_STRING,
        1,
        1,
        mismatch_body,
    );

    // A bootstrap method that is not `StringConcatFactory`.
    let other_bootstrap = builder.bootstrap_method(
        "demo/OtherFactory",
        "makeOther",
        "()Ljava/lang/invoke/CallSite;",
        Vec::new(),
    );
    let other_call = builder.invoke_dynamic(other_bootstrap, "makeOther", CONCAT_STRING);
    let mut other_body = Assembler::new();
    other_body.aload(0);
    other_body.invokedynamic(other_call);
    other_body.areturn();
    static_method(
        &mut builder,
        "unsupportedBootstrap",
        CONCAT_STRING,
        1,
        1,
        other_body,
    );

    // A bootstrap handle whose kind is not `REF_invokeStatic` is malformed.
    let virtual_target = builder.method_ref("java/lang/Object", "toString", "()Ljava/lang/String;");
    let virtual_handle = builder.method_handle(ReferenceKind::InvokeVirtual, virtual_target);
    let virtual_bootstrap = builder.bootstrap_method_handle(virtual_handle, Vec::new());
    let virtual_call =
        builder.invoke_dynamic(virtual_bootstrap, "makeConcatWithConstants", CONCAT_STRING);
    let mut virtual_body = Assembler::new();
    virtual_body.aload(0);
    virtual_body.invokedynamic(virtual_call);
    virtual_body.areturn();
    static_method(
        &mut builder,
        "malformedHandle",
        CONCAT_STRING,
        1,
        1,
        virtual_body,
    );

    builder.build()
}

/// `BrokenConcatProbe`: an `invokedynamic` whose bootstrap index has no table entry.
fn missing_bootstrap_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("BrokenConcatProbe", "java/lang/Object", 61);
    // No BootstrapMethods attribute is emitted, so index 3 is out of range.
    let call = builder.invoke_dynamic(3, "makeConcatWithConstants", NO_ARGUMENT_STRING);
    let mut body = Assembler::new();
    body.invokedynamic(call);
    body.areturn();
    static_method(
        &mut builder,
        "missingBootstrap",
        NO_ARGUMENT_STRING,
        1,
        0,
        body,
    );
    builder.build()
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
    call_site_probe: ClassId,
    missing_bootstrap: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    string::register_natives(&shared);

    let probe = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/ConcatProbe",
            include_bytes!("fixtures/invokedynamic/classes/demo/ConcatProbe.class"),
        )
        .expect("demo/ConcatProbe defines");
    let call_site_probe = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "ConcatCallSiteProbe",
            &call_site_probe_bytes(),
        )
        .expect("ConcatCallSiteProbe defines");
    let missing_bootstrap = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "BrokenConcatProbe",
            &missing_bootstrap_bytes(),
        )
        .expect("BrokenConcatProbe defines");

    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        probe,
        call_site_probe,
        missing_bootstrap,
    }
}

fn call(
    fixture: &Fixture,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<ObjRef, InterpError> {
    let value = run_static_method(
        &fixture.shared,
        &fixture.thread,
        class,
        name,
        descriptor,
        args,
    )?;
    match value {
        Some(Value::Ref(reference)) => Ok(reference),
        other => panic!("{name} did not return a reference: {other:?}"),
    }
}

fn text(shared: &Shared, reference: ObjRef) -> String {
    shared
        .heap()
        .string(reference)
        .expect("the call returned a String")
        .to_owned()
}

fn string_argument(fixture: &Fixture, text: &str) -> Value {
    Value::Ref(fixture.shared.intern_string(text))
}

#[test]
fn javac_recipe_concatenates_a_string_and_an_int() {
    let fixture = fixture();
    let result = call(
        &fixture,
        fixture.probe,
        "concat",
        CONCAT_STRING_INT,
        vec![string_argument(&fixture, "abc"), Value::Int(42)],
    )
    .expect("concat runs");
    assert_eq!(text(&fixture.shared, result), "v=abc/42");
}

#[test]
fn constant_text_and_recipe_constants_are_expanded() {
    let fixture = fixture();
    let constants = call(
        &fixture,
        fixture.probe,
        "constants",
        CONCAT_STRING,
        vec![string_argument(&fixture, "abc")],
    )
    .expect("constants runs");
    assert_eq!(text(&fixture.shared, constants), "[abc]");

    // javac spells this recipe as `\u0001\u0002\u0001` with a `\u0001` static constant:
    // the constant itself cannot sit inline because it is a recipe marker.
    let escaping = call(
        &fixture,
        fixture.probe,
        "escaping",
        CONCAT_STRING,
        vec![string_argument(&fixture, "abc")],
    )
    .expect("escaping runs");
    assert_eq!(text(&fixture.shared, escaping), "abc\u{1}abc");
}

#[test]
fn null_and_empty_strings_render_like_string_value_of() {
    let fixture = fixture();
    let null = call(
        &fixture,
        fixture.probe,
        "emptyAndNull",
        CONCAT_STRING,
        vec![Value::Ref(ObjRef::NULL)],
    )
    .expect("null runs");
    assert_eq!(text(&fixture.shared, null), "null");

    let empty = call(
        &fixture,
        fixture.probe,
        "emptyAndNull",
        CONCAT_STRING,
        vec![string_argument(&fixture, "")],
    )
    .expect("empty runs");
    assert_eq!(text(&fixture.shared, empty), "");
}

#[test]
fn primitive_arguments_use_string_value_of_rules() {
    let fixture = fixture();
    let result = call(
        &fixture,
        fixture.probe,
        "primitives",
        "(IZCLjava/lang/Object;)Ljava/lang/String;",
        vec![
            Value::Int(7),
            Value::Int(1),
            Value::Int('A' as i32),
            string_argument(&fixture, "tail"),
        ],
    )
    .expect("primitives runs");
    assert_eq!(text(&fixture.shared, result), "p=7,true,A,tail");
}

#[test]
fn make_concat_has_no_recipe() {
    let fixture = fixture();
    let result = call(
        &fixture,
        fixture.call_site_probe,
        "makeConcat",
        CONCAT_STRING_INT,
        vec![string_argument(&fixture, "abc"), Value::Int(42)],
    )
    .expect("makeConcat runs");
    assert_eq!(text(&fixture.shared, result), "abc42");
}

#[test]
fn make_concat_converts_every_primitive_descriptor() {
    let fixture = fixture();
    let result = call(
        &fixture,
        fixture.call_site_probe,
        "primitives",
        "(JFDZCB)Ljava/lang/String;",
        vec![
            Value::Long(123),
            Value::Float(1.5),
            Value::Double(2.25),
            Value::Int(1),
            Value::Int('A' as i32),
            Value::Int(0),
        ],
    )
    .expect("primitive makeConcat runs");
    assert_eq!(text(&fixture.shared, result), "1231.52.25trueA0");
}

#[test]
fn make_concat_uses_m29_reference_conversion() {
    let fixture = fixture();

    let null = call(
        &fixture,
        fixture.call_site_probe,
        "concatObject",
        "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/String;",
        vec![string_argument(&fixture, "x"), Value::Ref(ObjRef::NULL)],
    )
    .expect("null object runs");
    assert_eq!(text(&fixture.shared, null), "xnull");

    let boxed = call(
        &fixture,
        fixture.call_site_probe,
        "concatObject",
        "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/String;",
        vec![
            string_argument(&fixture, "x"),
            Value::Ref(fixture.shared.box_int(42)),
        ],
    )
    .expect("boxed object runs");
    assert_eq!(text(&fixture.shared, boxed), "x42");

    let object = fixture
        .shared
        .allocate_instance(fixture.probe)
        .expect("probe instance");
    let rendered = call(
        &fixture,
        fixture.call_site_probe,
        "concatObject",
        "(Ljava/lang/String;Ljava/lang/Object;)Ljava/lang/String;",
        vec![string_argument(&fixture, "x"), Value::Ref(object)],
    )
    .expect("ordinary object runs");
    let rendered = text(&fixture.shared, rendered);
    assert!(
        rendered.starts_with("xdemo.ConcatProbe@"),
        "expected the ClassName@hex shape, found {rendered:?}"
    );
}

#[test]
fn recipe_constants_cover_every_supported_static_argument() {
    let fixture = fixture();
    let result = call(
        &fixture,
        fixture.call_site_probe,
        "recipeConstants",
        NO_ARGUMENT_STRING,
        Vec::new(),
    )
    .expect("recipe constants run");
    assert_eq!(
        text(&fixture.shared, result),
        "s=x,i=7,c=java.lang.String,l=9,f=1.5,d=2.25"
    );
}

#[test]
fn repeated_calls_are_correct_and_interned() {
    let fixture = fixture();
    let first = call(
        &fixture,
        fixture.probe,
        "concat",
        CONCAT_STRING_INT,
        vec![string_argument(&fixture, "same"), Value::Int(1)],
    )
    .expect("first call");
    let second = call(
        &fixture,
        fixture.probe,
        "concat",
        CONCAT_STRING_INT,
        vec![string_argument(&fixture, "same"), Value::Int(1)],
    )
    .expect("second call");
    assert_eq!(text(&fixture.shared, first), "v=same/1");
    assert_eq!(text(&fixture.shared, second), "v=same/1");
    assert_eq!(
        first, second,
        "a repeated call must push the interned String object"
    );
}

#[test]
fn a_recipe_that_disagrees_with_its_descriptor_is_a_typed_error() {
    let fixture = fixture();
    let error = call(
        &fixture,
        fixture.call_site_probe,
        "recipeMismatch",
        CONCAT_STRING,
        vec![string_argument(&fixture, "abc")],
    )
    .expect_err("a short descriptor must fail");
    assert!(
        matches!(error, InterpError::Vm(VmError::Native(_))),
        "expected a typed VM native error, found {error:?}"
    );
}

#[test]
fn a_non_string_concat_factory_bootstrap_is_a_typed_error() {
    let fixture = fixture();
    let error = call(
        &fixture,
        fixture.call_site_probe,
        "unsupportedBootstrap",
        CONCAT_STRING,
        vec![string_argument(&fixture, "abc")],
    )
    .expect_err("an arbitrary bootstrap must fail");
    match error {
        InterpError::Vm(VmError::UnsupportedBootstrapMethod {
            owner,
            name,
            descriptor,
        }) => {
            assert_eq!(owner, "demo/OtherFactory");
            assert_eq!(name, "makeOther");
            assert_eq!(descriptor, "()Ljava/lang/invoke/CallSite;");
        }
        other => panic!("expected UnsupportedBootstrapMethod, found {other:?}"),
    }
}

#[test]
fn a_malformed_bootstrap_handle_is_a_typed_decode_error() {
    let fixture = fixture();
    let error = call(
        &fixture,
        fixture.call_site_probe,
        "malformedHandle",
        CONCAT_STRING,
        vec![string_argument(&fixture, "abc")],
    )
    .expect_err("a non-static handle must fail");
    assert!(
        matches!(
            error,
            InterpError::ClassFile(rava_classfile::Error::BadBootstrapMethod { .. })
        ),
        "expected BadBootstrapMethod, found {error:?}"
    );
}

#[test]
fn an_out_of_range_bootstrap_index_is_a_typed_decode_error() {
    let fixture = fixture();
    let error = call(
        &fixture,
        fixture.missing_bootstrap,
        "missingBootstrap",
        NO_ARGUMENT_STRING,
        Vec::new(),
    )
    .expect_err("an out-of-range bootstrap index must fail");
    assert!(
        matches!(
            error,
            InterpError::ClassFile(rava_classfile::Error::BadBootstrapMethod { .. })
        ),
        "expected BadBootstrapMethod, found {error:?}"
    );
}
