//! S18 acceptance: the `Unsafe` memory model.
//!
//! The natives are exercised directly (a fixture cannot call `Unsafe.getUnsafe` without the
//! bootstrap loader check): field offsets are the loader's slots, array offsets are byte offsets
//! with `base = 0` and `scale = element width`, and reference arrays are element-addressed.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::native::NativeCtx;
use rava_vm::{Array, ArrayKind, ClassPath, HeapObject, Shared, ThreadState, Value, VmError};

fn jdk_home() -> std::ffi::OsString {
    std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    })
}

fn fixture_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-unsafe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

/// A class with one primitive and one reference field, and a static field to prove statics are
/// not part of the instance layout.
fn build_holder(dir: &Path) {
    let mut builder = ClassBuilder::new("Holder", "java/lang/Object", 65);
    builder.field(0x0002, "n", "I");
    builder.field(0x0002, "s", "Ljava/lang/String;");
    builder.field(0x0009, "STATIC", "I");
    std::fs::write(dir.join("Holder.class"), builder.build()).expect("fixture class");
}

fn setup() -> (ClassPath, Shared, ThreadState, rava_vm::ClassId) {
    let dir = fixture_dir();
    build_holder(&dir);
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), &dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let thread = ThreadState::new();
    let holder = shared.load_class(&path, "Holder").expect("Holder loads");
    (path, shared, thread, holder)
}

/// Invokes a registered native by its `(owner, name, descriptor)` key.
fn call(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    name: &str,
    descriptor: &str,
    args: &[Value],
) -> Result<Value, VmError> {
    let key = rava_vm::native::key("jdk/internal/misc/Unsafe", name, descriptor);
    let function = *shared.natives.get(&key).unwrap_or_else(|| panic!("{key}"));
    let mut ctx = NativeCtx {
        shared,
        thread,
        path,
        args,
    };
    function(&mut ctx)
}

fn array(shared: &mut Shared, kind: ArrayKind, elements: Vec<Value>) -> rava_vm::ObjRef {
    shared.allocate(HeapObject::Array(Array {
        kind,
        elements,
        class_name: None,
    }))
}

#[test]
fn field_offsets_are_the_instance_slots() {
    let (path, mut shared, mut thread, holder) = setup();
    let class = shared.class_object(&path, holder).expect("class object");
    let offset = |shared: &mut Shared, thread: &mut ThreadState, name: &str| -> i64 {
        let name = shared.intern(&path, name).expect("intern");
        match call(
            shared,
            thread,
            &path,
            "objectFieldOffset1",
            "(Ljava/lang/Class;Ljava/lang/String;)J",
            &[Value::NULL, Value::Ref(class), Value::Ref(name)],
        )
        .expect("objectFieldOffset1")
        {
            Value::Long(slot) => slot,
            other => panic!("field offset: {other:?}"),
        }
    };
    let n = offset(&mut shared, &mut thread, "n");
    let s = offset(&mut shared, &mut thread, "s");
    assert_eq!(n, 0, "n is the first layout slot");
    assert_eq!(s, 1, "s is the second layout slot");

    let instance = shared.new_instance(holder);
    call(
        &mut shared,
        &mut thread,
        &path,
        "putInt",
        "(Ljava/lang/Object;JI)V",
        &[
            Value::NULL,
            Value::Ref(instance),
            Value::Long(n),
            Value::Int(42),
        ],
    )
    .expect("putInt");
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            &path,
            "getInt",
            "(Ljava/lang/Object;J)I",
            &[Value::NULL, Value::Ref(instance), Value::Long(n)],
        )
        .expect("getInt"),
        Value::Int(42)
    );

    let text = shared.intern(&path, "hi").expect("intern");
    call(
        &mut shared,
        &mut thread,
        &path,
        "putReference",
        "(Ljava/lang/Object;JLjava/lang/Object;)V",
        &[
            Value::NULL,
            Value::Ref(instance),
            Value::Long(s),
            Value::Ref(text),
        ],
    )
    .expect("putReference");
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            &path,
            "getReference",
            "(Ljava/lang/Object;J)Ljava/lang/Object;",
            &[Value::NULL, Value::Ref(instance), Value::Long(s)],
        )
        .expect("getReference"),
        Value::Ref(text)
    );

    // A field that was never written reads as null, and CAS installs the first value.
    let empty = shared.new_instance(holder);
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            &path,
            "getReference",
            "(Ljava/lang/Object;J)Ljava/lang/Object;",
            &[Value::NULL, Value::Ref(empty), Value::Long(s)],
        )
        .expect("getReference"),
        Value::NULL
    );
    let cas =
        |shared: &mut Shared, thread: &mut ThreadState, expected: Value, new: Value| -> Value {
            call(
                shared,
                thread,
                &path,
                "compareAndSetReference",
                "(Ljava/lang/Object;JLjava/lang/Object;Ljava/lang/Object;)Z",
                &[
                    Value::NULL,
                    Value::Ref(empty),
                    Value::Long(s),
                    expected,
                    new,
                ],
            )
            .expect("compareAndSetReference")
        };
    assert_eq!(
        cas(&mut shared, &mut thread, Value::NULL, Value::Ref(text)),
        Value::Int(1)
    );
    assert_eq!(
        cas(&mut shared, &mut thread, Value::NULL, Value::Ref(text)),
        Value::Int(0)
    );
}

