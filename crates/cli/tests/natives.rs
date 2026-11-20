//! Acceptance for issue #2 (the declared-versus-registered native report) and for S7 (the
//! first JCL natives: `System`'s setters, `FileDescriptor`, `FileOutputStream`).
//!
//! Fixtures are built with the class-file builder and written to a temporary directory, so the
//! tests need no JDK. The real-image test is gated on `JAVA21_HOME`, like the VM's image tests:
//! a machine without the devShell image is not the machine that acceptance covers.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::{HeapObject, NativeCtx, NativeFn, ThreadState, Value, VmError};

fn noop(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::NULL)
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-natives-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn write_class(dir: &Path, name: &str, bytes: Vec<u8>) {
    std::fs::write(dir.join(format!("{name}.class")), bytes).expect("fixture class");
}

fn class_path(dir: &Path) -> rava_vm::ClassPath {
    rava_cli::class_path(dir, dir).expect("class path")
}

/// A class with a `main` that does nothing: no native references at all.
fn plain(dir: &Path) {
    let mut builder = ClassBuilder::new("Plain", "java/lang/Object", 65);
    builder.method(
        0x0009, // public static
        "main",
        "([Ljava/lang/String;)V",
        Some((0, 1, vec![0xb1])), // return
    );
    write_class(dir, "Plain", builder.build());
}

/// The native a `Needs` class calls.
fn native_owner(dir: &Path) {
    let mut builder = ClassBuilder::new("NativeOwner", "java/lang/Object", 65);
    builder.method(
        0x0109, // public static native
        "ping", "()V", None,
    );
    write_class(dir, "NativeOwner", builder.build());
}

/// A class whose constant pool references `NativeOwner.ping()V`.
fn needs(dir: &Path) {
    let mut builder = ClassBuilder::new("Needs", "java/lang/Object", 65);
    let call = builder.method_ref("NativeOwner", "ping", "()V");
    let code = vec![
        0xb8, // invokestatic
        (call >> 8) as u8,
        call as u8,
        0xb1, // return
    ];
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((0, 0, code)));
    write_class(dir, "Needs", builder.build());
}

#[test]
fn a_class_without_natives_reports_nothing() {
    let dir = fixture_dir("plain");
    plain(&dir);
    let report = rava_cli::natives::collect(&class_path(&dir), "Plain", &HashMap::new(), 100)
        .expect("collect");
    assert!(report.declared.is_empty());
    assert!(report.missing.is_empty());
    assert!(report.dangling.is_empty());
    assert!(report.ok());
    // The walk also follows the superclass reference, which the fixture directory does not
    // contain: two classes visited, one unresolved, and still nothing to report.
    assert_eq!(report.visited, 2);
    assert_eq!(report.unresolved, 1);
}

#[test]
fn a_referenced_native_without_an_implementation_is_missing() {
    let dir = fixture_dir("missing");
    native_owner(&dir);
    needs(&dir);
    let report = rava_cli::natives::collect(&class_path(&dir), "Needs", &HashMap::new(), 100)
        .expect("collect");
    let keys: Vec<String> = report.declared.iter().map(|method| method.key()).collect();
    assert_eq!(keys, vec!["NativeOwner.ping()V".to_owned()]);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.dangling.len(), 0);
    assert!(!report.ok());
    // The walk reached the owner through the call's constant pool; `Object` is referenced too
    // but is not in the fixture directory.
    assert_eq!(report.visited, 3);
}

#[test]
fn a_registered_native_is_reconciled() {
    let dir = fixture_dir("registered");
    native_owner(&dir);
    needs(&dir);
    let registered: HashMap<String, NativeFn> =
        HashMap::from([("NativeOwner.ping()V".to_owned(), noop as NativeFn)]);
    let report =
        rava_cli::natives::collect(&class_path(&dir), "Needs", &registered, 100).expect("collect");
    assert_eq!(report.declared.len(), 1);
    assert!(report.missing.is_empty());
    assert!(report.dangling.is_empty());
    assert!(report.ok());
}

