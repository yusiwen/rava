//! M2.26 acceptance: `instanceof(null)` semantics and the `LinkedHashMap` constructor
//! surface Spring's startup path reaches.
//!
//! The fixes this slice makes are checked through the interpreter, not by calling a Rust
//! helper:
//!
//! - `instanceof` of a null operand pushes 0 for a named class, an interface and an array
//!   (JVMS §6.5), and `checkcast` still passes a null as before;
//! - `java/util/LinkedHashMap.<init>(IFZ)V` exists and its `accessOrder` flag is honoured:
//!   with `true`, a lookup or an update of an existing key moves that entry to the end, so
//!   `keySet()` reports the reordered iteration; with `false` (and for `(I)V`/`(IF)V`) the
//!   insertion order is kept. The adjacent `HashMap(int,float)` constructor is covered too;
//! - the further Spring-rerun fixes: `Class.getDeclaredClasses` over the `InnerClasses`
//!   metadata, and `Comparator.comparingInt`/`reversed` over the native lambda target.
//!
//! The fixtures are assembled in Rust so every path goes through `new`, `invokespecial`,
//! `invokeinterface`, `invokevirtual`, `invokestatic`, `instanceof` and `checkcast`. The
//! map-constructor error cases are called through `run_instance_method` so every invalid
//! argument shape (a NaN load factor included) is checked once; the valid constructor and
//! access-order tests stay on the bytecode path.

use std::path::PathBuf;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError, instance, maps,
    object, reflection, string,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;
/// `ACC_PUBLIC | ACC_INTERFACE | ACC_ABSTRACT`.
const ACC_INTERFACE_ABSTRACT: u16 = 0x0601;
/// The `pop` family has no named assembler helper.
const POP: u8 = 0x57;
/// `invokestatic` has no named assembler helper.
const INVOKESTATIC: u8 = 0xb8;

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

/// `static boolean name(Object value) { return value instanceof <class>; }`.
fn instanceof_method(builder: &mut ClassBuilder, name: &str, class: u16) {
    let mut body = Assembler::new();
    body.aload(0);
    body.instanceof(class);
    body.ireturn();
    static_method(builder, name, "(Ljava/lang/Object;)Z", 1, 1, body);
}

/// `static Object name(Object value) { return (<class>) value; }`.
fn checkcast_method(builder: &mut ClassBuilder, name: &str, class: u16) {
    let mut body = Assembler::new();
    body.aload(0);
    body.checkcast(class);
    body.areturn();
    static_method(
        builder,
        name,
        "(Ljava/lang/Object;)Ljava/lang/Object;",
        1,
        1,
        body,
    );
}

fn animal_bytes() -> Vec<u8> {
    ClassBuilder::new("Animal", "java/lang/Object", 61).build()
}

fn pet_bytes() -> Vec<u8> {
    ClassBuilder::new("Pet", "java/lang/Object", 61)
        .with_access(ACC_INTERFACE_ABSTRACT)
        .build()
}

fn dog_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("Dog", "Animal", 61);
    builder.interface("Pet");
    builder.build()
}

/// A `ToIntFunction` whose `applyAsInt` returns `((String) value).length()`.
fn key_fn_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("KeyFn", "java/lang/Object", 61);
    builder.interface("java/util/function/ToIntFunction");
    let string = builder.class("java/lang/String");
    let length = builder.method_ref("java/lang/String", "length", "()I");
    let mut body = Assembler::new();
    body.aload(1);
    body.checkcast(string);
    body.invokevirtual(length);
    body.ireturn();
    builder.method(
        0x0001,
        "applyAsInt",
        "(Ljava/lang/Object;)I",
        Some((1, 2, body.finish().unwrap())),
    );
    builder.build()
}

/// Build a `keySet()` probe over a `LinkedHashMap` built by `init`.
///
/// The method is `static Object name(String a, String b, String c)`: it creates the map,
/// puts the three arguments as keys, optionally touches the first key again, and returns
/// `map.keySet()`. `touch` is one of the `Touch` variants.
enum Touch {
    Get,
    GetOrDefault,
    Put,
}

