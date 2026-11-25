//! M3.4 acceptance: `java.lang.invoke.MethodHandles.Lookup` and `Lookup.defineClass`.
//!
//! The committed `demo.LookupProbe` fixture is compiled with `javac --release 17` and
//! committed under `tests/fixtures/lookup/classes`, so this suite needs no JDK, network or
//! files beyond that fixture and `stdlib/c1/classes`. Generated classes are assembled in
//! process with [`ClassBuilder`] + [`Assembler`].
//!
//! The suite covers the shape CGLIB's `org.springframework.cglib.core.ReflectUtils`
//! relies on:
//!
//! 1. the Java fixture takes a private lookup on `demo.LookupProbe`, resolves `add` and
//!    `echo` through it and prints one fact per line;
//! 2. the same path driven from Rust builds a `Class[]`, a `MethodType` and the handle,
//!    and invokes it with boxed arguments;
//! 3. `Lookup.defineClass` defines a generated `demo/Generated` in the lookup class's
//!    loader, runs it, and rejects a second definition without changing the class table;
//! 4. the reflective bootstrap is mirrored exactly: `Class.getMethod("privateLookupIn",
//!    ...)` and `Lookup.class.getMethod("defineClass", byte[].class)` invoked through
//!    `Method.invoke`;
//! 5. a package mismatch and a wrong find are typed errors that leave no class behind.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassRef, DirectorySource, LoaderId, ObjRef, PrimitiveKind, Shared, ThreadState,
    Value, VmError, library,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

const METHOD_HANDLES: &str = "java/lang/invoke/MethodHandles";
const LOOKUP: &str = "java/lang/invoke/MethodHandles$Lookup";
const METHOD_TYPE: &str = "java/lang/invoke/MethodType";
const METHOD_HANDLE: &str = "java/lang/invoke/MethodHandle";
const CLASS: &str = "java/lang/Class";
const REFLECT_METHOD: &str = "java/lang/reflect/Method";

/// The handle type's parameter/return descriptors, for `toMethodDescriptorString`.
const TO_DESCRIPTOR: &str = "()Ljava/lang/String;";
const INVOKE_WITH_ARGUMENTS: &str = "([Ljava/lang/Object;)Ljava/lang/Object;";
const METHOD_INVOKE: &str = "(Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;";
const CLASS_GET_METHOD: &str = "(Ljava/lang/String;[Ljava/lang/Class;)Ljava/lang/reflect/Method;";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn c1_classes() -> PathBuf {
    manifest_dir().join("../../stdlib/c1/classes")
}

fn lookup_fixture_classes() -> PathBuf {
    manifest_dir().join("tests/fixtures/lookup/classes")
}

/// A library-backed VM plus the lookup fixture as the application loader, with every native
/// the method-handle path needs registered.
fn fixture() -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let application = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(lookup_fixture_classes())),
        )
        .expect("the application loader is added");
    library::register_all(&shared).expect("the class library registers");
    rava_interp::reflect_invoke::register_natives(&shared);
    rava_interp::method_handles::register_natives(&shared);
    // Load every class the method-handle path resolves up front, so a class-count
    // assertion measures the generated class and not an incidental first load.
    for name in [
        "java/lang/Object",
        "java/lang/Class",
        "java/lang/String",
        "java/lang/invoke/MethodHandles",
        "java/lang/invoke/MethodHandles$Lookup",
        "java/lang/invoke/MethodType",
        "java/lang/invoke/MethodHandle",
        "java/lang/reflect/Method",
        "java/lang/NoSuchMethodException",
    ] {
        shared
            .load_library_class(name)
            .unwrap_or_else(|error| panic!("{name} loads: {error}"));
    }
    let thread = shared.thread_state(1);
    (shared, application, thread)
}

fn library_class(shared: &Shared, name: &str) -> ClassId {
    shared
        .load_library_class(name)
        .unwrap_or_else(|error| panic!("{name} loads: {error}"))
}