#[test]
fn a_registration_nothing_declares_is_dangling() {
    let dir = fixture_dir("dangling");
    plain(&dir);
    // The owner class exists and declares no such native: a dangling registration.
    let registered: HashMap<String, NativeFn> =
        HashMap::from([("Plain.missing()V".to_owned(), noop as NativeFn)]);
    let report =
        rava_cli::natives::collect(&class_path(&dir), "Plain", &registered, 100).expect("collect");
    assert_eq!(report.dangling, vec!["Plain.missing()V".to_owned()]);
    assert!(!report.ok());
}

#[test]
fn a_registration_for_an_absent_class_is_unverified_not_dangling() {
    let dir = fixture_dir("unverified");
    plain(&dir);
    let registered: HashMap<String, NativeFn> =
        HashMap::from([("Ghost.missing()V".to_owned(), noop as NativeFn)]);
    let report =
        rava_cli::natives::collect(&class_path(&dir), "Plain", &registered, 100).expect("collect");
    assert_eq!(report.unverified, vec!["Ghost.missing()V".to_owned()]);
    assert!(report.dangling.is_empty());
    assert!(report.ok(), "an unchecked registration is not a defect");
}

#[test]
fn the_cap_is_reported() {
    let dir = fixture_dir("cap");
    native_owner(&dir);
    needs(&dir);
    let report = rava_cli::natives::collect(&class_path(&dir), "Needs", &HashMap::new(), 1)
        .expect("collect");
    assert!(report.truncated, "a cap of one class must truncate");
    assert!(report.visited <= 2);
}

#[test]
fn the_cli_exits_nonzero_when_a_native_is_missing() {
    let dir = fixture_dir("cli");
    native_owner(&dir);
    needs(&dir);
    let bin = env!("CARGO_BIN_EXE_rava");

    let missing = std::process::Command::new(bin)
        .args(["natives", "Needs", "--home"])
        .arg(&dir)
        .args(["--cp"])
        .arg(&dir)
        .output()
        .expect("run rava natives");
    assert_eq!(missing.status.code(), Some(1), "a missing native must fail");
    let stdout = String::from_utf8_lossy(&missing.stdout);
    assert!(
        stdout.contains("NativeOwner") && stdout.contains("ping()V  missing"),
        "report was:\n{stdout}"
    );

    let plain = std::process::Command::new(bin)
        .args(["natives", "Plain", "--home"])
        .arg(&dir)
        .args(["--cp"])
        .arg(&dir)
        .output()
        .expect("run rava natives");
    assert_eq!(plain.status.code(), Some(0), "a clean program must pass");
}

#[test]
fn the_host_image_reports_hundreds_of_missing_natives() {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    let path =
        rava_cli::class_path(Path::new(&home), Path::new("/nonexistent")).expect("class path");
    let report = rava_cli::natives::collect(&path, "java/lang/System", &HashMap::new(), 20_000)
        .expect("collect");
    // The walk is an over-approximation of what boot touches; the VM registers nothing yet, so
    // every declared native is missing.
    assert!(
        report.declared.len() > 600,
        "only {} natives declared",
        report.declared.len()
    );
    assert_eq!(report.missing.len(), report.declared.len());
    assert!(report.visited > 4_000, "visited {}", report.visited);
    assert!(!report.truncated);
    assert!(
        report
            .declared
            .iter()
            .any(|method| method.key() == "java/lang/System.registerNatives()V")
    );
}

/// The class path of the devShell image; the io natives need the real class layouts.
fn host_path() -> rava_vm::ClassPath {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    rava_cli::class_path(Path::new(&home), Path::new("/nonexistent")).expect("class path")
}

#[test]
fn the_output_natives_are_registered() {
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    for key in [
        "java/lang/System.registerNatives()V",
        "java/lang/System.setIn0(Ljava/io/InputStream;)V",
        "java/lang/System.setOut0(Ljava/io/PrintStream;)V",
        "java/lang/System.setErr0(Ljava/io/PrintStream;)V",
        "java/io/FileDescriptor.initIDs()V",
        "java/io/FileOutputStream.initIDs()V",
        "java/io/FileOutputStream.write(IZ)V",
        "java/io/FileOutputStream.writeBytes([BIIZ)V",
    ] {
        assert!(shared.natives.contains_key(key), "{key} is not registered");
    }
}