fn key_set_method(
    builder: &mut ClassBuilder,
    name: &str,
    owner: &str,
    init: &str,
    load_prefix: impl FnOnce(&mut Assembler),
    touch: Touch,
) {
    let map_class = builder.class(owner);
    let init_ref = builder.method_ref(owner, "<init>", init);
    let put_ref = builder.method_ref(
        owner,
        "put",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let get_ref = builder.method_ref(owner, "get", "(Ljava/lang/Object;)Ljava/lang/Object;");
    let get_or_default_ref = builder.method_ref(
        owner,
        "getOrDefault",
        "(Ljava/lang/Object;Ljava/lang/Object;)Ljava/lang/Object;",
    );
    let key_set_ref = builder.method_ref(owner, "keySet", "()Ljava/util/Set;");

    let mut body = Assembler::new();
    body.new_instance(map_class);
    body.dup();
    load_prefix(&mut body);
    body.invokespecial(init_ref);
    body.astore(3); // the map

    for local in 0..3u8 {
        body.aload(3);
        body.aload(local);
        body.aload(local);
        body.invokevirtual(put_ref);
        body.op(POP, &[]);
    }
    match touch {
        Touch::Get => {
            body.aload(3);
            body.aload(0);
            body.invokevirtual(get_ref);
            body.op(POP, &[]);
        }
        Touch::GetOrDefault => {
            body.aload(3);
            body.aload(0);
            body.aload(1);
            body.invokevirtual(get_or_default_ref);
            body.op(POP, &[]);
        }
        Touch::Put => {
            body.aload(3);
            body.aload(0);
            body.aload(1);
            body.invokevirtual(put_ref);
            body.op(POP, &[]);
        }
    }
    body.aload(3);
    body.invokevirtual(key_set_ref);
    body.areturn();
    static_method(
        builder,
        name,
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;",
        5,
        4,
        body,
    );
}

fn probe_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new("SpringFixesProbe", "java/lang/Object", 61);

    let pet = builder.class("Pet");
    let dog = builder.class("Dog");
    let dog_array = builder.class("[LDog;");
    let int_array = builder.class("[I");
    let object = builder.class("java/lang/Object");

    // The null regression: a class, an interface and an array operand.
    instanceof_method(&mut builder, "isDog", dog);
    instanceof_method(&mut builder, "isPet", pet);
    instanceof_method(&mut builder, "isObject", object);
    instanceof_method(&mut builder, "isDogArray", dog_array);
    instanceof_method(&mut builder, "isIntArray", int_array);

    // A null must still pass every reference cast.
    checkcast_method(&mut builder, "castDog", dog);
    checkcast_method(&mut builder, "castPet", pet);
    checkcast_method(&mut builder, "castDogArray", dog_array);

    // `LinkedHashMap(int,float,boolean)`, the constructor Spring's
    // `CachingMetadataReaderFactory` reaches.
    let load_factor = builder.float(0.75);
    key_set_method(
        &mut builder,
        "accessOrderKeys",
        "java/util/LinkedHashMap",
        "(IFZ)V",
        |body| {
            body.iconst(16);
            body.ldc(load_factor);
            body.iconst(1);
        },
        Touch::Get,
    );
    key_set_method(
        &mut builder,
        "accessOrderPutKeys",
        "java/util/LinkedHashMap",
        "(IFZ)V",
        |body| {
            body.iconst(16);
            body.ldc(load_factor);
            body.iconst(1);
        },
        Touch::Put,
    );
    key_set_method(
        &mut builder,
        "accessOrderGetOrDefaultKeys",
        "java/util/LinkedHashMap",
        "(IFZ)V",
        |body| {
            body.iconst(16);
            body.ldc(load_factor);
            body.iconst(1);
        },
        Touch::GetOrDefault,
    );
    key_set_method(
        &mut builder,
        "insertionOrderKeys",
        "java/util/LinkedHashMap",
        "(IF)V",
        |body| {
            body.iconst(16);
            body.ldc(load_factor);
        },
        Touch::Get,
    );
    key_set_method(
        &mut builder,
        "capacityOnlyKeys",
        "java/util/LinkedHashMap",
        "(I)V",
        |body| {
            body.iconst(16);
        },
        Touch::Get,
    );
    // `Comparator.comparingInt(ToIntFunction)` and the comparator it returns.
    let comparator = builder.class("java/util/Comparator");
    let comparing_int = builder.method_ref(
        "java/util/Comparator",
        "comparingInt",
        "(Ljava/util/function/ToIntFunction;)Ljava/util/Comparator;",
    );
    let comparator_compare = builder.interface_method_ref(
        "java/util/Comparator",
        "compare",
        "(Ljava/lang/Object;Ljava/lang/Object;)I",
    );
    // static Object comparingIntComparator(ToIntFunction fn) {
    //     return Comparator.comparingInt(fn);
    // }
    let mut comparing = Assembler::new();
    comparing.aload(0);
    comparing.op(INVOKESTATIC, &comparing_int.to_be_bytes());
    comparing.areturn();
    static_method(
        &mut builder,
        "comparingIntComparator",
        "(Ljava/util/function/ToIntFunction;)Ljava/util/Comparator;",
        1,
        1,
        comparing,
    );
    // static Object reversedComparator(Object comparator) {
    //     return ((Comparator) comparator).reversed();
    // }
    let comparator_reversed = builder.interface_method_ref(
        "java/util/Comparator",
        "reversed",
        "()Ljava/util/Comparator;",
    );
    let mut reversed = Assembler::new();
    reversed.aload(0);
    reversed.checkcast(comparator);
    reversed.invokeinterface(comparator_reversed, 1);
    reversed.areturn();
    static_method(
        &mut builder,
        "reversedComparator",
        "(Ljava/lang/Object;)Ljava/util/Comparator;",
        1,
        1,
        reversed,
    );
    // static int compareWith(Object comparator, Object a, Object b) {
    //     return ((Comparator) comparator).compare(a, b);
    // }
    let mut compare = Assembler::new();
    compare.aload(0);
    compare.checkcast(comparator);
    compare.aload(1);
    compare.aload(2);
    compare.invokeinterface(comparator_compare, 3);
    compare.ireturn();
    static_method(
        &mut builder,
        "compareWith",
        "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)I",
        3,
        3,
        compare,
    );

    // `Class.getDeclaredClasses()` over a bootstrap class with member classes.
    let locale = builder.class("java/util/Locale");
    let int_primitive = builder.class("int");
    let get_declared_classes = builder.method_ref(
        "java/lang/Class",
        "getDeclaredClasses",
        "()[Ljava/lang/Class;",
    );
    // static Object localeDeclaredClasses() { return java.util.Locale.class.getDeclaredClasses(); }
    let mut declared_classes = Assembler::new();
    declared_classes.ldc(locale);
    declared_classes.invokevirtual(get_declared_classes);
    declared_classes.areturn();
    static_method(
        &mut builder,
        "localeDeclaredClasses",
        "()Ljava/lang/Object;",
        1,
        0,
        declared_classes,
    );
    // static Object primitiveDeclaredClasses() { return int.class.getDeclaredClasses(); }
    let mut primitive_classes = Assembler::new();
    primitive_classes.ldc(int_primitive);
    primitive_classes.invokevirtual(get_declared_classes);
    primitive_classes.areturn();
    static_method(
        &mut builder,
        "primitiveDeclaredClasses",
        "()Ljava/lang/Object;",
        1,
        0,
        primitive_classes,
    );

    // The adjacent `HashMap(int,float)` constructor.
    key_set_method(
        &mut builder,
        "hashMapThresholdKeys",
        "java/util/HashMap",
        "(IF)V",
        |body| {
            body.iconst(16);
            body.ldc(load_factor);
        },
        Touch::Get,
    );

    builder.build()
}