fn class_id(shared: &Shared, object: ObjRef) -> ClassId {
    shared
        .heap()
        .class(object)
        .expect("the result is a class object")
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn reference(value: Option<Value>) -> ObjRef {
    match value {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        other => panic!("expected a reference result, found {other:?}"),
    }
}

/// A heap `Class[]` holding the given class objects (a loaded class, a primitive or an
/// array class).
fn class_array(shared: &Shared, objects: &[ObjRef]) -> ObjRef {
    let class_class = library_class(shared, CLASS);
    let array = shared
        .heap()
        .allocate_array(ClassRef::Named(class_class), objects.len());
    {
        let mut heap = shared.heap();
        let elements = heap.array_mut(array).expect("a freshly allocated Class[]");
        for (slot, object) in elements.elements.iter_mut().zip(objects) {
            *slot = Value::Ref(*object);
        }
    }
    array
}

/// A heap `Object[]` holding the given values, the shape a varargs call site produces.
fn object_array(shared: &Shared, values: Vec<Value>) -> ObjRef {
    let object_class = library_class(shared, "java/lang/Object");
    let array = shared
        .heap()
        .allocate_array(ClassRef::Named(object_class), values.len());
    {
        let mut heap = shared.heap();
        let elements = heap.array_mut(array).expect("a freshly allocated Object[]");
        for (slot, value) in elements.elements.iter_mut().zip(values) {
            *slot = value;
        }
    }
    array
}

/// A heap `byte[]` holding `bytes`, the shape `javac`'s `new byte[]{...}` produces.
fn byte_array(shared: &Shared, bytes: &[u8]) -> ObjRef {
    let array = shared
        .heap()
        .allocate_array(ClassRef::Primitive(PrimitiveKind::Byte), bytes.len());
    {
        let mut heap = shared.heap();
        let elements = heap.array_mut(array).expect("a freshly allocated byte[]");
        for (slot, byte) in elements.elements.iter_mut().zip(bytes) {
            *slot = Value::Int(i32::from(*byte));
        }
    }
    array
}

/// `demo/Generated`: `public static int answer() { return <answer>; }`.
fn generated_class(name: &str, answer: i32) -> Vec<u8> {
    let mut builder = ClassBuilder::new(name, "java/lang/Object", 61);
    let mut body = Assembler::new();
    body.iconst(answer);
    body.ireturn();
    builder.method(
        ACC_PUBLIC_STATIC,
        "answer",
        "()I",
        Some((1, 0, body.finish().expect("assemble answer"))),
    );
    builder.build()
}

/// `MethodHandles.lookup()`.
fn method_handles_lookup(shared: &Shared, thread: &ThreadState) -> ObjRef {
    let handles = library_class(shared, METHOD_HANDLES);
    reference(
        run_static_method(
            shared,
            thread,
            handles,
            "lookup",
            "()Ljava/lang/invoke/MethodHandles$Lookup;",
            Vec::new(),
        )
        .expect("MethodHandles.lookup runs"),
    )
}

/// `MethodHandles.privateLookupIn(Class, Lookup)`.
fn private_lookup_in(
    shared: &Shared,
    thread: &ThreadState,
    target: ObjRef,
    lookup: ObjRef,
) -> Result<ObjRef, InterpError> {
    let handles = library_class(shared, METHOD_HANDLES);
    run_static_method(
        shared,
        thread,
        handles,
        "privateLookupIn",
        "(Ljava/lang/Class;Ljava/lang/invoke/MethodHandles$Lookup;)Ljava/lang/invoke/MethodHandles$Lookup;",
        vec![Value::Ref(target), Value::Ref(lookup)],
    )
    .map(reference)
}

/// `MethodType.methodType(Class, Class, Class[])`: a return type and the parameter types.
fn method_type(
    shared: &Shared,
    thread: &ThreadState,
    return_type: ObjRef,
    parameters: &[ObjRef],
) -> ObjRef {
    let method_type_class = library_class(shared, METHOD_TYPE);
    let first = *parameters.first().expect("a first parameter type");
    let rest = class_array(shared, &parameters[1..]);
    reference(
        run_static_method(
            shared,
            thread,
            method_type_class,
            "methodType",
            "(Ljava/lang/Class;Ljava/lang/Class;[Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            vec![Value::Ref(return_type), Value::Ref(first), Value::Ref(rest)],
        )
        .expect("MethodType.methodType runs"),
    )
}

/// `MethodType.methodType(Class)`.
fn method_type_of_return(shared: &Shared, thread: &ThreadState, return_type: ObjRef) -> ObjRef {
    let method_type_class = library_class(shared, METHOD_TYPE);
    reference(
        run_static_method(
            shared,
            thread,
            method_type_class,
            "methodType",
            "(Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            vec![Value::Ref(return_type)],
        )
        .expect("MethodType.methodType(Class) runs"),
    )
}

/// `Lookup.findStatic(Class, String, MethodType)`.
fn find_static(
    shared: &Shared,
    thread: &ThreadState,
    lookup: ObjRef,
    owner: ObjRef,
    name: &str,
    method_type: ObjRef,
) -> Result<ObjRef, InterpError> {
    let lookup_class = library_class(shared, LOOKUP);
    let name = shared.intern_string(name);
    run_instance_method(
        shared,
        thread,
        lookup_class,
        "findStatic",
        "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
        lookup,
        vec![
            Value::Ref(owner),
            Value::Ref(name),
            Value::Ref(method_type),
        ],
    )
    .map(reference)
}

/// `MethodHandle.invokeWithArguments(Object[])`.
fn invoke_with_arguments(
    shared: &Shared,
    thread: &ThreadState,
    handle: ObjRef,
    arguments: ObjRef,
) -> Result<Option<Value>, InterpError> {
    let handle_class = library_class(shared, METHOD_HANDLE);
    run_instance_method(
        shared,
        thread,
        handle_class,
        "invokeWithArguments",
        INVOKE_WITH_ARGUMENTS,
        handle,
        vec![Value::Ref(arguments)],
    )
}

/// `MethodType.toMethodDescriptorString()`.
fn descriptor_of(shared: &Shared, thread: &ThreadState, method_type: ObjRef) -> String {
    let method_type_class = library_class(shared, METHOD_TYPE);
    let descriptor = reference(
        run_instance_method(
            shared,
            thread,
            method_type_class,
            "toMethodDescriptorString",
            TO_DESCRIPTOR,
            method_type,
            Vec::new(),
        )
        .expect("MethodType.toMethodDescriptorString runs"),
    );
    shared
        .heap()
        .string(descriptor)
        .expect("a String")
        .to_owned()
}

/// `Class.getMethod(String, Class[])`, the reflective lookup CGLIB uses.
fn class_get_method(
    shared: &Shared,
    thread: &ThreadState,
    class_object: ObjRef,
    name: &str,
    parameters: &[ObjRef],
) -> ObjRef {
    let class_class = library_class(shared, CLASS);
    let name = shared.intern_string(name);
    let parameters = class_array(shared, parameters);
    reference(
        run_instance_method(
            shared,
            thread,
            class_class,
            "getMethod",
            CLASS_GET_METHOD,
            class_object,
            vec![Value::Ref(name), Value::Ref(parameters)],
        )
        .expect("Class.getMethod runs"),
    )
}

/// `Method.invoke(Object, Object[])`.
fn method_invoke(
    shared: &Shared,
    thread: &ThreadState,
    method: ObjRef,
    receiver: ObjRef,
    arguments: ObjRef,
) -> Result<Option<Value>, InterpError> {
    let method_class = library_class(shared, REFLECT_METHOD);
    run_instance_method(
        shared,
        thread,
        method_class,
        "invoke",
        METHOD_INVOKE,
        method,
        vec![Value::Ref(receiver), Value::Ref(arguments)],
    )
}

#[test]
fn the_lookup_fixture_prints_the_expected_facts() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");

    run_static_method(&shared, &thread, probe, "run", "()V", Vec::new())
        .expect("demo.LookupProbe.run() completes");

    // One fact per line: the summed arguments, the lookup class's name, the private-access
    // flag and the instance method's echo.
    assert_eq!(shared.stdout(), "5\ndemo.LookupProbe\ntrue\nlookup\n");
}

