//! `java.lang.System` natives beyond the standard streams (M2.23).
//!
//! The C1 `System` stub carries no `Code`, so every observable method is a native. This
//! module supplies the surface Spring's environment and diagnostics use:
//!
//! - the system property table (`getProperty`, `getProperties`, `setProperty`,
//!   `clearProperty`, `getenv`), seeded with the Java-17 properties Spring reads;
//! - identity hashes, clocks and `lineSeparator`;
//! - `arraycopy` over the heap's reference and primitive arrays.
//!
//! `System.out`/`System.err` and the `PrintStream` natives live in [`crate::library`];
//! this module does not touch them. A missing property is the Java `null` reference, not
//! an error. There is no security manager, no `System.exit` implementation yet (an exit
//! would have to unwind the runner, which is not part of M2.23), and `getProperties`
//! returns an ordinary mutable map with the `java.util.Properties` identity.

use crate::VmError;
use crate::heap::ObjRef;
use crate::loader::LoaderId;
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// The `java.lang.System` class.
const SYSTEM_CLASS: &str = "java/lang/System";

/// The `java.util.Properties` class.
const PROPERTIES_CLASS: &str = "java/util/Properties";

/// The `java.util.HashMap` class used for `System.getenv()`.
const HASH_MAP_CLASS: &str = "java/util/HashMap";

/// Register the `java.lang.System` natives.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        SYSTEM_CLASS,
        "identityHashCode",
        "(Ljava/lang/Object;)I",
        system_identity_hash_code,
    );
    shared.register_native(
        SYSTEM_CLASS,
        "currentTimeMillis",
        "()J",
        system_current_time_millis,
    );
    shared.register_native(SYSTEM_CLASS, "nanoTime", "()J", system_nano_time);
    shared.register_native(
        SYSTEM_CLASS,
        "lineSeparator",
        "()Ljava/lang/String;",
        system_line_separator,
    );
    shared.register_native(
        SYSTEM_CLASS,
        "getProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
        system_get_property,
    );
    shared.register_native(
        SYSTEM_CLASS,
        "getProperty",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        system_get_property_default,
    );
    shared.register_native(
        SYSTEM_CLASS,
        "getProperties",
        "()Ljava/util/Properties;",
        system_get_properties,
    );
    shared.register_native(
        SYSTEM_CLASS,
        "setProperty",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        system_set_property,
    );
    shared.register_native(
        SYSTEM_CLASS,
        "clearProperty",
        "(Ljava/lang/String;)Ljava/lang/String;",
        system_clear_property,
    );
    shared.register_native(SYSTEM_CLASS, "getenv", "()Ljava/util/Map;", system_getenv);
    shared.register_native(
        SYSTEM_CLASS,
        "getenv",
        "(Ljava/lang/String;)Ljava/lang/String;",
        system_getenv_name,
    );
    shared.register_native(
        SYSTEM_CLASS,
        "arraycopy",
        "(Ljava/lang/Object;ILjava/lang/Object;II)V",
        system_arraycopy,
    );
    shared.register_native(SYSTEM_CLASS, "gc", "()V", system_noop);
    shared.register_native(SYSTEM_CLASS, "runFinalization", "()V", system_noop);
    shared.register_native(
        SYSTEM_CLASS,
        "getSecurityManager",
        "()Ljava/lang/SecurityManager;",
        system_null,
    );
    // The VM is not attached to an interactive console, so `System.console()` is null and
    // Boot's color/terminal probes take the no-console path.
    shared.register_native(SYSTEM_CLASS, "console", "()Ljava/io/Console;", system_null);

    // `ManagementFactory.getRuntimeMXBean()`: Boot reads the process name (and Spring's
    // diagnostics read the VM/spec names). There is no management agent, so the MXBean is a
    // read-only view over the VM's fixed identity and the system-property table.
    shared.register_native(
        "java/lang/management/ManagementFactory",
        "getRuntimeMXBean",
        "()Ljava/lang/management/RuntimeMXBean;",
        management_runtime_mx_bean,
    );
    for (name, descriptor, function) in [
        (
            "getName",
            "()Ljava/lang/String;",
            runtime_bean_name as crate::NativeFn,
        ),
        ("getStartTime", "()J", runtime_bean_start_time),
        ("getUptime", "()J", runtime_bean_uptime),
        ("getVmName", "()Ljava/lang/String;", runtime_bean_vm_name),
        ("getVmVendor", "()Ljava/lang/String;", runtime_bean_vendor),
        ("getVmVersion", "()Ljava/lang/String;", runtime_bean_version),
        (
            "getSpecName",
            "()Ljava/lang/String;",
            runtime_bean_spec_name,
        ),
        (
            "getSpecVersion",
            "()Ljava/lang/String;",
            runtime_bean_spec_version,
        ),
        ("getSpecVendor", "()Ljava/lang/String;", runtime_bean_vendor),
        (
            "getManagementSpecVersion",
            "()Ljava/lang/String;",
            runtime_bean_spec_version,
        ),
        (
            "getClassPath",
            "()Ljava/lang/String;",
            runtime_bean_class_path,
        ),
        (
            "getBootClassPath",
            "()Ljava/lang/String;",
            runtime_bean_empty,
        ),
        (
            "getInputArguments",
            "()Ljava/util/List;",
            runtime_bean_input_arguments,
        ),
        (
            "getSystemProperties",
            "()Ljava/util/Map;",
            runtime_bean_system_properties,
        ),
    ] {
        shared.register_native(
            "java/lang/management/RuntimeMXBean",
            name,
            descriptor,
            function,
        );
    }

    // `Runtime.getRuntime()`: Boot's `BackgroundPreinitializer` asks for the processor
    // count while it decides whether to preinitialize in the background, and the
    // application shutdown hook registers through `Runtime`.
    shared.register_native(
        RUNTIME_CLASS,
        "getRuntime",
        "()Ljava/lang/Runtime;",
        runtime_get_runtime,
    );
    shared.register_native(
        RUNTIME_CLASS,
        "availableProcessors",
        "()I",
        runtime_available_processors,
    );
    shared.register_native(
        RUNTIME_CLASS,
        "addShutdownHook",
        "(Ljava/lang/Thread;)V",
        runtime_noop_thread,
    );
    shared.register_native(
        RUNTIME_CLASS,
        "removeShutdownHook",
        "(Ljava/lang/Thread;)Z",
        runtime_true,
    );

    // Boot's `ApplicationPid` asks the current process for its pid; the host pid is the
    // honest answer, and it is the only process surface this slice exposes.
    shared.register_native(
        PROCESS_HANDLE_CLASS,
        "current",
        "()Ljava/lang/ProcessHandle;",
        process_handle_current,
    );
    shared.register_native(PROCESS_HANDLE_CLASS, "pid", "()J", process_handle_pid);
    shared.register_native(PROCESS_HANDLE_CLASS, "isAlive", "()Z", runtime_true);
}

