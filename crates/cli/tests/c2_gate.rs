//! M3.7 acceptance: a real Spring AOP proxy of each kind, behind the C2 gate.
//!
//! The committed `demo.C2Gate` fixture is compiled with `javac --release 17` against the
//! pinned `compat/jars` set and committed under `tests/fixtures/aop/classes`, so this
//! suite needs no JDK, network or files beyond that fixture and the pinned jars.
//!
//! `C2Gate.run()` exercises one real path per proxy technology in a single run:
//!
//! 1. a full `@Configuration(proxyBeanMethods = true)` class is subclassed by Spring's
//!    repackaged CGLIB, and the `greeting()` bean method's call to `hello()` is routed
//!    through the bean factory (`hello!`, not `hello!` built from a second `"hello"`);
//! 2. a bare `Enhancer` over a superclass with a `MethodInterceptor`;
//! 3. `ObjenesisStd.newInstance` skipping a constructor but initializing the class;
//! 4. a JDK `Proxy` with an `InvocationHandler`;
//! 5. a `ProxyFactory` JDK dynamic proxy over a target and an advice;
//! 6. a `ProxyFactory` CGLIB proxy (`setProxyTargetClass(true)`), which is Spring's
//!    `CglibAopProxy` generating another subclass at run time.
//!
//! The committed fixture contains only `demo/C2Gate*` classes. Every proxy in the class
//! table below was therefore generated while the program ran, defined through the VM's
//! ordinary class-definition path and verified by the same verifier as any other class.
//!
//! Spring 6.2's repackaged CGLIB uses its own `SpringNamingPolicy`, so the marker on both
//! the enhanced configuration class and the CGLIB AOP proxy is `$$SpringCGLIB$$`; the
//! plain-CGLIB marker (`$$EnhancerByCGLIB$$`) names only the bare `Enhancer` result. The
//! fixture checks the truthful marker for each path.
//!
//! The pinned jars are fetched, not committed, so the suite skips with a clear message
//! when `compat/jars` is absent (a CI checkout without the fetch step). The milestone gate
//! sets `RAVA_C2_ACCEPTANCE=1`; in that mode a missing jar is a failure, because a skipped
//! C2 acceptance is not evidence for M3.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use rava_interp::run_static_method;
use rava_vm::{
    ClassPathSource, LoaderId, Shared, ThreadState, VerifyError, library, verify_method,
};

/// The marker a plain CGLIB `Enhancer` subclass carries in its name.
const CGLIB_MARKER: &str = "$$EnhancerByCGLIB$$";

/// The marker Spring 6.2's `SpringNamingPolicy` gives the enhanced `@Configuration` class
/// and every `CglibAopProxy` proxy.
const SPRING_CGLIB_MARKER: &str = "$$SpringCGLIB$$";

/// The name prefix of a proxy class the VM's `java.lang.reflect.Proxy` generates.
const PROXY_PREFIX: &str = "rava/proxy/$Proxy";

/// The exact stdout of `demo.C2Gate.run()`, one fact per line.
const EXPECTED_STDOUT: &str = "\
config-enhanced:true
greeting:hello!
cglib:cglib:hello
objenesis-value:0
objenesis-marker:7
jdk-proxy:proxy:greet
jdk-proxy-class:true
aop-jdk:aop:hello
aop-jdk-class:true
aop-cglib:aop:hello
aop-cglib-class:true
";

/// The Spring modules the fixture's classpath needs, in the pinned versions.
const SPRING_JARS: &[&str] = &[
    "jakarta.annotation-api-2.1.1.jar",
    "spring-aop-6.2.19.jar",
    "spring-beans-6.2.19.jar",
    "spring-context-6.2.19.jar",
    "spring-core-6.2.19.jar",
    "spring-expression-6.2.19.jar",
    "spring-jcl-6.2.19.jar",
];

/// The Unsafe U0 methods ADR-0005 allows in the call log.
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

fn aop_fixture_classes() -> PathBuf {
    manifest_dir().join("tests/fixtures/aop/classes")
}

