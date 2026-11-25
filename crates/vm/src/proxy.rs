//! `java.lang.reflect.Proxy` and `java.lang.reflect.InvocationHandler` — M3.2.
//!
//! A proxy is a real, generated class, not a special heap object and not a second
//! `getClass` identity: [`generate_proxy_class`] writes a class file with
//! [`rava_classfile::ClassBuilder`] and [`rava_classfile::Assembler`], and the class is
//! inserted through [`Shared::define_class`], so the M3.1 verifier checks every generated
//! method before the class becomes usable. The class extends `java.lang.reflect.Proxy`,
//! declares the requested interfaces, has one constructor that forwards the invocation
//! handler to the superclass constructor, and one method per interface method (plus the
//! `Object` methods a proxy must answer). Every generated method routes the call to the
//! handler with `InvocationHandler.invoke`, boxing the arguments and unboxing the result.
//!
//! The method object the handler receives is built by the runtime-private helper class
//! `rava/proxy/ProxySupport`, defined once in the bootstrap loader by
//! [`install_support`]. Its one native, `method(Class, String, String)`, resolves a
//! declaration by name and descriptor through the class table and returns the cached
//! [`crate::ReflectMember`] for the declaring class. This keeps the helper out of the C1
//! `Proxy` stub, whose surface is the JDK's and nothing else.
//!
//! Deliberate limits of this slice:
//!
//! - `newProxyInstance` and `getProxyClass` ignore the `ClassLoader` argument: the VM has
//!   one application loader, and a generated class is defined there.
//! - A handler exception crosses the native ABI as [`VmError::Thrown`] unchanged; the JDK's
//!   wrapping in `UndeclaredThrowableException` is not synthesized.
//! - There is no access-control check, no `Serializable` proxy special case, and no
//!   `Proxy.getInvocationHandler` type check beyond the proxy-superclass test.
//! - `InvocationHandler.invokeDefault` lives in `rava-interp` (`rava_interp::proxy`)
//!   because it re-enters the interpreter.

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};

use rava_classfile::{Assembler, ClassBuilder};

use crate::VmError;
use crate::heap::{ArrayKind, HeapError, HeapObject, MemberKind, ObjRef};
use crate::loader::{ClassId, LoaderId};
use crate::reflection::{self, ClassObject};
use crate::shared::Shared;
use crate::thread::ThreadState;
use crate::value::Value;

/// `java.lang.reflect.Proxy`'s internal name.
const PROXY_CLASS: &str = "java/lang/reflect/Proxy";

/// `java.lang.reflect.InvocationHandler`'s internal name.
const INVOCATION_HANDLER_CLASS: &str = "java/lang/reflect/InvocationHandler";

/// The runtime-private helper that resolves the reflected interface method.
const PROXY_SUPPORT_CLASS: &str = "rava/proxy/ProxySupport";

/// The invocation-handler field `java.lang.reflect.Proxy` declares.
const HANDLER_FIELD: &str = "h";

/// The invocation-handler field's descriptor.
const HANDLER_DESCRIPTOR: &str = "Ljava/lang/reflect/InvocationHandler;";

/// `Proxy(InvocationHandler)`.
const CONSTRUCTOR_DESCRIPTOR: &str = "(Ljava/lang/reflect/InvocationHandler;)V";

/// `Proxy.newProxyInstance(ClassLoader, Class[], InvocationHandler)`.
const NEW_PROXY_INSTANCE_DESCRIPTOR: &str = "(Ljava/lang/ClassLoader;[Ljava/lang/Class;Ljava/lang/reflect/InvocationHandler;)Ljava/lang/Object;";

/// `Proxy.getProxyClass(ClassLoader, Class[])`.
const GET_PROXY_CLASS_DESCRIPTOR: &str =
    "(Ljava/lang/ClassLoader;[Ljava/lang/Class;)Ljava/lang/Class;";

/// `Proxy.isProxyClass(Class)`.
const IS_PROXY_CLASS_DESCRIPTOR: &str = "(Ljava/lang/Class;)Z";

/// `Proxy.getInvocationHandler(Object)`.
const GET_INVOCATION_HANDLER_DESCRIPTOR: &str =
    "(Ljava/lang/Object;)Ljava/lang/reflect/InvocationHandler;";

/// `ProxySupport.method(Class, String, String)`.
const SUPPORT_METHOD_DESCRIPTOR: &str =
    "(Ljava/lang/Class;Ljava/lang/String;Ljava/lang/String;)Ljava/lang/reflect/Method;";

