//! S20 acceptance: `Class.getInterfaces0` answers from the loader's model.
//!
//! The host's `Class.getInterfaces()` calls this native and clones the array (array `clone` is
//! the stack-trace slice's; the direct native is what this slice can pin without it). A class
//! built by the fixture implements one interface; `String`'s set comes from the host image.

use std::path::{Path, PathBuf};

use rava_classfile::ClassBuilder;
use rava_vm::native::NativeCtx;
use rava_vm::{ClassPath, Shared, ThreadState, Value, VmError};

fn jdk_home() -> std::ffi::OsString {
    std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    })
}

fn fixture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-ifaces-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("fixture directory");
    dir
}

fn build_has_iface(dir: &Path) {
    let mut builder = ClassBuilder::new("HasIface", "java/lang/Object", 65);
    builder.interface("java/lang/Runnable");
    std::fs::write(dir.join("HasIface.class"), builder.build()).expect("fixture class");
}

fn call(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    args: &[Value],
) -> Result<Value, VmError> {
    let key = rava_vm::native::key("java/lang/Class", "getInterfaces0", "()[Ljava/lang/Class;");
    let function = *shared.natives.get(&key).expect("getInterfaces0 registered");
    let mut ctx = NativeCtx {
        shared,
        thread,
        path,
        args,
    };
    function(&mut ctx)
}

fn interface_names(shared: &Shared, array: rava_vm::ObjRef) -> Vec<String> {
    shared
        .array(array)
        .expect("an array")
        .elements
        .iter()
        .map(|element| {
            let Value::Ref(reference) = element else {
                panic!("{element:?}");
            };
            match shared.class_targets.get(reference) {
                Some(rava_vm::ClassTarget::Loaded(class)) => shared.class(*class).name.clone(),
                Some(rava_vm::ClassTarget::Type(descriptor)) => descriptor.clone(),
                None => panic!("{reference:?} is not a class object"),
            }
        })
        .collect()
}

#[test]
fn a_fixture_class_reports_its_declared_interface() {
    let dir = fixture_dir("fixture");
    build_has_iface(&dir);
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), &dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    let has_iface = shared
        .load_class(&path, "HasIface")
        .expect("HasIface loads");
    let class = shared.class_object(&path, has_iface).expect("class object");
    let array =
        call(&mut shared, &mut thread, &path, &[Value::Ref(class)]).expect("getInterfaces0");
    let Value::Ref(array) = array else {
        panic!("{array:?}");
    };
    assert_eq!(
        interface_names(&shared, array),
        vec!["java/lang/Runnable".to_owned()]
    );
}

#[test]
fn a_host_class_reports_its_interface_set() {
    let dir = fixture_dir("host");
    build_has_iface(&dir);
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), &dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    let string = shared
        .load_class(&path, "java/lang/String")
        .expect("String loads");
    let class = shared.class_object(&path, string).expect("class object");
    let array =
        call(&mut shared, &mut thread, &path, &[Value::Ref(class)]).expect("getInterfaces0");
    let Value::Ref(array) = array else {
        panic!("{array:?}");
    };
    let names = interface_names(&shared, array);
    assert_eq!(names.len(), 5, "JDK 21's String implements five interfaces");
    assert!(
        names.iter().any(|name| name == "java/lang/Comparable"),
        "{names:?}"
    );
    assert!(
        names.iter().any(|name| name == "java/lang/CharSequence"),
        "{names:?}"
    );
}

#[test]
fn an_array_class_reports_none() {
    let dir = fixture_dir("array");
    build_has_iface(&dir);
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), &dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    let array_class = shared.type_class(&path, "[I").expect("int[] class");
    let array =
        call(&mut shared, &mut thread, &path, &[Value::Ref(array_class)]).expect("getInterfaces0");
    let Value::Ref(array) = array else {
        panic!("{array:?}");
    };
    assert!(interface_names(&shared, array).is_empty());
}

/// The whole public path: `getInterfaces()` clones `getInterfaces0()`'s array, which needs the
/// array `clone` native from S19. The fixture runs the host method over a class it implements.
#[test]
fn the_public_get_interfaces_runs_end_to_end() {
    let mut builder = ClassBuilder::new("HasIface", "java/lang/Object", 65);
    builder.interface("java/lang/Runnable");
    builder.field(0x0009, "LEN", "I");
    builder.field(0x0009, "NAME", "Ljava/lang/String;");
    let len_field = builder.field_ref("HasIface", "LEN", "I");
    let name_field = builder.field_ref("HasIface", "NAME", "Ljava/lang/String;");
    let has_iface = builder.class("HasIface");
    let get_interfaces =
        builder.method_ref("java/lang/Class", "getInterfaces", "()[Ljava/lang/Class;");
    let get_name = builder.method_ref("java/lang/Class", "getName", "()Ljava/lang/String;");
    let mut code = Vec::new();
    let ldc = |index: u16, code: &mut Vec<u8>| {
        code.extend_from_slice(&[0x13, (index >> 8) as u8, index as u8]);
    };
    code.extend_from_slice(&[0x13, (has_iface >> 8) as u8, has_iface as u8]);
    code.extend_from_slice(&[0xb6, (get_interfaces >> 8) as u8, get_interfaces as u8]);
    code.push(0xbe); // arraylength
    code.extend_from_slice(&[0xb3, (len_field >> 8) as u8, len_field as u8]);
    ldc(has_iface, &mut code);
    code.extend_from_slice(&[0xb6, (get_interfaces >> 8) as u8, get_interfaces as u8]);
    code.push(0x03); // iconst_0
    code.push(0x32); // aaload
    code.extend_from_slice(&[0xb6, (get_name >> 8) as u8, get_name as u8]);
    code.extend_from_slice(&[0xb3, (name_field >> 8) as u8, name_field as u8]);
    code.push(0xb1); // return
    builder.method(0x0009, "main", "([Ljava/lang/String;)V", Some((2, 1, code)));

    let dir = fixture_dir("public");
    std::fs::write(dir.join("HasIface.class"), builder.build()).expect("fixture class");
    let path: ClassPath = rava_cli::class_path(Path::new(&jdk_home()), &dir).expect("class path");
    let mut shared = Shared::new();
    rava_native::register_jcl_natives(&mut shared);
    let mut thread = ThreadState::new();
    rava_interp::run_main(&mut shared, &mut thread, &path, "HasIface", &[])
        .unwrap_or_else(|error| panic!("HasIface failed: {error}"));
    let id = shared.class_ids["HasIface"];
    let statics = &shared.classes[id.index()].statics;
    assert_eq!(statics.get("LEN"), Some(&Value::Int(1)));
    let Some(Value::Ref(name)) = statics.get("NAME") else {
        panic!("NAME is {:?}", statics.get("NAME"));
    };
    assert_eq!(
        shared.string_text(*name).expect("a String"),
        "java.lang.Runnable"
    );
}
