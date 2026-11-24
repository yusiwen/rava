//! M2.3 acceptance: the committed C1 metadata tree loads through the runtime.
//!
//! The C1 stubs under `stdlib/c1/classes` are read through the ordinary
//! [`DirectorySource`] by the bootstrap loader. Loading parses a class file, decoding its
//! metadata reads attributes, and neither needs a `Code` attribute: nothing here is
//! initialized or executed.

use std::path::PathBuf;
use std::sync::Arc;

use rava_vm::{ClassEntry, ClassMetadata, DirectorySource, ElementValue, LoaderId, Shared};

fn c1_classes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/c1/classes")
}

fn library() -> Shared {
    Shared::with_library_dir(c1_classes())
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-m23-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn load(vm: &Shared, name: &str) -> (rava_vm::ClassId, ClassMetadata) {
    let id = vm
        .load_library_class(name)
        .unwrap_or_else(|error| panic!("cannot load {name}: {error}"));
    let metadata = vm
        .entry(id)
        .expect("a loaded class has an entry")
        .metadata()
        .unwrap_or_else(|error| panic!("cannot decode the metadata of {name}: {error}"));
    (id, metadata)
}

/// A stub with no `Code` attribute still loads: assert that no method carries one, and
/// that the class is not empty (so the check is not vacuous).
fn assert_no_code(entry: &ClassEntry) {
    assert!(
        !entry.file.methods.is_empty(),
        "{} has no methods to check",
        entry.name
    );
    for method in &entry.file.methods {
        let name = entry.file.method_name(method).unwrap();
        let descriptor = entry.file.method_descriptor(method).unwrap();
        assert!(
            entry.file.code(method).unwrap().is_none(),
            "{}.{name}{descriptor} carries a Code attribute",
            entry.name
        );
    }
}

#[test]
fn library_stubs_load_with_bootstrap_identity_and_no_code() {
    let vm = library();
    let names = [
        "java.lang.String",
        "java.lang.Class",
        "java.util.List",
        "java.lang.reflect.Executable",
    ];

    for name in names {
        let id = vm.load_library_class(name).unwrap();
        let entry = vm.entry(id).unwrap();
        assert_eq!(entry.id, id);
        assert_eq!(entry.loader, LoaderId::BOOTSTRAP);
        assert_eq!(entry.name, name);
        assert_eq!(entry.file.major_version, 61);
        assert_eq!(
            entry.file.this_class_name().unwrap(),
            name.replace('.', "/")
        );

        let metadata = entry.metadata().unwrap();
        assert_eq!(metadata.binary_name, name.replace('.', "/"));
        assert_eq!(metadata.java_name(), name);
        assert_no_code(&entry);
    }

    assert_eq!(vm.class_count(), names.len());

    // String has a concrete stub method (`length`) without a body. Loading it proves that
    // a signature-only class needs no code, while the metadata check above proves the
    // model does not decode `Code`.
    let string_id = vm.lookup(LoaderId::BOOTSTRAP, "java.lang.String").unwrap();
    let string = vm.entry(string_id).unwrap();
    let length = string
        .file
        .find_method("length", "()I")
        .unwrap()
        .expect("String.length()I");
    assert_eq!(length.access_flags & 0x0400, 0, "length is not abstract");
    assert!(string.file.code(length).unwrap().is_none());
}

#[test]
fn string_metadata_exposes_superclass_interfaces_and_members() {
    let vm = library();
    let (_, metadata) = load(&vm, "java.lang.String");

    assert_eq!(metadata.super_class.as_deref(), Some("java/lang/Object"));
    assert_eq!(
        metadata.interfaces,
        vec![
            "java/io/Serializable",
            "java/lang/Comparable",
            "java/lang/CharSequence",
            "java/lang/constant/Constable",
            "java/lang/constant/ConstantDesc",
        ]
    );
    let signature = metadata.signature.as_deref().expect("String signature");
    assert!(signature.contains("Ljava/io/Serializable;"), "{signature}");
    assert!(
        signature.contains("Ljava/lang/Comparable<Ljava/lang/String;>;"),
        "{signature}"
    );

    let field = metadata
        .field("CASE_INSENSITIVE_ORDER")
        .expect("CASE_INSENSITIVE_ORDER");
    assert_eq!(field.descriptor, "Ljava/util/Comparator;");
    assert!(field.is_static());
    assert!(field.is_public());

    let length = metadata.method("length", "()I").expect("length()I");
    assert!(!length.is_abstract());
    let char_at = metadata.method("charAt", "(I)C").expect("charAt(I)C");
    assert!(!char_at.is_static());
    assert!(
        metadata
            .method("compareTo", "(Ljava/lang/String;)I")
            .is_some()
    );
    assert!(
        metadata
            .method("compareTo", "(Ljava/lang/Object;)I")
            .is_some()
    );
}

#[test]
fn class_and_list_metadata_expose_generic_signatures_and_members() {
    let vm = library();
    let (_, class) = load(&vm, "java.lang.Class");

    assert!(!class.is_interface());
    assert_eq!(class.super_class.as_deref(), Some("java/lang/Object"));
    assert_eq!(
        class.interfaces,
        vec![
            "java/io/Serializable",
            "java/lang/reflect/GenericDeclaration",
            "java/lang/reflect/Type",
            "java/lang/reflect/AnnotatedElement",
            "java/lang/invoke/TypeDescriptor$OfField",
            "java/lang/constant/Constable",
        ]
    );
    let class_signature = class.signature.as_deref().expect("Class signature");
    assert!(
        class_signature.starts_with("<T:Ljava/lang/Object;>"),
        "{class_signature}"
    );
    assert!(
        class
            .method(
                "getRecordComponents",
                "()[Ljava/lang/reflect/RecordComponent;"
            )
            .is_some()
    );
    assert!(class.method("isRecord", "()Z").is_some());
    assert!(class.method("isSealed", "()Z").is_some());
    assert!(
        class
            .method("getPermittedSubclasses", "()[Ljava/lang/Class;")
            .is_some()
    );

    let (_, list) = load(&vm, "java.util.List");
    assert!(list.is_interface());
    assert_eq!(list.super_class.as_deref(), Some("java/lang/Object"));
    assert_eq!(list.interfaces, vec!["java/util/Collection"]);
    assert_eq!(
        list.signature.as_deref(),
        Some("<E:Ljava/lang/Object;>Ljava/lang/Object;Ljava/util/Collection<TE;>;")
    );

    let size = list.method("size", "()I").expect("List.size()I");
    assert!(size.is_abstract());
    let get = list
        .method("get", "(I)Ljava/lang/Object;")
        .expect("List.get");
    assert!(get.is_abstract());

    let of = list
        .method("of", "([Ljava/lang/Object;)Ljava/util/List;")
        .expect("List.of(E...)");
    assert!(of.is_static());
    assert!(of.is_varargs());
    assert!(
        of.annotations
            .iter()
            .any(|annotation| annotation.type_descriptor == "Ljava/lang/SafeVarargs;"),
        "List.of keeps its method annotation"
    );
}

#[test]
fn sealed_executable_metadata_keeps_permitted_subclasses() {
    let vm = library();
    let (_, metadata) = load(&vm, "java.lang.reflect.Executable");

    assert_eq!(
        metadata.super_class.as_deref(),
        Some("java/lang/reflect/AccessibleObject")
    );
    assert_eq!(
        metadata.interfaces,
        vec![
            "java/lang/reflect/Member",
            "java/lang/reflect/GenericDeclaration"
        ]
    );
    assert!(!metadata.is_interface());
    assert!(
        metadata.access_flags & 0x0400 != 0,
        "Executable is abstract"
    );
    assert!(metadata.is_sealed());
    let permitted: Vec<&str> = metadata
        .permitted_subclasses
        .as_deref()
        .expect("PermittedSubclasses")
        .iter()
        .map(String::as_str)
        .collect();
    assert_eq!(
        permitted,
        ["java/lang/reflect/Constructor", "java/lang/reflect/Method"]
    );
    assert!(
        metadata
            .method("getDeclaringClass", "()Ljava/lang/Class;")
            .expect("getDeclaringClass")
            .is_abstract()
    );

    // The committed JDK 17 API tree has sealed types but no record class; record metadata
    // is covered by the M2.1 fixtures, not by this loading slice.
    assert!(!metadata.is_record());
}

#[test]
fn annotation_metadata_keeps_meta_annotations_and_defaults() {
    let vm = library();
    let (_, metadata) = load(&vm, "java.lang.Deprecated");

    assert!(metadata.is_annotation_type());
    assert!(metadata.is_interface());
    assert_eq!(metadata.super_class.as_deref(), Some("java/lang/Object"));
    assert_eq!(metadata.interfaces, vec!["java/lang/annotation/Annotation"]);

    assert!(
        metadata
            .annotation("Ljava/lang/annotation/Documented;")
            .is_some()
    );
    let retention = metadata
        .annotation("Ljava/lang/annotation/Retention;")
        .expect("@Retention");
    assert!(matches!(
        retention.value("value"),
        Some(ElementValue::Enum {
            type_descriptor,
            constant,
        }) if type_descriptor == "Ljava/lang/annotation/RetentionPolicy;"
            && constant == "RUNTIME"
    ));

    let target = metadata
        .annotation("Ljava/lang/annotation/Target;")
        .expect("@Target");
    assert!(matches!(
        target.value("value"),
        Some(ElementValue::Array(values)) if values.len() == 8
    ));

    let since = metadata
        .method("since", "()Ljava/lang/String;")
        .expect("since()");
    assert!(matches!(
        since.annotation_default.as_ref(),
        Some(ElementValue::String(value)) if value.is_empty()
    ));
    let for_removal = metadata.method("forRemoval", "()Z").expect("forRemoval()");
    assert!(matches!(
        for_removal.annotation_default.as_ref(),
        Some(ElementValue::Boolean(false))
    ));
}

#[test]
fn library_metadata_convenience_matches_the_entry_path() {
    let vm = library();
    let combined = vm.library_metadata("java.util.List").unwrap();
    let id = vm.load_library_class("java.util.List").unwrap();
    let entry = vm.entry(id).unwrap();

    assert_eq!(combined, entry.metadata().unwrap());
    assert_eq!(entry.loader, LoaderId::BOOTSTRAP);
    assert_eq!(
        vm.lookup(LoaderId::BOOTSTRAP, "java.util.List"),
        Some(id),
        "a second load returns the same identity"
    );
}

#[test]
fn a_second_loader_gives_a_library_class_a_distinct_identity() {
    // Parent-first delegation would return the bootstrap class, so the identity case uses
    // an empty bootstrap and two child loaders over the same committed tree.
    let vm = Shared::with_bootstrap(Arc::new(DirectorySource::new(scratch("empty-bootstrap"))));
    let left = vm
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(c1_classes())),
        )
        .unwrap();
    let right = vm
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(c1_classes())),
        )
        .unwrap();

    let left_id = vm.load_class(left, "java.lang.String").unwrap();
    let right_id = vm.load_class(right, "java.lang.String").unwrap();

    assert_ne!(left_id, right_id);
    let left_entry = vm.entry(left_id).unwrap();
    let right_entry = vm.entry(right_id).unwrap();
    assert_eq!(left_entry.name, "java.lang.String");
    assert_eq!(right_entry.name, "java.lang.String");
    assert_eq!(left_entry.loader, left);
    assert_eq!(right_entry.loader, right);
    assert_eq!(
        left_entry.metadata().unwrap(),
        right_entry.metadata().unwrap()
    );
    assert_eq!(vm.class_count(), 2);
}
