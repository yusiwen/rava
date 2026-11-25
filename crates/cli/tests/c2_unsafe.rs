//! M3.5 acceptance: `sun.misc.Unsafe` U0 and the direct-buffer prerequisite.
//!
//! The committed `demo.UnsafeProbe` fixture is compiled with `javac --release 17` and
//! committed under `tests/fixtures/unsafe/classes`, so this suite needs no JDK, network or
//! files beyond that fixture and `stdlib/c1/classes`.
//!
//! The suite pins the contract Spring's repackaged Objenesis and Tomcat rely on:
//!
//! 1. the private static `sun.misc.Unsafe.theUnsafe` field and `getUnsafe()` are non-null
//!    and reference-identical;
//! 2. `allocateInstance(UnsafeProbe.class)` initializes the class (`marker == 7`) but
//!    skips the constructor (`value == 0`), while the ordinary constructor still sets
//!    `value == 42`;
//! 3. `jdk.internal.misc.Unsafe.shouldBeInitialized` follows the JDK's meaning (`true`
//!    while the class still needs initialization), `ensureClassInitialized` runs the
//!    initializer once, and both are idempotent afterwards;
//! 4. `ByteBuffer.allocateDirect` returns a `java.nio.DirectByteBuffer` whose
//!    `isDirect()` is `true`, and `Unsafe.invokeCleaner` leaves it untouched;
//! 5. the U0 call log contains only the five U0 method names, and an unregistered U1+
//!    method (`objectFieldOffset`) fails with the named "no native implementation" error
//!    without appending a U0 entry;
//! 6. the C1 projection's missing private members do not hide the VM-installed
//!    `theUnsafe`: `Class.getDeclaredField(s)` sees it once, with private-static-final
//!    flags, and static `Field.get`/`set` work (including on `System.out`/`err`, which
//!    stay unduplicated); and
//! 7. `Class.cast` returns an assignable instance, passes null through, and throws a real
//!    `ClassCastException` otherwise — the piece Objenesis needs after `theUnsafe`.

use std::path::PathBuf;
use std::sync::Arc;

use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassState, DirectorySource, LoaderId, ObjRef, Shared, ThreadState, Value, VmError,
    library,
};

const UNSAFE: &str = "sun/misc/Unsafe";
const JDK_UNSAFE: &str = "jdk/internal/misc/Unsafe";
const CLASS: &str = "java/lang/Class";
const BYTE_BUFFER: &str = "java/nio/ByteBuffer";
const DIRECT_BYTE_BUFFER: &str = "java/nio/DirectByteBuffer";
const REFLECT_FIELD: &str = "java/lang/reflect/Field";
const PROBE: &str = "demo/UnsafeProbe";

const SUN_GET_UNSAFE: &str = "()Lsun/misc/Unsafe;";
const JDK_GET_UNSAFE: &str = "()Ljdk/internal/misc/Unsafe;";
const ALLOCATE_INSTANCE: &str = "(Ljava/lang/Class;)Ljava/lang/Object;";
const INVOKE_CLEANER: &str = "(Ljava/nio/ByteBuffer;)V";
const ENSURE_INITIALIZED: &str = "(Ljava/lang/Class;)V";
const SHOULD_BE_INITIALIZED: &str = "(Ljava/lang/Class;)Z";
const OBJECT_FIELD_OFFSET: &str = "(Ljava/lang/reflect/Field;)J";
const ALLOCATE_DIRECT: &str = "(I)Ljava/nio/ByteBuffer;";
const IS_DIRECT: &str = "()Z";
const GET_DECLARED_FIELD: &str = "(Ljava/lang/String;)Ljava/lang/reflect/Field;";

