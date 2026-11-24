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
