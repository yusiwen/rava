//! M3.6 acceptance: Spring's repackaged CGLIB and Objenesis, unmodified.
//!
//! The committed `demo.CglibProbe`, `demo.ObjenesisProbe` and `demo.CglibRejectionProbe`
//! fixtures are compiled with `javac --release 17` against the pinned
//! `compat/jars/spring-core-6.2.19.jar` and committed under `tests/fixtures/cglib/classes`,
//! so this suite needs no JDK, network or files beyond that fixture and the pinned jar.
//!
//! The suite proves the run-time generation path end to end:
//!
//! 1. `Enhancer.create()` produces `demo/CglibProbe$Bean$$EnhancerByCGLIB$$<hash>` while
//!    the program runs; the fixture commits no such class, and the table has none before
//!    the probe runs;
//! 2. the generated class was defined in the application loader, extends
//!    `demo/CglibProbe$Bean`, declares `hello`/`add` with `Code`, and every method with a
//!    `Code` attribute passes the M3.1 verifier;
//! 3. the interceptor's answers are the whole captured stdout, so the generated overrides
//!    really dispatch through the CGLIB callback;
//! 4. `ObjenesisStd.newInstance` skips the constructor (`value=0`) but initializes the
//!    class (`marker=7`), and the `sun.misc.Unsafe` U0 log shows `allocateInstance` was
//!    reached through `Class.getDeclaredField("theUnsafe")`, not shortcut;
//! 5. a CGLIB rejection (a final superclass) stays CGLIB's real
//!    `IllegalArgumentException` and defines no class — there is no hand-built fallback.
//!
//! The pinned jar is fetched, not committed, so the suite skips with a clear message when
//! `compat/jars` is absent (a CI checkout without the fetch step). It never skips when the
//! jar is present: a missing fact is a failure, not a skip.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rava_interp::run_static_method;
use rava_vm::{
    ClassPathSource, LoaderId, Shared, ThreadState, VerifyError, library, verify_method,
};

/// The marker every run-time CGLIB subclass carries in its name.
const CGLIB_MARKER: &str = "$$EnhancerByCGLIB$$";

/// `demo/CglibProbe$Bean`, the superclass the enhancer subclasses.
const BEAN: &str = "demo/CglibProbe$Bean";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir()
        .parent()
        .and_then(Path::parent)
        .expect("the cli crate is inside the workspace")
        .to_path_buf()
}

fn c1_classes() -> PathBuf {
    repo_root().join("stdlib/c1/classes")
}

fn cglib_fixture_classes() -> PathBuf {
    manifest_dir().join("tests/fixtures/cglib/classes")
}

/// The pinned jar that carries Spring's repackaged CGLIB, ASM and Objenesis, if fetched.
///
/// The jars are fetched, not committed, so an ordinary `cargo test` checkout without
/// `compat/jars` skips. The milestone gate sets `RAVA_C2_ACCEPTANCE=1`; in that mode a
/// missing jar is a failure, because a skipped C2 acceptance is not evidence for M3.
fn spring_core_jar() -> Option<PathBuf> {
    let path = repo_root().join("compat/jars/spring-core-6.2.19.jar");
    if path.is_file() {
        return Some(path);
    }
    if std::env::var("RAVA_C2_ACCEPTANCE").as_deref() == Ok("1") {
        panic!(
            "RAVA_C2_ACCEPTANCE=1 but the pinned jar {} is absent; run `bash compat/fetch-jars.sh`",
            path.display()
        );
    }
    None
}

/// A library-backed VM plus the CGLIB fixture and the pinned `spring-core` jar as the
/// application classpath, with every native the CLI registers for a user program.
///
/// `spring-core` contains Spring's repackaged `org.springframework.cglib`,
/// `org.springframework.asm` and `org.springframework.objenesis`, so no other jar is
/// needed: neither CGLIB nor Objenesis is replaced or pre-generated here.
fn fixture(spring_core: &Path) -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let source = ClassPathSource::from_paths([cglib_fixture_classes(), spring_core.to_path_buf()])
        .expect("the fixture classpath is readable");
    let application = shared
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(source))
        .expect("the application loader is added");
    library::register_all(&shared).expect("the class library registers");
    rava_interp::reflect_invoke::register_natives(&shared);
    rava_interp::method_handles::register_natives(&shared);
    rava_interp::unsafe_natives::register_natives(&shared).expect("the Unsafe natives register");
    rava_interp::proxy::register_natives(&shared);
    rava_interp::callbacks::register_natives(&shared);
    rava_interp::streams::register_natives(&shared);
    // CGLIB's `LoadingCache` runs each generated class through a `FutureTask`, and its
    // cache key is a record whose `hashCode` is an `ObjectMethods` call site.
    rava_interp::futures::register_natives(&shared);
    rava_interp::object_methods::register_natives(&shared);
    let thread = shared.thread_state(1);
    (shared, application, thread)
}

