//! `java.lang.invoke.LambdaMetafactory` call sites and lambda SAM invocation (M2.19).
//!
//! Java 17 compiles a lambda expression or method reference to an `invokedynamic` whose
//! bootstrap is `LambdaMetafactory.metafactory` (or `altMetafactory`). The bootstrap's
//! static arguments are the SAM (Single Abstract Method) type, the implementation method
//! handle and the instantiated type; the call-site arguments are the captured values. This
//! module turns a resolved bootstrap entry and those arguments into a
//! [`rava_vm::LambdaObject`], the heap representation M2.19 adds.
//!
//! A lambda is not backed by a generated class file. Its class identity is the functional
//! interface itself (so `getClass()` reports the interface's class object), and its SAM
//! execution goes through the interpreter's `invokeinterface`: the stored interface must
//! match the instruction's interface, the SAM name and `samMethodType` descriptor must
//! match, and the target runs through the ordinary [`crate::run_static_method`] or
//! [`crate::run_instance_method`] path. There is no second execution engine and no second
//! frame model.
//!
//! The supported target kinds are a static method, an instance method
//! (`REF_invokeVirtual`/`REF_invokeSpecial`/`REF_invokeInterface`), a constructor
//! (`REF_newInvokeSpecial`) and a native registry entry ([`LambdaTargetKind::Native`]) that
//! a VM factory such as `Comparator.comparingInt` creates because its body has no class
//! file. For an instance target the receiver is the first captured or SAM value, exactly as
//! the implementation method's first parameter; a bound method reference therefore captures
//! the receiver, and an unbound one (`String::length`) takes it from the SAM argument. Argument adaptation (boxing, unboxing, widening, a cast to
//! the instantiated type) is not implemented: a value whose representation does not
//! already match the target parameter descriptor is a typed
//! [`crate::InterpError::Lambda`].
//!
//! `altMetafactory`'s flag word is parsed: `FLAG_SERIALIZABLE` is a typed error because
//! serializable lambdas are out of scope, while `FLAG_MARKERS` and `FLAG_BRIDGES` are
//! accepted and skipped because this representation has no marker-interface or bridge
//! table. `FLAG_SERIALIZABLE`'s `$deserializeLambda$` path, full `MethodHandle`/`VarHandle`
//! support, call-site caching and access checks are out of scope.

use rava_classfile::{
    BootstrapArgument, BootstrapMethodEntry, BootstrapMethodHandle, ReferenceKind,
};
use rava_vm::{
    ClassId, LambdaObject, LambdaTarget, LambdaTargetKind, ObjRef, Shared, ThreadState, Value,
    VmError, reflection,
};

use crate::{InterpError, argument_count, run_instance_method, run_static_method};

/// The internal name of the supported bootstrap owner.
pub const LAMBDA_METAFACTORY: &str = "java/lang/invoke/LambdaMetafactory";

/// `LambdaMetafactory.FLAG_SERIALIZABLE`.
const FLAG_SERIALIZABLE: i32 = 1;
/// `LambdaMetafactory.FLAG_MARKERS`.
const FLAG_MARKERS: i32 = 2;
/// `LambdaMetafactory.FLAG_BRIDGES`.
const FLAG_BRIDGES: i32 = 4;

