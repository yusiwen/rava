//! M2.19 acceptance: simple lambdas through `LambdaMetafactory` and `invokeinterface`.
//!
//! `demo.LambdaProbe`, `demo.IntFn` and `demo.Sink` are committed class files compiled with
//! `javac --release 17`, so the call sites are exactly the ones a Java 17 compiler emits:
//! a capturing static lambda, a static method reference, an unbound instance method
//! reference (`String::length`) and a no-argument void lambda. The tests load them with no
//! JDK at test time and invoke every SAM through `invokeinterface` bytecode inside the
//! fixture, never by calling a Rust helper directly.
//!
//! A second class is assembled in Rust for the shapes javac does not emit here: a *bound*
//! instance method reference (a captured receiver), `altMetafactory` with no flags, and the
//! documented failures. The failures cover a wrong `invokeinterface` receiver, a SAM name
//! that does not match the lambda, an unsupported bootstrap owner, a serializable
//! `altMetafactory` flag and a `metafactory` entry with missing static arguments.
//!
//! A lambda's class identity is its functional interface, so `getClass()` reports the
//! interface's class object; the tests assert that and the minimal `Object.toString`,
//! `equals` and `hashCode` behavior.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder, ReferenceKind};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, LambdaTargetKind, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, collections,
    instance, object, reflection, string,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

const METAFACTORY: &str = "java/lang/invoke/LambdaMetafactory";
const METAFACTORY_DESCRIPTOR: &str = "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodType;Ljava/lang/invoke/MethodHandle;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/CallSite;";
const ALT_METAFACTORY_DESCRIPTOR: &str = "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;[Ljava/lang/Object;)Ljava/lang/invoke/CallSite;";

const SAM_DESCRIPTOR: &str = "(Ljava/lang/String;)I";
const INT_FN: &str = "Ldemo/IntFn;";
const SINK: &str = "Ldemo/Sink;";

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