#[test]
fn arrays_are_addressed_by_byte_offsets() {
    let (path, mut shared, mut thread, _holder) = setup();
    let ints = array(
        &mut shared,
        ArrayKind::Int,
        vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
    );
    // Element 2 starts at byte offset 8 in an int[] (scale 4).
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            &path,
            "getInt",
            "(Ljava/lang/Object;J)I",
            &[Value::NULL, Value::Ref(ints), Value::Long(8)],
        )
        .expect("getInt"),
        Value::Int(3)
    );
    call(
        &mut shared,
        &mut thread,
        &path,
        "putInt",
        "(Ljava/lang/Object;JI)V",
        &[Value::NULL, Value::Ref(ints), Value::Long(0), Value::Int(9)],
    )
    .expect("putInt");
    assert_eq!(
        shared.array(ints).expect("array").elements[0],
        Value::Int(9)
    );

    // A byte view: getInt at offset 0 of [1,2,3,4] reads the four bytes in native order.
    let bytes = array(
        &mut shared,
        ArrayKind::Byte,
        vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)],
    );
    let expected = i32::from_ne_bytes([1, 2, 3, 4]);
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            &path,
            "getInt",
            "(Ljava/lang/Object;J)I",
            &[Value::NULL, Value::Ref(bytes), Value::Long(0)],
        )
        .expect("getInt"),
        Value::Int(expected)
    );
    call(
        &mut shared,
        &mut thread,
        &path,
        "putShort",
        "(Ljava/lang/Object;JS)V",
        &[
            Value::NULL,
            Value::Ref(bytes),
            Value::Long(1),
            Value::Int(0x0102),
        ],
    )
    .expect("putShort");
    let elements: Vec<i32> = shared
        .array(bytes)
        .expect("array")
        .elements
        .iter()
        .map(|value| value.as_int().expect("int"))
        .collect();
    assert_eq!(
        elements,
        vec![1, 0x02, 0x01, 4],
        "the short lands across bytes 1 and 2"
    );
}

#[test]
fn reference_arrays_are_element_addressed_and_cas_works() {
    let (path, mut shared, mut thread, _holder) = setup();
    let first = shared.intern(&path, "first").expect("intern");
    let second = shared.intern(&path, "second").expect("intern");
    let refs = array(
        &mut shared,
        ArrayKind::Reference,
        vec![Value::NULL, Value::Ref(first)],
    );
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            &path,
            "getReference",
            "(Ljava/lang/Object;J)Ljava/lang/Object;",
            &[Value::NULL, Value::Ref(refs), Value::Long(1)],
        )
        .expect("getReference"),
        Value::Ref(first)
    );
    let cas = call(
        &mut shared,
        &mut thread,
        &path,
        "compareAndSetReference",
        "(Ljava/lang/Object;JLjava/lang/Object;Ljava/lang/Object;)Z",
        &[
            Value::NULL,
            Value::Ref(refs),
            Value::Long(0),
            Value::NULL,
            Value::Ref(second),
        ],
    )
    .expect("compareAndSetReference");
    assert_eq!(cas, Value::Int(1));
    assert_eq!(
        shared.array(refs).expect("array").elements[0],
        Value::Ref(second)
    );

    // compareAndExchange returns the witness, not the success flag.
    let witness = call(
        &mut shared,
        &mut thread,
        &path,
        "compareAndExchangeReference",
        "(Ljava/lang/Object;JLjava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
        &[
            Value::NULL,
            Value::Ref(refs),
            Value::Long(0),
            Value::NULL,
            Value::Ref(first),
        ],
    )
    .expect("compareAndExchangeReference");
    assert_eq!(witness, Value::Ref(second));
}

#[test]
fn allocate_instance_has_default_fields_and_throw_exception_unwinds() {
    let (path, mut shared, mut thread, holder) = setup();
    let class = shared.class_object(&path, holder).expect("class object");
    let instance = call(
        &mut shared,
        &mut thread,
        &path,
        "allocateInstance",
        "(Ljava/lang/Class;)Ljava/lang/Object;",
        &[Value::NULL, Value::Ref(class)],
    )
    .expect("allocateInstance");
    let Value::Ref(instance) = instance else {
        panic!("{instance:?}");
    };
    assert!(matches!(
        shared.heap[instance.index()],
        HeapObject::Instance(_)
    ));

    let error = call(
        &mut shared,
        &mut thread,
        &path,
        "throwException",
        "(Ljava/lang/Throwable;)V",
        &[Value::NULL, Value::Ref(instance)],
    )
    .expect_err("throwException raises");
    assert!(matches!(error, VmError::Thrown { object } if object == instance));
}
