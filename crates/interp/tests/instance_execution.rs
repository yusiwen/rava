//! M2.5 acceptance: instance allocation, constructors, fields and instance calls.
//!
//! `Point` is an ordinary class with two instance `int` fields, a static `counter`
//! (which must stay out of the instance layout), a `(II)V` constructor that chains to
//! `Object.<init>`, and two instance methods. `PointProbe` drives it with the real
//! `new` / `dup` / `invokespecial` / `invokevirtual` sequence, and exposes the failure
//! paths (a static field through `getfield`, a wrong descriptor) as callable methods.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{ClassId, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, instance};

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PUBLIC_STATIC: u16 = 0x0009;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn point_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Point", "java/lang/Object", 61);
    builder.field(ACC_PUBLIC, "x", "I");
    builder.field(ACC_PUBLIC, "y", "I");
    // A static field must not become an instance slot.
    builder.field(ACC_PUBLIC_STATIC, "counter", "I");

    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    let x_ref = builder.field_ref("Point", "x", "I");
    let y_ref = builder.field_ref("Point", "y", "I");

    // Point(int x, int y) {
    //     super();
    //     this.x = x;
    //     this.y = y;
    // }
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.aload(0);
    init.iload(1);
    init.putfield(x_ref);
    init.aload(0);
    init.iload(2);
    init.putfield(y_ref);
    init.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "(II)V",
        Some((2, 3, init.finish().unwrap())),
    );

    // int getX() { return this.x; }
    let mut get_x = Assembler::new();
    get_x.aload(0);
    get_x.getfield(x_ref);
    get_x.ireturn();
    builder.method(
        ACC_PUBLIC,
        "getX",
        "()I",
        Some((1, 1, get_x.finish().unwrap())),
    );

    // int sum() { return this.x + this.y; }
    let mut sum = Assembler::new();
    sum.aload(0);
    sum.getfield(x_ref);
    sum.aload(0);
    sum.getfield(y_ref);
    sum.iadd();
    sum.ireturn();
    builder.method(
        ACC_PUBLIC,
        "sum",
        "()I",
        Some((2, 1, sum.finish().unwrap())),
    );

    builder.build()
}

fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("PointProbe", "java/lang/Object", 61);

    let point_class = builder.class("Point");
    let point_init = builder.method_ref("Point", "<init>", "(II)V");
    let get_x = builder.method_ref("Point", "getX", "()I");
    let sum = builder.method_ref("Point", "sum", "()I");
    let counter = builder.field_ref("Point", "counter", "I");
    let wrong_descriptor = builder.field_ref("Point", "x", "J");
    let aux = builder.method_ref("PointProbe", "aux", "(LPoint;)Z");

    // static Point nullPoint() { return null; }
    let mut null_point = Assembler::new();
    null_point.aconst_null();
    null_point.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "nullPoint",
        "()LPoint;",
        Some((1, 0, null_point.finish().unwrap())),
    );

    // static Point roundTrip(Point a, Point b, Point c, Point d, Point e) {
    //     Point t = e;
    //     return t;
    // }
    // Slots above 3 use the wide `aload`/`astore` forms.
    let mut round_trip = Assembler::new();
    round_trip.aload(4);
    round_trip.astore(5);
    round_trip.aload(5);
    round_trip.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "roundTrip",
        "(LPoint;LPoint;LPoint;LPoint;LPoint;)LPoint;",
        Some((1, 6, round_trip.finish().unwrap())),
    );

    // static int run() {
    //     Point p = new Point(2, 3);
    //     return p.getX() + p.sum();
    // }
    let mut run = Assembler::new();
    run.new_instance(point_class);
    run.dup();
    run.iconst(2);
    run.iconst(3);
    run.invokespecial(point_init);
    run.astore(0);
    run.aload(0);
    run.invokevirtual(get_x);
    run.aload(0);
    run.invokevirtual(sum);
    run.iadd();
    run.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "run",
        "()I",
        Some((4, 1, run.finish().unwrap())),
    );

    // static Point make() { return new Point(2, 3); }
    let mut make = Assembler::new();
    make.new_instance(point_class);
    make.dup();
    make.iconst(2);
    make.iconst(3);
    make.invokespecial(point_init);
    make.areturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "make",
        "()LPoint;",
        Some((4, 0, make.finish().unwrap())),
    );

    // static int xOf(Point p) { return p.x; }
    let mut x_of = Assembler::new();
    x_of.aload(0);
    x_of.getfield(builder.field_ref("Point", "x", "I"));
    x_of.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "xOf",
        "(LPoint;)I",
        Some((1, 1, x_of.finish().unwrap())),
    );

    // static int badStatic(Point p) { return p.counter; }
    let mut bad_static = Assembler::new();
    bad_static.aload(0);
    bad_static.getfield(counter);
    bad_static.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "badStatic",
        "(LPoint;)I",
        Some((1, 1, bad_static.finish().unwrap())),
    );

    // static int badDescriptor(Point p) { return p.x; } as if x were a long.
    let mut bad_descriptor = Assembler::new();
    bad_descriptor.aload(0);
    bad_descriptor.getfield(wrong_descriptor);
    bad_descriptor.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "badDescriptor",
        "(LPoint;)I",
        Some((1, 1, bad_descriptor.finish().unwrap())),
    );

    // static boolean isRooted(Point p) { return PointProbe.aux(p); }
    let mut is_rooted = Assembler::new();
    is_rooted.aload(0);
    is_rooted.op(0xb8, &aux.to_be_bytes());
    is_rooted.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "isRooted",
        "(LPoint;)Z",
        Some((1, 1, is_rooted.finish().unwrap())),
    );

    // Declared native: the body is the Rust `aux` below, resolved through the registry.
    builder.method(ACC_PUBLIC_STATIC | 0x0100, "aux", "(LPoint;)Z", None);

    builder.build()
}

