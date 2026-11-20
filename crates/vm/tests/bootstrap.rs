//! S6 acceptance: the core classes are the host library's, not a substitute.
//!
//! R4: `java/lang/Object`, `java/lang/String`, `java/lang/Class`, `java/lang/System` and
//! `java/lang/Thread` come out of the image, with their real superclasses, interfaces and
//! method bodies. The loader resolves each class's superclass and interfaces on the way, so
//! loading the five is also a test of the closure they pull in — and of the verifier, which
//! runs on every method body at load.

use std::path::PathBuf;

use rava_vm::{Class, ClassId, ClassPath, Shared, Value};

const CORE: [&str; 5] = [
    "java/lang/Object",
    "java/lang/String",
    "java/lang/Class",
    "java/lang/System",
    "java/lang/Thread",
];

fn jdk_home() -> PathBuf {
    let home = std::env::var_os("JAVA21_HOME").unwrap_or_else(|| {
        panic!(
            "JAVA21_HOME is not set: run the tests inside `nix develop` (the devShell exports it)"
        )
    });
    PathBuf::from(home)
}

fn setup() -> (Shared, ClassPath) {
    let path = ClassPath::from_jdk_home(jdk_home(), "/nonexistent")
        .unwrap_or_else(|error| panic!("cannot open the host image: {error}"));
    (Shared::new(), path)
}

fn class_of(shared: &Shared, id: ClassId) -> &Class {
    &shared.classes[id.index()]
}

#[test]
fn the_core_classes_come_from_the_image() {
    let (mut shared, path) = setup();
    for name in CORE {
        let id = shared
            .load_class(&path, name)
            .unwrap_or_else(|error| panic!("{name} does not load: {error}"));
        let loaded = class_of(&shared, id);
        assert_eq!(loaded.name, name);
        assert!(
            loaded.file.is_some(),
            "{name} was synthesized instead of loaded"
        );
        assert!(!loaded.methods.is_empty(), "{name} has no methods");
    }
}

#[test]
fn the_hierarchy_is_the_host_librarys() {
    let (mut shared, path) = setup();
    let object = shared
        .load_class(&path, "java/lang/Object")
        .expect("Object");
    let string = shared
        .load_class(&path, "java/lang/String")
        .expect("String");
    let thread = shared
        .load_class(&path, "java/lang/Thread")
        .expect("Thread");
    let class_id = shared.load_class(&path, "java/lang/Class").expect("Class");

    let object_class = class_of(&shared, object);
    assert_eq!(object_class.super_class, None, "Object has no superclass");
    assert!(
        object_class
            .methods
            .iter()
            .any(|method| method.name == "getClass"),
        "Object is the host's: it declares getClass"
    );

    assert_eq!(class_of(&shared, string).super_class, Some(object));
    assert_eq!(class_of(&shared, thread).super_class, Some(object));
    assert_eq!(class_of(&shared, class_id).super_class, Some(object));

    // The interfaces are resolved, not just recorded as strings.
    let string_interfaces: Vec<&str> = class_of(&shared, string)
        .interfaces
        .iter()
        .map(|id| class_of(&shared, *id).name.as_str())
        .collect();
    assert!(
        string_interfaces.contains(&"java/lang/Comparable"),
        "String implements Comparable, found {string_interfaces:?}"
    );
    let thread_interfaces: Vec<&str> = class_of(&shared, thread)
        .interfaces
        .iter()
        .map(|id| class_of(&shared, *id).name.as_str())
        .collect();
    assert!(
        thread_interfaces.contains(&"java/lang/Runnable"),
        "Thread implements Runnable, found {thread_interfaces:?}"
    );
}

#[test]
fn the_closure_is_all_host_classes() {
    let (mut shared, path) = setup();
    for name in CORE {
        shared.load_class(&path, name).expect(name);
    }

    assert!(
        shared.classes.len() > 10,
        "the five pull in a closure, got {}",
        shared.classes.len()
    );
    for loaded in &shared.classes {
        assert!(
            loaded.file.is_some(),
            "{} was synthesized instead of loaded",
            loaded.name
        );
    }
    // The host library's own methods carry bodies; the decoded form is what the interpreter
    // runs, and a decode failure would have failed the load above.
    let with_bodies = shared
        .classes
        .iter()
        .flat_map(|class| class.methods.iter())
        .filter(|method| method.code.is_some())
        .count();
    assert!(
        with_bodies > 100,
        "only {with_bodies} method bodies in the closure"
    );

    // The native surface is real, not something this repository provides: Object.hashCode and
    // String.intern are declared native by the host library and have no Rust implementation yet.
    let natives = shared
        .classes
        .iter()
        .flat_map(|class| class.methods.iter())
        .filter(|method| method.is_native)
        .count();
    assert!(natives > 0, "the closure declares no native methods");
    let object = shared
        .classes
        .iter()
        .find(|class| class.name == "java/lang/Object")
        .expect("Object");
    assert!(
        object
            .methods
            .iter()
            .any(|method| method.name == "hashCode" && method.is_native && method.code.is_none()),
        "Object.hashCode is a native with no body"
    );
}

#[test]
fn static_finals_start_at_their_constant_value() {
    let (mut shared, path) = setup();

    // `String.LATIN1`/`UTF16` are the case that matters: without the attribute they read 0,
    // so the two coders would be indistinguishable.
    let string = shared
        .load_class(&path, "java/lang/String")
        .expect("String");
    let string = class_of(&shared, string);
    assert_eq!(string.statics.get("LATIN1"), Some(&Value::Int(0)));
    assert_eq!(string.statics.get("UTF16"), Some(&Value::Int(1)));

    let integer = shared
        .load_class(&path, "java/lang/Integer")
        .expect("Integer");
    assert_eq!(
        class_of(&shared, integer).statics.get("MAX_VALUE"),
        Some(&Value::Int(i32::MAX))
    );
    let long = shared.load_class(&path, "java/lang/Long").expect("Long");
    assert_eq!(
        class_of(&shared, long).statics.get("MAX_VALUE"),
        Some(&Value::Long(i64::MAX))
    );
}

#[test]
fn object_is_not_synthesized() {
    let (mut shared, path) = setup();
    shared
        .load_class(&path, "java/lang/Object")
        .expect("Object");
    let objects: Vec<&Class> = shared
        .classes
        .iter()
        .filter(|class| class.name == "java/lang/Object")
        .collect();
    assert_eq!(objects.len(), 1, "Object is loaded exactly once");
    assert!(objects[0].file.is_some(), "Object must come from the image");
}

#[test]
fn assignability_follows_the_host_hierarchy() {
    let (mut shared, path) = setup();
    let string = shared
        .load_class(&path, "java/lang/String")
        .expect("String");
    let object = shared
        .load_class(&path, "java/lang/Object")
        .expect("Object");
    let comparable = shared
        .load_class(&path, "java/lang/Comparable")
        .expect("Comparable");
    let runnable = shared
        .load_class(&path, "java/lang/Runnable")
        .expect("Runnable");

    assert!(
        shared.is_assignable(string, string),
        "a class is assignable to itself"
    );
    assert!(shared.is_assignable(string, object), "String is an Object");
    assert!(
        shared.is_assignable(string, comparable),
        "String implements Comparable"
    );
    assert!(
        !shared.is_assignable(object, string),
        "Object is not a String"
    );
    assert!(
        !shared.is_assignable(string, runnable),
        "String is not Runnable"
    );
}
