//! M0.3 acceptance: class sources and `(loader, name)` identity.

use std::path::PathBuf;
use std::sync::Arc;

use rava_vm::{DirectorySource, JarSource, LoaderId, Shared, VmError};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn hello_class() -> PathBuf {
    manifest_dir().join("../classfile/tests/fixtures/Hello.class")
}

fn hello_jar() -> PathBuf {
    manifest_dir().join("tests/fixtures/hello.jar")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rava-m03-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn class_dir(name: &str) -> PathBuf {
    let dir = scratch(name);
    std::fs::copy(hello_class(), dir.join("Hello.class")).unwrap();
    dir
}

#[test]
fn directory_source_loads_a_class() {
    let root = manifest_dir().join("../classfile/tests/fixtures");
    let vm = Shared::with_bootstrap(Arc::new(DirectorySource::new(root)));

    let id = vm.load_class(LoaderId::BOOTSTRAP, "Hello").unwrap();
    let entry = vm.entry(id).unwrap();
    assert_eq!(entry.loader, LoaderId::BOOTSTRAP);
    assert_eq!(entry.name, "Hello");
    assert_eq!(entry.file.major_version, 52);
}

#[test]
fn jar_source_loads_a_deflated_class() {
    let source = JarSource::open(hello_jar()).unwrap();
    let vm = Shared::with_bootstrap(Arc::new(source));

    let id = vm.load_class(LoaderId::BOOTSTRAP, "Hello").unwrap();
    assert_eq!(vm.entry(id).unwrap().name, "Hello");
}

#[test]
fn same_name_in_two_loaders_has_two_identities() {
    let bootstrap = scratch("bootstrap-empty");
    let left = class_dir("left");
    let right = class_dir("right");
    let vm = Shared::with_bootstrap(Arc::new(DirectorySource::new(bootstrap)));

    let left_loader = vm
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(DirectorySource::new(left)))
        .unwrap();
    let right_loader = vm
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(DirectorySource::new(right)))
        .unwrap();

    let left_id = vm.load_class(left_loader, "Hello").unwrap();
    let right_id = vm.load_class(right_loader, "Hello").unwrap();

    assert_ne!(left_id, right_id);
    assert_eq!(vm.entry(left_id).unwrap().loader, left_loader);
    assert_eq!(vm.entry(right_id).unwrap().loader, right_loader);
    assert_eq!(vm.class_count(), 2);
}

#[test]
fn parent_delegation_wins_over_the_application_source() {
    let bootstrap = class_dir("bootstrap-parent");
    let app = class_dir("app-child");
    let vm = Shared::with_bootstrap(Arc::new(DirectorySource::new(bootstrap)));
    let app_loader = vm
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(DirectorySource::new(app)))
        .unwrap();

    let id = vm.load_class(app_loader, "Hello").unwrap();
    assert_eq!(vm.entry(id).unwrap().loader, LoaderId::BOOTSTRAP);
}

#[test]
fn define_class_records_the_defining_loader_and_rejects_duplicates() {
    let bootstrap = scratch("define-bootstrap");
    let vm = Shared::with_bootstrap(Arc::new(DirectorySource::new(bootstrap)));
    let app = vm
        .add_loader(
            LoaderId::BOOTSTRAP,
            Arc::new(DirectorySource::new(scratch("define-app"))),
        )
        .unwrap();
    let bytes = std::fs::read(hello_class()).unwrap();

    let id = vm.define_class(app, "Hello", &bytes).unwrap();
    assert_eq!(vm.entry(id).unwrap().loader, app);
    assert!(matches!(
        vm.define_class(app, "Hello", &bytes),
        Err(VmError::DuplicateClass { .. })
    ));
}

#[test]
fn a_missing_class_is_reported_with_its_loader() {
    let bootstrap = scratch("missing-bootstrap");
    let vm = Shared::with_bootstrap(Arc::new(DirectorySource::new(bootstrap)));
    assert!(matches!(
        vm.load_class(LoaderId::BOOTSTRAP, "Missing"),
        Err(VmError::ClassNotFound { .. })
    ));
}