struct Fixture {
    shared: Shared,
    thread: ThreadState,
    probe: ClassId,
    key_fn: ClassId,
    linked_hash_map: ClassId,
}

fn fixture() -> Fixture {
    let shared = Shared::with_library_dir(c1_classes());
    instance::register_natives(&shared);
    object::register_natives(&shared);
    maps::register_natives(&shared);
    reflection::register_natives(&shared);
    string::register_natives(&shared);
    rava_interp::callbacks::register_natives(&shared);
    let define = |name: &str, bytes: Vec<u8>| {
        shared
            .define_class(LoaderId::BOOTSTRAP, name, &bytes)
            .unwrap_or_else(|error| panic!("{name} defines: {error}"))
    };
    define("Animal", animal_bytes());
    define("Pet", pet_bytes());
    // `Animal` and `Pet` must be loaded before `Dog` references them.
    define("Dog", dog_bytes());
    let key_fn = define("KeyFn", key_fn_bytes());
    let probe = define("SpringFixesProbe", probe_bytes());
    let linked_hash_map = shared
        .load_library_class("java/util/LinkedHashMap")
        .expect("the bootstrap library declares java.util.LinkedHashMap");
    let thread = shared.thread_state(1);
    Fixture {
        shared,
        thread,
        probe,
        key_fn,
        linked_hash_map,
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

fn ok_call(fixture: &Fixture, name: &str, descriptor: &str, args: Vec<Value>) -> Option<Value> {
    call(fixture, name, descriptor, args)
        .unwrap_or_else(|error| panic!("{name}{descriptor} runs: {error}"))
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int, found {other:?}"),
    }
}

fn ref_value(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) => reference,
        other => panic!("expected a reference, found {other:?}"),
    }
}