/// The U0 method names. ADR-0005's log must contain no other name.
const U0_METHODS: &[&str] = &[
    "getUnsafe",
    "allocateInstance",
    "invokeCleaner",
    "ensureClassInitialized",
    "shouldBeInitialized",
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn c1_classes() -> PathBuf {
    manifest_dir().join("../../stdlib/c1/classes")
}

fn unsafe_fixture_classes() -> PathBuf {
    manifest_dir().join("tests/fixtures/unsafe/classes")
}

/// A library-backed VM plus the Unsafe fixture as the application loader, with every
/// native these paths need registered.
fn fixture() -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let application = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(unsafe_fixture_classes())),
        )
        .expect("the application loader is added");
    library::register_all(&shared).expect("the class library registers");
    rava_interp::reflect_invoke::register_natives(&shared);
    rava_interp::unsafe_natives::register_natives(&shared).expect("the Unsafe natives register");
    let thread = shared.thread_state(1);
    (shared, application, thread)
}

fn library_class(shared: &Shared, name: &str) -> ClassId {
    shared
        .load_library_class(name)
        .unwrap_or_else(|error| panic!("{name} loads: {error}"))
}

fn probe_class(shared: &Shared, application: LoaderId) -> ClassId {
    shared
        .load_class(application, PROBE)
        .expect("demo/UnsafeProbe loads from the fixture")
}

fn class_object(shared: &Shared, class: ClassId) -> ObjRef {
    shared.class_object(class).expect("the class object exists")
}

fn reference(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        other => panic!("expected a non-null reference result, found {other:?}"),
    }
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

/// `sun.misc.Unsafe.getUnsafe()`.
fn unsafe_instance(shared: &Shared, thread: &ThreadState) -> ObjRef {
    let class = library_class(shared, UNSAFE);
    reference(
        run_static_method(
            shared,
            thread,
            class,
            "getUnsafe",
            SUN_GET_UNSAFE,
            Vec::new(),
        )
        .expect("Unsafe.getUnsafe runs"),
    )
}

/// `jdk.internal.misc.Unsafe.getUnsafe()`.
fn jdk_unsafe_instance(shared: &Shared, thread: &ThreadState) -> ObjRef {
    let class = library_class(shared, JDK_UNSAFE);
    reference(
        run_static_method(
            shared,
            thread,
            class,
            "getUnsafe",
            JDK_GET_UNSAFE,
            Vec::new(),
        )
        .expect("jdk.internal.misc.Unsafe.getUnsafe runs"),
    )
}

/// `Unsafe.allocateInstance(Class)` on `owner`.
fn allocate_instance(
    shared: &Shared,
    thread: &ThreadState,
    owner: &str,
    receiver: ObjRef,
    target: ClassId,
) -> Result<ObjRef, InterpError> {
    let owner = library_class(shared, owner);
    run_instance_method(
        shared,
        thread,
        owner,
        "allocateInstance",
        ALLOCATE_INSTANCE,
        receiver,
        vec![Value::Ref(class_object(shared, target))],
    )
    .map(reference)
}

/// The `int` instance field `name` of `object`.
fn instance_int(shared: &Shared, object: ObjRef, name: &str) -> i32 {
    let class = {
        let heap = shared.heap();
        heap.instance(object)
            .unwrap_or_else(|error| panic!("{object} is an instance: {error}"))
            .class
    };
    let slot = shared
        .field_layout(class)
        .expect("the instance layout is available")
        .slot(name, "I")
        .unwrap_or_else(|| panic!("class {} has no int field {name}", class.0));
    let heap = shared.heap();
    match heap
        .instance(object)
        .expect("a fresh instance")
        .fields
        .get(slot)
        .copied()
    {
        Some(Value::Int(value)) => value,
        other => panic!("field {name} holds {other:?}, not an int"),
    }
}

/// The `int` static field `name`, or `None` when it has no value yet.
fn static_int(shared: &Shared, class: ClassId, name: &str) -> Option<i32> {
    match shared
        .static_field_value(class, name, "I")
        .expect("the static field is readable")
    {
        Some(Value::Int(value)) => Some(value),
        None => None,
        other => panic!("static field {name} holds {other:?}, not an int"),
    }
}

fn class_state(shared: &Shared, class: ClassId) -> ClassState {
    let entry = shared.entry(class).expect("the class is loaded");
    *entry.state.lock().expect("class state lock")
}