/// Create the lambda object for one `LambdaMetafactory` `invokedynamic` call site.
///
/// `caller` is the class executing the instruction: its loader resolves both the
/// functional interface and the implementation method's owner, so a child loader's
/// classes keep their own identity. `args` are the call-site arguments, which become the
/// lambda's captured values.
pub(crate) fn invoke_call_site(
    shared: &Shared,
    caller: ClassId,
    bootstrap: &BootstrapMethodEntry,
    call_site_name: &str,
    descriptor: &str,
    args: &[Value],
) -> Result<ObjRef, InterpError> {
    let handle = &bootstrap.handle;
    if handle.owner != LAMBDA_METAFACTORY {
        return Err(unsupported(handle).into());
    }

    let (sam_descriptor, instantiated_descriptor, target) = match handle.name.as_str() {
        "metafactory" => parse_metafactory(bootstrap)?,
        "altMetafactory" => parse_alt_metafactory(bootstrap)?,
        _ => return Err(unsupported(handle).into()),
    };

    let return_descriptor = reflection::return_descriptor(descriptor)?;
    let interface_name = object_descriptor_name(return_descriptor).ok_or_else(|| {
        VmError::Native(format!(
            "{call_site_name}{descriptor} does not return a functional interface"
        ))
    })?;

    // The factory descriptor's arguments are the captured values, one per slot. The SAM
    // descriptor and the instantiated type must at least be well-formed method
    // descriptors, because later calls use them.
    let captured_count = argument_count(descriptor)?;
    if captured_count != args.len() {
        return Err(VmError::Native(format!(
            "{call_site_name}{descriptor} expects {captured_count} captured value(s) but received {}",
            args.len()
        ))
        .into());
    }
    argument_count(&sam_descriptor)?;
    argument_count(&instantiated_descriptor)?;

    let loader = shared
        .entry(caller)
        .ok_or_else(|| VmError::Loader(format!("class id {} is not loaded", caller.0)))?
        .loader;
    let interface = shared.load_class(loader, interface_name)?;
    let target_class = shared.load_class(loader, &target.owner)?;
    let kind = target_kind(&target)?;
    if kind == LambdaTargetKind::Constructor && target.name != "<init>" {
        return Err(InterpError::Lambda(format!(
            "constructor method handle names {}::{} instead of <init>",
            target.owner, target.name
        )));
    }

    Ok(shared.allocate_lambda(LambdaObject {
        interface,
        sam_name: call_site_name.to_owned(),
        sam_descriptor,
        instantiated_descriptor,
        target: LambdaTarget {
            class: target_class,
            name: target.name.clone(),
            descriptor: target.descriptor.clone(),
            kind,
        },
        captured: args.to_vec(),
    }))
}