/// The pinned Spring jars the fixture needs, if fetched.
///
/// The jars are fetched, not committed, so an ordinary `cargo test` checkout without
/// `compat/jars` skips. The milestone gate sets `RAVA_C2_ACCEPTANCE=1`; in that mode a
/// missing jar is a failure, because a skipped C2 acceptance is not evidence for M3.
fn spring_jars() -> Option<Vec<PathBuf>> {
    let jars = repo_root().join("compat/jars");
    let mut paths = Vec::with_capacity(SPRING_JARS.len());
    for name in SPRING_JARS {
        let path = jars.join(name);
        if path.is_file() {
            paths.push(path);
            continue;
        }
        if std::env::var("RAVA_C2_ACCEPTANCE").as_deref() == Ok("1") {
            panic!(
                "RAVA_C2_ACCEPTANCE=1 but the pinned jar {} is absent; run \
                 `bash compat/fetch-jars.sh`",
                path.display()
            );
        }
        return None;
    }
    Some(paths)
}

/// A library-backed VM plus the AOP fixture and the pinned Spring jars as the application
/// classpath, with every native the CLI registers for a user program.
///
/// `library::register_all` installs the `java.lang.module.ModuleFinder` natives too, so the
/// list matches `crates/cli/src/run.rs` exactly.
fn fixture(jars: &[PathBuf]) -> (Shared, LoaderId, ThreadState) {
    let shared = Shared::with_library_dir(c1_classes());
    let mut paths = jars.to_vec();
    paths.push(aop_fixture_classes());
    let source =
        ClassPathSource::from_paths(paths).expect("the fixture and jar classpath is readable");
    let application = shared
        .add_loader(LoaderId::BOOTSTRAP, Arc::new(source))
        .expect("the application loader is added");
    library::register_all(&shared).expect("the class library registers");
    rava_interp::reflect_invoke::register_natives(&shared);
    rava_interp::method_handles::register_natives(&shared);
    rava_interp::proxy::register_natives(&shared);
    rava_interp::unsafe_natives::register_natives(&shared).expect("the Unsafe natives register");
    rava_interp::callbacks::register_natives(&shared);
    rava_interp::streams::register_natives(&shared);
    rava_interp::futures::register_natives(&shared);
    rava_interp::object_methods::register_natives(&shared);
    let thread = shared.thread_state(1);
    (shared, application, thread)
}

/// Run `body` against a fresh fixture on a thread with the stack the interpreter needs.
///
/// Every Java call re-enters the interpreter as a Rust call, and Spring AOP's generation
/// path nests deeply (configuration enhancement, CGLIB/ASM emission, a JDK proxy, two
/// `ProxyFactory`s). The harness's default thread is too small, so the fixture runs on a
/// 64 MiB stack like a user program's main thread. The closure also decides the skip when
/// `compat/jars` is absent.
fn with_fixture<F>(body: F)
where
    F: FnOnce(&Shared, LoaderId, &ThreadState) + Send + 'static,
{
    let Some(jars) = spring_jars() else {
        eprintln!(
            "skipping the M3.7 Spring AOP acceptance: compat/jars is absent (run \
             `bash compat/fetch-jars.sh` to fetch the pinned jars)"
        );
        return;
    };
    std::thread::Builder::new()
        .name("aop-gate".to_owned())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let (shared, application, thread) = fixture(&jars);
            body(&shared, application, &thread);
        })
        .expect("the gate thread starts")
        .join()
        .expect("the gate thread did not panic");
}

fn run_gate(shared: &Shared, application: LoaderId, thread: &ThreadState) {
    let class = shared
        .load_class(application, "demo/C2Gate")
        .unwrap_or_else(|error| panic!("demo/C2Gate loads from the fixture: {error}"));
    run_static_method(shared, thread, class, "run", "()V", Vec::new())
        .unwrap_or_else(|error| panic!("demo/C2Gate.run() runs: {error}"));
}

/// Every class in the table whose name contains `marker`.
fn classes_with(shared: &Shared, marker: &str) -> Vec<Arc<rava_vm::ClassEntry>> {
    shared
        .class_entries()
        .into_iter()
        .filter(|entry| entry.name.contains(marker))
        .collect()
}

