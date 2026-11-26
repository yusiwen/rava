//! `java.util.ServiceLoader` over `META-INF/services` (M4.2).
//!
//! The provider configuration is a classpath resource: `META-INF/services/<service binary
//! name>`, read through [`Shared::read_resources`] so every source supplies its providers
//! in classpath order. `load` eagerly loads and validates every provider class, matching
//! the JDK's configuration error timing, but it never constructs one. `iterator` records
//! the provider list in the [`Shared::service_state`] side table and returns an ordinary
//! `java.util.Iterator` heap object; `hasNext` only reads the cursor, and `next`
//! allocates the provider and runs `<init>()V` through [`crate::run_instance_method`], so
//! class initialization and constructor effects happen on first use.
//!
//! These natives live in `rava-interp`, not `rava-vm`, for the same reason
//! `Method.invoke` does: instantiating a provider re-enters the interpreter's frame
//! machinery, and `rava-vm` may not depend on `rava-interp`. The `Iterator.hasNext` and
//! `Iterator.next` registry entries are overridden here and delegate to
//! [`rava_vm::collections`] for every receiver without service state, so ordinary
//! snapshot iterators keep their existing behavior. `Iterator.remove` is deliberately not
//! overridden: the service iterator's snapshot is empty and has no mutable source, which
//! the VM already reports as a typed error.
//!
//! Limits: one provider name per line (a `#` starts a comment), no module layer or
//! `loadInstalled`, no `Provider`/`stream`/`findFirst` surface, no deduplication beyond
//! the same class identity, and a configuration or constructor failure is a typed
//! [`rava_vm::VmError::Native`] rather than a `ServiceConfigurationError` object.

use rava_vm::collections::{
    iterator_has_next as snapshot_has_next, iterator_next as snapshot_next,
};
use rava_vm::reflection;
use rava_vm::services::{ServiceIteratorState, ServiceLoaderState};
use rava_vm::{ClassId, HeapObject, LoaderId, ObjRef, Shared, ThreadState, Value, VmError};

use crate::{InterpError, run_instance_method};

/// The runtime `java.util.ServiceLoader` class.
const SERVICE_LOADER_CLASS: &str = "java/util/ServiceLoader";

/// The `java.util.Iterator` interface whose natives the lazy iterator overrides.
const ITERATOR_CLASS: &str = "java/util/Iterator";

/// The resource prefix; the service binary name is appended with dots.
const SERVICE_PREFIX: &str = "META-INF/services/";

/// Registers the `ServiceLoader` natives and the lazy `Iterator` overrides.
///
/// The runner installs this after [`rava_vm::library::register_all`], because the two
/// `Iterator` entries replace the snapshot implementations for every iterator and must
/// fall back to them.
pub fn register_natives(shared: &Shared) {
    shared.register_native(
        SERVICE_LOADER_CLASS,
        "load",
        "(Ljava/lang/Class;)Ljava/util/ServiceLoader;",
        load,
    );
    shared.register_native(
        SERVICE_LOADER_CLASS,
        "load",
        "(Ljava/lang/Class;Ljava/lang/ClassLoader;)Ljava/util/ServiceLoader;",
        load_with_loader,
    );
    shared.register_native(
        SERVICE_LOADER_CLASS,
        "iterator",
        "()Ljava/util/Iterator;",
        iterator,
    );
    shared.register_native(ITERATOR_CLASS, "hasNext", "()Z", iterator_has_next);
    shared.register_native(
        ITERATOR_CLASS,
        "next",
        "()Ljava/lang/Object;",
        iterator_next,
    );
}

/// `ServiceLoader.load(Class)`: use the application loader, which is this VM's context
/// loader and the only loader a runner installs for a program.
fn load(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let service = class_argument(shared, args, 0, "ServiceLoader.load")?;
    let loader = shared.application_loader();
    load_service(shared, service, loader).map(|instance| Some(Value::Ref(instance)))
}