/// `InvocationHandler.invoke(Object, Method, Object[])`.
const HANDLER_INVOKE_DESCRIPTOR: &str =
    "(Ljava/lang/Object;Ljava/lang/reflect/Method;[Ljava/lang/Object;)Ljava/lang/Object;";

/// `java.lang.Object`'s internal name.
const OBJECT_CLASS: &str = "java/lang/Object";

const ACC_PUBLIC: u16 = 0x0001;
const ACC_PRIVATE: u16 = 0x0002;
const ACC_STATIC: u16 = 0x0008;
const ACC_FINAL: u16 = 0x0010;
const ACC_SUPER: u16 = 0x0020;
const ACC_NATIVE: u16 = 0x0100;

/// A generated proxy class is `public final super`.
const PROXY_CLASS_ACCESS: u16 = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;

/// The helper class is `public final super` like the proxy class.
const SUPPORT_CLASS_ACCESS: u16 = ACC_PUBLIC | ACC_FINAL | ACC_SUPER;

/// `ProxySupport.method` is `public static native`.
const SUPPORT_METHOD_ACCESS: u16 = ACC_PUBLIC | ACC_STATIC | ACC_NATIVE;

/// The class-file major version every generated class uses (Java 17).
const CLASS_VERSION: u16 = 61;

/// A proxy method's operand stack high-water mark.
///
/// Building `Object[]` for a `long`/`double` argument needs five slots
/// (`array`, `dup`, `index`, the two-slot value); the call to the handler needs at most
/// four, and unboxing a `long`/`double` result pushes two. Six covers every case.
const PROXY_METHOD_MAX_STACK: u16 = 6;

/// The largest local index a one-byte local instruction can address.
///
/// Generated methods load every parameter with its one-byte opcode, so the last parameter's
/// JVM slot must fit. A method whose parameter slots exceed the bound is rejected instead of
/// silently truncating an index.
const MAX_ONE_BYTE_LOCAL: usize = u8::MAX as usize;

/// The process-local counter that names generated proxy classes.
static NEXT_PROXY_CLASS: AtomicU32 = AtomicU32::new(1);

/// Registers the `java.lang.reflect.Proxy` and `ProxySupport` natives.
///
/// The C1 `Proxy` stub carries no `Code`, so every method is a registry entry keyed by the
/// class that declares it. `ProxySupport` is defined by [`install_support`], which
/// [`crate::library::register_all`] calls.
pub fn register_natives(shared: &Shared) {
    shared.register_native(PROXY_CLASS, "<init>", CONSTRUCTOR_DESCRIPTOR, proxy_init);
    shared.register_native(
        PROXY_CLASS,
        "newProxyInstance",
        NEW_PROXY_INSTANCE_DESCRIPTOR,
        new_proxy_instance,
    );
    shared.register_native(
        PROXY_CLASS,
        "getProxyClass",
        GET_PROXY_CLASS_DESCRIPTOR,
        get_proxy_class,
    );
    shared.register_native(
        PROXY_CLASS,
        "isProxyClass",
        IS_PROXY_CLASS_DESCRIPTOR,
        is_proxy_class,
    );
    shared.register_native(
        PROXY_CLASS,
        "getInvocationHandler",
        GET_INVOCATION_HANDLER_DESCRIPTOR,
        get_invocation_handler,
    );
    shared.register_native(
        PROXY_SUPPORT_CLASS,
        "method",
        SUPPORT_METHOD_DESCRIPTOR,
        support_method,
    );
}

/// Defines the runtime-private `rava/proxy/ProxySupport` class in the bootstrap loader.
///
/// The definition is idempotent: the class table is checked first, so calling
/// [`crate::library::register_all`] twice does not attempt a duplicate definition. The
/// helper has one `public static native Method method(Class, String, String)`; it is never
/// instantiated and carries no constructor.
pub fn install_support(shared: &Shared) -> Result<(), VmError> {
    if shared
        .lookup(LoaderId::BOOTSTRAP, PROXY_SUPPORT_CLASS)
        .is_some()
    {
        return Ok(());
    }
    shared.define_class(
        LoaderId::BOOTSTRAP,
        PROXY_SUPPORT_CLASS,
        &support_class_bytes(),
    )?;
    Ok(())
}