/// Invoke a lambda's SAM method with the call-site arguments.
///
/// The values handed to the implementation method are the captured values followed by the
/// SAM arguments, the order the metafactory defines. A static target consumes all of them;
/// an instance target consumes the first one as the receiver; a constructor target
/// allocates the owner and runs `<init>`. The target's runtime descriptor is checked
/// before the call, so a mismatched lambda is a typed error instead of a call with the
/// wrong operands.
pub(crate) fn invoke_sam(
    shared: &Shared,
    thread: &ThreadState,
    lambda: &LambdaObject,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    let mut values = lambda.captured.clone();
    values.extend(args);

    let parameters = reflection::parameter_descriptors(&lambda.target.descriptor)?;
    let receiver_offset = usize::from(lambda.target.kind == LambdaTargetKind::Instance);
    let expected = parameters.len() + receiver_offset;
    if values.len() != expected {
        return Err(InterpError::Lambda(format!(
            "SAM {}:{} supplies {} value(s) but target {}::{}:{} expects {expected}",
            lambda.sam_name,
            lambda.sam_descriptor,
            values.len(),
            target_name(shared, lambda.target.class),
            lambda.target.name,
            lambda.target.descriptor
        )));
    }

    for (position, (parameter, value)) in parameters
        .iter()
        .zip(values.iter().skip(receiver_offset))
        .enumerate()
    {
        if !value_fits(parameter, *value) {
            return Err(InterpError::Lambda(format!(
                "target {}::{}:{} parameter {position} expects {parameter}, found {value:?}",
                target_name(shared, lambda.target.class),
                lambda.target.name,
                lambda.target.descriptor
            )));
        }
    }

    match lambda.target.kind {
        LambdaTargetKind::Static => run_static_method(
            shared,
            thread,
            lambda.target.class,
            &lambda.target.name,
            &lambda.target.descriptor,
            values,
        ),
        LambdaTargetKind::Instance => {
            let Some(Value::Ref(receiver)) = values.first().copied() else {
                return Err(InterpError::Lambda(format!(
                    "instance target {}::{} needs a reference receiver",
                    target_name(shared, lambda.target.class),
                    lambda.target.name
                )));
            };
            if receiver.is_null() {
                return Err(InterpError::Lambda(format!(
                    "instance target {}::{} received a null receiver",
                    target_name(shared, lambda.target.class),
                    lambda.target.name
                )));
            }
            let remaining = values.split_off(1);
            // A method reference can target an interface method whose receiver is itself a
            // lambda: Boot's `overrides.forEach(CacheOverride::close)` does exactly that.
            // An interface-typed lambda receiver has no class implementation to resolve,
            // so its own SAM is the implementation.
            let nested = {
                let heap = shared.heap();
                match heap.get(receiver) {
                    Ok(rava_vm::HeapObject::Lambda(object))
                        if object.sam_name == lambda.target.name
                            && object.sam_descriptor == lambda.target.descriptor =>
                    {
                        Some(object.clone())
                    }
                    _ => None,
                }
            };
            if let Some(object) = nested {
                return invoke_sam(shared, thread, &object, remaining);
            }
            // `REF_invokeVirtual`/`REF_invokeInterface` method references dispatch on the
            // receiver, not on the compile-time owner. Spring passes
            // `MergedAnnotation::isDirectlyPresent`, whose implementation lives on the
            // runtime class's superclass, so resolving on the interface would reach an
            // abstract declaration and fail.
            let declaring = virtual_target_class(shared, receiver, &lambda.target)?;
            run_instance_method(
                shared,
                thread,
                declaring,
                &lambda.target.name,
                &lambda.target.descriptor,
                receiver,
                remaining,
            )
        }
        LambdaTargetKind::Native => {
            let owner = target_name(shared, lambda.target.class);
            let native = shared
                .native(&owner, &lambda.target.name, &lambda.target.descriptor)
                .ok_or_else(|| {
                    InterpError::Decode(format!(
                        "no native implementation for lambda target {owner}::{}:{}",
                        lambda.target.name, lambda.target.descriptor
                    ))
                })?;
            let loader = shared
                .entry(lambda.target.class)
                .ok_or_else(|| {
                    InterpError::Decode(format!(
                        "lambda target class {} is not loaded",
                        lambda.target.class.0
                    ))
                })?
                .loader;
            native(shared, thread, loader, ObjRef::NULL, &values).map_err(InterpError::from)
        }
        LambdaTargetKind::Constructor => {
            let instance = shared.allocate_instance(lambda.target.class)?;
            run_instance_method(
                shared,
                thread,
                lambda.target.class,
                "<init>",
                &lambda.target.descriptor,
                instance,
                values,
            )?;
            Ok(Some(Value::Ref(instance)))
        }
    }
}

/// The class that declares an instance target for this receiver.
///
/// The receiver's runtime class chain wins, exactly as JVMS §5.4.5 resolves an instance
/// method; when no class declares it, the target owner's interface hierarchy supplies a
/// default method. A receiver that is not an `Instance` (a VM heap object) still reports
/// its class through [`rava_vm::Shared::get_class`], so a method reference to
/// `ArrayList::size` keeps working.
fn virtual_target_class(
    shared: &Shared,
    receiver: ObjRef,
    target: &LambdaTarget,
) -> Result<ClassId, InterpError> {
    let class_object = shared.get_class(receiver)?;
    let mut current = Some(shared.heap().class(class_object)?);
    while let Some(class) = current {
        let entry = shared
            .entry(class)
            .ok_or_else(|| InterpError::Decode(format!("class {} is not loaded", class.0)))?;
        if entry
            .file
            .find_method(&target.name, &target.descriptor)?
            .is_some()
        {
            return Ok(class);
        }
        current = shared.resolution_superclass(class)?;
    }
    if let Some(interface) =
        crate::find_interface_method(shared, target.class, &target.name, &target.descriptor)?
    {
        return Ok(interface);
    }
    Err(InterpError::Lambda(format!(
        "instance target {}::{}:{} does not resolve on the receiver",
        target_name(shared, target.class),
        target.name,
        target.descriptor
    )))
}