/// Native probe: is the popped argument still published as a root by the caller frame?
fn aux(
    _shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let Some(Value::Ref(point)) = args.first().copied() else {
        return Err(VmError::Native("aux expects a Point".to_owned()));
    };
    let rooted = thread.frame_roots().contains(&point);
    Ok(Some(Value::Int(i32::from(rooted))))
}

struct Fixture {
    shared: Shared,
    point: ClassId,
    probe: ClassId,
    thread: ThreadState,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    let point = shared
        .define_class(LoaderId::BOOTSTRAP, "Point", &point_bytes())
        .expect("Point defines");
    let probe = shared
        .define_class(LoaderId::BOOTSTRAP, "PointProbe", &probe_bytes())
        .expect("PointProbe defines");
    shared.register_native("PointProbe", "aux", "(LPoint;)Z", aux);
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        point,
        probe,
        thread,
    }
}

fn static_call(
    fixture: &Fixture,
    method: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Option<Value> {
    run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        method,
        descriptor,
        args,
    )
    .unwrap()
}

fn make_point(fixture: &Fixture) -> ObjRef {
    match static_call(fixture, "make", "()LPoint;", Vec::new()) {
        Some(Value::Ref(reference)) => reference,
        other => panic!("make returned {other:?}"),
    }
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

#[test]
fn a_constructor_sets_instance_fields_and_the_probe_returns_the_sum() {
    let fixture = fixture();
    // run() = getX() + sum() = 2 + (2 + 3) = 7.
    let result = static_call(&fixture, "run", "()I", Vec::new());
    assert_eq!(int_value(result), 7);
    assert_eq!(
        fixture.thread.frame_depth(),
        0,
        "every instance frame is popped"
    );
    assert!(fixture.thread.frame_roots().is_empty());
}

#[test]
fn new_allocates_defaults_and_the_constructor_writes_the_runtime_layout() {
    let fixture = fixture();
    let point = make_point(&fixture);

    let layout = fixture
        .shared
        .field_layout(fixture.point)
        .expect("the layout builds");
    // The static `counter` is not an instance slot: superclass prefix (none) + x + y.
    assert_eq!(layout.len(), 2);
    let x_slot = layout.slot("x", "I").expect("x is in the layout");
    let y_slot = layout.slot("y", "I").expect("y is in the layout");

    let heap = fixture.shared.heap();
    let instance = heap.instance(point).expect("is an instance");
    assert_eq!(instance.class, fixture.point);
    assert_eq!(instance.fields[x_slot], Value::Int(2));
    assert_eq!(instance.fields[y_slot], Value::Int(3));
    drop(heap);

    // The same field read through `getfield` inside the probe.
    let value = static_call(&fixture, "xOf", "(LPoint;)I", vec![Value::Ref(point)]);
    assert_eq!(int_value(value), 2);
}

#[test]
fn instance_methods_run_with_this_in_local_zero() {
    let fixture = fixture();
    let point = make_point(&fixture);

    let x = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.point,
        "getX",
        "()I",
        point,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(x, Some(Value::Int(2)));

    let sum = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.point,
        "sum",
        "()I",
        point,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(sum, Some(Value::Int(5)));

    // A null receiver is an error, not a silent call with a null `this`.
    let error = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.point,
        "sum",
        "()I",
        ObjRef::NULL,
        Vec::new(),
    );
    assert!(matches!(error, Err(InterpError::WrongValue)));

    assert_eq!(fixture.thread.frame_depth(), 0);
}