/// The class file of the runtime-private helper class.
fn support_class_bytes() -> Vec<u8> {
    let mut builder = ClassBuilder::new(PROXY_SUPPORT_CLASS, OBJECT_CLASS, CLASS_VERSION)
        .with_access(SUPPORT_CLASS_ACCESS);
    builder.method(
        SUPPORT_METHOD_ACCESS,
        "method",
        SUPPORT_METHOD_DESCRIPTOR,
        None,
    );
    builder.build()
}

fn heap_error(error: HeapError) -> VmError {
    VmError::Native(error.to_string())
}

fn class_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("class {}", class.0))
}

/// `Proxy(InvocationHandler)`: store the handler in the inherited `h` field.
fn proxy_init(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let handler = match args.first().copied() {
        Some(Value::Ref(handler)) => handler,
        _ => {
            return Err(VmError::Native(
                "Proxy.<init> expects an InvocationHandler argument".to_owned(),
            ));
        }
    };
    set_handler(shared, receiver, handler)?;
    Ok(None)
}

/// `Proxy.newProxyInstance(ClassLoader, Class[], InvocationHandler)`.
///
/// The loader is ignored: a generated proxy class is defined in this VM's one application
/// loader. The interface array is resolved and deduplicated, the class is generated or read
/// from the cache, and the instance's `h` field is set directly.
fn new_proxy_instance(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let interfaces = interface_arguments(shared, args, 1, "Proxy.newProxyInstance")?;
    let handler = match args.get(2).copied() {
        Some(Value::Ref(handler)) => handler,
        _ => {
            return Err(VmError::Native(
                "Proxy.newProxyInstance expects an InvocationHandler argument".to_owned(),
            ));
        }
    };
    let class = proxy_class_for(shared, &interfaces)?;
    let instance = shared.allocate_instance(class)?;
    set_handler(shared, instance, handler)?;
    Ok(Some(Value::Ref(instance)))
}

/// `Proxy.getProxyClass(ClassLoader, Class[])`: the cached generated class object.
fn get_proxy_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let interfaces = interface_arguments(shared, args, 1, "Proxy.getProxyClass")?;
    let class = proxy_class_for(shared, &interfaces)?;
    Ok(Some(Value::Ref(shared.class_object(class)?)))
}

/// `Proxy.isProxyClass(Class)`: true iff the class's superclass is `java.lang.reflect.Proxy`.
///
/// A primitive class object and an array class object are never proxy classes, and neither
/// is an ordinary class; only a generated class (or a hand-written subclass of `Proxy`)
/// answers true.
fn is_proxy_class(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) => reference,
        _ => {
            return Err(VmError::Native(
                "Proxy.isProxyClass expects a Class argument".to_owned(),
            ));
        }
    };
    let result = match reflection::class_object_of(shared, reference) {
        Ok(ClassObject::Named(class)) => superclass_is_proxy(shared, class)?,
        Ok(ClassObject::Primitive(_) | ClassObject::Array(_)) => false,
        Err(_) => {
            return Err(VmError::Native(
                "Proxy.isProxyClass expects a Class argument".to_owned(),
            ));
        }
    };
    Ok(Some(Value::Int(i32::from(result))))
}

/// `Proxy.getInvocationHandler(Object)`: the handler a proxy instance stores.
fn get_invocation_handler(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(
                "Proxy.getInvocationHandler expects a non-null proxy instance".to_owned(),
            ));
        }
    };
    let class = {
        let heap = shared.heap();
        match heap.get(reference).map_err(heap_error)? {
            HeapObject::Instance(instance) => instance.class,
            _ => {
                return Err(VmError::Native(
                    "Proxy.getInvocationHandler expects a proxy instance".to_owned(),
                ));
            }
        }
    };
    if !superclass_is_proxy(shared, class)? {
        return Err(VmError::Native(format!(
            "Proxy.getInvocationHandler: {} is not a proxy class",
            class_name(shared, class)
        )));
    }
    let slot = handler_field_slot(shared, class)?;
    let handler = {
        let heap = shared.heap();
        let fields = heap.object_fields(reference).map_err(heap_error)?;
        fields.get(slot).copied().ok_or_else(|| {
            VmError::Native(format!(
                "Proxy.getInvocationHandler: {} has no field slot {slot}",
                class_name(shared, class)
            ))
        })?
    };
    Ok(Some(handler))
}