#[test]
fn get_unsafe_and_the_static_field_are_reference_identical() {
    let (shared, _application, thread) = fixture();
    let class = library_class(&shared, UNSAFE);

    let from_method = unsafe_instance(&shared, &thread);
    assert!(!from_method.is_null());

    let from_field = match shared
        .static_field_value(class, "theUnsafe", "Lsun/misc/Unsafe;")
        .expect("theUnsafe is readable")
    {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        other => panic!("theUnsafe holds {other:?}, not the installed instance"),
    };
    assert_eq!(
        from_field, from_method,
        "the static field and getUnsafe() are the same object"
    );
    assert_eq!(
        unsafe_instance(&shared, &thread),
        from_method,
        "registration installs exactly one instance"
    );
}

#[test]
fn allocate_instance_initializes_the_class_and_skips_the_constructor() {
    let (shared, application, thread) = fixture();
    let probe = probe_class(&shared, application);
    assert_eq!(
        static_int(&shared, probe, "marker"),
        None,
        "a loaded class has not run its initializer"
    );

    let unsafe_object = unsafe_instance(&shared, &thread);
    let instance = allocate_instance(&shared, &thread, UNSAFE, unsafe_object, probe)
        .expect("Unsafe.allocateInstance returns an object");

    assert_eq!(
        shared
            .heap()
            .instance(instance)
            .expect("an allocated instance")
            .class,
        probe,
        "the object is an UnsafeProbe"
    );
    assert_eq!(
        instance_int(&shared, instance, "value"),
        0,
        "allocateInstance skipped the constructor"
    );
    assert_eq!(
        static_int(&shared, probe, "marker"),
        Some(7),
        "allocateInstance initialized the class first"
    );

    // The ordinary constructor path is unaffected: allocate, then run `<init>()V`.
    let normal = shared
        .allocate_instance(probe)
        .expect("an ordinary instance allocates");
    run_instance_method(&shared, &thread, probe, "<init>", "()V", normal, Vec::new())
        .expect("the no-arg constructor runs");
    assert_eq!(
        instance_int(&shared, normal, "value"),
        42,
        "the constructor still runs normally"
    );
    assert_eq!(
        instance_int(&shared, instance, "value"),
        0,
        "the Unsafe-allocated object never ran the constructor"
    );
}

#[test]
fn the_jdk_initialization_queries_are_idempotent() {
    let (shared, application, thread) = fixture();
    let probe = probe_class(&shared, application);
    let jdk = library_class(&shared, JDK_UNSAFE);
    let unsafe_object = jdk_unsafe_instance(&shared, &thread);
    let probe_object = class_object(&shared, probe);

    // The JDK's meaning: true means the class still needs initialization.
    let before = int_value(
        run_instance_method(
            &shared,
            &thread,
            jdk,
            "shouldBeInitialized",
            SHOULD_BE_INITIALIZED,
            unsafe_object,
            vec![Value::Ref(probe_object)],
        )
        .expect("shouldBeInitialized runs"),
    );
    assert_eq!(before, 1, "the class still needs initialization");
    assert_eq!(static_int(&shared, probe, "marker"), None);
    assert_eq!(class_state(&shared, probe), ClassState::Uninitialized);

    run_instance_method(
        &shared,
        &thread,
        jdk,
        "ensureClassInitialized",
        ENSURE_INITIALIZED,
        unsafe_object,
        vec![Value::Ref(probe_object)],
    )
    .expect("ensureClassInitialized runs");
    assert_eq!(static_int(&shared, probe, "marker"), Some(7));
    assert_eq!(class_state(&shared, probe), ClassState::Initialized);

    let after = int_value(
        run_instance_method(
            &shared,
            &thread,
            jdk,
            "shouldBeInitialized",
            SHOULD_BE_INITIALIZED,
            unsafe_object,
            vec![Value::Ref(probe_object)],
        )
        .expect("shouldBeInitialized runs"),
    );
    assert_eq!(after, 0, "the class is initialized");

    // A second ensure is a no-op: the marker keeps the value the initializer set.
    run_instance_method(
        &shared,
        &thread,
        jdk,
        "ensureClassInitialized",
        ENSURE_INITIALIZED,
        unsafe_object,
        vec![Value::Ref(probe_object)],
    )
    .expect("the second ensureClassInitialized runs");
    assert_eq!(static_int(&shared, probe, "marker"), Some(7));
    assert_eq!(class_state(&shared, probe), ClassState::Initialized);
}