#[test]
fn spring_aop_proxies_run_through_the_verifier() {
    with_fixture(|shared, application, thread| {
        assert!(
            classes_with(shared, CGLIB_MARKER).is_empty()
                && classes_with(shared, SPRING_CGLIB_MARKER).is_empty()
                && classes_with(shared, PROXY_PREFIX).is_empty(),
            "the fixture commits no proxy class: every one is generated while the program runs"
        );
        let before = shared.class_count();

        run_gate(shared, application, thread);

        assert_eq!(
            shared.stdout(),
            EXPECTED_STDOUT,
            "the fixture's facts are exactly the pinned Spring behavior"
        );
        assert!(
            shared.class_count() > before,
            "the run defined proxy classes into the class table"
        );

        let enhancer = classes_with(shared, CGLIB_MARKER);
        assert_eq!(
            enhancer.len(),
            1,
            "the bare Enhancer defined one subclass, found {:?}",
            names(&enhancer)
        );
        let spring_cglib = classes_with(shared, SPRING_CGLIB_MARKER);
        assert!(
            spring_cglib.len() >= 2,
            "the configuration enhancement and the CGLIB AOP proxy defined Spring CGLIB \
             subclasses, found {:?}",
            names(&spring_cglib)
        );
        let proxies = classes_with(shared, PROXY_PREFIX);
        assert!(
            !proxies.is_empty(),
            "the JDK Proxy path defined rava/proxy/$ProxyN classes, found {:?}",
            names(&proxies)
        );

        // The two Spring paths really are the configuration class and the AOP target.
        assert!(
            spring_cglib
                .iter()
                .any(|entry| entry.name.starts_with("demo/C2Gate$Config$$SpringCGLIB$$")),
            "the @Configuration class was enhanced, found {:?}",
            names(&spring_cglib)
        );
        assert!(
            spring_cglib.iter().any(|entry| entry
                .name
                .starts_with("demo/C2Gate$GreeterImpl$$SpringCGLIB$$")),
            "the CGLIB AOP proxy subclasses the target, found {:?}",
            names(&spring_cglib)
        );

        let mut verified = 0;
        for entry in enhancer.iter().chain(&spring_cglib).chain(&proxies) {
            assert_eq!(
                entry.loader, application,
                "{} was defined in the application loader",
                entry.name
            );
            for method in &entry.file.methods {
                let name = entry
                    .file
                    .method_name(method)
                    .expect("the method name reads");
                let descriptor = entry
                    .file
                    .method_descriptor(method)
                    .expect("the descriptor reads");
                match verify_method(&entry.file, method) {
                    Ok(_) => verified += 1,
                    Err(VerifyError::NoCode) => {}
                    Err(error) => panic!(
                        "generated method {}.{name}{descriptor} failed verification: {error}",
                        entry.name
                    ),
                }
            }
        }
        assert!(
            verified > 0,
            "the generated proxy classes carry bytecode that the verifier accepts"
        );
    });
}

#[test]
fn the_gate_touches_only_the_unsafe_u0_surface() {
    with_fixture(|shared, application, thread| {
        run_gate(shared, application, thread);

        let calls = shared.unsafe_calls();
        assert!(!calls.is_empty(), "the run recorded its Unsafe calls");
        for call in &calls {
            let (owner, remainder) = call
                .split_once('.')
                .unwrap_or_else(|| panic!("call log entry {call} has no owner"));
            assert!(
                owner == "sun/misc/Unsafe" || owner == "jdk/internal/misc/Unsafe",
                "unexpected Unsafe owner in {call}"
            );
            let name = remainder
                .split_once('(')
                .unwrap_or_else(|| panic!("call log entry {call} has no descriptor"))
                .0;
            assert!(
                U0_METHODS.contains(&name),
                "the gate touched a non-U0 Unsafe method: {call}"
            );
        }
        assert!(
            calls
                .iter()
                .any(|call| call.contains("Unsafe.allocateInstance")),
            "Objenesis' allocation went through Unsafe.allocateInstance, found {calls:?}"
        );
    });
}

fn names(entries: &[Arc<rava_vm::ClassEntry>]) -> Vec<&str> {
    entries.iter().map(|entry| entry.name.as_str()).collect()
}