#[test]
fn a_rust_built_lookup_resolves_and_invokes_add() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");
    let probe_object = shared
        .class_object(probe)
        .expect("the LookupProbe class object exists");
    let int_class = shared.primitive_class(PrimitiveKind::Int);

    let lookup = method_handles_lookup(&shared, &thread);
    let lookup = private_lookup_in(&shared, &thread, probe_object, lookup)
        .expect("privateLookupIn succeeds for a loaded class");

    // The lookup names demo.LookupProbe and has private access.
    let lookup_class = library_class(&shared, LOOKUP);
    assert_eq!(
        reference(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "lookupClass",
                "()Ljava/lang/Class;",
                lookup,
                Vec::new(),
            )
            .expect("lookupClass runs")
        ),
        probe_object,
        "privateLookupIn returns a lookup on the target class"
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "lookupModes",
                "()I",
                lookup,
                Vec::new(),
            )
            .expect("lookupModes runs")
        ),
        63,
        "a private lookup has full access modes"
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "hasPrivateAccess",
                "()Z",
                lookup,
                Vec::new(),
            )
            .expect("hasPrivateAccess runs")
        ),
        1
    );

    // MethodType.methodType(int.class, int.class, int.class) and the handle's own type.
    let method_type = method_type(&shared, &thread, int_class, &[int_class, int_class]);
    assert_eq!(descriptor_of(&shared, &thread, method_type), "(II)I");

    let handle = find_static(&shared, &thread, lookup, probe_object, "add", method_type)
        .expect("findStatic resolves add");
    let handle_class = library_class(&shared, METHOD_HANDLE);
    let handle_type = reference(
        run_instance_method(
            &shared,
            &thread,
            handle_class,
            "type",
            "()Ljava/lang/invoke/MethodType;",
            handle,
            Vec::new(),
        )
        .expect("MethodHandle.type runs"),
    );
    assert_eq!(
        descriptor_of(&shared, &thread, handle_type),
        "(II)I",
        "a static handle keeps the MethodType it was found with"
    );

    // invokeWithArguments(2, 3) boxes two Integers and returns a boxed Integer.
    let arguments = object_array(
        &shared,
        vec![Value::Ref(shared.box_int(2)), Value::Ref(shared.box_int(3))],
    );
    let result = reference(
        invoke_with_arguments(&shared, &thread, handle, arguments).expect("the handle invokes"),
    );
    assert_eq!(shared.unbox_int(result).expect("a boxed Integer"), 5);
    assert!(
        shared.stdout().is_empty(),
        "the Rust path does not print anything"
    );
}