/// `LambdaAssembledProbe`: the call sites javac does not emit in the fixture.
fn assembled_probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("LambdaAssembledProbe", "java/lang/Object", 61);

    let sam_type = builder.method_type(SAM_DESCRIPTOR);
    let instance_type = builder.method_type(SAM_DESCRIPTOR);
    let add_length = builder.method_ref("demo/Sink", "addLength", SAM_DESCRIPTOR);
    let add_length_handle = builder.method_handle(ReferenceKind::InvokeVirtual, add_length);
    let static_length = builder.method_ref("demo/LambdaProbe", "staticLength", SAM_DESCRIPTOR);
    let static_length_handle = builder.method_handle(ReferenceKind::InvokeStatic, static_length);

    // A bound instance method reference: the captured `Sink` is the target's receiver.
    let bound_bootstrap = builder.bootstrap_method(
        METAFACTORY,
        "metafactory",
        METAFACTORY_DESCRIPTOR,
        vec![sam_type, add_length_handle, instance_type],
    );
    let bound_call = builder.invoke_dynamic(bound_bootstrap, "apply", &format!("({SINK}){INT_FN}"));
    let mut bound = Assembler::new();
    bound.aload(0);
    bound.invokedynamic(bound_call);
    bound.areturn();
    static_method(
        &mut builder,
        "bound",
        &format!("({SINK}){INT_FN}"),
        1,
        1,
        bound,
    );

    // `altMetafactory` with a zero flag word is the simple-lambda path.
    let no_flags = builder.integer(0);
    let alt_bootstrap = builder.bootstrap_method(
        METAFACTORY,
        "altMetafactory",
        ALT_METAFACTORY_DESCRIPTOR,
        vec![sam_type, static_length_handle, instance_type, no_flags],
    );
    let alt_call = builder.invoke_dynamic(alt_bootstrap, "apply", &format!("(){INT_FN}"));
    let mut alt = Assembler::new();
    alt.invokedynamic(alt_call);
    alt.areturn();
    static_method(&mut builder, "altPlain", &format!("(){INT_FN}"), 1, 0, alt);

    // A serializable lambda is out of scope and must be rejected on the flags.
    let serializable_flags = builder.integer(1);
    let serializable_bootstrap = builder.bootstrap_method(
        METAFACTORY,
        "altMetafactory",
        ALT_METAFACTORY_DESCRIPTOR,
        vec![
            sam_type,
            static_length_handle,
            instance_type,
            serializable_flags,
        ],
    );
    let serializable_call =
        builder.invoke_dynamic(serializable_bootstrap, "apply", &format!("(){INT_FN}"));
    let mut serializable = Assembler::new();
    serializable.invokedynamic(serializable_call);
    serializable.areturn();
    static_method(
        &mut builder,
        "serializable",
        &format!("(){INT_FN}"),
        1,
        0,
        serializable,
    );

    // A metafactory bootstrap without its three static arguments.
    let missing_bootstrap = builder.bootstrap_method(
        METAFACTORY,
        "metafactory",
        METAFACTORY_DESCRIPTOR,
        Vec::new(),
    );
    let missing_call = builder.invoke_dynamic(missing_bootstrap, "apply", &format!("(){INT_FN}"));
    let mut missing = Assembler::new();
    missing.invokedynamic(missing_call);
    missing.areturn();
    static_method(
        &mut builder,
        "missingArguments",
        &format!("(){INT_FN}"),
        1,
        0,
        missing,
    );

    // A bootstrap that is neither factory.
    let other_bootstrap = builder.bootstrap_method(
        "demo/OtherFactory",
        "makeOther",
        "()Ljava/lang/invoke/CallSite;",
        Vec::new(),
    );
    let other_call = builder.invoke_dynamic(other_bootstrap, "apply", &format!("(){INT_FN}"));
    let mut other = Assembler::new();
    other.invokedynamic(other_call);
    other.areturn();
    static_method(
        &mut builder,
        "unsupportedBootstrap",
        &format!("(){INT_FN}"),
        1,
        0,
        other,
    );

    // `invokeinterface` through the fixture's interface: used for a valid lambda and for
    // the wrong-receiver failure.
    let apply_ref = builder.interface_method_ref("demo/IntFn", "apply", SAM_DESCRIPTOR);
    let mut call_apply = Assembler::new();
    call_apply.aload(0);
    call_apply.aload(1);
    call_apply.invokeinterface(apply_ref, 2);
    call_apply.ireturn();
    static_method(
        &mut builder,
        "callApply",
        "(Ljava/lang/Object;Ljava/lang/String;)I",
        2,
        2,
        call_apply,
    );

    // A SAM name the lambda did not record.
    let other_sam = builder.interface_method_ref("demo/IntFn", "other", SAM_DESCRIPTOR);
    let mut wrong_sam = Assembler::new();
    wrong_sam.aload(0);
    wrong_sam.aload(1);
    wrong_sam.invokeinterface(other_sam, 2);
    wrong_sam.ireturn();
    static_method(
        &mut builder,
        "callWrongSam",
        &format!("({INT_FN}Ljava/lang/String;)I"),
        2,
        2,
        wrong_sam,
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
    assembled: ClassId,
    int_fn: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    reflection::register_natives(&shared);
    string::register_natives(&shared);
    collections::register_natives(&shared);

    let int_fn = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/IntFn",
            include_bytes!("fixtures/lambdas/classes/demo/IntFn.class"),
        )
        .expect("demo/IntFn defines");
    // `define_class` takes the class bytes; loading `demo/Sink` through the probe's own
    // loader would work too, but defining them together keeps the fixture loader explicit.
    let _sink = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/Sink",
            include_bytes!("fixtures/lambdas/classes/demo/Sink.class"),
        )
        .expect("demo/Sink defines");
    let _impl = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/IntFnImpl",
            include_bytes!("fixtures/lambdas/classes/demo/IntFnImpl.class"),
        )
        .expect("demo/IntFnImpl defines");
    let _factory = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/SinkFactory",
            include_bytes!("fixtures/lambdas/classes/demo/SinkFactory.class"),
        )
        .expect("demo/SinkFactory defines");
    let probe = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "demo/LambdaProbe",
            include_bytes!("fixtures/lambdas/classes/demo/LambdaProbe.class"),
        )
        .expect("demo/LambdaProbe defines");
    let assembled = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "LambdaAssembledProbe",
            &assembled_probe_bytes(),
        )
        .expect("LambdaAssembledProbe defines");

    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        probe,
        assembled,
        int_fn,
    }
}

fn call(
    fixture: &Fixture,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        class,
        name,
        descriptor,
        args,
    )
}