/// `ProxySupport.method(Class, String, String)`: resolve an interface method declaration.
///
/// The search order is the JDK's `Class.getMethod` order: the class itself, its superclass
/// chain, then its interfaces, with every class visited once. The returned member object
/// names the declaring class, so `Method.getDeclaringClass()` reports the interface that
/// actually declares the method the proxy routes.
fn support_method(
    shared: &Shared,
    _thread: &ThreadState,
    _loader: LoaderId,
    _receiver: ObjRef,
    args: &[Value],
) -> Result<Option<Value>, VmError> {
    let reference = match args.first().copied() {
        Some(Value::Ref(reference)) => reference,
        _ => {
            return Err(VmError::Native(
                "ProxySupport.method expects a Class argument".to_owned(),
            ));
        }
    };
    let class = match reflection::class_object_of(shared, reference) {
        Ok(ClassObject::Named(class)) => class,
        Ok(ClassObject::Primitive(kind)) => {
            return Err(VmError::Native(format!(
                "ProxySupport.method expects a loaded class, found the primitive {}",
                kind.name()
            )));
        }
        Ok(ClassObject::Array(_)) => {
            return Err(VmError::Native(
                "ProxySupport.method expects a loaded class, found an array class".to_owned(),
            ));
        }
        Err(_) => {
            return Err(VmError::Native(
                "ProxySupport.method expects a Class argument".to_owned(),
            ));
        }
    };
    let name = string_argument(shared, args, 1, "ProxySupport.method")?;
    let descriptor = string_argument(shared, args, 2, "ProxySupport.method")?;
    for candidate in reflection::lookup_order(shared, class)? {
        let entry = shared
            .entry(candidate)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", candidate.0)))?;
        if entry.file.find_method(&name, &descriptor)?.is_some() {
            let member =
                shared.reflect_member(candidate, MemberKind::Method, &name, &descriptor)?;
            return Ok(Some(Value::Ref(member)));
        }
    }
    Err(VmError::Native(format!(
        "class {} has no method {name}{descriptor}",
        class_name(shared, class)
    )))
}

/// The `String` argument at `position`, or a typed error.
fn string_argument(
    shared: &Shared,
    args: &[Value],
    position: usize,
    operation: &str,
) -> Result<String, VmError> {
    let reference = match args.get(position).copied() {
        Some(Value::Ref(reference)) if !reference.is_null() => reference,
        _ => {
            return Err(VmError::Native(format!(
                "{operation} expects a non-null String argument at position {position}"
            )));
        }
    };
    let text = {
        let heap = shared.heap();
        heap.string(reference).map_err(heap_error)?.to_owned()
    };
    Ok(text)
}

/// Whether `class` declares `java.lang.reflect.Proxy` as its superclass.
fn superclass_is_proxy(shared: &Shared, class: ClassId) -> Result<bool, VmError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
    Ok(entry.metadata()?.super_class.as_deref() == Some(PROXY_CLASS))
}

/// The `h` field's slot in `class`'s instance layout.
fn handler_field_slot(shared: &Shared, class: ClassId) -> Result<usize, VmError> {
    let layout = shared.field_layout(class)?;
    layout
        .slot(HANDLER_FIELD, HANDLER_DESCRIPTOR)
        .ok_or_else(|| {
            VmError::Native(format!(
                "class {} has no {HANDLER_FIELD}:{HANDLER_DESCRIPTOR} field",
                class_name(shared, class)
            ))
        })
}

/// Store `handler` in an instance's inherited `h` field.
fn set_handler(shared: &Shared, instance: ObjRef, handler: ObjRef) -> Result<(), VmError> {
    let class = {
        let heap = shared.heap();
        heap.instance(instance).map_err(heap_error)?.class
    };
    let slot = handler_field_slot(shared, class)?;
    let mut heap = shared.heap();
    let fields = heap.object_fields_mut(instance).map_err(heap_error)?;
    let field = fields.get_mut(slot).ok_or_else(|| {
        VmError::Native(format!(
            "class {} has no field slot {slot}",
            class_name(shared, class)
        ))
    })?;
    *field = Value::Ref(handler);
    Ok(())
}