fn process_handle_current(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(PROCESS_HANDLE_CLASS)?;
    Ok(Some(Value::Ref(shared.allocate_instance(class)?)))
}

fn process_handle_pid(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(i64::from(std::process::id()))))
}

/// The `java.lang.Runtime` class.
const RUNTIME_CLASS: &str = "java/lang/Runtime";

/// The `java.lang.ProcessHandle` interface.
const PROCESS_HANDLE_CLASS: &str = "java/lang/ProcessHandle";

/// `Runtime.getRuntime()`: a fresh `Runtime` instance.
///
/// The JDK returns one singleton; nothing in the pinned startup path compares its
/// identity, and a fresh object keeps the VM free of a permanent root.
fn runtime_get_runtime(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(RUNTIME_CLASS)?;
    Ok(Some(Value::Ref(shared.allocate_instance(class)?)))
}

/// `Runtime.availableProcessors()`: the single interpreter thread.
fn runtime_available_processors(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(1)))
}

fn runtime_noop_thread(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    // A hook would run on a VM shutdown path this slice does not have; accepting the
    // registration keeps the application's startup flow intact.
    if matches!(args.first(), Some(Value::Ref(reference)) if reference.is_null()) {
        return Err(VmError::Native(
            "Runtime.addShutdownHook: null hook".to_owned(),
        ));
    }
    Ok(None)
}

fn runtime_true(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Int(1)))
}

fn heap_error(error: crate::HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<String, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => {
            let text = shared
                .heap()
                .string(reference)
                .map_err(heap_error)?
                .to_owned();
            Ok(text)
        }
        Some(Value::Ref(_)) => Ok("null".to_owned()),
        _ => Err(VmError::Native(format!(
            "{method} expects a String argument"
        ))),
    }
}

fn reference_argument(
    args: &[Value],
    position: usize,
    method: &'static str,
) -> Result<ObjRef, VmError> {
    match args.get(position).copied() {
        Some(Value::Ref(reference)) => Ok(reference),
        _ => Err(VmError::Native(format!(
            "{method} expects a reference argument"
        ))),
    }
}