/// Run `body` against a fresh fixture on a thread with the stack the interpreter needs.
///
/// Every Java call re-enters the interpreter as a Rust call, and CGLIB's generation path
/// nests deeply (class loading, reflection, ASM emission, class definition). The test
/// harness's default 2 MiB thread is too small, so the fixture runs on a 64 MiB stack like
/// a user program's main thread. The closure also decides the skip when the jar is absent.
fn with_fixture<F>(body: F)
where
    F: FnOnce(&Shared, LoaderId, &ThreadState) + Send + 'static,
{
    let Some(spring_core) = spring_core_jar() else {
        eprintln!(
            "skipping the M3.6 CGLIB acceptance: compat/jars is absent (run \
             `bash compat/fetch-jars.sh` to fetch the pinned jars)"
        );
        return;
    };
    std::thread::Builder::new()
        .name("cglib-probe".to_owned())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let (shared, application, thread) = fixture(&spring_core);
            body(&shared, application, &thread);
        })
        .expect("the probe thread starts")
        .join()
        .expect("the probe thread did not panic");
}

fn run_probe(shared: &Shared, application: LoaderId, thread: &ThreadState, name: &str) {
    let class = shared
        .load_class(application, name)
        .unwrap_or_else(|error| panic!("{name} loads from the fixture: {error}"));
    run_static_method(shared, thread, class, "run", "()V", Vec::new())
        .unwrap_or_else(|error| panic!("{name}.run() runs: {error}"));
}

/// Every class in the table whose name carries CGLIB's marker.
fn enhancer_classes(shared: &Shared) -> Vec<Arc<rava_vm::ClassEntry>> {
    shared
        .class_entries()
        .into_iter()
        .filter(|entry| entry.name.contains(CGLIB_MARKER))
        .collect()
}

#[test]
fn cglib_generates_an_interceptor_subclass_through_the_verifier() {
    with_fixture(|shared, application, thread| {
        assert!(
            enhancer_classes(shared).is_empty(),
            "no CGLIB class exists before the probe runs: the fixture commits none"
        );
        let before = shared.class_count();

        run_probe(shared, application, thread, "demo/CglibProbe");

        assert_eq!(
            shared.stdout(),
            "intercepted:hello\n5\ntrue\n",
            "the interceptor answers and the generated class name carries CGLIB's marker"
        );

        let generated = enhancer_classes(shared);
        assert_eq!(
            generated.len(),
            1,
            "exactly one class was generated at run time, found {:?}",
            generated
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>()
        );
        let entry = &generated[0];
        assert!(
            entry
                .name
                .starts_with("demo/CglibProbe$Bean$$EnhancerByCGLIB$$"),
            "CGLIB's naming policy named the class: {}",
            entry.name
        );
        assert_eq!(
            entry.loader, application,
            "the generated class was defined in the application loader"
        );
        assert_eq!(
            entry
                .file
                .super_class_name()
                .expect("the superclass name reads"),
            Some(BEAN),
            "the generated class subclasses the enhancer's superclass"
        );
        assert!(
            shared.lookup(application, BEAN).is_some(),
            "the superclass is the fixture's Bean, loaded in the application loader"
        );
        assert!(
            shared.class_count() > before,
            "the generated class was inserted into the class table"
        );

        // Every method CGLIB emitted passes the M3.1 verifier, and the two intercepted
        // methods are real overrides with bytecode.
        let file = &entry.file;
        let mut verified = 0;
        for method in &file.methods {
            let name = file.method_name(method).expect("the method name reads");
            let descriptor = file
                .method_descriptor(method)
                .expect("the descriptor reads");
            match verify_method(file, method) {
                Ok(_) => verified += 1,
                Err(VerifyError::NoCode) => {}
                Err(error) => {
                    panic!("generated method {name}{descriptor} failed verification: {error}")
                }
            }
        }
        assert!(
            verified >= 2,
            "the generated class carries the intercepted overrides"
        );
        for (name, descriptor) in [("hello", "()Ljava/lang/String;"), ("add", "(II)I")] {
            let method = file
                .find_method(name, descriptor)
                .expect("the constant pool resolves")
                .unwrap_or_else(|| panic!("the generated class overrides {name}{descriptor}"));
            assert!(
                method.has_code(),
                "the generated override {name}{descriptor} has a Code attribute"
            );
        }
    });
}

#[test]
fn objenesis_allocates_without_the_constructor_through_unsafe() {
    with_fixture(|shared, application, thread| {
        run_probe(shared, application, thread, "demo/ObjenesisProbe");

        assert_eq!(
            shared.stdout(),
            "value=0\nmarker=7\n",
            "Objenesis skipped the constructor but initializing the class ran its static block"
        );
        assert!(
            shared
                .unsafe_calls()
                .iter()
                .any(|call| call.contains("Unsafe.allocateInstance")),
            "the allocation went through Unsafe.allocateInstance, found {:?}",
            shared.unsafe_calls()
        );
    });
}

#[test]
fn a_cglib_rejection_stays_a_real_error_without_a_fallback() {
    with_fixture(|shared, application, thread| {
        run_probe(shared, application, thread, "demo/CglibRejectionProbe");

        assert_eq!(
            shared.stdout(),
            "Cannot subclass final class demo.CglibRejectionProbe$FinalBean\n",
            "the enhancer's own IllegalArgumentException reached the probe unchanged"
        );
        assert!(
            enhancer_classes(shared).is_empty(),
            "a rejected enhancement defines no class at all"
        );
    });
}