#[test]
fn a_virtual_handle_dispatches_on_the_receiver() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");
    let probe_object = shared
        .class_object(probe)
        .expect("the LookupProbe class object exists");
    let lookup = private_lookup_in(
        &shared,
        &thread,
        probe_object,
        method_handles_lookup(&shared, &thread),
    )
    .expect("privateLookupIn succeeds");
    let lookup_class = library_class(&shared, LOOKUP);

    // MethodType.methodType(String.class): no parameters, a String return.
    let string_class = shared
        .class_object(library_class(&shared, "java/lang/String"))
        .expect("the String class object exists");
    let method_type = method_type_of_return(&shared, &thread, string_class);
    assert_eq!(
        descriptor_of(&shared, &thread, method_type),
        "()Ljava/lang/String;"
    );

    let handle = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "findVirtual",
            "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
            lookup,
            vec![
                Value::Ref(probe_object),
                Value::Ref(shared.intern_string("echo")),
                Value::Ref(method_type),
            ],
        )
        .expect("findVirtual resolves echo"),
    );

    // The handle type gains the receiver: (LookupProbe)String.
    let handle_class = library_class(&shared, METHOD_HANDLE);
    let handle_type = reference(
        run_instance_method(
            &shared,
            &thread,
            handle_class,
            "type",
            "()Ljava/lang/invoke/MethodType;",
            handle,
            Vec::new(),
        )
        .expect("MethodHandle.type runs"),
    );
    assert_eq!(
        descriptor_of(&shared, &thread, handle_type),
        "(Ldemo/LookupProbe;)Ljava/lang/String;"
    );

    let instance = shared
        .allocate_instance(probe)
        .expect("a LookupProbe instance allocates");
    let arguments = object_array(&shared, vec![Value::Ref(instance)]);
    let result = reference(
        invoke_with_arguments(&shared, &thread, handle, arguments)
            .expect("the virtual handle invokes"),
    );
    assert_eq!(
        shared.heap().string(result).expect("a String result"),
        "lookup"
    );
}

#[test]
fn define_class_defines_a_generated_class_and_rejects_a_duplicate() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");
    let probe_object = shared
        .class_object(probe)
        .expect("the LookupProbe class object exists");
    let lookup = private_lookup_in(
        &shared,
        &thread,
        probe_object,
        method_handles_lookup(&shared, &thread),
    )
    .expect("privateLookupIn succeeds");
    let lookup_class = library_class(&shared, LOOKUP);

    let bytes = generated_class("demo/Generated", 9);
    let buffer = byte_array(&shared, &bytes);
    let before = shared.class_count();
    let defined = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "defineClass",
            "([B)Ljava/lang/Class;",
            lookup,
            vec![Value::Ref(buffer)],
        )
        .expect("Lookup.defineClass runs"),
    );
    let generated = class_id(&shared, defined);
    assert_eq!(
        shared.entry(generated).map(|entry| entry.loader),
        Some(application),
        "the class is defined in the lookup class's loader"
    );
    assert_eq!(
        shared.lookup(application, "demo/Generated"),
        Some(generated),
        "the class table has the generated class"
    );
    assert_eq!(
        shared.class_count(),
        before + 1,
        "one definition adds one class"
    );

    let answer = run_static_method(&shared, &thread, generated, "answer", "()I", Vec::new())
        .expect("the generated class runs");
    assert_eq!(int_value(answer), 9);

    // A second definition is a duplicate: the class table is unchanged and the first class
    // stays.
    let second = run_instance_method(
        &shared,
        &thread,
        lookup_class,
        "defineClass",
        "([B)Ljava/lang/Class;",
        lookup,
        vec![Value::Ref(buffer)],
    );
    match second {
        Err(InterpError::Vm(VmError::DuplicateClass { loader, name })) => {
            assert_eq!(loader, application);
            assert_eq!(name, "demo/Generated");
        }
        other => panic!("expected VmError::DuplicateClass, found {other:?}"),
    }
    assert_eq!(shared.class_count(), before + 1);
}

#[test]
fn the_cglib_reflective_bootstrap_defines_a_class() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");
    let probe_object = shared
        .class_object(probe)
        .expect("the LookupProbe class object exists");
    let handles_object = shared
        .class_object(library_class(&shared, METHOD_HANDLES))
        .expect("the MethodHandles class object exists");
    let lookup_object = shared
        .class_object(library_class(&shared, LOOKUP))
        .expect("the Lookup class object exists");
    let class_object = shared
        .class_object(library_class(&shared, CLASS))
        .expect("the Class class object exists");

    // MethodHandles.class.getMethod("privateLookupIn", Class.class, Lookup.class), then
    // Method.invoke(null, new Object[]{LookupProbe.class, MethodHandles.lookup()}).
    let private_lookup_in_method = class_get_method(
        &shared,
        &thread,
        handles_object,
        "privateLookupIn",
        &[class_object, lookup_object],
    );
    let start_lookup = method_handles_lookup(&shared, &thread);
    let invoke_arguments = object_array(
        &shared,
        vec![Value::Ref(probe_object), Value::Ref(start_lookup)],
    );
    let lookup = reference(
        method_invoke(
            &shared,
            &thread,
            private_lookup_in_method,
            ObjRef::NULL,
            invoke_arguments,
        )
        .expect("the reflected privateLookupIn runs"),
    );

    // Lookup.class.getMethod("defineClass", byte[].class), then Method.invoke(lookup,
    // new Object[]{bytes}).
    let byte_array_class = shared
        .array_class(ClassRef::Primitive(PrimitiveKind::Byte))
        .expect("the byte[] class object exists");
    let define_class_method = class_get_method(
        &shared,
        &thread,
        lookup_object,
        "defineClass",
        &[byte_array_class],
    );
    let bytes = generated_class("demo/ReflectiveGenerated", 11);
    let buffer = byte_array(&shared, &bytes);
    let invoke_arguments = object_array(&shared, vec![Value::Ref(buffer)]);
    let defined = reference(
        method_invoke(
            &shared,
            &thread,
            define_class_method,
            lookup,
            invoke_arguments,
        )
        .expect("the reflected defineClass runs"),
    );

    let generated = class_id(&shared, defined);
    assert_eq!(
        shared.lookup(application, "demo/ReflectiveGenerated"),
        Some(generated),
        "the class is defined in the application loader"
    );
    let answer = run_static_method(&shared, &thread, generated, "answer", "()I", Vec::new())
        .expect("the generated class runs");
    assert_eq!(int_value(answer), 11);
}