fn int_argument(args: &[Value], position: usize, method: &'static str) -> Result<i32, VmError> {
    match args.get(position).copied() {
        Some(Value::Int(value)) => Ok(value),
        _ => Err(VmError::Native(format!("{method} expects an int argument"))),
    }
}

/// `System.identityHashCode(Object)`: the stable handle-derived hash; null is 0.
fn system_identity_hash_code(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = reference_argument(args, 0, "System.identityHashCode")?;
    let hash = if reference.is_null() {
        0
    } else {
        reference.identity_hash()
    };
    Ok(Some(Value::Int(hash)))
}

/// `System.currentTimeMillis()`.
fn system_current_time_millis(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Ok(Some(Value::Long(millis)))
}

/// `System.nanoTime()`.
fn system_nano_time(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    Ok(Some(Value::Long(nanos)))
}

/// `System.lineSeparator()`: this runtime's text line break.
fn system_line_separator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let text = shared.intern_string("\n");
    Ok(Some(Value::Ref(text)))
}

/// `System.getProperty(String)`: the value or null.
fn system_get_property(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "System.getProperty")?;
    match shared.property(&name) {
        Some(text) => {
            let value = shared.intern_string(&text);
            Ok(Some(Value::Ref(value)))
        }
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

/// `System.getProperty(String, String)`: the value or the caller's default.
fn system_get_property_default(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "System.getProperty")?;
    let fallback = reference_argument(args, 1, "System.getProperty")?;
    match shared.property(&name) {
        Some(text) => {
            let value = shared.intern_string(&text);
            Ok(Some(Value::Ref(value)))
        }
        None => Ok(Some(Value::Ref(fallback))),
    }
}

/// `System.getProperties()`: a `java.util.Properties` map with the current table.
fn system_get_properties(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(PROPERTIES_CLASS)?;
    let map = shared.heap().allocate_map(class);
    let mut entries = Vec::new();
    for (name, value) in shared.properties() {
        let key = shared.intern_string(&name);
        let value = shared.intern_string(&value);
        entries.push((Value::Ref(key), Value::Ref(value)));
    }
    {
        let mut heap = shared.heap();
        *heap.map_mut(map).expect("a freshly allocated map is a map") = entries;
    }
    Ok(Some(Value::Ref(map)))
}