/// Read a `keySet()` result back as text, in the set's iteration order.
fn key_texts(fixture: &Fixture, value: Option<Value>) -> Vec<String> {
    let reference = ref_value(value);
    let heap = fixture.shared.heap();
    let elements = heap
        .set(reference)
        .expect("keySet returns the set shape")
        .to_vec();
    elements
        .into_iter()
        .map(|element| match element {
            Value::Ref(key) => heap.string(key).expect("a key text is a String").to_owned(),
            other => panic!("keySet holds a non-reference, found {other:?}"),
        })
        .collect()
}

fn keys(fixture: &Fixture, method: &str) -> Vec<String> {
    key_texts(
        fixture,
        ok_call(
            fixture,
            method,
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/Object;",
            vec![
                Value::Ref(fixture.shared.intern_string("a")),
                Value::Ref(fixture.shared.intern_string("b")),
                Value::Ref(fixture.shared.intern_string("c")),
            ],
        ),
    )
}

#[test]
fn instanceof_of_null_pushes_zero_for_class_interface_and_array() {
    let fixture = fixture();
    let is = |method: &str| {
        int_value(ok_call(
            &fixture,
            method,
            "(Ljava/lang/Object;)Z",
            vec![Value::Ref(ObjRef::NULL)],
        ))
    };

    assert_eq!(is("isDog"), 0);
    assert_eq!(is("isPet"), 0);
    assert_eq!(is("isObject"), 0);
    assert_eq!(is("isDogArray"), 0);
    assert_eq!(is("isIntArray"), 0);
}

#[test]
fn checkcast_of_null_still_passes_for_class_interface_and_array() {
    let fixture = fixture();
    let cast = |method: &str| {
        ref_value(ok_call(
            &fixture,
            method,
            "(Ljava/lang/Object;)Ljava/lang/Object;",
            vec![Value::Ref(ObjRef::NULL)],
        ))
    };

    assert!(cast("castDog").is_null());
    assert!(cast("castPet").is_null());
    assert!(cast("castDogArray").is_null());
}

