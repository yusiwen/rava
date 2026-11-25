//! Issue #52 acceptance: `long`/`double` locals occupy two JVM slots.
//!
//! The committed `demo.SlotLayoutProbe` fixture (`javac --release 17`, see
//! `tests/fixtures/slot_layout/README.md`) puts a wide parameter before a narrower one, so
//! a frame that copies the argument values compactly loads the wrong slot. Its expected
//! values are the ones stock JDK 17 prints.
//!
//! The receiver occupies local 0 of an instance method, each parameter starts at the next
//! JVM slot, and the second slot of a `long`/`double` is a hole. A wide store reserves the
//! following slot the same way. A load of a hole is a typed [`InterpError::LocalHole`],
//! never a stale neighbour.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{ClassId, DirectorySource, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

const PROBE: &str = "demo/SlotLayoutProbe";
const ACC_PUBLIC_STATIC: u16 = 0x0009;
const ACC_PUBLIC_STATIC_NATIVE: u16 = 0x0109;

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn fixture_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/slot_layout/classes")
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    let loader = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(fixture_classes())),
        )
        .expect("the fixture loader is added");
    let probe = shared
        .load_class(loader, PROBE)
        .expect("demo/SlotLayoutProbe loads");
    Fixture {
        thread: shared.thread_state(1),
        shared,
        probe,
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

#[test]
fn an_int_parameter_after_a_long_reads_its_jvm_slot() {
    let fixture = fixture();

    // `shift(long value, int distance)` is `(JI)J`: the long spans locals 0..=1 and the
    // distance is local 2. Compact binding would leave slot 2 holding 0 and return 1.
    assert_eq!(
        call(
            &fixture,
            "shift",
            "(JI)J",
            vec![Value::Long(1), Value::Int(40)]
        )
        .expect("shift(1, 40) runs"),
        Some(Value::Long(1_i64 << 40))
    );
    assert_eq!(
        call(
            &fixture,
            "shift",
            "(JI)J",
            vec![Value::Long(3), Value::Int(2)]
        )
        .expect("shift(3, 2) runs"),
        Some(Value::Long(12))
    );
}

#[test]
fn a_long_parameter_after_a_double_reads_its_jvm_slot() {
    let fixture = fixture();

    // `scale(long value, double factor)` is `(JD)I`: the double occupies locals 2..=3.
    assert_eq!(
        call(
            &fixture,
            "scale",
            "(JD)I",
            vec![Value::Long(4), Value::Double(2.5)]
        )
        .expect("scale(4, 2.5) runs"),
        Some(Value::Int(10))
    );
    // The Java cast truncates toward zero: -4.5 becomes -4.
    assert_eq!(
        call(
            &fixture,
            "scale",
            "(JD)I",
            vec![Value::Long(3), Value::Double(-1.5)]
        )
        .expect("scale(3, -1.5) runs"),
        Some(Value::Int(-4))
    );
}

#[test]
fn a_float_and_an_int_after_a_long_read_their_jvm_slots() {
    let fixture = fixture();

    // `blend(long value, float scale, int extra)` is `(JFI)J`: locals 0..=1, 2 and 3.
    assert_eq!(
        call(
            &fixture,
            "blend",
            "(JFI)J",
            vec![Value::Long(10), Value::Float(2.5), Value::Int(4)]
        )
        .expect("blend(10, 2.5, 4) runs"),
        Some(Value::Long(20))
    );
    assert_eq!(
        call(
            &fixture,
            "blend",
            "(JFI)J",
            vec![Value::Long(1), Value::Float(0.5), Value::Int(3)]
        )
        .expect("blend(1, 0.5, 3) runs"),
        Some(Value::Long(2))
    );
}

#[test]
fn an_instance_receiver_occupies_local_zero() {
    let fixture = fixture();
    let receiver = fixture
        .shared
        .allocate_instance(fixture.probe)
        .expect("the probe allocates");
    let call_instance = |args: Vec<Value>| {
        run_instance_method(
            &fixture.shared,
            &fixture.thread,
            fixture.probe,
            "instanceShift",
            "(JI)J",
            receiver,
            args,
        )
    };

    // `this` is local 0, so the long parameter spans 1..=2 and the distance is local 3.
    assert_eq!(
        call_instance(vec![Value::Long(1), Value::Int(40)]).expect("instanceShift(1, 40) runs"),
        Some(Value::Long(1_i64 << 40))
    );
    assert_eq!(
        call_instance(vec![Value::Long(5), Value::Int(3)]).expect("instanceShift(5, 3) runs"),
        Some(Value::Long(40))
    );
}

#[test]
fn a_wrong_argument_count_is_a_typed_error() {
    let fixture = fixture();

    assert!(matches!(
        call(&fixture, "shift", "(JI)J", vec![Value::Long(1)]),
        Err(InterpError::ArgumentCount {
            expected: 2,
            found: 1
        })
    ));
    assert!(matches!(
        call(
            &fixture,
            "shift",
            "(JI)J",
            vec![Value::Long(1), Value::Int(40), Value::Int(0)]
        ),
        Err(InterpError::ArgumentCount {
            expected: 2,
            found: 3
        })
    ));
}

#[test]
fn a_direct_load_of_a_holes_second_slot_is_rejected() {
    // Valid class files never load the second half of a wide local, so the probe is built
    // here: `(JI)J` with `lload 0; iload 1` reads the hole instead of the int at local 2.
    let mut builder = ClassBuilder::new("HoleProbe", "java/lang/Object", 61);
    let mut body = Assembler::new();
    body.lload(0);
    body.iload(1);
    body.op(0x79, &[]); // lshl
    body.lreturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "brokenShift",
        "(JI)J",
        Some((3, 3, body.finish().unwrap())),
    );

    let shared = Shared::with_library_dir(c1_classes());
    let class = shared
        .define_class(LoaderId::BOOTSTRAP, "HoleProbe", &builder.build())
        .expect("HoleProbe defines");
    let thread = shared.thread_state(1);
    let result = run_static_method(
        &shared,
        &thread,
        class,
        "brokenShift",
        "(JI)J",
        vec![Value::Long(1), Value::Int(40)],
    );

    assert!(
        matches!(result, Err(InterpError::LocalHole(1))),
        "expected a typed local-hole error, found {result:?}"
    );
}