#[test]
fn file_output_stream_writes_to_file_descriptor_one() {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);

    let stream_class = shared
        .load_class(&path, "java/io/FileOutputStream")
        .expect("FileOutputStream");
    let descriptor_class = shared
        .load_class(&path, "java/io/FileDescriptor")
        .expect("FileDescriptor");

    // `FileDescriptor.out` is `new FileDescriptor(1)`; the native reads the int field.
    let descriptor = shared.allocate(rava_vm::HeapObject::Instance(rava_vm::Instance {
        class: descriptor_class,
        fields: vec![Value::Int(1)],
    }));
    let mut fields = vec![Value::NULL; shared.class(stream_class).layout.len()];
    let slot = shared
        .class(stream_class)
        .field_slot("fd", "Ljava/io/FileDescriptor;")
        .expect("FileOutputStream.fd");
    fields[slot] = Value::Ref(descriptor);
    let stream = shared.allocate(rava_vm::HeapObject::Instance(rava_vm::Instance {
        class: stream_class,
        fields,
    }));

    let data = shared.allocate(rava_vm::HeapObject::Array(rava_vm::Array {
        kind: rava_vm::ArrayKind::Byte,
        elements: vec![Value::Int(i32::from(b'h')), Value::Int(i32::from(b'i'))],
    }));

    let function = *shared
        .natives
        .get("java/io/FileOutputStream.writeBytes([BIIZ)V")
        .expect("registered");
    let mut thread = rava_vm::ThreadState::new();
    let args = [
        Value::Ref(stream),
        Value::Ref(data),
        Value::Int(0),
        Value::Int(2),
        Value::Int(0),
    ];
    let mut ctx = rava_vm::NativeCtx {
        shared: &mut shared,
        thread: &mut thread,
        path: &path,
        args: &args,
    };
    function(&mut ctx).expect("writeBytes");
    assert_eq!(ctx.shared.out.as_slice(), b"hi");
}

#[test]
fn set_out0_replaces_the_static_field() {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let system = shared
        .load_class(&path, "java/lang/System")
        .expect("System");
    shared.classes[system.index()]
        .statics
        .insert("out".to_owned(), Value::Int(1));

    let function = *shared
        .natives
        .get("java/lang/System.setOut0(Ljava/io/PrintStream;)V")
        .expect("registered");
    let mut thread = rava_vm::ThreadState::new();
    let args = [Value::NULL];
    let mut ctx = rava_vm::NativeCtx {
        shared: &mut shared,
        thread: &mut thread,
        path: &path,
        args: &args,
    };
    function(&mut ctx).expect("setOut0");
    assert_eq!(
        ctx.shared.classes[system.index()].statics.get("out"),
        Some(&Value::NULL)
    );
}

#[test]
fn the_report_sees_the_registered_natives() {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    let bin = env!("CARGO_BIN_EXE_rava");
    let output = std::process::Command::new(bin)
        .args(["natives", "java.lang.System", "--home"])
        .arg(&home)
        .args(["--max-classes", "200"])
        .output()
        .expect("run rava natives");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("registerNatives()V  registered"),
        "report was:\n{stdout}"
    );
    assert!(
        stdout.contains("setOut0(Ljava/io/PrintStream;)V  registered"),
        "report was:\n{stdout}"
    );
    // Many natives are still missing, so the run still fails.
    assert_eq!(output.status.code(), Some(1));
}

/// Runs a built fixture through the interpreter, with the real image as the boot class path.
fn run_fixture(dir: &Path, class: &str) -> rava_vm::Shared {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    let path = rava_cli::class_path(Path::new(&home), dir).expect("class path");
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, class, &[])
        .unwrap_or_else(|error| panic!("{class} failed: {error}"));
    shared
}

