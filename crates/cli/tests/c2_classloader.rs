//! M3.3 acceptance: the `java.lang.ClassLoader` hierarchy and `defineClass`.
//!
//! The committed `demo.CustomLoader` fixture is compiled with `javac --release 17` and
//! committed under `tests/fixtures/loader/classes`, so this suite needs no JDK, network or
//! files beyond that fixture and `stdlib/c1/classes`. The generated classes are assembled
//! in process with [`ClassBuilder`] + [`Assembler`].
//!
//! The suite covers the loader object model end to end: a user `ClassLoader` subclass
//! instantiated with `new` gets its own child loader and defines a generated class in it,
//! `loadClass`/`Class.forName` delegate parent-first to the application loader, the
//! system/parent objects line up, and a failed `defineClass` or a duplicate definition
//! leaves the class table unchanged.

use std::path::PathBuf;
use std::sync::Arc;

use rava_classfile::{Assembler, ClassBuilder};
use rava_interp::{InterpError, run_instance_method, run_static_method};
use rava_vm::{
    ClassId, ClassRef, DirectorySource, LoaderId, ObjRef, PrimitiveKind, Shared, ThreadState,
    Value, VmError, library,
};

const ACC_PUBLIC_STATIC: u16 = 0x0009;

const CLASS_LOADER: &str = "java/lang/ClassLoader";
const CLASS: &str = "java/lang/Class";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn c1_classes() -> PathBuf {
    manifest_dir().join("../../stdlib/c1/classes")
}

fn loader_fixture_classes() -> PathBuf {
    manifest_dir().join("tests/fixtures/loader/classes")
}

/// A library-backed VM plus the class-loader fixture as the application loader, with every
/// native registered.
fn fixture() -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let application = shared
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(loader_fixture_classes())),
        )
        .expect("the application loader is added");
    library::register_all(&shared).expect("the class library registers");
    rava_interp::reflect_invoke::register_natives(&shared);
    let thread = shared.thread_state(1);
    (shared, application, thread)
}

fn int_value(value: Option<Value>) -> i32 {
    match value {
        Some(Value::Int(value)) => value,
        other => panic!("expected an int result, found {other:?}"),
    }
}

fn class_object(result: Option<Value>) -> ObjRef {
    match result {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        other => panic!("expected a Class result, found {other:?}"),
    }
}