#[test]
fn a_package_mismatch_and_a_wrong_find_are_typed_errors() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");
    let probe_object = shared
        .class_object(probe)
        .expect("the LookupProbe class object exists");
    let lookup = private_lookup_in(
        &shared,
        &thread,
        probe_object,
        method_handles_lookup(&shared, &thread),
    )
    .expect("privateLookupIn succeeds");
    let lookup_class = library_class(&shared, LOOKUP);
    let method_type_class = library_class(&shared, METHOD_TYPE);

    // A class in another package cannot be defined from a demo lookup; nothing is inserted.
    let bytes = generated_class("other/Generated", 13);
    let buffer = byte_array(&shared, &bytes);
    let before = shared.class_count();
    let error = run_instance_method(
        &shared,
        &thread,
        lookup_class,
        "defineClass",
        "([B)Ljava/lang/Class;",
        lookup,
        vec![Value::Ref(buffer)],
    )
    .expect_err("a package mismatch is rejected");
    match error {
        InterpError::Vm(VmError::Native(detail)) => assert!(
            detail.contains("package"),
            "the error names the package rule: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }
    assert_eq!(
        shared.lookup(application, "other/Generated"),
        None,
        "the rejected class is not defined"
    );
    assert_eq!(
        shared.class_count(),
        before,
        "the rejected definition leaves the class table unchanged"
    );

    // privateLookupIn on a primitive is a typed error: a lookup class must be a class.
    let int_class = shared.primitive_class(PrimitiveKind::Int);
    match private_lookup_in(
        &shared,
        &thread,
        int_class,
        method_handles_lookup(&shared, &thread),
    )
    .expect_err("a primitive target is rejected")
    {
        InterpError::Vm(VmError::Native(detail)) => assert!(
            detail.contains("primitive"),
            "the error names the primitive target: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }

    // A find for a method that does not exist is the JDK's NoSuchMethodException, so Java
    // code can catch it.
    let int_object = shared.primitive_class(PrimitiveKind::Int);
    let missing_type = method_type_of_return(&shared, &thread, int_object);
    match find_static(
        &shared,
        &thread,
        lookup,
        probe_object,
        "missing",
        missing_type,
    )
    .expect_err("a missing method is rejected")
    {
        InterpError::Thrown(throwable) => {
            let class = class_id(
                &shared,
                shared.get_class(throwable).expect("a class object"),
            );
            assert_eq!(
                shared.entry(class).map(|entry| entry.name.clone()),
                Some("java/lang/NoSuchMethodException".to_owned())
            );
        }
        other => panic!("expected a NoSuchMethodException throwable, found {other:?}"),
    }

    // Calling the static `add` as virtual is a typed error.
    let add_type = method_type(
        &shared,
        &thread,
        int_object,
        &[
            shared.primitive_class(PrimitiveKind::Int),
            shared.primitive_class(PrimitiveKind::Int),
        ],
    );
    match run_instance_method(
        &shared,
        &thread,
        lookup_class,
        "findVirtual",
        "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
        lookup,
        vec![
            Value::Ref(probe_object),
            Value::Ref(shared.intern_string("add")),
            Value::Ref(add_type),
        ],
    ) {
        Err(InterpError::Vm(VmError::Native(detail))) => assert!(
            detail.contains("static"),
            "the error names the static method: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }

    // A constructor MethodType that does not return void is a typed error, while the real
    // `LookupProbe()` constructor resolves to a working Constructor handle.
    let non_void_constructor_type =
        method_type_of_return(&shared, &thread, shared.primitive_class(PrimitiveKind::Int));
    match run_instance_method(
        &shared,
        &thread,
        lookup_class,
        "findConstructor",
        "(Ljava/lang/Class;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
        lookup,
        vec![
            Value::Ref(probe_object),
            Value::Ref(non_void_constructor_type),
        ],
    ) {
        Err(InterpError::Vm(VmError::Native(detail))) => assert!(
            detail.contains("void"),
            "the error names the void return rule: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }
    let void_type = method_type_of_return(
        &shared,
        &thread,
        shared.primitive_class(PrimitiveKind::Void),
    );
    let constructor = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "findConstructor",
            "(Ljava/lang/Class;Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
            lookup,
            vec![Value::Ref(probe_object), Value::Ref(void_type)],
        )
        .expect("findConstructor resolves the probe's no-arg constructor"),
    );
    let created = reference(
        invoke_with_arguments(&shared, &thread, constructor, ObjRef::NULL)
            .expect("the constructor handle invokes with a null Object[]"),
    );
    assert_eq!(
        shared.heap().instance(created).expect("an instance").class,
        probe
    );

    // The MethodType accessors are structural.
    let structural_type = method_type(
        &shared,
        &thread,
        shared.primitive_class(PrimitiveKind::Int),
        &[
            shared.primitive_class(PrimitiveKind::Int),
            shared.primitive_class(PrimitiveKind::Long),
        ],
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                method_type_class,
                "parameterCount",
                "()I",
                structural_type,
                Vec::new(),
            )
            .expect("parameterCount runs")
        ),
        2
    );
    let parameters = reference(
        run_instance_method(
            &shared,
            &thread,
            method_type_class,
            "parameterArray",
            "()[Ljava/lang/Class;",
            structural_type,
            Vec::new(),
        )
        .expect("parameterArray runs"),
    );
    let elements = shared
        .heap()
        .array(parameters)
        .expect("a Class[]")
        .elements
        .clone();
    assert_eq!(elements.len(), 2);
    assert_eq!(
        reference(
            run_instance_method(
                &shared,
                &thread,
                method_type_class,
                "parameterType",
                "(I)Ljava/lang/Class;",
                structural_type,
                vec![Value::Int(1)],
            )
            .expect("parameterType runs")
        ),
        shared.primitive_class(PrimitiveKind::Long)
    );
    assert_eq!(
        reference(
            run_instance_method(
                &shared,
                &thread,
                method_type_class,
                "returnType",
                "()Ljava/lang/Class;",
                structural_type,
                Vec::new(),
            )
            .expect("returnType runs")
        ),
        shared.primitive_class(PrimitiveKind::Int)
    );
    let changed = reference(
        run_instance_method(
            &shared,
            &thread,
            method_type_class,
            "changeReturnType",
            "(Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            structural_type,
            vec![Value::Ref(shared.primitive_class(PrimitiveKind::Void))],
        )
        .expect("changeReturnType runs"),
    );
    assert_eq!(descriptor_of(&shared, &thread, changed), "(IJ)V");
    let appended = reference(
        run_instance_method(
            &shared,
            &thread,
            method_type_class,
            "appendParameterTypes",
            "([Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            structural_type,
            vec![Value::Ref(class_array(
                &shared,
                &[shared.primitive_class(PrimitiveKind::Boolean)],
            ))],
        )
        .expect("appendParameterTypes runs"),
    );
    assert_eq!(descriptor_of(&shared, &thread, appended), "(IJZ)I");
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                method_type_class,
                "equals",
                "(Ljava/lang/Object;)Z",
                structural_type,
                vec![Value::Ref(structural_type)],
            )
            .expect("equals runs")
        ),
        1
    );

    // A parameter index outside the type is a typed error.
    match run_instance_method(
        &shared,
        &thread,
        method_type_class,
        "parameterType",
        "(I)Ljava/lang/Class;",
        structural_type,
        vec![Value::Int(9)],
    ) {
        Err(InterpError::Vm(VmError::Native(detail))) => assert!(
            detail.contains("outside"),
            "the error names the index: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }
}

#[test]
fn lookup_queries_and_method_type_factories_match_the_stub_surface() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");
    let probe_object = shared
        .class_object(probe)
        .expect("the LookupProbe class object exists");
    let lookup = private_lookup_in(
        &shared,
        &thread,
        probe_object,
        method_handles_lookup(&shared, &thread),
    )
    .expect("privateLookupIn succeeds");
    let lookup_class = library_class(&shared, LOOKUP);
    let method_type_class = library_class(&shared, METHOD_TYPE);
    let int_class = shared.primitive_class(PrimitiveKind::Int);

    // previousLookupClass is null: this VM never chains lookups.
    let previous = run_instance_method(
        &shared,
        &thread,
        lookup_class,
        "previousLookupClass",
        "()Ljava/lang/Class;",
        lookup,
        Vec::new(),
    )
    .expect("previousLookupClass runs");
    assert_eq!(previous, Some(Value::Ref(ObjRef::NULL)));

    // in(Class) keeps the modes and changes the class.
    let string_object = shared
        .class_object(library_class(&shared, "java/lang/String"))
        .expect("the String class object exists");
    let in_string = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "in",
            "(Ljava/lang/Class;)Ljava/lang/invoke/MethodHandles$Lookup;",
            lookup,
            vec![Value::Ref(string_object)],
        )
        .expect("Lookup.in runs"),
    );
    assert_eq!(
        reference(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "lookupClass",
                "()Ljava/lang/Class;",
                in_string,
                Vec::new(),
            )
            .expect("lookupClass runs")
        ),
        string_object
    );

    // dropLookupMode(PRIVATE) clears private access but keeps the class.
    let dropped = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "dropLookupMode",
            "(I)Ljava/lang/invoke/MethodHandles$Lookup;",
            lookup,
            vec![Value::Int(rava_vm::method_handles::LOOKUP_PRIVATE)],
        )
        .expect("dropLookupMode runs"),
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "hasPrivateAccess",
                "()Z",
                dropped,
                Vec::new(),
            )
            .expect("hasPrivateAccess runs")
        ),
        0,
        "dropping PRIVATE clears private access"
    );
    assert_eq!(
        reference(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "lookupClass",
                "()Ljava/lang/Class;",
                dropped,
                Vec::new(),
            )
            .expect("lookupClass runs")
        ),
        probe_object,
        "dropping a mode keeps the lookup class"
    );

    // findClass resolves through the lookup class's loader.
    let found = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "findClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            lookup,
            vec![Value::Ref(shared.intern_string("demo.LookupProbe"))],
        )
        .expect("findClass runs"),
    );
    assert_eq!(found, probe_object);

    // The three MethodType factories agree on the same signature.
    let by_array = reference(
        run_static_method(
            &shared,
            &thread,
            method_type_class,
            "methodType",
            "(Ljava/lang/Class;[Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            vec![
                Value::Ref(int_class),
                Value::Ref(class_array(&shared, &[int_class])),
            ],
        )
        .expect("methodType(Class, Class[]) runs"),
    );
    let by_one = reference(
        run_static_method(
            &shared,
            &thread,
            method_type_class,
            "methodType",
            "(Ljava/lang/Class;Ljava/lang/Class;)Ljava/lang/invoke/MethodType;",
            vec![Value::Ref(int_class), Value::Ref(int_class)],
        )
        .expect("methodType(Class, Class) runs"),
    );
    let by_none = method_type_of_return(&shared, &thread, int_class);
    assert_eq!(descriptor_of(&shared, &thread, by_array), "(I)I");
    assert_eq!(descriptor_of(&shared, &thread, by_one), "(I)I");
    assert_eq!(descriptor_of(&shared, &thread, by_none), "()I");
    for (index, method_type) in [by_array, by_one].into_iter().enumerate() {
        assert_eq!(
            int_value(
                run_instance_method(
                    &shared,
                    &thread,
                    method_type_class,
                    "equals",
                    "(Ljava/lang/Object;)Z",
                    method_type,
                    vec![Value::Ref(by_one)],
                )
                .expect("equals runs")
            ),
            1,
            "structurally equal method types are equal ({index})"
        );
    }
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                method_type_class,
                "equals",
                "(Ljava/lang/Object;)Z",
                by_none,
                vec![Value::Ref(by_one)],
            )
            .expect("equals runs")
        ),
        0
    );
    let text = reference(
        run_instance_method(
            &shared,
            &thread,
            method_type_class,
            "toString",
            "()Ljava/lang/String;",
            by_one,
            Vec::new(),
        )
        .expect("MethodType.toString runs"),
    );
    assert_eq!(shared.heap().string(text).expect("a String"), "(int)int");

    // asType returns the same handle for a matching type and rejects a conversion.
    let handle = find_static(&shared, &thread, lookup, probe_object, "add", {
        method_type(&shared, &thread, int_class, &[int_class, int_class])
    })
    .expect("findStatic resolves add");
    let handle_class = library_class(&shared, METHOD_HANDLE);
    let same = reference(
        run_instance_method(
            &shared,
            &thread,
            handle_class,
            "asType",
            "(Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
            handle,
            vec![Value::Ref({
                method_type(&shared, &thread, int_class, &[int_class, int_class])
            })],
        )
        .expect("asType with the same descriptor returns the handle"),
    );
    assert_eq!(same, handle);
    match run_instance_method(
        &shared,
        &thread,
        handle_class,
        "asType",
        "(Ljava/lang/invoke/MethodType;)Ljava/lang/invoke/MethodHandle;",
        handle,
        vec![Value::Ref(by_none)],
    ) {
        Err(InterpError::Vm(VmError::Native(detail))) => assert!(
            detail.contains("asType"),
            "the error names asType: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }
}

#[test]
fn unreflect_find_special_and_the_public_lookup_complete_the_surface() {
    let (shared, application, thread) = fixture();
    let probe = shared
        .load_class(application, "demo/LookupProbe")
        .expect("demo/LookupProbe loads from the fixture");
    let probe_object = shared
        .class_object(probe)
        .expect("the LookupProbe class object exists");
    let lookup = private_lookup_in(
        &shared,
        &thread,
        probe_object,
        method_handles_lookup(&shared, &thread),
    )
    .expect("privateLookupIn succeeds");
    let lookup_class = library_class(&shared, LOOKUP);
    let handle_class = library_class(&shared, METHOD_HANDLE);
    let int_class = shared.primitive_class(PrimitiveKind::Int);

    // unreflect(Method) for the static add: a Static handle with the same descriptor.
    let add_method = class_get_method(
        &shared,
        &thread,
        probe_object,
        "add",
        &[int_class, int_class],
    );
    let handle = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "unreflect",
            "(Ljava/lang/reflect/Method;)Ljava/lang/invoke/MethodHandle;",
            lookup,
            vec![Value::Ref(add_method)],
        )
        .expect("Lookup.unreflect runs"),
    );
    let handle_type = reference(
        run_instance_method(
            &shared,
            &thread,
            handle_class,
            "type",
            "()Ljava/lang/invoke/MethodType;",
            handle,
            Vec::new(),
        )
        .expect("MethodHandle.type runs"),
    );
    assert_eq!(descriptor_of(&shared, &thread, handle_type), "(II)I");
    let arguments = object_array(
        &shared,
        vec![Value::Ref(shared.box_int(4)), Value::Ref(shared.box_int(5))],
    );
    let result = reference(
        invoke_with_arguments(&shared, &thread, handle, arguments)
            .expect("the unreflected static handle invokes"),
    );
    assert_eq!(shared.unbox_int(result).expect("a boxed Integer"), 9);

    // unreflectConstructor(Constructor) for the private no-arg constructor: the handle
    // returns a new LookupProbe.
    let class_class = library_class(&shared, CLASS);
    let constructor = reference(
        run_instance_method(
            &shared,
            &thread,
            class_class,
            "getDeclaredConstructor",
            "([Ljava/lang/Class;)Ljava/lang/reflect/Constructor;",
            probe_object,
            vec![Value::Ref(class_array(&shared, &[]))],
        )
        .expect("Class.getDeclaredConstructor runs"),
    );
    let constructor_handle = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "unreflectConstructor",
            "(Ljava/lang/reflect/Constructor;)Ljava/lang/invoke/MethodHandle;",
            lookup,
            vec![Value::Ref(constructor)],
        )
        .expect("Lookup.unreflectConstructor runs"),
    );
    let constructor_type = reference(
        run_instance_method(
            &shared,
            &thread,
            handle_class,
            "type",
            "()Ljava/lang/invoke/MethodType;",
            constructor_handle,
            Vec::new(),
        )
        .expect("MethodHandle.type runs"),
    );
    assert_eq!(
        descriptor_of(&shared, &thread, constructor_type),
        "()Ldemo/LookupProbe;"
    );
    let created = reference(
        invoke_with_arguments(&shared, &thread, constructor_handle, ObjRef::NULL)
            .expect("the constructor handle invokes"),
    );
    assert_eq!(
        shared.heap().instance(created).expect("an instance").class,
        probe
    );

    // findSpecial accepts its special-caller argument and runs the owner's method.
    let string_object = shared
        .class_object(library_class(&shared, "java/lang/String"))
        .expect("the String class object exists");
    let echo_type = method_type_of_return(&shared, &thread, string_object);
    let special = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "findSpecial",
            "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/Class;)Ljava/lang/invoke/MethodHandle;",
            lookup,
            vec![
                Value::Ref(probe_object),
                Value::Ref(shared.intern_string("echo")),
                Value::Ref(echo_type),
                Value::Ref(probe_object),
            ],
        )
        .expect("findSpecial resolves echo"),
    );
    let special_type = reference(
        run_instance_method(
            &shared,
            &thread,
            handle_class,
            "type",
            "()Ljava/lang/invoke/MethodType;",
            special,
            Vec::new(),
        )
        .expect("MethodHandle.type runs"),
    );
    assert_eq!(
        descriptor_of(&shared, &thread, special_type),
        "(Ldemo/LookupProbe;)Ljava/lang/String;"
    );
    let instance = shared
        .allocate_instance(probe)
        .expect("a LookupProbe instance allocates");
    let arguments = object_array(&shared, vec![Value::Ref(instance)]);
    let echoed = reference(
        invoke_with_arguments(&shared, &thread, special, arguments)
            .expect("the special handle invokes"),
    );
    assert_eq!(shared.heap().string(echoed).expect("a String"), "lookup");

    // publicLookup is public-only, and toString names the lookup class.
    let handles = library_class(&shared, METHOD_HANDLES);
    let public = reference(
        run_static_method(
            &shared,
            &thread,
            handles,
            "publicLookup",
            "()Ljava/lang/invoke/MethodHandles$Lookup;",
            Vec::new(),
        )
        .expect("MethodHandles.publicLookup runs"),
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "lookupModes",
                "()I",
                public,
                Vec::new(),
            )
            .expect("lookupModes runs")
        ),
        rava_vm::method_handles::PUBLIC_ACCESS
    );
    assert_eq!(
        int_value(
            run_instance_method(
                &shared,
                &thread,
                lookup_class,
                "hasPrivateAccess",
                "()Z",
                public,
                Vec::new(),
            )
            .expect("hasPrivateAccess runs")
        ),
        0
    );
    let text = reference(
        run_instance_method(
            &shared,
            &thread,
            lookup_class,
            "toString",
            "()Ljava/lang/String;",
            lookup,
            Vec::new(),
        )
        .expect("Lookup.toString runs"),
    );
    assert!(
        shared
            .heap()
            .string(text)
            .expect("a String")
            .contains("demo.LookupProbe"),
        "Lookup.toString names the lookup class"
    );
}