#[test]
fn linked_hash_map_access_order_reorders_observed_iteration() {
    let fixture = fixture();

    // With accessOrder = true, reading `a` moves it behind `b` and `c`.
    assert_eq!(keys(&fixture, "accessOrderKeys"), ["b", "c", "a"]);
    // `getOrDefault` is a lookup too.
    assert_eq!(
        keys(&fixture, "accessOrderGetOrDefaultKeys"),
        ["b", "c", "a"]
    );
    // An update of an existing key is an access too.
    assert_eq!(keys(&fixture, "accessOrderPutKeys"), ["b", "c", "a"]);
}

#[test]
fn linked_hash_map_without_access_order_keeps_insertion_order() {
    let fixture = fixture();

    // `(IF)V` never enables access order, so reading `a` changes nothing.
    assert_eq!(keys(&fixture, "insertionOrderKeys"), ["a", "b", "c"]);
    // `(I)V` is the same insertion-ordered map.
    assert_eq!(keys(&fixture, "capacityOnlyKeys"), ["a", "b", "c"]);
    // `HashMap(int,float)` is insertion-ordered.
    assert_eq!(keys(&fixture, "hashMapThresholdKeys"), ["a", "b", "c"]);
}

#[test]
fn linked_hash_map_constructor_checks_capacity_and_load_factor() {
    let fixture = fixture();
    let map = fixture
        .shared
        .allocate_instance(fixture.linked_hash_map)
        .expect("a LinkedHashMap allocates");

    let init = |args: Vec<Value>| {
        run_instance_method(
            &fixture.shared,
            &fixture.thread,
            fixture.linked_hash_map,
            "<init>",
            "(IFZ)V",
            map,
            args,
        )
    };

    // A negative capacity is rejected.
    match init(vec![Value::Int(-1), Value::Float(0.75), Value::Int(1)]) {
        Err(InterpError::Vm(VmError::Native(message))) => {
            assert!(message.contains("negative capacity"), "{message}");
        }
        other => panic!("expected a typed negative-capacity error, found {other:?}"),
    }

    // The JDK rejects a non-positive or NaN load factor; so does this constructor.
    for load_factor in [0.0f32, -0.5, f32::NAN] {
        match init(vec![
            Value::Int(16),
            Value::Float(load_factor),
            Value::Int(1),
        ]) {
            Err(InterpError::Vm(VmError::Native(message))) => {
                assert!(message.contains("not positive"), "{message}");
            }
            other => panic!("expected a typed load-factor error, found {other:?}"),
        }
    }

    // A valid call leaves the receiver empty.
    assert!(init(vec![Value::Int(16), Value::Float(0.75), Value::Int(1)]).is_ok());

    // `HashMap(int,float)` shares the load-factor rule and its `(IF)V` registry entry.
    let hash_map = fixture
        .shared
        .load_library_class("java/util/HashMap")
        .expect("the bootstrap library declares java.util.HashMap");
    let receiver = fixture
        .shared
        .allocate_instance(hash_map)
        .expect("a HashMap allocates");
    match run_instance_method(
        &fixture.shared,
        &fixture.thread,
        hash_map,
        "<init>",
        "(IF)V",
        receiver,
        vec![Value::Int(16), Value::Float(0.0)],
    ) {
        Err(InterpError::Vm(VmError::Native(message))) => {
            assert!(message.contains("not positive"), "{message}");
        }
        other => panic!("expected a typed load-factor error, found {other:?}"),
    }
}

#[test]
fn access_order_flag_is_false_until_the_constructor_sets_it() {
    let fixture = fixture();
    // The allocator starts every map insertion-ordered; only `(IFZ)V` with true flips it.
    let map = fixture
        .shared
        .allocate_instance(fixture.linked_hash_map)
        .expect("a LinkedHashMap allocates");
    assert!(
        !fixture
            .shared
            .heap()
            .map_access_order(map)
            .expect("the receiver is a map")
    );

    let _ = run_instance_method(
        &fixture.shared,
        &fixture.thread,
        fixture.linked_hash_map,
        "<init>",
        "(IFZ)V",
        map,
        vec![Value::Int(16), Value::Float(0.75), Value::Int(1)],
    )
    .expect("the access-order constructor runs");
    assert!(
        fixture
            .shared
            .heap()
            .map_access_order(map)
            .expect("the receiver is a map")
    );
}