fn class_id(shared: &Shared, object: ObjRef) -> ClassId {
    shared
        .heap()
        .class(object)
        .expect("the result is a class object")
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

/// A heap `byte[]` holding `bytes`, the shape `javac`'s `new byte[]{...}` produces.
fn byte_array(shared: &Shared, bytes: &[u8]) -> ObjRef {
    let reference = shared
        .heap()
        .allocate_array(ClassRef::Primitive(PrimitiveKind::Byte), bytes.len());
    {
        let mut heap = shared.heap();
        let array = heap
            .array_mut(reference)
            .expect("a freshly allocated byte[]");
        for (slot, byte) in array.elements.iter_mut().zip(bytes) {
            *slot = Value::Int(i32::from(*byte));
        }
    }
    reference
}

/// Instantiate `demo.CustomLoader` with `new` + `<init>`, the shape Java code produces.
fn custom_loader(
    shared: &Shared,
    thread: &ThreadState,
    application: LoaderId,
) -> (ClassId, ObjRef) {
    let class = shared
        .load_class(application, "demo/CustomLoader")
        .expect("demo/CustomLoader loads from the fixture");
    let object = shared
        .allocate_instance(class)
        .expect("a CustomLoader instance allocates");
    run_instance_method(shared, thread, class, "<init>", "()V", object, Vec::new())
        .expect("CustomLoader.<init> runs");
    (class, object)
}

fn call_define(
    shared: &Shared,
    thread: &ThreadState,
    custom_class: ClassId,
    loader: ObjRef,
    name: &str,
    bytes: &[u8],
) -> Result<ObjRef, InterpError> {
    let name = shared.intern_string(name);
    let bytes = byte_array(shared, bytes);
    let result = run_instance_method(
        shared,
        thread,
        custom_class,
        "define",
        "(Ljava/lang/String;[B)Ljava/lang/Class;",
        loader,
        vec![Value::Ref(name), Value::Ref(bytes)],
    )?;
    Ok(class_object(result))
}

fn call_get_class_loader(shared: &Shared, thread: &ThreadState, class: ObjRef) -> ObjRef {
    let class_class = shared
        .load_library_class(CLASS)
        .expect("java.lang.Class loads");
    let result = run_instance_method(
        shared,
        thread,
        class_class,
        "getClassLoader",
        "()Ljava/lang/ClassLoader;",
        class,
        Vec::new(),
    )
    .expect("Class.getClassLoader runs");
    match result {
        Some(Value::Ref(reference)) => reference,
        other => panic!("getClassLoader returned {other:?}"),
    }
}

fn call_get_parent(
    shared: &Shared,
    thread: &ThreadState,
    loader_class: ClassId,
    loader: ObjRef,
) -> ObjRef {
    let result = run_instance_method(
        shared,
        thread,
        loader_class,
        "getParent",
        "()Ljava/lang/ClassLoader;",
        loader,
        Vec::new(),
    )
    .expect("ClassLoader.getParent runs");
    match result {
        Some(Value::Ref(reference)) => reference,
        other => panic!("getParent returned {other:?}"),
    }
}

fn call_get_name(
    shared: &Shared,
    thread: &ThreadState,
    loader_class: ClassId,
    loader: ObjRef,
) -> String {
    let result = run_instance_method(
        shared,
        thread,
        loader_class,
        "getName",
        "()Ljava/lang/String;",
        loader,
        Vec::new(),
    )
    .expect("ClassLoader.getName runs");
    let Some(Value::Ref(reference)) = result else {
        panic!("getName returned {result:?}");
    };
    shared
        .heap()
        .string(reference)
        .expect("getName returns a String")
        .to_owned()
}

fn call_load_class(
    shared: &Shared,
    thread: &ThreadState,
    loader_class: ClassId,
    loader: ObjRef,
    name: &str,
) -> Result<ObjRef, InterpError> {
    let name = shared.intern_string(name);
    let result = run_instance_method(
        shared,
        thread,
        loader_class,
        "loadClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        loader,
        vec![Value::Ref(name)],
    )?;
    Ok(class_object(result))
}

fn call_find_loaded_class(
    shared: &Shared,
    thread: &ThreadState,
    loader_class: ClassId,
    loader: ObjRef,
    name: &str,
) -> ObjRef {
    let name = shared.intern_string(name);
    let result = run_instance_method(
        shared,
        thread,
        loader_class,
        "findLoadedClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        loader,
        vec![Value::Ref(name)],
    )
    .expect("findLoadedClass runs");
    match result {
        Some(Value::Ref(reference)) => reference,
        other => panic!("findLoadedClass returned {other:?}"),
    }
}

fn call_for_name(
    shared: &Shared,
    thread: &ThreadState,
    name: &str,
    loader: ObjRef,
) -> Result<ObjRef, InterpError> {
    let class_class = shared
        .load_library_class(CLASS)
        .expect("java.lang.Class loads");
    let name = shared.intern_string(name);
    let result = run_static_method(
        shared,
        thread,
        class_class,
        "forName",
        "(Ljava/lang/String;ZLjava/lang/ClassLoader;)Ljava/lang/Class;",
        vec![Value::Ref(name), Value::Int(0), Value::Ref(loader)],
    )?;
    Ok(class_object(result))
}

fn call_for_name_one_argument(
    shared: &Shared,
    thread: &ThreadState,
    name: &str,
) -> Result<ObjRef, InterpError> {
    let class_class = shared
        .load_library_class(CLASS)
        .expect("java.lang.Class loads");
    let name = shared.intern_string(name);
    let result = run_static_method(
        shared,
        thread,
        class_class,
        "forName",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        vec![Value::Ref(name)],
    )?;
    Ok(class_object(result))
}

/// Call `ClassLoader.defineClass(String, byte[], int, int)` directly on `loader`.
fn call_define_class(
    shared: &Shared,
    thread: &ThreadState,
    loader_class: ClassId,
    loader: ObjRef,
    name: Option<&str>,
    bytes: &[u8],
    range: (i32, i32),
) -> Result<ObjRef, InterpError> {
    let (offset, length) = range;
    let name = match name {
        Some(name) => Value::Ref(shared.intern_string(name)),
        None => Value::Ref(ObjRef::NULL),
    };
    let bytes = byte_array(shared, bytes);
    let result = run_instance_method(
        shared,
        thread,
        loader_class,
        "defineClass",
        "(Ljava/lang/String;[BII)Ljava/lang/Class;",
        loader,
        vec![
            name,
            Value::Ref(bytes),
            Value::Int(offset),
            Value::Int(length),
        ],
    )?;
    Ok(class_object(result))
}

fn answer_of(shared: &Shared, thread: &ThreadState, class: ClassId) -> i32 {
    let result =
        run_static_method(shared, thread, class, "answer", "()I", Vec::new()).expect("answer runs");
    int_value(result)
}

#[test]
fn a_custom_loader_defines_a_generated_class_in_its_own_loader() {
    let (shared, application, thread) = fixture();
    let loader_class = shared
        .load_library_class(CLASS_LOADER)
        .expect("java.lang.ClassLoader loads");
    let (custom_class, custom_loader) = custom_loader(&shared, &thread, application);
    let bytes = generated_class("demo/Generated", 7);

    let defined = call_define(
        &shared,
        &thread,
        custom_class,
        custom_loader,
        "demo.Generated",
        &bytes,
    )
    .expect("CustomLoader.define runs");
    let defined_id = class_id(&shared, defined);

    assert_eq!(
        answer_of(&shared, &thread, defined_id),
        7,
        "the defined class runs"
    );
    assert_eq!(
        call_get_class_loader(&shared, &thread, defined),
        custom_loader,
        "the defined class reports the user loader instance"
    );

    let child = shared
        .loader_id_for(custom_loader)
        .expect("the custom loader has a loader id");
    assert_ne!(child, application, "the user loader is a child loader");
    assert_eq!(shared.loader_parent(child), Some(application));
    assert_eq!(
        shared.entry(defined_id).map(|entry| entry.loader),
        Some(child),
        "the class is defined in the custom loader"
    );
    assert_eq!(
        shared.lookup(child, "demo/Generated"),
        Some(defined_id),
        "the class is in the custom loader's table"
    );
    assert_eq!(
        shared.lookup(application, "demo/Generated"),
        None,
        "the class is not in the application loader's table"
    );

    // `findLoadedClass` is a local lookup: the child defined this class, so it is found
    // there, but the application loader never defined it.
    assert_eq!(
        call_find_loaded_class(
            &shared,
            &thread,
            loader_class,
            custom_loader,
            "demo.Generated"
        ),
        defined,
        "findLoadedClass returns the class the child defined"
    );

    // The names are stable and distinguish the application loader from a child.
    let system = shared
        .class_loader_object()
        .expect("the system loader object exists");
    assert_eq!(
        call_get_name(&shared, &thread, loader_class, system),
        "rava"
    );
    assert!(
        call_get_name(&shared, &thread, loader_class, custom_loader).starts_with("rava-child-"),
        "a child loader has a child name"
    );

    // Parallel-capable registration is reported truthfully: the VM's class table is
    // internally synchronized, so both answers are true.
    let registered = run_static_method(
        &shared,
        &thread,
        loader_class,
        "registerAsParallelCapable",
        "()Z",
        Vec::new(),
    )
    .expect("registerAsParallelCapable runs");
    assert_eq!(int_value(registered), 1);
    let capable = run_instance_method(
        &shared,
        &thread,
        loader_class,
        "isRegisteredAsParallelCapable",
        "()Z",
        custom_loader,
        Vec::new(),
    )
    .expect("isRegisteredAsParallelCapable runs");
    assert_eq!(int_value(capable), 1);
}

#[test]
fn load_class_and_for_name_delegate_parent_first_to_the_application_loader() {
    let (shared, application, thread) = fixture();
    let loader_class = shared
        .load_library_class(CLASS_LOADER)
        .expect("java.lang.ClassLoader loads");
    let (_custom_class, custom_loader) = custom_loader(&shared, &thread, application);
    let system = shared
        .class_loader_object()
        .expect("the system loader object exists");

    let loaded = call_load_class(
        &shared,
        &thread,
        loader_class,
        custom_loader,
        "demo.Existing",
    )
    .expect("loadClass finds the fixture class");
    let for_named = call_for_name(&shared, &thread, "demo.Existing", custom_loader)
        .expect("Class.forName finds the fixture class");
    let one_argument = call_for_name_one_argument(&shared, &thread, "demo.Existing")
        .expect("Class.forName(String) finds the fixture class");

    assert_eq!(
        loaded, for_named,
        "loadClass and Class.forName(name, false, loader) return the same Class object"
    );
    assert_eq!(
        loaded, one_argument,
        "Class.forName(String) also resolves through the application loader"
    );

    let existing = class_id(&shared, loaded);
    assert_eq!(
        shared.entry(existing).map(|entry| entry.loader),
        Some(application),
        "parent-first delegation defines the fixture class in the application loader"
    );
    assert_eq!(shared.lookup(application, "demo/Existing"), Some(existing));
    assert_eq!(
        call_get_class_loader(&shared, &thread, loaded),
        system,
        "a class the application loader defined reports the system loader"
    );

    // The child did not define the delegated class, so its local lookup misses; the
    // application loader finds it.
    assert_eq!(
        call_find_loaded_class(
            &shared,
            &thread,
            loader_class,
            custom_loader,
            "demo.Existing"
        ),
        ObjRef::NULL,
        "findLoadedClass on the child is a local lookup only"
    );
    let system_lookup = run_instance_method(
        &shared,
        &thread,
        loader_class,
        "findSystemClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        custom_loader,
        vec![Value::Ref(shared.intern_string("demo.Existing"))],
    )
    .expect("findSystemClass runs");
    assert_eq!(
        class_object(system_lookup),
        loaded,
        "findSystemClass resolves through the application loader"
    );
    assert_eq!(
        call_find_loaded_class(&shared, &thread, loader_class, system, "demo.Existing"),
        loaded,
        "the application loader did define it"
    );

    // The hierarchy: system is non-null, the child's parent is the application loader, and
    // the application loader's parent is null (this VM has no platform loader).
    let system_again = run_static_method(
        &shared,
        &thread,
        loader_class,
        "getSystemClassLoader",
        "()Ljava/lang/ClassLoader;",
        Vec::new(),
    )
    .expect("getSystemClassLoader runs");
    let system_again = match system_again {
        Some(Value::Ref(reference)) => reference,
        other => panic!("getSystemClassLoader returned {other:?}"),
    };
    assert_eq!(system_again, system, "getSystemClassLoader is non-null");
    assert_eq!(
        call_get_parent(&shared, &thread, loader_class, custom_loader),
        system,
        "the custom loader's parent is the application loader"
    );
    assert_eq!(
        call_get_parent(&shared, &thread, loader_class, system),
        ObjRef::NULL,
        "the application loader's parent is null"
    );

    // A bootstrap class still reports no loader.
    let string_class = shared
        .load_library_class("java/lang/String")
        .expect("java.lang.String loads");
    let string_object = shared
        .class_object(string_class)
        .expect("the String class object exists");
    assert_eq!(
        call_get_class_loader(&shared, &thread, string_object),
        ObjRef::NULL,
        "a bootstrap class reports a null loader"
    );

    // A null ClassLoader argument means bootstrap, as the JDK documents: the application
    // loader's fixture class is not visible and the miss is a ClassNotFoundException.
    let error = call_for_name(&shared, &thread, "demo.Existing", ObjRef::NULL)
        .expect_err("a null loader means bootstrap, which cannot see the fixture");
    assert!(
        matches!(error, InterpError::Thrown(_)),
        "the miss is a ClassNotFoundException throwable, found {error:?}"
    );

    // Primitive and array names keep resolving from the caches through the 3-argument
    // form regardless of the loader.
    let primitive =
        call_for_name(&shared, &thread, "int", ObjRef::NULL).expect("a primitive name resolves");
    assert_eq!(
        primitive,
        shared.primitive_class(PrimitiveKind::Int),
        "int resolves to the cached primitive class object"
    );
    let array =
        call_for_name(&shared, &thread, "[I", ObjRef::NULL).expect("an array name resolves");
    assert_eq!(
        shared
            .heap()
            .array_class(array)
            .expect("the result is an array class object")
            .component,
        ClassRef::Primitive(PrimitiveKind::Int)
    );
}

#[test]
fn the_system_loader_define_class_creates_classes_and_derives_a_missing_name() {
    let (shared, application, thread) = fixture();
    let loader_class = shared
        .load_library_class(CLASS_LOADER)
        .expect("java.lang.ClassLoader loads");
    let system = shared
        .class_loader_object()
        .expect("the system loader object exists");

    // A dotted name is converted to the class table's internal spelling.
    let named = generated_class("demo/SystemNamed", 9);
    let defined = call_define_class(
        &shared,
        &thread,
        loader_class,
        system,
        Some("demo.SystemNamed"),
        &named,
        (0, named.len() as i32),
    )
    .expect("defineClass on the system loader runs");
    let defined_id = class_id(&shared, defined);
    assert_eq!(answer_of(&shared, &thread, defined_id), 9);
    assert_eq!(
        shared.lookup(application, "demo/SystemNamed"),
        Some(defined_id)
    );
    assert_eq!(
        call_get_class_loader(&shared, &thread, defined),
        system,
        "a class the system loader defined reports the system loader"
    );

    // A null name is derived from the class-file header.
    let unnamed = generated_class("demo/SystemDerived", 11);
    let derived = call_define_class(
        &shared,
        &thread,
        loader_class,
        system,
        None,
        &unnamed,
        (0, unnamed.len() as i32),
    )
    .expect("an unnamed defineClass runs");
    let derived_id = class_id(&shared, derived);
    assert_eq!(answer_of(&shared, &thread, derived_id), 11);
    assert_eq!(
        shared.lookup(application, "demo/SystemDerived"),
        Some(derived_id),
        "the name came from the class file"
    );

    // The unnamed overload works the same way.
    let unnamed_overload = generated_class("demo/SystemUnnamedOverload", 13);
    let array = byte_array(&shared, &unnamed_overload);
    let result = run_instance_method(
        &shared,
        &thread,
        loader_class,
        "defineClass",
        "([BII)Ljava/lang/Class;",
        system,
        vec![
            Value::Ref(array),
            Value::Int(0),
            Value::Int(unnamed_overload.len() as i32),
        ],
    )
    .expect("defineClass(byte[], int, int) runs");
    let overload_id = class_id(&shared, class_object(result));
    assert_eq!(answer_of(&shared, &thread, overload_id), 13);
    assert_eq!(
        shared.lookup(application, "demo/SystemUnnamedOverload"),
        Some(overload_id)
    );
}

#[test]
fn a_bad_range_or_a_duplicate_leaves_the_class_table_unchanged() {
    let (shared, application, thread) = fixture();
    let loader_class = shared
        .load_library_class(CLASS_LOADER)
        .expect("java.lang.ClassLoader loads");
    let (custom_class, custom_loader) = custom_loader(&shared, &thread, application);
    let bytes = generated_class("demo/Generated", 7);
    let defined = call_define(
        &shared,
        &thread,
        custom_class,
        custom_loader,
        "demo/Generated",
        &bytes,
    )
    .expect("the first definition succeeds");
    let child = shared
        .loader_id_for(custom_loader)
        .expect("the custom loader has a loader id");
    let before = shared.class_count();

    let assert_native_error = |result: Result<ObjRef, InterpError>, needle: &str| match result {
        Err(InterpError::Vm(VmError::Native(detail))) => {
            assert!(
                detail.contains(needle),
                "the error mentions {needle:?}: {detail}"
            );
        }
        other => panic!("expected a typed native error, found {other:?}"),
    };

    // A negative offset, an over-long length, a null array and a wrong array kind are all
    // typed errors and none of them defines a class.
    assert_native_error(
        call_define_class(
            &shared,
            &thread,
            loader_class,
            custom_loader,
            Some("demo/BadOffset"),
            &bytes,
            (-1, bytes.len() as i32),
        ),
        "negative",
    );
    assert_native_error(
        call_define_class(
            &shared,
            &thread,
            loader_class,
            custom_loader,
            Some("demo/BadLength"),
            &bytes,
            (0, bytes.len() as i32 + 1),
        ),
        "outside",
    );
    {
        // A null byte[]: the named argument is present but the array reference is not.
        let name = shared.intern_string("demo/NullBytes");
        let error = run_instance_method(
            &shared,
            &thread,
            loader_class,
            "defineClass",
            "(Ljava/lang/String;[BII)Ljava/lang/Class;",
            custom_loader,
            vec![
                Value::Ref(name),
                Value::Ref(ObjRef::NULL),
                Value::Int(0),
                Value::Int(0),
            ],
        )
        .expect_err("a null byte[] is rejected");
        match error {
            InterpError::Vm(VmError::Native(detail)) => {
                assert!(
                    detail.contains("byte[]"),
                    "the error names the array: {detail}"
                );
            }
            other => panic!("expected a typed native error, found {other:?}"),
        }
    }
    {
        // A wrong array kind: the receiver still names a loader, but the argument is an
        // `int[]`. A primitive component needs no class load, so this cannot change the
        // class count on its own.
        let int_array = shared
            .heap()
            .allocate_array(ClassRef::Primitive(PrimitiveKind::Int), 3);
        let name = shared.intern_string("demo/WrongArray");
        let error = run_instance_method(
            &shared,
            &thread,
            loader_class,
            "defineClass",
            "(Ljava/lang/String;[BII)Ljava/lang/Class;",
            custom_loader,
            vec![
                Value::Ref(name),
                Value::Ref(int_array),
                Value::Int(0),
                Value::Int(3),
            ],
        )
        .expect_err("an int[] is not a byte[]");
        match error {
            InterpError::Vm(VmError::Native(detail)) => {
                assert!(
                    detail.contains("byte[]"),
                    "the error names the array kind: {detail}"
                );
            }
            other => panic!("expected a typed native error, found {other:?}"),
        }
    }

    for name in [
        "demo/BadOffset",
        "demo/BadLength",
        "demo/NullBytes",
        "demo/WrongArray",
    ] {
        assert_eq!(shared.lookup(child, name), None, "{name} is not defined");
    }
    assert_eq!(
        shared.class_count(),
        before,
        "no rejected definition changed the class table"
    );

    // A duplicate definition stays a typed `DuplicateClass` and does not replace the class.
    let error = call_define(
        &shared,
        &thread,
        custom_class,
        custom_loader,
        "demo.Generated",
        &bytes,
    )
    .expect_err("the second definition is rejected");
    match error {
        InterpError::Vm(VmError::DuplicateClass { loader, name }) => {
            assert_eq!(loader, child);
            assert_eq!(name, "demo/Generated");
        }
        other => panic!("expected VmError::DuplicateClass, found {other:?}"),
    }
    assert_eq!(shared.class_count(), before);
    assert_eq!(
        shared.lookup(child, "demo/Generated"),
        Some(class_id(&shared, defined))
    );

    // A receiver that is not a ClassLoader is a typed error.
    let string = shared.intern_string("not a loader");
    match shared.loader_id_for(string) {
        Err(VmError::Native(detail)) => assert!(
            detail.contains("not a java.lang.ClassLoader"),
            "the error names the receiver kind: {detail}"
        ),
        other => panic!("expected a typed native error, found {other:?}"),
    }
}