/// `ServiceLoader.load(Class, ClassLoader)`: a null loader selects the bootstrap loader,
/// and any other object is mapped to the child loader it represents.
fn load_with_loader(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let service = class_argument(shared, args, 0, "ServiceLoader.load")?;
    let Some(Value::Ref(reference)) = args.get(1).copied() else {
        return Err(VmError::Native(
            "ServiceLoader.load expects a ClassLoader argument".to_owned(),
        ));
    };
    let loader = if reference.is_null() {
        LoaderId::BOOTSTRAP
    } else {
        shared.loader_id_for(reference)?
    };
    load_service(shared, service, loader).map(|instance| Some(Value::Ref(instance)))
}

/// Read every provider configuration, load and validate each provider, and attach the
/// resulting state to a new ordinary `java.util.ServiceLoader` instance.
///
/// Only class loading happens here: no provider is allocated or constructed until the
/// returned loader's iterator calls `next`.
fn load_service(shared: &Shared, service: ClassId, loader: LoaderId) -> Result<ObjRef, VmError> {
    let service_name = class_binary_name(shared, service)?;
    let resource = format!("{SERVICE_PREFIX}{service_name}");
    let mut providers = Vec::new();
    for bytes in shared.read_resources(loader, &resource)? {
        for provider_name in parse_provider_config(&bytes, &resource)? {
            // The configuration names a class with dots, but the class table's identity is
            // the internal name every other loader path uses (`ldc`, `Class.forName`,
            // `ClassLoader.loadClass`). Normalize before loading so the provider has the
            // same identity as a later lookup by internal name.
            let internal_name = provider_name.replace('.', "/");
            let provider =
                shared
                    .load_class(loader, &internal_name)
                    .map_err(|error| match error {
                        VmError::ClassNotFound { .. } => VmError::Native(format!(
                            "ServiceLoader provider {provider_name} of service {service_name} \
                         was not found from loader {}",
                            loader.0
                        )),
                        other => other,
                    })?;
            if !reflection::named_assignable(shared, service, provider)? {
                return Err(VmError::Native(format!(
                    "ServiceLoader provider {provider_name} is not assignable to service \
                     {service_name}"
                )));
            }
            // The JDK's provider set is deduplicated by class; keep the first occurrence
            // so a directory and a jar that both list one class do not construct it twice.
            if !providers.contains(&provider) {
                providers.push(provider);
            }
        }
    }

    let service_loader_class = shared.load_class(loader, SERVICE_LOADER_CLASS)?;
    let instance = shared.allocate_instance(service_loader_class)?;
    shared.service_state().insert_loader(
        instance,
        ServiceLoaderState {
            service,
            loader,
            providers,
        },
    );
    Ok(instance)
}

/// Parse one provider-configuration file.
///
/// A `#` starts a comment anywhere on a line, surrounding whitespace is trimmed, and a
/// blank line is skipped. The remaining order is the file's order; the caller preserves
/// resource order across files.
fn parse_provider_config(bytes: &[u8], resource: &str) -> Result<Vec<String>, VmError> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        VmError::Native(format!(
            "ServiceLoader configuration {resource} is not valid UTF-8: {error}"
        ))
    })?;
    let mut providers = Vec::new();
    for line in text.lines() {
        let name = line.split('#').next().unwrap_or("").trim();
        if !name.is_empty() {
            providers.push(name.to_owned());
        }
    }
    Ok(providers)
}

/// `ServiceLoader.iterator()`: allocate a real `java.util.Iterator` over the provider
/// list and record its cursor in the side table.
///
/// The provider classes are already loaded by `load`; nothing is allocated or
/// constructed here.
fn iterator(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    _args: &[Value],
) -> Result<Option<Value>, VmError> {
    let state = shared.service_state().loader(receiver).cloned();
    let Some(state) = state else {
        return Err(VmError::Native(
            "ServiceLoader.iterator receiver was not created by ServiceLoader.load".to_owned(),
        ));
    };
    let iterator_class = shared.load_class(state.loader, ITERATOR_CLASS)?;
    let iterator = shared.heap().allocate_iterator(iterator_class, Vec::new());
    shared.service_state().insert_iterator(
        iterator,
        ServiceIteratorState {
            service: state.service,
            loader: state.loader,
            providers: state.providers,
            position: 0,
        },
    );
    Ok(Some(Value::Ref(iterator)))
}