fn build_get_descriptor(dir: &Path) {
    let mut builder = ClassBuilder::new("GetDescriptor", "java/lang/Object", 65);
    let out = builder.field_ref("java/io/FileDescriptor", "out", "Ljava/io/FileDescriptor;");
    let code = vec![0xb2, (out >> 8) as u8, out as u8, 0x57, 0xb1];
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((1, 1, code)));
    write_class(dir, "GetDescriptor", builder.build());
}

fn build_set_out(dir: &Path) {
    let mut builder = ClassBuilder::new("SetOut", "java/lang/Object", 65);
    let out = builder.field_ref("java/io/FileDescriptor", "out", "Ljava/io/FileDescriptor;");
    let set_out = builder.method_ref("java/lang/System", "setOut0", "(Ljava/io/PrintStream;)V");
    let code = vec![
        0xb2,
        (out >> 8) as u8,
        out as u8, // getstatic FileDescriptor.out
        0xb8,
        (set_out >> 8) as u8,
        set_out as u8, // invokestatic System.setOut0
        0xb1,
    ];
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((1, 1, code)));
    write_class(dir, "SetOut", builder.build());
}

#[test]
fn file_descriptor_initializes_from_java() {
    // `FileDescriptor.<clinit>` calls `initIDs` and its constructor calls `getHandle`/`getAppend`;
    // reading `FileDescriptor.out` therefore exercises three natives and the class initializer.
    let dir = fixture_dir("fd-init");
    build_get_descriptor(&dir);
    run_fixture(&dir, "GetDescriptor");
}

#[test]
fn the_system_setter_runs_from_java() {
    let dir = fixture_dir("set-out");
    build_set_out(&dir);
    let shared = run_fixture(&dir, "SetOut");
    let system = shared
        .class_ids
        .get("java/lang/System")
        .expect("System is loaded");
    let out = shared.classes[system.index()]
        .statics
        .get("out")
        .expect("System.out exists");
    let Value::Ref(reference) = out else {
        panic!("System.out is {out:?}, not a reference");
    };
    match &shared.heap[reference.index()] {
        HeapObject::Instance(instance) => {
            assert_eq!(
                shared.classes[instance.class.index()].name,
                "java/io/FileDescriptor"
            );
        }
        other => panic!("System.out is {other:?}, not an object"),
    }
}

// --- natives the class-object probe reached on the io path -----------------------------------

fn call_native(name: &str, args: &[Value]) -> (Value, rava_vm::Shared) {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let function = *shared.natives.get(name).unwrap_or_else(|| panic!("{name}"));
    let mut thread = ThreadState::new();
    let value = {
        let mut ctx = NativeCtx {
            shared: &mut shared,
            thread: &mut thread,
            path: &path,
            args,
        };
        function(&mut ctx).unwrap_or_else(|error| panic!("{name}: {error}"))
    };
    (value, shared)
}

#[test]
fn cds_answers_no() {
    for key in [
        "jdk/internal/misc/CDS.isDumpingClassList0()Z",
        "jdk/internal/misc/CDS.isDumpingArchive0()Z",
        "jdk/internal/misc/CDS.isSharingEnabled0()Z",
    ] {
        assert_eq!(call_native(key, &[]).0, Value::Int(0), "{key}");
    }
}

#[test]
fn the_clock_moves_forward() {
    let (first, _) = call_native("java/lang/System.nanoTime()J", &[]);
    let (second, _) = call_native("java/lang/System.nanoTime()J", &[]);
    let Value::Long(first) = first else {
        panic!("nanoTime is not a long");
    };
    let Value::Long(second) = second else {
        panic!("nanoTime is not a long");
    };
    assert!(
        second >= first,
        "nanoTime went backwards: {first} then {second}"
    );

    let (millis, _) = call_native("java/lang/System.currentTimeMillis()J", &[]);
    let Value::Long(millis) = millis else {
        panic!("currentTimeMillis is not a long");
    };
    // After 2020 and before 2100: a wrong unit or epoch would fall outside.
    assert!(
        (1_577_836_800_000..4_102_444_800_000).contains(&millis),
        "currentTimeMillis is {millis}"
    );
}