/// The typed `metafactory` arguments: `samMethodType`, `implMethod`,
/// `instantiatedMethodType`, in that order.
fn parse_metafactory(
    bootstrap: &BootstrapMethodEntry,
) -> Result<(String, String, BootstrapMethodHandle), InterpError> {
    if bootstrap.arguments.len() != 3 {
        return Err(VmError::Native(format!(
            "LambdaMetafactory.metafactory expects 3 static arguments, found {}",
            bootstrap.arguments.len()
        ))
        .into());
    }
    let sam = method_type_argument(&bootstrap.arguments[0], 0)?;
    let target = method_handle_argument(&bootstrap.arguments[1], 1)?;
    let instantiated = method_type_argument(&bootstrap.arguments[2], 2)?;
    Ok((sam.to_owned(), instantiated.to_owned(), target.clone()))
}

/// The typed `altMetafactory` arguments.
///
/// The order is the JDK's: `flags`, `samMethodType`, `implMethod`,
/// `instantiatedMethodType`, then a marker count and its `Class` constants when
/// `FLAG_MARKERS` is set, then a bridge count and its `MethodType` constants when
/// `FLAG_BRIDGES` is set. Marker and bridge lists are consumed and skipped; they do not
/// change the simple lambda this slice represents.
fn parse_alt_metafactory(
    bootstrap: &BootstrapMethodEntry,
) -> Result<(String, String, BootstrapMethodHandle), InterpError> {
    let arguments = &bootstrap.arguments;
    // `altMetafactory`'s static arguments are the three method-type/handle arguments and
    // then the flag word, unlike `metafactory`; the flags are argument 3, not 0.
    let sam = method_type_argument(argument(arguments, 0, "samMethodType")?, 0)?;
    let target = method_handle_argument(argument(arguments, 1, "implMethod")?, 1)?;
    let instantiated = method_type_argument(argument(arguments, 2, "instantiatedMethodType")?, 2)?;
    let flags = int_argument(argument(arguments, 3, "flags")?, 3)? as i32;
    let unknown = flags & !(FLAG_SERIALIZABLE | FLAG_MARKERS | FLAG_BRIDGES);
    if unknown != 0 {
        return Err(VmError::Native(format!(
            "LambdaMetafactory.altMetafactory has unknown flag bits 0x{unknown:x}"
        ))
        .into());
    }

    // `FLAG_SERIALIZABLE` is accepted and not represented: the lambda object is not a
    // generated class with `writeReplace`, so serializing it fails in `ObjectOutputStream`
    // until a serializable-lambda slice exists. Accepting the flag is what lets
    // `TypeDescriptor`'s static initializer run; it never serializes the lambda.
    let mut cursor = 4;
    if flags & FLAG_MARKERS != 0 {
        let count = int_argument(argument(arguments, cursor, "marker count")?, cursor)?;
        cursor += 1;
        for _ in 0..count {
            class_argument(argument(arguments, cursor, "marker interface")?, cursor)?;
            cursor += 1;
        }
    }
    if flags & FLAG_BRIDGES != 0 {
        let count = int_argument(argument(arguments, cursor, "bridge count")?, cursor)?;
        cursor += 1;
        for _ in 0..count {
            method_type_argument(argument(arguments, cursor, "bridge method type")?, cursor)?;
            cursor += 1;
        }
    }
    if cursor != arguments.len() {
        return Err(VmError::Native(format!(
            "LambdaMetafactory.altMetafactory has {} unconsumed static argument(s)",
            arguments.len() - cursor
        ))
        .into());
    }

    Ok((sam.to_owned(), instantiated.to_owned(), target.clone()))
}