#[test]
fn allocate_direct_returns_a_direct_buffer_and_the_cleaner_is_a_no_op() {
    let (shared, _application, thread) = fixture();
    let byte_buffer = library_class(&shared, BYTE_BUFFER);
    let direct = library_class(&shared, DIRECT_BYTE_BUFFER);

    let buffer = reference(
        run_static_method(
            &shared,
            &thread,
            byte_buffer,
            "allocateDirect",
            ALLOCATE_DIRECT,
            vec![Value::Int(0)],
        )
        .expect("ByteBuffer.allocateDirect(0) runs"),
    );
    assert_eq!(
        shared
            .heap()
            .object_class(buffer)
            .expect("a buffer instance"),
        direct,
        "allocateDirect returns the direct-buffer class"
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                direct,
                "isDirect",
                IS_DIRECT,
                buffer,
                Vec::new(),
            )
            .expect("DirectByteBuffer.isDirect runs")
        ),
        1,
        "a direct buffer is direct"
    );

    // The C1 `Buffer`/`ByteBuffer` stubs project only public members, so the private
    // `capacity`/`position`/`limit` fields do not exist on the instance; capacity 0 is
    // the allocator's default and there is no storage to observe. The direct-buffer
    // contract here is the object identity and `isDirect`, not a capacity accessor.

    // A negative capacity is a typed error, not a negative-sized object.
    match run_static_method(
        &shared,
        &thread,
        byte_buffer,
        "allocateDirect",
        ALLOCATE_DIRECT,
        vec![Value::Int(-1)],
    ) {
        Err(InterpError::Vm(VmError::Native(detail))) => {
            assert!(
                detail.contains("negative capacity"),
                "the error names the capacity: {detail}"
            );
        }
        other => panic!("expected a typed native error, found {other:?}"),
    }

    // invokeCleaner is a no-op for a VM-managed buffer: the same object survives, keeps
    // its class and is still direct.
    let unsafe_object = unsafe_instance(&shared, &thread);
    let unsafe_class = library_class(&shared, UNSAFE);
    run_instance_method(
        &shared,
        &thread,
        unsafe_class,
        "invokeCleaner",
        INVOKE_CLEANER,
        unsafe_object,
        vec![Value::Ref(buffer)],
    )
    .expect("Unsafe.invokeCleaner returns normally");
    assert_eq!(
        shared
            .heap()
            .object_class(buffer)
            .expect("the buffer survives"),
        direct
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                direct,
                "isDirect",
                IS_DIRECT,
                buffer,
                Vec::new(),
            )
            .expect("DirectByteBuffer.isDirect still runs")
        ),
        1,
        "invokeCleaner did not clear the buffer"
    );
}

/// `Class.getDeclaredFields()` as the reflected `Field` objects.
fn declared_fields(shared: &Shared, thread: &ThreadState, class: ClassId) -> Vec<ObjRef> {
    let class_class = library_class(shared, CLASS);
    let array = reference(
        run_instance_method(
            shared,
            thread,
            class_class,
            "getDeclaredFields",
            "()[Ljava/lang/reflect/Field;",
            class_object(shared, class),
            Vec::new(),
        )
        .expect("Class.getDeclaredFields runs"),
    );
    let elements = shared
        .heap()
        .array(array)
        .expect("a Field[]")
        .elements
        .clone();
    elements
        .into_iter()
        .map(|value| match value {
            Value::Ref(reference) if !reference.is_null() => reference,
            other => panic!("a Field[] element is {other:?}, not a Field"),
        })
        .collect()
}

/// The name a reflected `Field` reports.
fn reflected_name(shared: &Shared, field: ObjRef) -> String {
    shared
        .heap()
        .reflect(field)
        .expect("a reflected field")
        .name
        .clone()
}