/// The interface list named by the `Class[]` argument at `position`.
///
/// Every element must be a `Class` object for a loaded interface: a null, primitive or
/// array class element and a non-interface class are typed errors. Repeated interfaces are
/// deduplicated in first-seen order, so a generated class never lists the same interface
/// twice.
fn interface_arguments(
    shared: &Shared,
    args: &[Value],
    position: usize,
    operation: &str,
) -> Result<Vec<ClassId>, VmError> {
    let array = match args.get(position).copied() {
        Some(Value::Ref(array)) if !array.is_null() => array,
        Some(Value::Ref(_)) => {
            return Err(VmError::Native(format!(
                "{operation} received a null Class[] of interfaces"
            )));
        }
        _ => {
            return Err(VmError::Native(format!(
                "{operation} expects a Class[] of interfaces"
            )));
        }
    };
    let elements = {
        let heap = shared.heap();
        let array = heap.array(array).map_err(heap_error)?;
        if array.kind != ArrayKind::Reference {
            return Err(VmError::Native(format!(
                "{operation} expects a reference array of interfaces, found a {} array",
                array.kind
            )));
        }
        array.elements.clone()
    };
    let mut interfaces = Vec::new();
    let mut seen = HashSet::new();
    for (index, element) in elements.iter().enumerate() {
        let Value::Ref(reference) = *element else {
            return Err(VmError::Native(format!(
                "{operation} interface {index} is not a Class object"
            )));
        };
        let class = match reflection::class_object_of(shared, reference) {
            Ok(ClassObject::Named(class)) => class,
            Ok(ClassObject::Primitive(_) | ClassObject::Array(_)) => {
                return Err(VmError::Native(format!(
                    "{operation} interface {index} is a primitive or array class, not a loaded interface"
                )));
            }
            Err(_) => {
                return Err(VmError::Native(format!(
                    "{operation} interface {index} is not a Class object"
                )));
            }
        };
        let entry = shared
            .entry(class)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", class.0)))?;
        if !entry.metadata()?.is_interface() {
            return Err(VmError::Native(format!(
                "{operation} interface {index} names {}, which is not an interface",
                entry.name
            )));
        }
        if seen.insert(class) {
            interfaces.push(class);
        }
    }
    Ok(interfaces)
}

/// The generated proxy class for `interfaces`, generated and defined on first use.
///
/// The cache key is `(application loader, deduplicated interfaces)`; the same call twice
/// returns the same [`ClassId`], which is what makes repeated `Proxy.getProxyClass` calls
/// observe the same `Class`. The class is defined through [`Shared::define_class`], so the
/// M3.1 verifier runs before it can be used.
fn proxy_class_for(shared: &Shared, interfaces: &[ClassId]) -> Result<ClassId, VmError> {
    let loader = shared.application_loader();
    if let Some(class) = shared.proxy_class(loader, interfaces) {
        return Ok(class);
    }
    let name = format!(
        "rava/proxy/$Proxy{}",
        NEXT_PROXY_CLASS.fetch_add(1, Ordering::Relaxed)
    );
    let bytes = generate_proxy_class(shared, &name, interfaces)?;
    let class = shared.define_class(loader, &name, &bytes)?;
    shared.record_proxy_class(loader, interfaces.to_vec(), class);
    Ok(class)
}

/// One method a generated proxy class declares.
struct ProxyMethod {
    /// The interface that declares the method; `Object` for the three universal methods.
    interface: ClassId,
    name: String,
    descriptor: String,
}

/// The methods a proxy class declares: every non-static, non-private interface method in
/// the requested interfaces' hierarchy, plus the three `Object` methods a proxy must
/// answer when no interface declares them.
fn proxy_methods(shared: &Shared, interfaces: &[ClassId]) -> Result<Vec<ProxyMethod>, VmError> {
    let mut methods = Vec::new();
    let mut seen = HashSet::new();
    let mut visited = HashSet::new();
    let mut queue: VecDeque<ClassId> = interfaces.iter().copied().collect();
    while let Some(interface) = queue.pop_front() {
        if !visited.insert(interface) {
            continue;
        }
        let entry = shared
            .entry(interface)
            .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", interface.0)))?;
        let metadata = entry.metadata()?;
        for method in &metadata.methods {
            if method.is_static() || method.access_flags & ACC_PRIVATE != 0 {
                continue;
            }
            // An interface has no `<init>`/`<clinit>` of its own; the guard keeps a
            // malformed or hand-written interface from contributing one.
            if method.name.starts_with('<') {
                continue;
            }
            if seen.insert((method.name.clone(), method.descriptor.clone())) {
                methods.push(ProxyMethod {
                    interface,
                    name: method.name.clone(),
                    descriptor: method.descriptor.clone(),
                });
            }
        }
        let loader = entry.loader;
        for name in &metadata.interfaces {
            // The C1 tree omits a few transitive interfaces; they cannot contribute a
            // method, while a genuinely missing class elsewhere is still a loader error.
            match shared.load_class(loader, name) {
                Ok(interface) => queue.push_back(interface),
                Err(VmError::ClassNotFound { .. }) => {}
                Err(error) => return Err(error),
            }
        }
    }

    let object = shared.load_library_class(OBJECT_CLASS)?;
    for (name, descriptor) in [
        ("equals", "(Ljava/lang/Object;)Z"),
        ("hashCode", "()I"),
        ("toString", "()Ljava/lang/String;"),
    ] {
        if seen.insert((name.to_owned(), descriptor.to_owned())) {
            methods.push(ProxyMethod {
                interface: object,
                name: name.to_owned(),
                descriptor: descriptor.to_owned(),
            });
        }
    }
    Ok(methods)
}

