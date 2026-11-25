//! `java.lang.module.ModuleFinder` — the empty system module set (M3.6).
//!
//! This VM has no JPMS module layer (ADR-0009 assigns resources, jars and modules to M4),
//! so the system has no observable modules. Spring's `PathMatchingResourcePatternResolver`
//! asks `ModuleFinder.ofSystem().findAll()` while it initializes, unless
//! `NativeDetector` reports a native image. Earlier slices advertised a GraalVM native
//! image to skip that call; M3.6 removes that claim because Spring's repackaged CGLIB
//! refuses run-time enhancement in a native image. The truthful answer for a VM without
//! modules is an empty set, which is what `ofSystem` returns here.
//!
//! `ofSystem` returns a real generated class, `rava/module/SystemModuleFinder`, defined
//! through [`crate::Shared::define_class`] and verified like any other class. It
//! implements `java.lang.module.ModuleFinder` and answers `findAll()` with an empty
//! `java.util.HashSet`; the remaining `ModuleFinder` methods (`find`, `of`, `compose`) are
//! not registered and fail with the ordinary "no native implementation" error.

use rava_classfile::{Assembler, ClassBuilder};

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `java.lang.module.ModuleFinder`'s internal name.
const MODULE_FINDER_CLASS: &str = "java/lang/module/ModuleFinder";

/// The runtime-private finder implementation's internal name.
const SYSTEM_FINDER_CLASS: &str = "rava/module/SystemModuleFinder";

/// `ModuleFinder.ofSystem()` and `findAll()`.
const OF_SYSTEM_DESCRIPTOR: &str = "()Ljava/lang/module/ModuleFinder;";
const FIND_ALL_DESCRIPTOR: &str = "()Ljava/util/Set;";

const ACC_PUBLIC: u16 = 0x0001;
const ACC_SUPER: u16 = 0x0020;
const ACC_NATIVE: u16 = 0x0100;

/// The class-file version every generated class uses (Java 17).
const CLASS_VERSION: u16 = 61;

/// The finder is `public super`, like every generated class.
const FINDER_CLASS_ACCESS: u16 = ACC_PUBLIC | ACC_SUPER;

/// `findAll` is `public native`.
const FIND_ALL_ACCESS: u16 = ACC_PUBLIC | ACC_NATIVE;

/// Install the finder class and register the two natives.
pub fn register_natives(shared: &Shared) -> Result<(), VmError> {
    install_system_finder(shared)?;
    shared.register_native(
        MODULE_FINDER_CLASS,
        "ofSystem",
        OF_SYSTEM_DESCRIPTOR,
        module_finder_of_system,
    );
    shared.register_native(
        SYSTEM_FINDER_CLASS,
        "findAll",
        FIND_ALL_DESCRIPTOR,
        system_finder_find_all,
    );
    Ok(())
}

/// Define the finder class once, in the bootstrap loader.
fn install_system_finder(shared: &Shared) -> Result<(), VmError> {
    if shared
        .lookup(LoaderId::BOOTSTRAP, SYSTEM_FINDER_CLASS)
        .is_some()
    {
        return Ok(());
    }
    shared.define_class(
        LoaderId::BOOTSTRAP,
        SYSTEM_FINDER_CLASS,
        &system_finder_bytes(),
    )?;
    Ok(())
}

/// The class file of the runtime-private finder implementation.
fn system_finder_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new(SYSTEM_FINDER_CLASS, "java/lang/Object", CLASS_VERSION)
        .with_access(FINDER_CLASS_ACCESS);
    builder.interface(MODULE_FINDER_CLASS);
    let object_init = builder.method_ref("java/lang/Object", "<init>", "()V");
    // `<init>()V { super(); }`
    let mut init = Assembler::new();
    init.aload(0);
    init.invokespecial(object_init);
    init.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        "()V",
        Some((
            1,
            1,
            init.finish().expect("assemble SystemModuleFinder.<init>"),
        )),
    );
    builder.method(FIND_ALL_ACCESS, "findAll", FIND_ALL_DESCRIPTOR, None);
    builder.build()
}

/// `ModuleFinder.ofSystem()`: a finder over this VM's (empty) system module set.
///
/// The generated finder has no fields, so allocation is enough; its `<init>` only calls
/// `Object.<init>`, which is the VM's no-op.
fn module_finder_of_system(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(SYSTEM_FINDER_CLASS)?;
    let finder = shared.allocate_instance(class)?;
    Ok(Some(Value::Ref(finder)))
}

/// `ModuleFinder.findAll()`: no system modules exist in this VM.
fn system_finder_find_all(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(crate::collections::HASH_SET_CLASS)?;
    let modules = shared.heap().allocate_set(class);
    Ok(Some(Value::Ref(modules)))
}