/// `System.setProperty(String, String)`: store and return the previous value or null.
fn system_set_property(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "System.setProperty")?;
    let value = string_argument(shared, args, 1, "System.setProperty")?;
    match shared.set_property(&name, &value) {
        Some(previous) => {
            let text = shared.intern_string(&previous);
            Ok(Some(Value::Ref(text)))
        }
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

/// `System.clearProperty(String)`: remove and return the previous value or null.
fn system_clear_property(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "System.clearProperty")?;
    match shared.remove_property(&name) {
        Some(previous) => {
            let text = shared.intern_string(&previous);
            Ok(Some(Value::Ref(text)))
        }
        None => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

/// `System.getenv()`: a map of the host environment.
fn system_getenv(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class(HASH_MAP_CLASS)?;
    let map = shared.heap().allocate_map(class);
    let mut entries = Vec::new();
    for (name, value) in std::env::vars() {
        let key = shared.intern_string(&name);
        let value = shared.intern_string(&value);
        entries.push((Value::Ref(key), Value::Ref(value)));
    }
    {
        let mut heap = shared.heap();
        *heap.map_mut(map).expect("a freshly allocated map is a map") = entries;
    }
    Ok(Some(Value::Ref(map)))
}

/// `System.getenv(String)`: one environment value or null.
fn system_getenv_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = string_argument(shared, args, 0, "System.getenv")?;
    match std::env::var(&name) {
        Ok(value) => {
            let text = shared.intern_string(&value);
            Ok(Some(Value::Ref(text)))
        }
        Err(_) => Ok(Some(Value::Ref(ObjRef::NULL))),
    }
}

/// `System.arraycopy(Object, int, Object, int, int)`.
///
/// Both arguments must be arrays, the ranges must be inside them, and the source and
/// destination element kinds must agree. An overlapping copy within one array is handled
/// by copying the source range out first, so the result matches the JDK's `memmove`
/// semantics.
fn system_arraycopy(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let source = reference_argument(args, 0, "System.arraycopy")?;
    let source_position = int_argument(args, 1, "System.arraycopy")?;
    let destination = reference_argument(args, 2, "System.arraycopy")?;
    let destination_position = int_argument(args, 3, "System.arraycopy")?;
    let length = int_argument(args, 4, "System.arraycopy")?;
    if source.is_null() || destination.is_null() {
        return Err(VmError::Native(
            "System.arraycopy received a null array".to_owned(),
        ));
    }
    if source_position < 0 || destination_position < 0 || length < 0 {
        return Err(VmError::Native(
            "System.arraycopy received a negative position or length".to_owned(),
        ));
    }
    let copy: Vec<Value> = {
        let heap = shared.heap();
        let source_array = heap.array(source).map_err(heap_error)?;
        let destination_array = heap.array(destination).map_err(heap_error)?;
        if source_array.kind != destination_array.kind {
            return Err(VmError::Native(
                "System.arraycopy argument arrays have different element kinds".to_owned(),
            ));
        }
        let start = usize::try_from(source_position)
            .map_err(|_| VmError::Native("System.arraycopy source position overflow".to_owned()))?;
        let end = start
            .checked_add(
                usize::try_from(length)
                    .map_err(|_| VmError::Native("System.arraycopy length overflow".to_owned()))?,
            )
            .ok_or_else(|| VmError::Native("System.arraycopy range overflow".to_owned()))?;
        if end > source_array.elements.len() {
            return Err(VmError::Native(format!(
                "System.arraycopy source range {start}..{end} exceeds length {}",
                source_array.elements.len()
            )));
        }
        source_array.elements[start..end].to_vec()
    };
    {
        let mut heap = shared.heap();
        let destination_array = heap.array_mut(destination).map_err(heap_error)?;
        let start = usize::try_from(destination_position).map_err(|_| {
            VmError::Native("System.arraycopy destination position overflow".to_owned())
        })?;
        let end = start
            .checked_add(copy.len())
            .ok_or_else(|| VmError::Native("System.arraycopy range overflow".to_owned()))?;
        if end > destination_array.elements.len() {
            return Err(VmError::Native(format!(
                "System.arraycopy destination range {start}..{end} exceeds length {}",
                destination_array.elements.len()
            )));
        }
        destination_array.elements[start..end].copy_from_slice(&copy);
    }
    Ok(None)
}

fn system_noop(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(None)
}

fn system_null(
    _shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(ObjRef::NULL)))
}

// ---------------------------------------------------------------------------
// java.lang.management.RuntimeMXBean
// ---------------------------------------------------------------------------

fn management_runtime_mx_bean(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class("java/lang/management/RuntimeMXBean")?;
    Ok(Some(Value::Ref(shared.allocate_instance(class)?)))
}

fn runtime_bean_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let name = format!("{}@localhost", std::process::id());
    Ok(Some(Value::Ref(shared.heap().allocate_string(name))))
}

fn runtime_bean_start_time(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(shared.start_time_millis())))
}

fn runtime_bean_uptime(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Long(shared.uptime_millis())))
}

fn runtime_bean_vm_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.heap().allocate_string("rava".to_owned()),
    )))
}

fn runtime_bean_vendor(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.heap().allocate_string("rava".to_owned()),
    )))
}

fn runtime_bean_version(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.heap().allocate_string("17".to_owned()),
    )))
}

fn runtime_bean_spec_name(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(shared.heap().allocate_string(
        "Java Virtual Machine Specification".to_owned(),
    ))))
}

fn runtime_bean_spec_version(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.heap().allocate_string("17".to_owned()),
    )))
}

fn runtime_bean_class_path(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class_path = shared.property("java.class.path").unwrap_or_default();
    Ok(Some(Value::Ref(shared.heap().allocate_string(class_path))))
}

fn runtime_bean_empty(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    Ok(Some(Value::Ref(
        shared.heap().allocate_string(String::new()),
    )))
}

fn runtime_bean_input_arguments(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let class = shared.load_library_class("java/util/ArrayList")?;
    Ok(Some(Value::Ref(shared.heap().allocate_list(class))))
}

fn runtime_bean_system_properties(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let properties = shared.system_properties_snapshot();
    let class = shared.load_library_class("java/util/Properties")?;
    let map = shared.heap().allocate_map(class);
    let entries: Vec<(Value, Value)> = properties
        .into_iter()
        .map(|(key, value)| {
            (
                Value::Ref(shared.intern_string(&key)),
                Value::Ref(shared.intern_string(&value)),
            )
        })
        .collect();
    shared
        .heap()
        .map_mut(map)
        .map_err(|error| VmError::Native(error.to_string()))?
        .extend(entries);
    Ok(Some(Value::Ref(map)))
}