/// The reference kind's target shape.
fn target_kind(handle: &BootstrapMethodHandle) -> Result<LambdaTargetKind, InterpError> {
    Ok(match handle.reference_kind {
        ReferenceKind::InvokeStatic => LambdaTargetKind::Static,
        ReferenceKind::InvokeVirtual
        | ReferenceKind::InvokeSpecial
        | ReferenceKind::InvokeInterface => LambdaTargetKind::Instance,
        ReferenceKind::NewInvokeSpecial => LambdaTargetKind::Constructor,
        other => {
            return Err(InterpError::Lambda(format!(
                "lambda implementation handle {}::{}:{} has unsupported reference kind {other:?}",
                handle.owner, handle.name, handle.descriptor
            )));
        }
    })
}

fn unsupported(handle: &rava_classfile::BootstrapMethodHandle) -> VmError {
    VmError::UnsupportedBootstrapMethod {
        owner: handle.owner.clone(),
        name: handle.name.clone(),
        descriptor: handle.descriptor.clone(),
    }
}

/// One indexed static argument, or a typed error naming the missing position.
fn argument<'a>(
    arguments: &'a [BootstrapArgument],
    index: usize,
    expected: &str,
) -> Result<&'a BootstrapArgument, InterpError> {
    arguments.get(index).ok_or_else(|| {
        VmError::Native(format!(
            "LambdaMetafactory bootstrap is missing its {expected} argument at position {index}"
        ))
        .into()
    })
}

fn method_type_argument(
    argument: &BootstrapArgument,
    position: usize,
) -> Result<&str, InterpError> {
    match argument {
        BootstrapArgument::MethodType(descriptor) => Ok(descriptor),
        other => Err(VmError::Native(format!(
            "LambdaMetafactory static argument {position} is {other:?}, not a MethodType"
        ))
        .into()),
    }
}

fn method_handle_argument(
    argument: &BootstrapArgument,
    position: usize,
) -> Result<&BootstrapMethodHandle, InterpError> {
    match argument {
        BootstrapArgument::MethodHandle(handle) => Ok(handle),
        other => Err(VmError::Native(format!(
            "LambdaMetafactory static argument {position} is {other:?}, not a MethodHandle"
        ))
        .into()),
    }
}

fn int_argument(argument: &BootstrapArgument, position: usize) -> Result<usize, InterpError> {
    match argument {
        BootstrapArgument::Integer(value) if *value >= 0 => Ok(*value as usize),
        other => Err(VmError::Native(format!(
            "LambdaMetafactory static argument {position} is {other:?}, not a non-negative int"
        ))
        .into()),
    }
}

fn class_argument(argument: &BootstrapArgument, position: usize) -> Result<&str, InterpError> {
    match argument {
        BootstrapArgument::Class(name) => Ok(name),
        other => Err(VmError::Native(format!(
            "LambdaMetafactory static argument {position} is {other:?}, not a Class"
        ))
        .into()),
    }
}

/// Whether a value's representation already matches a target parameter descriptor.
///
/// This is representation matching, not Java assignability: a reference value fits any
/// `L`/`[` parameter and a primitive must be the exact `Value` variant. Adaptation such
/// as boxing or widening is deliberately absent.
fn value_fits(descriptor: &str, value: Value) -> bool {
    match descriptor.as_bytes().first().copied() {
        Some(b'L' | b'[') => matches!(value, Value::Ref(_)),
        Some(b'Z' | b'B' | b'C' | b'S' | b'I') => matches!(value, Value::Int(_)),
        Some(b'J') => matches!(value, Value::Long(_)),
        Some(b'F') => matches!(value, Value::Float(_)),
        Some(b'D') => matches!(value, Value::Double(_)),
        _ => false,
    }
}

fn target_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("class {}", class.0))
}

/// The internal name of an object descriptor (`Lname;`), if `descriptor` has that shape.
fn object_descriptor_name(descriptor: &str) -> Option<&str> {
    descriptor
        .strip_prefix('L')
        .and_then(|rest| rest.strip_suffix(';'))
}