#[test]
fn object_init_is_a_registered_noop_for_a_stub_without_code() {
    let fixture = fixture();
    // The C1 stub declares the constructor but has no `Code` body.
    let object = fixture
        .shared
        .load_library_class("java/lang/Object")
        .unwrap();
    let entry = fixture.shared.entry(object).unwrap();
    let method = entry
        .file
        .find_method("<init>", "()V")
        .unwrap()
        .expect("Object declares <init>()V");
    assert!(!method.is_native());
    assert!(entry.file.code(method).unwrap().is_none());
    assert!(
        fixture
            .shared
            .native("java/lang/Object", "<init>", "()V")
            .is_some(),
        "the no-op is registered"
    );

    // Calling it directly on a Point receiver is the `super()` path: it returns void and
    // changes no field.
    let point = make_point(&fixture);
    let result = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        object,
        "<init>",
        "()V",
        point,
        Vec::new(),
    )
    .unwrap();
    assert_eq!(result, None);

    let layout = fixture.shared.field_layout(fixture.point).unwrap();
    let heap = fixture.shared.heap();
    let instance = heap.instance(point).unwrap();
    assert_eq!(
        instance.fields[layout.slot("x", "I").unwrap()],
        Value::Int(2)
    );
    assert_eq!(
        instance.fields[layout.slot("y", "I").unwrap()],
        Value::Int(3)
    );
}

#[test]
fn a_static_field_through_getfield_is_rejected() {
    let fixture = fixture();
    let point = make_point(&fixture);
    let error = run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        "badStatic",
        "(LPoint;)I",
        vec![Value::Ref(point)],
    );
    assert!(
        matches!(
            error,
            Err(InterpError::StaticFieldAccess { ref name, .. }) if name == "counter"
        ),
        "found {error:?}"
    );
}

#[test]
fn a_wrong_field_descriptor_is_rejected() {
    let fixture = fixture();
    let point = make_point(&fixture);
    let error = run_static_method(
        &fixture.shared,
        &fixture.thread,
        fixture.probe,
        "badDescriptor",
        "(LPoint;)I",
        vec![Value::Ref(point)],
    );
    assert!(
        matches!(
            error,
            Err(InterpError::FieldDescriptorMismatch { ref name, ref found, .. })
                if name == "x" && found == "I"
        ),
        "found {error:?}"
    );
}

#[test]
fn the_receiver_stays_a_root_at_the_native_boundary_and_the_safepoint_is_handled() {
    let fixture = fixture();
    let point = make_point(&fixture);

    fixture.thread.request_safepoint();
    let rooted = static_call(&fixture, "isRooted", "(LPoint;)Z", vec![Value::Ref(point)]);
    assert_eq!(int_value(rooted), 1, "the caller frame published the Point");

    assert!(!fixture.thread.safepoint_requested());
    assert_eq!(fixture.thread.frame_depth(), 0);
    assert!(fixture.thread.frame_roots().is_empty());
}

#[test]
fn aconst_null_and_wide_reference_slots_round_trip() {
    let fixture = fixture();
    assert_eq!(
        static_call(&fixture, "nullPoint", "()LPoint;", Vec::new()),
        Some(Value::Ref(ObjRef::NULL))
    );

    // Slots 4 and 5 force the wide `aload`/`astore` encodings.
    let point = make_point(&fixture);
    let result = static_call(
        &fixture,
        "roundTrip",
        "(LPoint;LPoint;LPoint;LPoint;LPoint;)LPoint;",
        vec![Value::Ref(point); 5],
    );
    assert_eq!(result, Some(Value::Ref(point)));
}