fn call_ref(
    fixture: &Fixture,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> ObjRef {
    match call(fixture, class, name, descriptor, args) {
        Ok(Some(Value::Ref(reference))) => reference,
        other => panic!("{name} did not return a reference: {other:?}"),
    }
}

fn string_argument(fixture: &Fixture, text: &str) -> Value {
    Value::Ref(fixture.shared.intern_string(text))
}

fn text(shared: &Shared, reference: ObjRef) -> String {
    shared
        .heap()
        .string(reference)
        .expect("the call returned a String")
        .to_owned()
}

fn apply(fixture: &Fixture, lambda: ObjRef, argument: &str) -> i32 {
    match call(
        fixture,
        fixture.probe,
        "applyIt",
        &format!("({INT_FN}Ljava/lang/String;)I"),
        vec![Value::Ref(lambda), string_argument(fixture, argument)],
    )
    .expect("applyIt runs")
    {
        Some(Value::Int(value)) => value,
        other => panic!("applyIt did not return an int: {other:?}"),
    }
}

#[test]
fn capturing_static_lambda_stores_its_values_and_runs_through_invokeinterface() {
    let fixture = fixture();
    let lambda = call_ref(
        &fixture,
        fixture.probe,
        "capture",
        &format!("(Ljava/lang/String;I){INT_FN}"),
        vec![string_argument(&fixture, "ab"), Value::Int(10)],
    );

    let object = fixture
        .shared
        .heap()
        .lambda(lambda)
        .expect("the call returned a lambda")
        .clone();
    assert_eq!(object.interface, fixture.int_fn);
    assert_eq!(object.sam_name, "apply");
    assert_eq!(object.sam_descriptor, SAM_DESCRIPTOR);
    assert_eq!(object.target.kind, LambdaTargetKind::Static);
    assert_eq!(object.target.name, "lambda$capture$0");
    assert_eq!(
        object.captured,
        vec![string_argument(&fixture, "ab"), Value::Int(10)]
    );

    // "ab".length() + 10 + "xyz".length() == 15.
    assert_eq!(apply(&fixture, lambda, "xyz"), 15);
    // The captured values are immutable across calls.
    assert_eq!(apply(&fixture, lambda, "x"), 13);
    assert_eq!(
        fixture.shared.heap().lambda(lambda).unwrap().captured,
        vec![string_argument(&fixture, "ab"), Value::Int(10)]
    );
}

#[test]
fn unbound_instance_method_reference_takes_the_receiver_from_the_sam() {
    let fixture = fixture();
    let lambda = call_ref(
        &fixture,
        fixture.probe,
        "unbound",
        &format!("(){INT_FN}"),
        Vec::new(),
    );
    let object = fixture.shared.heap().lambda(lambda).unwrap().clone();
    assert_eq!(object.target.kind, LambdaTargetKind::Instance);
    assert!(object.captured.is_empty());
    assert_eq!(object.target.name, "length");
    assert_eq!(apply(&fixture, lambda, "hello"), 5);
}

#[test]
fn static_method_reference_is_a_static_target() {
    let fixture = fixture();
    let lambda = call_ref(
        &fixture,
        fixture.probe,
        "staticRef",
        &format!("(){INT_FN}"),
        Vec::new(),
    );
    assert_eq!(
        fixture.shared.heap().lambda(lambda).unwrap().target.kind,
        LambdaTargetKind::Static
    );
    assert_eq!(apply(&fixture, lambda, "abcd"), 4);
}

#[test]
fn a_no_argument_void_lambda_runs_and_captures_a_receiver() {
    let fixture = fixture();
    let sink = call_ref(
        &fixture,
        fixture.probe,
        "sink",
        &format!("(Ljava/lang/String;){SINK}"),
        vec![string_argument(&fixture, "start")],
    );
    let runnable = call_ref(
        &fixture,
        fixture.probe,
        "runnable",
        &format!("({SINK}Ljava/lang/String;)Ljava/lang/Runnable;"),
        vec![Value::Ref(sink), string_argument(&fixture, "changed")],
    );
    let object = fixture.shared.heap().lambda(runnable).unwrap().clone();
    assert_eq!(object.target.kind, LambdaTargetKind::Static);
    assert_eq!(object.sam_name, "run");
    assert_eq!(object.sam_descriptor, "()V");
    assert_eq!(object.captured.len(), 2);

    let value = call(
        &fixture,
        fixture.probe,
        "runAndRead",
        &format!("(Ljava/lang/Runnable;{SINK})Ljava/lang/String;"),
        vec![Value::Ref(runnable), Value::Ref(sink)],
    )
    .expect("runAndRead runs")
    .expect("runAndRead returns a String");
    let Value::Ref(value) = value else {
        panic!("runAndRead did not return a reference");
    };
    assert_eq!(text(&fixture.shared, value), "changed");

    // A void SAM can be invoked repeatedly through the fixture's invokeinterface.
    for _ in 0..2 {
        call(
            &fixture,
            fixture.probe,
            "runIt",
            "(Ljava/lang/Runnable;)V",
            vec![Value::Ref(runnable)],
        )
        .expect("runIt runs");
    }
}

#[test]
fn a_bound_instance_method_reference_uses_the_captured_receiver() {
    let fixture = fixture();
    let sink = call_ref(
        &fixture,
        fixture.probe,
        "sink",
        &format!("(Ljava/lang/String;){SINK}"),
        vec![string_argument(&fixture, "abc")],
    );
    let lambda = call_ref(
        &fixture,
        fixture.assembled,
        "bound",
        &format!("({SINK}){INT_FN}"),
        vec![Value::Ref(sink)],
    );
    let object = fixture.shared.heap().lambda(lambda).unwrap().clone();
    assert_eq!(object.target.kind, LambdaTargetKind::Instance);
    assert_eq!(object.captured, vec![Value::Ref(sink)]);
    // "abc".length() + "xy".length() == 5.
    assert_eq!(apply(&fixture, lambda, "xy"), 5);
}

#[test]
fn alt_metafactory_with_no_flags_creates_a_working_lambda() {
    let fixture = fixture();
    let lambda = call_ref(
        &fixture,
        fixture.assembled,
        "altPlain",
        &format!("(){INT_FN}"),
        Vec::new(),
    );
    assert_eq!(apply(&fixture, lambda, "abcd"), 4);
}

#[test]
fn a_constructor_method_reference_allocates_its_target() {
    let fixture = fixture();
    let factory = call_ref(
        &fixture,
        fixture.probe,
        "factory",
        "()Ldemo/SinkFactory;",
        Vec::new(),
    );
    let object = fixture.shared.heap().lambda(factory).unwrap().clone();
    assert_eq!(object.target.kind, LambdaTargetKind::Constructor);
    assert_eq!(object.target.name, "<init>");
    assert_eq!(object.target.descriptor, "(Ljava/lang/String;)V");

    let sink = call_ref(
        &fixture,
        fixture.probe,
        "makeSink",
        "(Ldemo/SinkFactory;Ljava/lang/String;)Ldemo/Sink;",
        vec![Value::Ref(factory), string_argument(&fixture, "made")],
    );
    let value = call(
        &fixture,
        fixture.probe,
        "sinkValue",
        &format!("({SINK})Ljava/lang/String;"),
        vec![Value::Ref(sink)],
    )
    .expect("sinkValue runs")
    .expect("sinkValue returns a String");
    let Value::Ref(value) = value else {
        panic!("sinkValue did not return a reference");
    };
    assert_eq!(text(&fixture.shared, value), "made");
}

#[test]
fn invokeinterface_dispatches_ordinary_instances_and_registered_receivers() {
    let fixture = fixture();

    // An ordinary instance implementing the interface: resolution starts at the
    // receiver's runtime class, which declares `apply`.
    let implementation = call_ref(
        &fixture,
        fixture.probe,
        "impl",
        &format!("(){INT_FN}"),
        Vec::new(),
    );
    assert_eq!(apply(&fixture, implementation, "abc"), 4);

    // A String receiver where the interface method reaches the registered
    // `java/lang/String.length` native.
    let length = call(
        &fixture,
        fixture.probe,
        "charSequenceLength",
        "(Ljava/lang/CharSequence;)I",
        vec![string_argument(&fixture, "hello")],
    )
    .expect("charSequenceLength runs");
    assert_eq!(length, Some(Value::Int(5)));

    // An ArrayList receiver where the interface method reaches the registered
    // `java/util/ArrayList.size` native.
    let list = call_ref(
        &fixture,
        fixture.probe,
        "oneElementList",
        "()Ljava/util/List;",
        Vec::new(),
    );
    let size = call(
        &fixture,
        fixture.probe,
        "listSize",
        "(Ljava/util/List;)I",
        vec![Value::Ref(list)],
    )
    .expect("listSize runs");
    assert_eq!(size, Some(Value::Int(1)));
}

#[test]
fn lambda_class_identity_is_the_interface() {
    let fixture = fixture();
    let lambda = call_ref(
        &fixture,
        fixture.probe,
        "staticRef",
        &format!("(){INT_FN}"),
        Vec::new(),
    );

    let name = call(
        &fixture,
        fixture.probe,
        "className",
        "(Ljava/lang/Object;)Ljava/lang/String;",
        vec![Value::Ref(lambda)],
    )
    .expect("className runs")
    .expect("className returns a String");
    let Value::Ref(name) = name else {
        panic!("className did not return a reference");
    };
    assert_eq!(text(&fixture.shared, name), "demo.IntFn");

    let class_object = fixture.shared.get_class(lambda).expect("get_class");
    assert_eq!(
        fixture.shared.heap().class(class_object).unwrap(),
        fixture.int_fn
    );

    let object_class = fixture
        .shared
        .load_library_class("java/lang/Object")
        .unwrap();
    let rendered = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        object_class,
        "toString",
        "()Ljava/lang/String;",
        lambda,
        Vec::new(),
    )
    .expect("Object.toString runs")
    .expect("toString returns a String");
    let Value::Ref(rendered) = rendered else {
        panic!("toString did not return a reference");
    };
    let rendered = text(&fixture.shared, rendered);
    assert!(
        rendered.starts_with("demo.IntFn@"),
        "expected the interface's ClassName@hex shape, found {rendered:?}"
    );

    let same = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        object_class,
        "equals",
        "(Ljava/lang/Object;)Z",
        lambda,
        vec![Value::Ref(lambda)],
    )
    .expect("Object.equals runs");
    assert_eq!(same, Some(Value::Int(1)));

    let hash = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        object_class,
        "hashCode",
        "()I",
        lambda,
        Vec::new(),
    )
    .expect("Object.hashCode runs");
    assert_eq!(hash, Some(Value::Int(lambda.identity_hash())));
}