#[test]
fn class_get_declared_classes_returns_member_classes() {
    let fixture = fixture();
    let array = ref_value(ok_call(
        &fixture,
        "localeDeclaredClasses",
        "()Ljava/lang/Object;",
        Vec::new(),
    ));
    let names = {
        let heap = fixture.shared.heap();
        heap.array(array)
            .expect("getDeclaredClasses returns a Class[]")
            .elements
            .iter()
            .map(|element| {
                let Value::Ref(object) = element else {
                    panic!("the Class[] holds a non-reference");
                };
                let HeapObject::Class(class) = heap.get(*object).expect("a class object") else {
                    panic!("the Class[] holds a non-Class object");
                };
                fixture
                    .shared
                    .entry(*class)
                    .expect("a member class is loaded")
                    .name
                    .replace('/', ".")
            })
            .collect::<Vec<_>>()
    };
    // `java.util.Locale` declares five member classes; `IsoCountryCode` and
    // `FilteringMode` are outside the C1 manifest subset and are skipped, while the
    // rows for `java.io.ObjectOutputStream$PutField` are not members and are filtered.
    let mut names = names;
    names.sort();
    assert_eq!(
        names,
        [
            "java.util.Locale$Builder",
            "java.util.Locale$Category",
            "java.util.Locale$LanguageRange",
        ]
    );

    // A primitive class has no class file, so it declares no member class.
    let empty = ref_value(ok_call(
        &fixture,
        "primitiveDeclaredClasses",
        "()Ljava/lang/Object;",
        Vec::new(),
    ));
    assert!(
        fixture
            .shared
            .heap()
            .array(empty)
            .expect("a primitive class returns an empty Class[]")
            .elements
            .is_empty()
    );
}

#[test]
fn comparator_comparing_int_compares_the_extracted_keys() {
    let fixture = fixture();
    let key_fn = fixture
        .shared
        .allocate_instance(fixture.key_fn)
        .expect("a KeyFn allocates");
    let comparator = ref_value(ok_call(
        &fixture,
        "comparingIntComparator",
        "(Ljava/util/function/ToIntFunction;)Ljava/util/Comparator;",
        vec![Value::Ref(key_fn)],
    ));
    let compare = |left: &str, right: &str| {
        int_value(ok_call(
            &fixture,
            "compareWith",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)I",
            vec![
                Value::Ref(comparator),
                Value::Ref(fixture.shared.intern_string(left)),
                Value::Ref(fixture.shared.intern_string(right)),
            ],
        ))
    };

    assert_eq!(compare("a", "bbb"), -1);
    assert_eq!(compare("bbbb", "bb"), 1);
    assert_eq!(compare("xx", "yy"), 0);
}

#[test]
fn comparator_reversed_flips_the_comparing_int_order() {
    let fixture = fixture();
    let key_fn = fixture
        .shared
        .allocate_instance(fixture.key_fn)
        .expect("a KeyFn allocates");
    let comparator = ref_value(ok_call(
        &fixture,
        "comparingIntComparator",
        "(Ljava/util/function/ToIntFunction;)Ljava/util/Comparator;",
        vec![Value::Ref(key_fn)],
    ));
    let reversed = ref_value(ok_call(
        &fixture,
        "reversedComparator",
        "(Ljava/lang/Object;)Ljava/util/Comparator;",
        vec![Value::Ref(comparator)],
    ));
    let compare = |left: &str, right: &str| {
        int_value(ok_call(
            &fixture,
            "compareWith",
            "(Ljava/lang/Object;Ljava/lang/Object;Ljava/lang/Object;)I",
            vec![
                Value::Ref(reversed),
                Value::Ref(fixture.shared.intern_string(left)),
                Value::Ref(fixture.shared.intern_string(right)),
            ],
        ))
    };

    assert_eq!(compare("a", "bbb"), 1);
    assert_eq!(compare("bbbb", "bb"), -1);
    assert_eq!(compare("xx", "yy"), 0);
}