/// `Field.get(Object)`.
fn field_get(
    shared: &Shared,
    thread: &ThreadState,
    field: ObjRef,
    receiver: Value,
) -> Option<Value> {
    run_instance_method(
        shared,
        thread,
        library_class(shared, REFLECT_FIELD),
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        field,
        vec![receiver],
    )
    .expect("Field.get runs")
}

/// `Field.set(Object, Object)`.
fn field_set(
    shared: &Shared,
    thread: &ThreadState,
    field: ObjRef,
    receiver: Value,
    value: Value,
) -> Result<(), InterpError> {
    run_instance_method(
        shared,
        thread,
        library_class(shared, REFLECT_FIELD),
        "set",
        "(Ljava/lang/Object;Ljava/lang/Object;)V",
        field,
        vec![receiver, value],
    )
    .map(|_| ())
}

/// `Field.getModifiers()`.
fn field_modifiers(shared: &Shared, thread: &ThreadState, field: ObjRef) -> i32 {
    int_value(
        run_instance_method(
            shared,
            thread,
            library_class(shared, REFLECT_FIELD),
            "getModifiers",
            "()I",
            field,
            Vec::new(),
        )
        .expect("Field.getModifiers runs"),
    )
}

/// Count the declared fields called `name`.
fn declared_field_count(
    shared: &Shared,
    thread: &ThreadState,
    class: ClassId,
    name: &str,
) -> usize {
    declared_fields(shared, thread, class)
        .into_iter()
        .filter(|field| reflected_name(shared, *field) == name)
        .count()
}

/// `Class.getDeclaredField(String)`.
fn declared_field(shared: &Shared, thread: &ThreadState, class: ClassId, name: &str) -> ObjRef {
    let class_class = library_class(shared, CLASS);
    let name = shared.intern_string(name);
    reference(
        run_instance_method(
            shared,
            thread,
            class_class,
            "getDeclaredField",
            GET_DECLARED_FIELD,
            class_object(shared, class),
            vec![Value::Ref(name)],
        )
        .expect("Class.getDeclaredField runs"),
    )
}