#[test]
fn identity_hashes_are_stable_and_distinct() {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let object = shared
        .load_class(&path, "java/lang/Object")
        .expect("Object");
    let first = shared.allocate(HeapObject::Instance(rava_vm::Instance {
        class: object,
        fields: Vec::new(),
    }));
    let second = shared.allocate(HeapObject::Instance(rava_vm::Instance {
        class: object,
        fields: Vec::new(),
    }));
    let a = shared.identity_hash(first);
    assert_eq!(a, shared.identity_hash(first), "the hash must be stable");
    assert_ne!(a, shared.identity_hash(second), "two objects share a hash");
}

#[test]
fn float_and_double_bits_round_trip() {
    let (bits, _) = call_native(
        "java/lang/Float.floatToRawIntBits(F)I",
        &[Value::Float(1.5)],
    );
    assert_eq!(bits, Value::Int(0x3FC0_0000));
    let (value, _) = call_native("java/lang/Float.intBitsToFloat(I)F", &[bits]);
    assert_eq!(value, Value::Float(1.5));

    let (bits, _) = call_native(
        "java/lang/Double.doubleToRawLongBits(D)J",
        &[Value::Double(1.5)],
    );
    assert_eq!(bits, Value::Long(0x3FF8_0000_0000_0000));
    let (value, _) = call_native("java/lang/Double.longBitsToDouble(J)D", &[bits]);
    assert_eq!(value, Value::Double(1.5));
}

#[test]
fn primitive_class_objects_come_from_names() {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let name = shared.intern(&path, "int").expect("int");
    let function = *shared
        .natives
        .get("java/lang/Class.getPrimitiveClass(Ljava/lang/String;)Ljava/lang/Class;")
        .expect("registered");
    let mut thread = ThreadState::new();
    let args = [Value::Ref(name)];
    let mut ctx = NativeCtx {
        shared: &mut shared,
        thread: &mut thread,
        path: &path,
        args: &args,
    };
    let Value::Ref(int_class) = function(&mut ctx).expect("getPrimitiveClass") else {
        panic!("getPrimitiveClass did not return a class");
    };
    assert_eq!(ctx.shared.type_class_ref(int_class), Some("I"));
}

#[test]
fn available_processors_is_at_least_one() {
    let (value, _) = call_native("java/lang/Runtime.availableProcessors()I", &[]);
    let Value::Int(count) = value else {
        panic!("availableProcessors is not an int");
    };
    assert!(count >= 1, "availableProcessors is {count}");
}

#[test]
fn the_caller_class_is_the_frame_below_the_immediate_caller() {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let object = shared
        .load_class(&path, "java/lang/Object")
        .expect("Object");
    let string = shared
        .load_class(&path, "java/lang/String")
        .expect("String");
    let mut thread = ThreadState::new();
    thread.frames.push(rava_vm::Frame {
        class: object,
        method: 0,
        pc: 0,
        locals: Vec::new(),
        stack: Vec::new(),
    });
    thread.frames.push(rava_vm::Frame {
        class: string,
        method: 0,
        pc: 0,
        locals: Vec::new(),
        stack: Vec::new(),
    });
    let function = *shared
        .natives
        .get("jdk/internal/reflect/Reflection.getCallerClass()Ljava/lang/Class;")
        .expect("registered");
    let args: [Value; 0] = [];
    let mut ctx = NativeCtx {
        shared: &mut shared,
        thread: &mut thread,
        path: &path,
        args: &args,
    };
    let Value::Ref(caller) = function(&mut ctx).expect("getCallerClass") else {
        panic!("getCallerClass did not return a class");
    };
    assert_eq!(
        ctx.shared.class_ref(caller),
        Some(object),
        "the caller is String's caller"
    );
}