/// A class with wide stores followed by a one-slot load of the reserved slot, plus a
/// native probe that reports the active frame's root count.
fn wide_store_probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("WideStoreProbe", "java/lang/Object", 61);

    // static int lstoreHole() { lconst_0; lstore 0; iload 1; ireturn; }
    let mut lstore = Assembler::new();
    lstore.op(0x09, &[]); // lconst_0
    lstore.op(0x37, &[0]); // lstore 0: slot 1 is reserved
    lstore.iload(1);
    lstore.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "lstoreHole",
        "()I",
        Some((2, 2, lstore.finish().unwrap())),
    );

    // static int dstoreHole() { dconst_0; dstore 0; iload 1; ireturn; }
    let mut dstore = Assembler::new();
    dstore.op(0x0e, &[]); // dconst_0
    dstore.op(0x39, &[0]); // dstore 0: slot 1 is reserved
    dstore.iload(1);
    dstore.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "dstoreHole",
        "()I",
        Some((2, 2, dstore.finish().unwrap())),
    );

    // static int wideStoreClearsRoot(String value) {
    //     aload_0; astore_1; lconst_0; lstore 0; invokestatic probe; ireturn;
    // }
    let probe = builder.method_ref("WideStoreProbe", "probe", "()I");
    let mut roots = Assembler::new();
    roots.aload(0);
    roots.astore(1);
    roots.op(0x09, &[]); // lconst_0
    roots.op(0x37, &[0]); // lstore 0: slot 1 becomes a hole, not a reference root
    roots.op(0xb8, &probe.to_be_bytes()); // invokestatic probe
    roots.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "wideStoreClearsRoot",
        "(Ljava/lang/String;)I",
        Some((2, 2, roots.finish().unwrap())),
    );

    builder.method(ACC_PUBLIC_STATIC_NATIVE, "probe", "()I", None);
    builder.build()
}

fn wide_store_fixture() -> (Shared, ClassId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let class = shared
        .define_class(
            LoaderId::BOOTSTRAP,
            "WideStoreProbe",
            &wide_store_probe_bytes(),
        )
        .expect("WideStoreProbe defines");
    let thread = shared.thread_state(1);
    (shared, class, thread)
}

/// Report how many roots the active frame has published at the native boundary.
fn probe_roots(
    _shared: &Shared,
    thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(thread.frame_roots().len() as i32)))
}

#[test]
fn a_wide_store_reserves_the_next_slot_for_a_load() {
    let (shared, class, thread) = wide_store_fixture();

    for name in ["lstoreHole", "dstoreHole"] {
        let result = run_static_method(&shared, &thread, class, name, "()I", Vec::new());
        assert!(
            matches!(result, Err(InterpError::LocalHole(1))),
            "{name} should read the hole at local 1, found {result:?}"
        );
    }
}

#[test]
fn a_wide_store_is_not_a_reference_root() {
    let (shared, class, thread) = wide_store_fixture();
    shared.register_native("WideStoreProbe", "probe", "()I", probe_roots);
    let root = shared.heap().allocate_string("root");

    let result = run_static_method(
        &shared,
        &thread,
        class,
        "wideStoreClearsRoot",
        "(Ljava/lang/String;)I",
        vec![Value::Ref(root)],
    )
    .expect("wideStoreClearsRoot runs");

    assert_eq!(
        result,
        Some(Value::Int(0)),
        "the wide store's second slot is a hole, not a reference root"
    );
}