#[test]
fn the_unsafe_static_is_reflection_visible() {
    let (shared, application, thread) = fixture();
    let unsafe_class = library_class(&shared, UNSAFE);
    let unsafe_object = unsafe_instance(&shared, &thread);
    let unsafe_class_object = class_object(&shared, unsafe_class);

    // Objenesis' exact shape: Unsafe.class.getDeclaredField("theUnsafe"),
    // setAccessible(true), get(null).
    let field = declared_field(&shared, &thread, unsafe_class, "theUnsafe");
    run_instance_method(
        &shared,
        &thread,
        library_class(&shared, REFLECT_FIELD),
        "setAccessible",
        "(Z)V",
        field,
        vec![Value::Int(1)],
    )
    .expect("Field.setAccessible(true) runs");
    let read = reference(field_get(&shared, &thread, field, Value::Ref(ObjRef::NULL)));
    assert_eq!(
        read, unsafe_object,
        "Field.get(null) returns the object getUnsafe() returns"
    );
    match shared
        .static_field_value(unsafe_class, "theUnsafe", "Lsun/misc/Unsafe;")
        .expect("theUnsafe is readable")
    {
        Some(Value::Ref(installed)) => assert_eq!(installed, read),
        other => panic!("theUnsafe holds {other:?}, not a reference"),
    }

    // The synthesized declaration carries the JDK's private static final flags, its
    // descriptor resolves to sun.misc.Unsafe, and its declaring class is the receiver.
    assert_eq!(
        field_modifiers(&shared, &thread, field),
        0x001A,
        "an installed static is private static final"
    );
    let field_type = reference(
        run_instance_method(
            &shared,
            &thread,
            library_class(&shared, REFLECT_FIELD),
            "getType",
            "()Ljava/lang/Class;",
            field,
            Vec::new(),
        )
        .expect("Field.getType runs"),
    );
    assert_eq!(field_type, unsafe_class_object);
    let declaring = reference(
        run_instance_method(
            &shared,
            &thread,
            library_class(&shared, REFLECT_FIELD),
            "getDeclaringClass",
            "()Ljava/lang/Class;",
            field,
            Vec::new(),
        )
        .expect("Field.getDeclaringClass runs"),
    );
    assert_eq!(declaring, unsafe_class_object);

    // getDeclaredFields lists the installed field exactly once, and it is the same
    // cached Field object the named query returned.
    let listed: Vec<ObjRef> = declared_fields(&shared, &thread, unsafe_class)
        .into_iter()
        .filter(|candidate| reflected_name(&shared, *candidate) == "theUnsafe")
        .collect();
    assert_eq!(listed.len(), 1, "theUnsafe appears exactly once");
    assert_eq!(
        listed[0], field,
        "the named and array queries share a Field"
    );

    // A static the metadata already declares is not duplicated by its installed value.
    let system = library_class(&shared, "java/lang/System");
    assert_eq!(declared_field_count(&shared, &thread, system, "out"), 1);
    assert_eq!(declared_field_count(&shared, &thread, system, "err"), 1);
    let out = declared_field(&shared, &thread, system, "out");
    assert_eq!(
        field_modifiers(&shared, &thread, out),
        0x0019,
        "System.out keeps its declared public static final flags"
    );
    let out_value = reference(field_get(&shared, &thread, out, Value::Ref(ObjRef::NULL)));
    match shared
        .static_field_value(system, "out", "Ljava/io/PrintStream;")
        .expect("System.out is readable")
    {
        Some(Value::Ref(stream)) => assert_eq!(stream, out_value),
        other => panic!("System.out holds {other:?}, not a reference"),
    }

    // A non-null receiver for a static field is a typed error.
    match run_instance_method(
        &shared,
        &thread,
        library_class(&shared, REFLECT_FIELD),
        "get",
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        field,
        vec![Value::Ref(unsafe_object)],
    ) {
        Err(InterpError::Vm(VmError::Native(detail))) => assert!(
            detail.contains("null receiver"),
            "the error names the receiver rule: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }

    // A static Field.set round-trip on a declared static: initialize UnsafeProbe first so
    // the class has an observable marker, then write and read it through reflection.
    let probe = probe_class(&shared, application);
    let _ = allocate_instance(&shared, &thread, UNSAFE, unsafe_object, probe)
        .expect("Unsafe.allocateInstance initializes the probe");
    assert_eq!(static_int(&shared, probe, "marker"), Some(7));
    let marker = declared_field(&shared, &thread, probe, "marker");
    field_set(
        &shared,
        &thread,
        marker,
        Value::Ref(ObjRef::NULL),
        Value::Ref(shared.box_int(11)),
    )
    .expect("Field.set(null, 11) runs");
    assert_eq!(static_int(&shared, probe, "marker"), Some(11));
    let read_back = reference(field_get(
        &shared,
        &thread,
        marker,
        Value::Ref(ObjRef::NULL),
    ));
    assert_eq!(
        shared.unbox_int(read_back).expect("a boxed Integer"),
        11,
        "Field.get(null) boxes the static value it just wrote"
    );
}

#[test]
fn class_cast_returns_the_instance_or_throws_class_cast_exception() {
    let (shared, application, thread) = fixture();
    let probe = probe_class(&shared, application);
    let instance = shared
        .allocate_instance(probe)
        .expect("an UnsafeProbe instance allocates");
    let probe_object = class_object(&shared, probe);
    let class_class = library_class(&shared, CLASS);
    let cast_descriptor = "(Ljava/lang/Object;)Ljava/lang/Object;";

    let cast = |argument: Value| {
        run_instance_method(
            &shared,
            &thread,
            class_class,
            "cast",
            cast_descriptor,
            probe_object,
            vec![argument],
        )
    };
    assert_eq!(
        reference(cast(Value::Ref(instance)).expect("Class.cast(instance) runs")),
        instance,
        "an assignable argument is returned unchanged"
    );
    assert_eq!(
        cast(Value::Ref(ObjRef::NULL)).expect("Class.cast(null) runs"),
        Some(Value::Ref(ObjRef::NULL)),
        "null passes through"
    );
    match cast(Value::Ref(shared.intern_string("not a probe"))) {
        Err(InterpError::Thrown(throwable)) => {
            let class_object = shared
                .get_class(throwable)
                .expect("the throwable's class object");
            let class = shared
                .heap()
                .class(class_object)
                .expect("the throwable's class");
            assert_eq!(
                shared.entry(class).expect("the throwable's entry").name,
                "java/lang/ClassCastException"
            );
        }
        other => panic!("expected a ClassCastException, found {other:?}"),
    }
}

#[test]
fn the_call_log_contains_only_u0_methods_and_u1_fails_loudly() {
    let (shared, application, thread) = fixture();
    let unsafe_object = unsafe_instance(&shared, &thread);
    let jdk = jdk_unsafe_instance(&shared, &thread);
    let probe = probe_class(&shared, application);
    let probe_object = class_object(&shared, probe);
    let jdk_class = library_class(&shared, JDK_UNSAFE);

    // Exercise every U0 entry on both owners.
    let _ = allocate_instance(&shared, &thread, UNSAFE, unsafe_object, probe)
        .expect("sun allocateInstance runs");
    run_instance_method(
        &shared,
        &thread,
        jdk_class,
        "shouldBeInitialized",
        SHOULD_BE_INITIALIZED,
        jdk,
        vec![Value::Ref(probe_object)],
    )
    .expect("jdk shouldBeInitialized runs");
    run_instance_method(
        &shared,
        &thread,
        jdk_class,
        "ensureClassInitialized",
        ENSURE_INITIALIZED,
        jdk,
        vec![Value::Ref(probe_object)],
    )
    .expect("jdk ensureClassInitialized runs");
    let _ = allocate_instance(&shared, &thread, JDK_UNSAFE, jdk, probe)
        .expect("jdk allocateInstance runs");
    let byte_buffer = library_class(&shared, BYTE_BUFFER);
    let buffer = reference(
        run_static_method(
            &shared,
            &thread,
            byte_buffer,
            "allocateDirect",
            ALLOCATE_DIRECT,
            vec![Value::Int(0)],
        )
        .expect("allocateDirect runs"),
    );
    for (owner, receiver) in [(UNSAFE, unsafe_object), (JDK_UNSAFE, jdk)] {
        run_instance_method(
            &shared,
            &thread,
            library_class(&shared, owner),
            "invokeCleaner",
            INVOKE_CLEANER,
            receiver,
            vec![Value::Ref(buffer)],
        )
        .expect("invokeCleaner runs");
    }

    let calls = shared.unsafe_calls();
    assert!(!calls.is_empty(), "the U0 calls were recorded");
    for call in &calls {
        let (owner, remainder) = call
            .split_once('.')
            .unwrap_or_else(|| panic!("call log entry {call} has no owner"));
        assert!(
            owner == UNSAFE || owner == JDK_UNSAFE,
            "unexpected Unsafe owner in {call}"
        );
        let name = remainder
            .split_once('(')
            .unwrap_or_else(|| panic!("call log entry {call} has no descriptor"))
            .0;
        assert!(
            U0_METHODS.contains(&name),
            "call log contains a non-U0 method: {call}"
        );
    }
    for expected in U0_METHODS {
        assert!(
            calls
                .iter()
                .any(|call| call.contains(&format!(".{expected}("))),
            "the log records {expected}: {calls:?}"
        );
    }

    // A U1+ method is declared but never registered: the call fails by name and leaves
    // no U0 entry. `objectFieldOffset` is the representative U1 tripwire.
    let field = declared_field(&shared, &thread, probe, "value");
    let before = calls.len();
    match run_instance_method(
        &shared,
        &thread,
        jdk_class,
        "objectFieldOffset",
        OBJECT_FIELD_OFFSET,
        jdk,
        vec![Value::Ref(field)],
    ) {
        Err(InterpError::Decode(detail)) => assert!(
            detail.contains("no native implementation"),
            "the error is the named missing-native error: {detail}"
        ),
        other => panic!("expected a missing-native error, found {other:?}"),
    }
    assert_eq!(
        shared.unsafe_calls().len(),
        before,
        "a U1+ call appends no U0 log entry"
    );
}