#[test]
fn class_queries_answer_from_the_target() {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();

    let string = shared
        .load_class(&path, "java/lang/String")
        .expect("String");
    let comparable = shared
        .load_class(&path, "java/lang/Comparable")
        .expect("Comparable");
    let string_class = shared.class_object(&path, string).expect("String.class");
    let comparable_class = shared
        .class_object(&path, comparable)
        .expect("Comparable.class");
    let int_class = shared.type_class(&path, "I").expect("int.class");
    let array_class = shared.type_class(&path, "[I").expect("[I.class");

    let call = |shared: &mut rava_vm::Shared,
                thread: &mut ThreadState,
                name: &str,
                args: &[Value]|
     -> Value {
        let function = *shared.natives.get(name).expect(name);
        let mut ctx = NativeCtx {
            shared,
            thread,
            path: &path,
            args,
        };
        function(&mut ctx).unwrap_or_else(|error| panic!("{name}: {error}"))
    };

    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            "java/lang/Class.isPrimitive()Z",
            &[Value::Ref(int_class)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            "java/lang/Class.isPrimitive()Z",
            &[Value::Ref(string_class)]
        ),
        Value::Int(0)
    );
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            "java/lang/Class.isArray()Z",
            &[Value::Ref(array_class)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            "java/lang/Class.isInterface()Z",
            &[Value::Ref(comparable_class)]
        ),
        Value::Int(1)
    );
    assert_eq!(
        call(
            &mut shared,
            &mut thread,
            "java/lang/Class.isInterface()Z",
            &[Value::Ref(string_class)]
        ),
        Value::Int(0)
    );

    let name = call(
        &mut shared,
        &mut thread,
        "java/lang/Class.initClassName()Ljava/lang/String;",
        &[Value::Ref(string_class)],
    );
    let Value::Ref(name) = name else {
        panic!("initClassName is not a string");
    };
    assert_eq!(
        shared
            .string_text(name)
            .expect("initClassName returns a String"),
        "java.lang.String"
    );
    // The native caches the name in the object, so a second call reads the field like the host's
    // `getName` does; both answers must agree.
    let class_class = shared.class_of(string_class).expect("java.lang.Class");
    let slot = shared
        .class(class_class)
        .field_slot("name", "Ljava/lang/String;")
        .expect("Class.name");
    assert!(matches!(
        shared.instance_field(string_class, slot).unwrap(),
        Value::Ref(cached) if cached == name
    ));

    let primitive_name = call(
        &mut shared,
        &mut thread,
        "java/lang/Class.initClassName()Ljava/lang/String;",
        &[Value::Ref(int_class)],
    );
    let Value::Ref(primitive_name) = primitive_name else {
        panic!("initClassName is not a string");
    };
    assert_eq!(shared.string_text(primitive_name).expect("a String"), "int");
}

#[test]
fn unsafe_reports_the_array_geometry() {
    let path = host_path();
    let mut shared = rava_vm::Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();

    for (descriptor, scale) in [
        ("[B", 1),
        ("[C", 2),
        ("[I", 4),
        ("[J", 8),
        ("[Ljava/lang/Object;", 1),
    ] {
        let class = shared.type_class(&path, descriptor).expect(descriptor);
        let call = |shared: &mut rava_vm::Shared, thread: &mut ThreadState, name: &str| -> Value {
            let function = *shared.natives.get(name).expect(name);
            let args = [Value::NULL, Value::Ref(class)];
            let mut ctx = NativeCtx {
                shared,
                thread,
                path: &path,
                args: &args,
            };
            function(&mut ctx).unwrap_or_else(|error| panic!("{name}: {error}"))
        };
        assert_eq!(
            call(
                &mut shared,
                &mut thread,
                "jdk/internal/misc/Unsafe.arrayBaseOffset0(Ljava/lang/Class;)I"
            ),
            Value::Int(0)
        );
        assert_eq!(
            call(
                &mut shared,
                &mut thread,
                "jdk/internal/misc/Unsafe.arrayIndexScale0(Ljava/lang/Class;)I"
            ),
            Value::Int(scale),
            "{descriptor}"
        );
    }
}