/// Generate the class file of one proxy class.
///
/// Layout: the constructor `(InvocationHandler)V` first, then one method per collected
/// interface method, then the `Object` methods. Every generated method has the same shape:
/// build the boxed `Object[]`, load the handler, `this`, the reflected method and the
/// array, call `InvocationHandler.invoke`, and convert the result back to the declared
/// return type.
fn generate_proxy_class(
    shared: &Shared,
    name: &str,
    interfaces: &[ClassId],
) -> Result<Vec<u8>, VmError> {
    let methods = proxy_methods(shared, interfaces)?;
    let mut builder =
        ClassBuilder::new(name, PROXY_CLASS, CLASS_VERSION).with_access(PROXY_CLASS_ACCESS);
    for interface in interfaces {
        builder.interface(&class_name(shared, *interface));
    }

    let super_init = builder.method_ref(PROXY_CLASS, "<init>", CONSTRUCTOR_DESCRIPTOR);
    let mut init = Assembler::new();
    init.aload(0);
    init.aload(1);
    init.invokespecial(super_init);
    init.return_();
    builder.method(
        ACC_PUBLIC,
        "<init>",
        CONSTRUCTOR_DESCRIPTOR,
        Some((2, 2, init.finish()?)),
    );

    for method in &methods {
        generate_proxy_method(shared, &mut builder, method)?;
    }
    Ok(builder.build())
}

/// Generate one proxy method's `Code` and add it to `builder`.
fn generate_proxy_method(
    shared: &Shared,
    builder: &mut ClassBuilder,
    method: &ProxyMethod,
) -> Result<(), VmError> {
    let parameters = reflection::parameter_descriptors(&method.descriptor)?;
    let slot_count = parameter_slots(&parameters)?;
    let last_parameter_slot = usize::from(slot_count) - 1;
    if last_parameter_slot > MAX_ONE_BYTE_LOCAL {
        return Err(VmError::Native(format!(
            "proxy method {}{} needs local slot {last_parameter_slot} for its last parameter, but a one-byte local index reaches only {MAX_ONE_BYTE_LOCAL}",
            method.name, method.descriptor
        )));
    }

    // Constant-pool entries are written before the code that names them.
    let interface_class = builder.class(&class_name(shared, method.interface));
    let name_constant = builder.string(&method.name);
    let descriptor_constant = builder.string(&method.descriptor);
    let handler_field = builder.field_ref(PROXY_CLASS, HANDLER_FIELD, HANDLER_DESCRIPTOR);
    let support_method =
        builder.method_ref(PROXY_SUPPORT_CLASS, "method", SUPPORT_METHOD_DESCRIPTOR);
    let handler_invoke = builder.interface_method_ref(
        INVOCATION_HANDLER_CLASS,
        "invoke",
        HANDLER_INVOKE_DESCRIPTOR,
    );
    let object_array = builder.class(OBJECT_CLASS);
    let boxes: Vec<Option<u16>> = parameters
        .iter()
        .map(|descriptor| box_method_ref(builder, descriptor))
        .collect();

    // `Object[] args = new Object[n]` with one boxed element per parameter.
    let mut code = Assembler::new();
    code.iconst(parameters.len() as i32);
    code.anewarray(object_array);
    let mut parameter_slot = 1usize; // `this` is local 0.
    for (index, (descriptor, value_of)) in parameters.iter().zip(&boxes).enumerate() {
        code.dup();
        code.iconst(index as i32);
        load_parameter(&mut code, descriptor, parameter_slot)?;
        if let Some(value_of) = value_of {
            invokestatic(&mut code, *value_of);
        }
        code.aastore();
        parameter_slot += usize::from(parameter_width(descriptor)?);
    }
    // The parameters occupy slots 0..slot_count, so the temporary `Object[]` local goes
    // after the last parameter slot.
    let temp = usize::from(slot_count);
    store_local(&mut code, temp);

    code.aload(0);
    code.getfield(handler_field);
    code.aload(0);
    code.ldc(interface_class);
    code.ldc(name_constant);
    code.ldc(descriptor_constant);
    invokestatic(&mut code, support_method);
    load_local(&mut code, temp);
    code.invokeinterface(handler_invoke, 4);

    emit_return(builder, &mut code, &method.descriptor)?;

    let max_locals = slot_count
        .checked_add(1)
        .ok_or_else(|| VmError::Native("proxy method local count overflows".to_owned()))?;
    builder.method(
        ACC_PUBLIC,
        &method.name,
        &method.descriptor,
        Some((PROXY_METHOD_MAX_STACK, max_locals, code.finish()?)),
    );
    Ok(())
}