#[test]
fn invokeinterface_on_a_matching_lambda_and_a_wrong_receiver() {
    let fixture = fixture();
    let lambda = call_ref(
        &fixture,
        fixture.probe,
        "capture",
        &format!("(Ljava/lang/String;I){INT_FN}"),
        vec![string_argument(&fixture, "ab"), Value::Int(10)],
    );
    let value = call(
        &fixture,
        fixture.assembled,
        "callApply",
        "(Ljava/lang/Object;Ljava/lang/String;)I",
        vec![Value::Ref(lambda), string_argument(&fixture, "xyz")],
    )
    .expect("callApply on a lambda runs");
    assert_eq!(value, Some(Value::Int(15)));

    let error = call(
        &fixture,
        fixture.assembled,
        "callApply",
        "(Ljava/lang/Object;Ljava/lang/String;)I",
        vec![
            string_argument(&fixture, "not a lambda"),
            string_argument(&fixture, "x"),
        ],
    )
    .expect_err("a String is not an IntFn");
    assert!(
        matches!(error, InterpError::NotAssignable { .. }),
        "expected NotAssignable, found {error:?}"
    );
}

#[test]
fn invokeinterface_with_a_name_the_lambda_did_not_record_is_a_typed_error() {
    let fixture = fixture();
    let lambda = call_ref(
        &fixture,
        fixture.probe,
        "capture",
        &format!("(Ljava/lang/String;I){INT_FN}"),
        vec![string_argument(&fixture, "ab"), Value::Int(10)],
    );
    let error = call(
        &fixture,
        fixture.assembled,
        "callWrongSam",
        &format!("({INT_FN}Ljava/lang/String;)I"),
        vec![Value::Ref(lambda), string_argument(&fixture, "x")],
    )
    .expect_err("the SAM name does not match");
    assert!(
        matches!(error, InterpError::Lambda(_)),
        "expected a typed lambda error, found {error:?}"
    );
}

#[test]
fn unsupported_bootstrap_owner_is_a_typed_error() {
    let fixture = fixture();
    let error = call(
        &fixture,
        fixture.assembled,
        "unsupportedBootstrap",
        &format!("(){INT_FN}"),
        Vec::new(),
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
fn serializable_alt_metafactory_is_accepted_and_missing_arguments_are_typed_errors() {
    let fixture = fixture();

    // `FLAG_SERIALIZABLE` is accepted: the lambda object itself has no `writeReplace`, so
    // serializing it fails later, but the call site still creates a working lambda.
    let serializable = call_ref(
        &fixture,
        fixture.assembled,
        "serializable",
        &format!("(){INT_FN}"),
        Vec::new(),
    );
    assert_eq!(apply(&fixture, serializable, "abcd"), 4);

    let missing = call(
        &fixture,
        fixture.assembled,
        "missingArguments",
        &format!("(){INT_FN}"),
        Vec::new(),
    )
    .expect_err("a three-argument metafactory needs its arguments");
    assert!(
        matches!(missing, InterpError::Vm(VmError::Native(_))),
        "expected a typed native error, found {missing:?}"
    );
}