/// `Iterator.hasNext()`: the lazy cursor check, or the snapshot implementation for every
/// ordinary iterator.
fn iterator_has_next(
    shared: &Shared,
    thread: &ThreadState,
    loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let state = shared.service_state().iterator(receiver).cloned();
    let Some(state) = state else {
        return snapshot_has_next(shared, thread, loader, receiver, args);
    };
    Ok(Some(Value::Int(i32::from(
        state.position < state.providers.len(),
    ))))
}

/// `Iterator.next()`: instantiate the next provider on first use, or delegate to the
/// snapshot implementation for every ordinary iterator.
fn iterator_next(
    shared: &Shared,
    thread: &ThreadState,
    loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let state = shared.service_state().iterator(receiver).cloned();
    let Some(state) = state else {
        return snapshot_next(shared, thread, loader, receiver, args);
    };
    let Some(&provider) = state.providers.get(state.position) else {
        return Err(VmError::Native(
            "ServiceLoader iterator.next past the last provider".to_owned(),
        ));
    };
    let provider_name = class_binary_name(shared, provider)?;
    let instance = shared.allocate_instance(provider).map_err(|error| {
        VmError::Native(format!(
            "ServiceLoader provider {provider_name} could not be allocated: {error}"
        ))
    })?;
    // The provider class is initialized and its no-arg constructor runs here, on first
    // use. A Java throwable keeps its identity so the caller's exception table can catch
    // it; every other failure names the provider.
    match run_instance_method(
        shared,
        thread,
        provider,
        "<init>",
        "()V",
        instance,
        Vec::new(),
    ) {
        Ok(_) => {}
        Err(InterpError::Thrown(throwable)) => return Err(VmError::Thrown(throwable)),
        Err(error) => {
            return Err(VmError::Native(format!(
                "ServiceLoader provider {provider_name} could not be constructed: {error}"
            )));
        }
    }
    // Advance only after the constructor succeeded, so a failure leaves the cursor on the
    // provider it attempted.
    shared.service_state().advance_iterator(receiver);
    Ok(Some(Value::Ref(instance)))
}

/// The class named by a `Class` argument, rejecting the primitive and array class objects
/// that have no provider-configuration name.
fn class_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    operation: &str,
) -> Result<ClassId, VmError> {
    let Some(Value::Ref(reference)) = args.get(position).copied() else {
        return Err(VmError::Native(format!(
            "{operation} expects a Class argument"
        )));
    };
    if reference.is_null() {
        return Err(VmError::Native(format!(
            "{operation} received a null service class"
        )));
    }
    let heap = shared.heap();
    match heap
        .get(reference)
        .map_err(|error| VmError::Native(error.to_string()))?
    {
        HeapObject::Class(class) => Ok(*class),
        HeapObject::PrimitiveClass(_) => Err(VmError::Native(format!(
            "{operation} does not accept a primitive service type"
        ))),
        HeapObject::ArrayClass(_) => Err(VmError::Native(format!(
            "{operation} does not accept an array service type"
        ))),
        _ => Err(VmError::Native(format!(
            "{operation} expects a Class argument"
        ))),
    }
}

/// A class's binary name with dots, which is also its `META-INF/services` file name.
fn class_binary_name(shared: &Shared, class: ClassId) -> Result<String, VmError> {
    shared
        .entry(class)
        .map(|entry| entry.name.replace('/', "."))
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_configuration_parses_comments_blanks_and_whitespace() {
        let text = b"\
# the directory provider
   demo.HelloGreeter   # trailing comment

demo.WorldGreeter\n";
        assert_eq!(
            parse_provider_config(text, "META-INF/services/demo.Greeter").unwrap(),
            vec!["demo.HelloGreeter", "demo.WorldGreeter"]
        );
    }

    #[test]
    fn provider_configuration_keeps_file_order_and_rejects_non_utf8() {
        assert_eq!(
            parse_provider_config(b"demo.WorldGreeter\ndemo.HelloGreeter\n", "config").unwrap(),
            vec!["demo.WorldGreeter", "demo.HelloGreeter"]
        );
        assert!(matches!(
            parse_provider_config(&[0xFF], "config"),
            Err(VmError::Native(detail)) if detail.contains("UTF-8")
        ));
    }
}