/// The `valueOf` `Methodref` that boxes one parameter, or `None` for a reference type.
fn box_method_ref(builder: &mut ClassBuilder, descriptor: &str) -> Option<u16> {
    let (owner, method, value_of_descriptor) = match descriptor {
        "Z" => ("java/lang/Boolean", "valueOf", "(Z)Ljava/lang/Boolean;"),
        "B" => ("java/lang/Byte", "valueOf", "(B)Ljava/lang/Byte;"),
        "C" => ("java/lang/Character", "valueOf", "(C)Ljava/lang/Character;"),
        "S" => ("java/lang/Short", "valueOf", "(S)Ljava/lang/Short;"),
        "I" => ("java/lang/Integer", "valueOf", "(I)Ljava/lang/Integer;"),
        "J" => ("java/lang/Long", "valueOf", "(J)Ljava/lang/Long;"),
        "F" => ("java/lang/Float", "valueOf", "(F)Ljava/lang/Float;"),
        "D" => ("java/lang/Double", "valueOf", "(D)Ljava/lang/Double;"),
        _ => return None,
    };
    Some(builder.method_ref(owner, method, value_of_descriptor))
}

/// Load the parameter whose first JVM slot is `slot` with the opcode its descriptor needs.
fn load_parameter(code: &mut Assembler, descriptor: &str, slot: usize) -> Result<(), VmError> {
    let slot = u8::try_from(slot).map_err(|_| {
        VmError::Native(format!(
            "parameter slot {slot} does not fit a one-byte local index"
        ))
    })?;
    match descriptor.as_bytes().first().copied() {
        Some(b'Z' | b'B' | b'C' | b'S' | b'I') => code.iload(slot),
        Some(b'J') => code.lload(slot),
        Some(b'F') => code.fload(slot),
        Some(b'D') => code.dload(slot),
        Some(b'L' | b'[') => code.aload(slot),
        _ => {
            return Err(VmError::Native(format!(
                "unsupported proxy parameter descriptor {descriptor}"
            )));
        }
    }
    Ok(())
}

/// The JVM local slots one parameter descriptor occupies (`long`/`double` count two).
fn parameter_width(descriptor: &str) -> Result<u16, VmError> {
    match descriptor.as_bytes().first().copied() {
        Some(b'J' | b'D') => Ok(2),
        Some(b'Z' | b'B' | b'C' | b'S' | b'I' | b'F' | b'L' | b'[') => Ok(1),
        _ => Err(VmError::Native(format!(
            "unsupported proxy parameter descriptor {descriptor}"
        ))),
    }
}

/// The JVM slot count of a parameter list (`long`/`double` count two), plus `this`.
fn parameter_slots(parameters: &[&str]) -> Result<u16, VmError> {
    let mut slots = 1u16; // `this`
    for descriptor in parameters {
        slots = slots
            .checked_add(parameter_width(descriptor)?)
            .ok_or_else(|| VmError::Native("proxy method slot count overflows".to_owned()))?;
    }
    Ok(slots)
}

/// Unbox the handler's result and return it in the method's declared type.
fn emit_return(
    builder: &mut ClassBuilder,
    code: &mut Assembler,
    descriptor: &str,
) -> Result<(), VmError> {
    let return_type = reflection::return_descriptor(descriptor)?;
    if return_type == "V" {
        code.op(0x57, &[]); // pop
        code.return_();
        return Ok(());
    }
    if let Some((owner, method, method_descriptor, kind)) = unbox_method(return_type) {
        let wrapper = builder.class(owner);
        let unbox = builder.method_ref(owner, method, method_descriptor);
        code.checkcast(wrapper);
        code.invokevirtual(unbox);
        match kind {
            ReturnKind::Int => code.ireturn(),
            ReturnKind::Long => code.lreturn(),
            ReturnKind::Float => code.freturn(),
            ReturnKind::Double => code.dreturn(),
        }
        return Ok(());
    }
    if return_type != "Ljava/lang/Object;" {
        let class_name = reference_class_name(return_type).ok_or_else(|| {
            VmError::Native(format!(
                "unsupported proxy return descriptor {return_type} in {descriptor}"
            ))
        })?;
        let target = builder.class(class_name);
        code.checkcast(target);
    }
    code.areturn();
    Ok(())
}

/// The return kind a primitive unboxing method produces.
#[derive(Clone, Copy)]
enum ReturnKind {
    Int,
    Long,
    Float,
    Double,
}

/// The wrapper class, accessor and result kind that unbox one primitive return type.
fn unbox_method(
    return_type: &str,
) -> Option<(&'static str, &'static str, &'static str, ReturnKind)> {
    let result = match return_type {
        "Z" => ("java/lang/Boolean", "booleanValue", "()Z", ReturnKind::Int),
        "B" => ("java/lang/Byte", "byteValue", "()B", ReturnKind::Int),
        "C" => ("java/lang/Character", "charValue", "()C", ReturnKind::Int),
        "S" => ("java/lang/Short", "shortValue", "()S", ReturnKind::Int),
        "I" => ("java/lang/Integer", "intValue", "()I", ReturnKind::Int),
        "J" => ("java/lang/Long", "longValue", "()J", ReturnKind::Long),
        "F" => ("java/lang/Float", "floatValue", "()F", ReturnKind::Float),
        "D" => ("java/lang/Double", "doubleValue", "()D", ReturnKind::Double),
        _ => return None,
    };
    Some(result)
}

/// The class a reference return descriptor names: `Lx/y;` becomes `x/y`, and an array
/// descriptor stays the descriptor the constant pool expects.
fn reference_class_name(descriptor: &str) -> Option<&str> {
    if let Some(name) = descriptor
        .strip_prefix('L')
        .and_then(|rest| rest.strip_suffix(';'))
    {
        return Some(name);
    }
    descriptor.starts_with('[').then_some(descriptor)
}

/// `invokestatic`: the assembler exposes every other invoke form as a helper, not this one.
fn invokestatic(code: &mut Assembler, index: u16) {
    code.op(0xb8, &index.to_be_bytes());
}

/// `astore` with a wide index when `slot` exceeds the one-byte range.
fn store_local(code: &mut Assembler, slot: usize) {
    match u8::try_from(slot) {
        Ok(slot) => code.astore(slot),
        Err(_) => {
            let index = u16::try_from(slot).expect("a local index fits a u16");
            code.op(
                0xc4,
                &[0x3a, index.to_be_bytes()[0], index.to_be_bytes()[1]],
            );
        }
    }
}

/// `aload` with a wide index when `slot` exceeds the one-byte range.
fn load_local(code: &mut Assembler, slot: usize) {
    match u8::try_from(slot) {
        Ok(slot) => code.aload(slot),
        Err(_) => {
            let index = u16::try_from(slot).expect("a local index fits a u16");
            code.op(
                0xc4,
                &[0x19, index.to_be_bytes()[0], index.to_be_bytes()[1]],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_return_descriptors_become_class_constants() {
        assert_eq!(
            reference_class_name("Ljava/lang/String;"),
            Some("java/lang/String")
        );
        assert_eq!(
            reference_class_name("[Ljava/lang/String;"),
            Some("[Ljava/lang/String;")
        );
        assert_eq!(reference_class_name("I"), None);
    }

    #[test]
    fn every_primitive_return_has_an_unboxing_accessor() {
        for (descriptor, owner) in [
            ("Z", "java/lang/Boolean"),
            ("B", "java/lang/Byte"),
            ("C", "java/lang/Character"),
            ("S", "java/lang/Short"),
            ("I", "java/lang/Integer"),
            ("J", "java/lang/Long"),
            ("F", "java/lang/Float"),
            ("D", "java/lang/Double"),
        ] {
            let (found, _, _, _) = unbox_method(descriptor).expect("a primitive return type");
            assert_eq!(found, owner);
            assert!(box_method_ref_for_test(descriptor));
        }
        assert!(unbox_method("V").is_none());
        assert!(unbox_method("Ljava/lang/String;").is_none());
    }

    fn box_method_ref_for_test(descriptor: &str) -> bool {
        let mut builder = ClassBuilder::new("demo/BoxCheck", OBJECT_CLASS, CLASS_VERSION);
        box_method_ref(&mut builder, descriptor).is_some()
    }
}
