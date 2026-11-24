//! Bytecode decoding, frames and the interpreter.
//!
//! M0.5a implements the integer subset and `invokestatic`; M2.4 added native dispatch
//! through the receiver-carrying ABI, M2.5 adds instance allocation, constructors,
//! instance fields and instance method calls, M2.6 adds primitive/reference arrays and
//! the integer control-flow instructions, M2.7 adds `athrow` and the decoded exception
//! table, M2.8 dispatches a reflected `Field`/`Method`/`Constructor` receiver to its
//! `java.lang.reflect` class, and M2.9 adds the `java.lang.Object` and `java.lang.String`
//! natives in `rava-vm`. M2.10 adds boxed `Integer` dispatch and the reflective-invocation
//! natives (`Method.invoke`, `Constructor.newInstance`) in [`reflect_invoke`], which
//! re-enter this crate's existing instance/static machinery. M2.12 dispatches a
//! `StringBuilder` and an `ArrayList` receiver to `java/lang/StringBuilder` and
//! `java/util/ArrayList`, whose natives live in `rava-vm`'s `collections` module. M2.13
//! dispatches a `HashMap` receiver to `java/util/HashMap`, whose natives live in
//! `rava-vm`'s `maps` module. The decoded form and the frame are the contracts a later JIT
//! consumes; there is no second frame model.

#![forbid(unsafe_code)]

mod error;
mod frame;
mod instr;
pub mod reflect_invoke;

pub use error::InterpError;
pub use frame::Frame;
pub use instr::{DecodedMethod, ExceptionHandler, Instr, IntCondition, decode};

use std::sync::Arc;

use rava_classfile::model::ACC_STATIC;
use rava_vm::{
    ArrayKind, BoxedPrimitive, ClassEntry, ClassId, ClassState, FrameRootsGuard, HeapObject,
    MemberKind, ObjRef, Shared, ThreadState, Value,
};

/// Run a static method with `args` and return its result (`None` for `void`).
pub fn run_static_method(
    shared: &Shared,
    thread: &ThreadState,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    run_method(shared, thread, class, name, descriptor, None, args)
}

/// Run an instance method on `receiver` and return its result (`None` for `void`).
///
/// `class` is the class that declares `name` and `descriptor`; a virtual caller resolves
/// the method first, then passes the declaring class. `receiver` becomes local 0 (`this`)
/// and `args` fill the following locals, in the same [`Frame`] a static method uses.
/// The class is initialized on entry, so a direct caller that allocated through
/// [`rava_vm::Shared::allocate_instance`] still triggers `<clinit>`.
pub fn run_instance_method(
    shared: &Shared,
    thread: &ThreadState,
    class: ClassId,
    name: &str,
    descriptor: &str,
    receiver: ObjRef,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    if receiver.is_null() {
        return Err(InterpError::WrongValue);
    }
    run_method(
        shared,
        thread,
        class,
        name,
        descriptor,
        Some(receiver),
        args,
    )
}

/// The one execution path: resolve on `class`, consult the native registry, then decode
/// and run `Code`. `receiver` selects the instance shape (local 0) and the native ABI's
/// receiver; `None` is a static call.
fn run_method(
    shared: &Shared,
    thread: &ThreadState,
    class: ClassId,
    name: &str,
    descriptor: &str,
    receiver: Option<ObjRef>,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    shared.check_no_locks_held()?;
    let entry = shared
        .entry(class)
        .ok_or_else(|| InterpError::Decode(format!("class {} is not loaded", class.0)))?;
    if name != "<clinit>" {
        ensure_initialized(shared, thread, class)?;
    }
    // An annotation instance answers its own members: `annotationType` and the element
    // methods are not declared on a superclass chain the JVM can resolve, so the
    // annotation path runs before method lookup. A name it does not handle (`getClass`)
    // falls through to the ordinary path.
    if let Some(reference) = receiver {
        if let Some(annotation) = shared.annotation(reference)? {
            if let Some(value) =
                rava_vm::annotation::invoke_member(shared, &annotation, name, descriptor, &args)?
            {
                return Ok(Some(value));
            }
        }
    }
    let file = entry.file.clone();
    let method =
        file.find_method(name, descriptor)?
            .ok_or_else(|| InterpError::MethodNotFound {
                class: entry.name.clone(),
                name: name.to_owned(),
                descriptor: descriptor.to_owned(),
            })?;
    // The native registry wins over the class file: the C1 stubs carry no `Code` and
    // are not marked `ACC_NATIVE`, so "has a native implementation" is a registry
    // question, not an access-flag question.
    if let Some(native) = shared.native(&entry.name, name, descriptor) {
        return native(
            shared,
            thread,
            entry.loader,
            receiver.unwrap_or(ObjRef::NULL),
            &args,
        )
        .map_err(InterpError::from);
    }
    if method.is_native() {
        return Err(InterpError::Decode(format!(
            "no native implementation for {}.{name}{descriptor}",
            entry.name
        )));
    }
    if method.is_abstract() {
        return Err(InterpError::NativeOrAbstract {
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        });
    }
    let code = file
        .code(method)?
        .ok_or_else(|| InterpError::NativeOrAbstract {
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        })?;
    let decoded = decode(&file, &code.data)?;

    // A bytecode method's receiver must be an instance of the declaring class (or a
    // subclass). Natives keep their own receiver rules, so this runs only on the `Code`
    // path.
    if let Some(reference) = receiver {
        let actual = shared.heap().instance(reference)?.class;
        if !is_assignable(shared, actual, class)? {
            let actual_name = shared
                .entry(actual)
                .map(|entry| entry.name.clone())
                .unwrap_or_else(|| format!("class {}", actual.0));
            return Err(InterpError::NotAssignable {
                expected: entry.name.clone(),
                found: actual_name,
            });
        }
    }

    let mut locals = vec![Value::Int(0); usize::from(decoded.max_locals)];
    let base = match receiver {
        Some(reference) => {
            let slot = locals
                .first_mut()
                .ok_or_else(|| InterpError::Decode("instance method has no local 0".to_owned()))?;
            *slot = Value::Ref(reference);
            1
        }
        None => 0,
    };
    if args.len() + base > locals.len() {
        return Err(InterpError::Decode(format!(
            "{} args do not fit in {} locals",
            args.len(),
            locals.len() - base
        )));
    }
    locals[base..base + args.len()].copy_from_slice(&args);
    let mut frame = Frame::new(
        class,
        locals,
        decoded.instructions,
        decoded.handlers,
        decoded.max_stack,
    );
    let guard = thread.enter_frame();
    guard.publish(&frame.roots());
    execute(shared, thread, &mut frame, &guard)
}

fn execute(
    shared: &Shared,
    thread: &ThreadState,
    frame: &mut Frame,
    guard: &FrameRootsGuard<'_>,
) -> Result<Option<Value>, InterpError> {
    loop {
        // Safepoint poll: when a request is pending, the frame publishes its fresh
        // roots before the request is acknowledged, so a collector can never observe
        // the acknowledgement with the previous epoch's snapshot. There is no
        // stop-the-world yet; the state machine is in place.
        thread.poll_with(|| guard.publish(&frame.roots()));
        let instruction = frame.instructions.get(frame.pc).cloned().ok_or_else(|| {
            InterpError::Decode("execution ran past the instruction list".to_owned())
        })?;
        frame.pc += 1;

        match instruction {
            Instr::Iconst(value) => frame.push(Value::Int(value))?,
            Instr::AconstNull => frame.push(Value::Ref(ObjRef::NULL))?,
            Instr::Dup => {
                let value = *frame.stack.last().ok_or(InterpError::StackUnderflow)?;
                frame.push(value)?;
            }
            Instr::New { class } => {
                // `new` initializes the class, then allocates an instance with the
                // layout's default field values. The JVM's "uninitialized" state until
                // `<init>` is not modeled: the constructor still runs and initializes
                // the fields, and no instruction can observe the object in between.
                let target = resolve_from(shared, frame.class, &class)?;
                ensure_initialized(shared, thread, target)?;
                let reference = shared.allocate_instance(target)?;
                frame.push(Value::Ref(reference))?;
            }
            Instr::Iinc(index, increment) => {
                let slot = frame.locals.get_mut(usize::from(index)).ok_or_else(|| {
                    InterpError::Decode(format!("local {index} is outside the frame"))
                })?;
                let value = slot.as_int().ok_or(InterpError::WrongValue)?;
                *slot = Value::Int(value.wrapping_add(i32::from(increment)));
            }
            Instr::ILoad(index) | Instr::ALoad(index) => {
                let value = *frame.locals.get(usize::from(index)).ok_or_else(|| {
                    InterpError::Decode(format!("local {index} is outside the frame"))
                })?;
                frame.push(value)?;
            }
            Instr::IStore(index) | Instr::AStore(index) => {
                let value = frame.pop()?;
                let slot = frame.locals.get_mut(usize::from(index)).ok_or_else(|| {
                    InterpError::Decode(format!("local {index} is outside the frame"))
                })?;
                *slot = value;
            }
            Instr::NewArray(kind) => {
                let length = checked_length(int(frame.pop()?)?)?;
                let reference = shared.heap().allocate_array(kind, length);
                frame.push(Value::Ref(reference))?;
            }
            Instr::ANewArray { component } => {
                let length = checked_length(int(frame.pop()?)?)?;
                // `anewarray` resolves its component type (JVMS §6.5.anewarray). Array
                // component types have no class identity in this slice, so only an object
                // component is resolved; the descriptor alone describes an array one.
                resolve_array_component(shared, frame.class, &component)?;
                let reference = shared.heap().allocate_array(ArrayKind::Reference, length);
                frame.push(Value::Ref(reference))?;
            }
            Instr::MultiANewArray {
                component,
                dimensions,
            } => {
                let mut counts = Vec::with_capacity(usize::from(dimensions));
                for _ in 0..dimensions {
                    counts.push(int(frame.pop()?)?);
                }
                counts.reverse();
                let reference = allocate_multidim(shared, &component, &counts)?;
                frame.push(Value::Ref(reference))?;
            }
            Instr::ArrayLength => {
                let reference = array_ref(frame)?;
                let length = shared.heap().array(reference)?.elements.len();
                let length = i32::try_from(length).map_err(|_| {
                    InterpError::Decode("array is longer than an int can hold".to_owned())
                })?;
                frame.push(Value::Int(length))?;
            }
            Instr::ArrayLoad(kind) => {
                let index = int(frame.pop()?)?;
                let reference = array_ref(frame)?;
                let value = {
                    let heap = shared.heap();
                    let array = heap.array(reference)?;
                    ensure_array_kind(kind, array.kind)?;
                    array.elements[element_position(index, array.elements.len())?]
                };
                frame.push(value)?;
            }
            Instr::ArrayStore(kind) => {
                // JVMS §6.5.iastore: the value is on top, then the index, then the array.
                let value = frame.pop()?;
                let index = int(frame.pop()?)?;
                let reference = array_ref(frame)?;
                {
                    let mut heap = shared.heap();
                    let array = heap.array_mut(reference)?;
                    ensure_array_kind(kind, array.kind)?;
                    let position = element_position(index, array.elements.len())?;
                    if !value_fits_array(kind, value) {
                        return Err(InterpError::WrongValue);
                    }
                    array.elements[position] = value;
                }
            }
            Instr::Goto(target) => frame.pc = target,
            Instr::If(condition, target) => {
                if condition.satisfied(int(frame.pop()?)?, 0) {
                    frame.pc = target;
                }
            }
            Instr::IfICmp(condition, target) => {
                let right = int(frame.pop()?)?;
                let left = int(frame.pop()?)?;
                if condition.satisfied(left, right) {
                    frame.pc = target;
                }
            }
            Instr::IfNull(target) => {
                if pop_is_null(frame)? {
                    frame.pc = target;
                }
            }
            Instr::IfNonNull(target) => {
                if !pop_is_null(frame)? {
                    frame.pc = target;
                }
            }
            Instr::LdcString(text) => {
                // JVMS §5.1: a string constant is interned. Using the intern table here
                // makes `ldc` of the same constant reference-identical and keeps a loop
                // from allocating one fresh string per execution.
                let reference = shared.intern_string(&text);
                frame.push(Value::Ref(reference))?;
            }
            Instr::GetStatic {
                class,
                name,
                descriptor,
            } => {
                let target = resolve_from(shared, frame.class, &class)?;
                ensure_initialized(shared, thread, target)?;
                let entry = shared
                    .entry(target)
                    .ok_or_else(|| InterpError::Decode("static class vanished".to_owned()))?;
                let value = entry
                    .statics
                    .lock()
                    .expect("static field lock poisoned")
                    .get(&(name.clone(), descriptor.clone()))
                    .copied()
                    .ok_or_else(|| {
                        InterpError::Decode(format!(
                            "no static field {}.{name}:{descriptor}",
                            entry.name
                        ))
                    })?;
                frame.push(value)?;
            }
            Instr::GetField {
                class,
                name,
                descriptor,
            } => {
                let (declaring, is_static) =
                    resolve_field(shared, frame.class, &class, &name, &descriptor)?;
                if is_static {
                    return Err(InterpError::StaticFieldAccess {
                        class,
                        name,
                        descriptor,
                    });
                }
                let receiver = pop_receiver(frame)?;
                let slot = instance_field_slot(shared, receiver, declaring, &name, &descriptor)?;
                let value = shared.heap().instance(receiver)?.fields[slot];
                frame.push(value)?;
            }
            Instr::PutField {
                class,
                name,
                descriptor,
            } => {
                let (declaring, is_static) =
                    resolve_field(shared, frame.class, &class, &name, &descriptor)?;
                if is_static {
                    return Err(InterpError::StaticFieldAccess {
                        class,
                        name,
                        descriptor,
                    });
                }
                // JVMS §6.5.putfield: the value is on top, the object reference below it.
                let value = frame.pop()?;
                let receiver = pop_receiver(frame)?;
                let slot = instance_field_slot(shared, receiver, declaring, &name, &descriptor)?;
                shared.heap().instance_mut(receiver)?.fields[slot] = value;
            }
            Instr::InvokeVirtual {
                class: _,
                name,
                descriptor,
            } => {
                let caller_pc = frame.pc - 1;
                let args = pop_arguments(frame, &descriptor)?;
                let receiver = pop_receiver(frame)?;
                let start = dispatch_class(shared, frame.class, receiver)?;
                let result = invoke_instance(
                    shared,
                    thread,
                    frame,
                    guard,
                    start,
                    receiver,
                    args,
                    &name,
                    &descriptor,
                );
                deliver_call(shared, frame, caller_pc, &descriptor, result)?;
            }
            Instr::InvokeSpecial {
                class,
                name,
                descriptor,
            } => {
                let caller_pc = frame.pc - 1;
                let args = pop_arguments(frame, &descriptor)?;
                let receiver = pop_receiver(frame)?;
                // `invokespecial` resolves on the instruction's owner, not the receiver's
                // class: that is what keeps a constructor on its declared class and a
                // superclass call non-virtual.
                let owner = resolve_from(shared, frame.class, &class)?;
                let result = invoke_instance(
                    shared,
                    thread,
                    frame,
                    guard,
                    owner,
                    receiver,
                    args,
                    &name,
                    &descriptor,
                );
                deliver_call(shared, frame, caller_pc, &descriptor, result)?;
            }
            Instr::IAdd | Instr::ISub | Instr::IMul | Instr::IDiv => {
                let right = int(frame.pop()?)?;
                let left = int(frame.pop()?)?;
                let result = match instruction {
                    Instr::IAdd => left.wrapping_add(right),
                    Instr::ISub => left.wrapping_sub(right),
                    Instr::IMul => left.wrapping_mul(right),
                    Instr::IDiv => left.checked_div(right).ok_or_else(|| {
                        InterpError::Decode("integer division by zero".to_owned())
                    })?,
                    _ => unreachable!(),
                };
                frame.push(Value::Int(result))?;
            }
            Instr::InvokeStatic {
                class,
                name,
                descriptor,
            } => {
                let caller_pc = frame.pc - 1;
                let count = argument_count(&descriptor)?;
                let mut args = Vec::with_capacity(count);
                for _ in 0..count {
                    args.push(frame.pop()?);
                }
                args.reverse();
                let target = resolve_from(shared, frame.class, &class)?;
                shared.check_no_locks_held()?;
                // The arguments have been popped from this frame; publish them so
                // they stay roots while the callee runs.
                publish_roots(guard, frame, &args);
                let result = run_static_method(shared, thread, target, &name, &descriptor, args);
                deliver_call(shared, frame, caller_pc, &descriptor, result)?;
            }
            Instr::Athrow => {
                let throwable = throw_reference(frame)?;
                // `frame.pc` already names the next instruction, so the throwing one is
                // the index just behind it.
                let throwing_pc = frame.pc - 1;
                if !handle_thrown(shared, frame, throwing_pc, throwable)? {
                    // No handler here: the throwable leaves this frame unchanged and the
                    // caller's table gets the same handle.
                    return Err(InterpError::Thrown(throwable));
                }
            }
            Instr::IReturn | Instr::AReturn => return Ok(Some(frame.pop()?)),
            Instr::Return => return Ok(None),
        }
    }
}

fn pop_arguments(frame: &mut Frame, descriptor: &str) -> Result<Vec<Value>, InterpError> {
    let count = argument_count(descriptor)?;
    let mut args = Vec::with_capacity(count);
    for _ in 0..count {
        args.push(frame.pop()?);
    }
    args.reverse();
    Ok(args)
}

/// Pop `athrow`'s reference, distinguishing a null handle from a non-reference value.
///
/// Java would raise a `NullPointerException` for the null case; this slice has no
/// exception-object synthesis, so both violations are typed interpreter errors and
/// neither searches the handler table.
fn throw_reference(frame: &mut Frame) -> Result<ObjRef, InterpError> {
    match frame.pop()? {
        Value::Ref(reference) if reference.is_null() => Err(InterpError::NullThrow),
        Value::Ref(reference) => Ok(reference),
        _ => Err(InterpError::NonReferenceThrow),
    }
}

/// Route a throwable through `frame`'s exception table.
///
/// Entries are searched in class-file order for one whose protected range contains the
/// throwing instruction and whose catch type is assignable from the throwable's runtime
/// class; a catch-all entry (`catch_type = 0`) matches any throwable. On a match the
/// operand stack is cleared, the throwable becomes its only value and the frame jumps to
/// the handler. Returns `false` when no entry matches, leaving the throwable for the
/// caller.
fn handle_thrown(
    shared: &Shared,
    frame: &mut Frame,
    throwing_pc: usize,
    throwable: ObjRef,
) -> Result<bool, InterpError> {
    // The runtime class is resolved lazily: a frame whose matching entry is the
    // catch-all does not need it.
    let mut throwable_class: Option<ClassId> = None;
    let mut target: Option<usize> = None;
    for handler in &frame.handlers {
        if throwing_pc < handler.start || throwing_pc >= handler.end {
            continue;
        }
        let catches = match &handler.catch_type {
            None => true,
            Some(catch_type) => {
                let actual = match throwable_class {
                    Some(class) => class,
                    None => {
                        let class = dispatch_class(shared, frame.class, throwable)?;
                        throwable_class = Some(class);
                        class
                    }
                };
                let expected = resolve_from(shared, frame.class, catch_type)?;
                is_assignable(shared, actual, expected)?
            }
        };
        if catches {
            target = Some(handler.handler);
            break;
        }
    }

    let Some(target) = target else {
        return Ok(false);
    };
    frame.stack.clear();
    frame.push(Value::Ref(throwable))?;
    frame.pc = target;
    Ok(true)
}

/// Deliver a call's outcome to the caller frame.
///
/// A `Thrown` error is not the caller's failure: it searches the caller's exception
/// table at `caller_pc` and either jumps to a handler or re-raises the same throwable.
/// Every other error is returned unchanged, so a VM error is never mistaken for a Java
/// exception.
fn deliver_call(
    shared: &Shared,
    frame: &mut Frame,
    caller_pc: usize,
    descriptor: &str,
    result: Result<Option<Value>, InterpError>,
) -> Result<(), InterpError> {
    match result {
        Ok(value) => push_result(frame, descriptor, value),
        Err(InterpError::Thrown(throwable)) => {
            if handle_thrown(shared, frame, caller_pc, throwable)? {
                Ok(())
            } else {
                Err(InterpError::Thrown(throwable))
            }
        }
        Err(error) => Err(error),
    }
}

/// Pop the receiver of an instance call and require a non-null reference.
fn pop_receiver(frame: &mut Frame) -> Result<ObjRef, InterpError> {
    match frame.pop()? {
        Value::Ref(reference) if !reference.is_null() => Ok(reference),
        _ => Err(InterpError::WrongValue),
    }
}

/// Push a call's result unless the descriptor returns `void`.
fn push_result(
    frame: &mut Frame,
    descriptor: &str,
    result: Option<Value>,
) -> Result<(), InterpError> {
    if descriptor.ends_with(")V") {
        return Ok(());
    }
    let value = result.ok_or_else(|| InterpError::Decode("missing return value".to_owned()))?;
    frame.push(value)
}

/// Resolve an instance call from `start`, then prefer the native registry and fall back
/// to the method's `Code`.
///
/// The receiver and arguments have already been popped from `frame`; they are published
/// as call roots before the call so a callee (or a native) cannot lose them.
#[allow(clippy::too_many_arguments)]
fn invoke_instance(
    shared: &Shared,
    thread: &ThreadState,
    frame: &Frame,
    guard: &FrameRootsGuard<'_>,
    start: ClassId,
    receiver: ObjRef,
    args: Vec<Value>,
    name: &str,
    descriptor: &str,
) -> Result<Option<Value>, InterpError> {
    // An annotation receiver dispatches to its annotation type. The members this slice
    // answers (`annotationType`, the element methods, `equals`, `hashCode`, `toString`)
    // are not all declared on that type's superclass chain, so they are served before
    // resolution; anything else (`getClass`) resolves normally.
    if let Some(annotation) = shared.annotation(receiver)? {
        if let Some(value) =
            rava_vm::annotation::invoke_member(shared, &annotation, name, descriptor, &args)?
        {
            return Ok(Some(value));
        }
    }
    let (entry, is_native) = resolve_virtual(shared, start, name, descriptor)?;
    shared.check_no_locks_held()?;
    let mut call_roots = args.clone();
    call_roots.push(Value::Ref(receiver));
    publish_roots(guard, frame, &call_roots);
    // Registry first: a C1 stub method has no `Code` and no `ACC_NATIVE` flag, and the
    // native is still the implementation.
    if let Some(native) = shared.native(&entry.name, name, descriptor) {
        return native(shared, thread, entry.loader, receiver, &args).map_err(InterpError::from);
    }
    if is_native {
        return Err(InterpError::Decode(format!(
            "no native implementation for {}.{name}{descriptor}",
            entry.name
        )));
    }
    run_instance_method(shared, thread, entry.id, name, descriptor, receiver, args)
}

/// Resolve `owner.name:descriptor` up the superclass chain from the instruction's owner.
///
/// Returns the declaring class and whether the field is `static`. A field with the right
/// name but the wrong descriptor is an error, not a silent miss: the interpreter never
/// reinterprets a slot under another type.
fn resolve_field(
    shared: &Shared,
    current: ClassId,
    owner: &str,
    name: &str,
    descriptor: &str,
) -> Result<(ClassId, bool), InterpError> {
    let mut target = resolve_from(shared, current, owner)?;
    let mut mismatch: Option<(String, String)> = None;
    loop {
        let entry = shared
            .entry(target)
            .ok_or_else(|| InterpError::Decode("field owner vanished".to_owned()))?;
        for field in &entry.file.fields {
            let field_name = entry.file.field_name(field)?;
            if field_name != name {
                continue;
            }
            let field_descriptor = entry.file.field_descriptor(field)?;
            if field_descriptor == descriptor {
                return Ok((target, field.access_flags & ACC_STATIC != 0));
            }
            if mismatch.is_none() {
                mismatch = Some((entry.name.clone(), field_descriptor.to_owned()));
            }
        }
        let Some(super_class) = shared.resolution_superclass(target)? else {
            break;
        };
        target = super_class;
    }
    match mismatch {
        Some((class, found)) => Err(InterpError::FieldDescriptorMismatch {
            class,
            name: name.to_owned(),
            found,
        }),
        None => Err(InterpError::FieldNotFound {
            class: owner.to_owned(),
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        }),
    }
}

/// The layout slot of `name:descriptor` on the receiver's runtime class.
///
/// The field was resolved on the instruction's owner; the slot is read from the
/// receiver's own layout, so a subclass keeps the superclass prefix and inheritance
/// works without a second offset table.
fn instance_field_slot(
    shared: &Shared,
    receiver: ObjRef,
    declaring: ClassId,
    name: &str,
    descriptor: &str,
) -> Result<usize, InterpError> {
    let actual = shared.heap().instance(receiver)?.class;
    if !is_assignable(shared, actual, declaring)? {
        return Err(InterpError::NotAssignable {
            expected: class_name(shared, declaring),
            found: class_name(shared, actual),
        });
    }
    let layout = shared.field_layout(actual)?;
    layout.slot(name, descriptor).ok_or_else(|| {
        InterpError::Decode(format!(
            "class {} has no slot for field {name}:{descriptor}",
            class_name(shared, actual)
        ))
    })
}

/// Is `from` the same class as `to`, or a subclass of it?
fn is_assignable(shared: &Shared, from: ClassId, to: ClassId) -> Result<bool, InterpError> {
    let mut current = from;
    loop {
        if current == to {
            return Ok(true);
        }
        let Some(super_class) = shared.resolution_superclass(current)? else {
            return Ok(false);
        };
        current = super_class;
    }
}

fn class_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("class {}", class.0))
}

fn int(value: Value) -> Result<i32, InterpError> {
    value.as_int().ok_or(InterpError::WrongValue)
}

/// Pop a reference for an array instruction. A null handle is not rejected here: the
/// heap reports it as `HeapError::NullReference`, the same typed error every other
/// reference access produces.
fn array_ref(frame: &mut Frame) -> Result<ObjRef, InterpError> {
    match frame.pop()? {
        Value::Ref(reference) => Ok(reference),
        _ => Err(InterpError::WrongValue),
    }
}

/// Pop a reference and report whether it is null, for `ifnull`/`ifnonnull`.
fn pop_is_null(frame: &mut Frame) -> Result<bool, InterpError> {
    match frame.pop()? {
        Value::Ref(reference) => Ok(reference.is_null()),
        _ => Err(InterpError::WrongValue),
    }
}

/// A non-negative array length, or `NegativeArraySize`.
fn checked_length(length: i32) -> Result<usize, InterpError> {
    usize::try_from(length).map_err(|_| InterpError::NegativeArraySize(length))
}

/// A non-negative element index inside `length`, or `ArrayIndexOutOfBounds`.
fn element_position(index: i32, length: usize) -> Result<usize, InterpError> {
    let position =
        usize::try_from(index).map_err(|_| InterpError::ArrayIndexOutOfBounds { index, length })?;
    if position >= length {
        return Err(InterpError::ArrayIndexOutOfBounds { index, length });
    }
    Ok(position)
}

/// Check that an array instruction addresses the array's element kind.
///
/// `baload`/`bastore` are decoded as [`ArrayKind::Byte`] but accept a `boolean[]` too;
/// every other opcode matches its kind exactly.
fn ensure_array_kind(expected: ArrayKind, found: ArrayKind) -> Result<(), InterpError> {
    let byte_or_boolean = expected == ArrayKind::Byte && found == ArrayKind::Boolean;
    if expected == found || byte_or_boolean {
        return Ok(());
    }
    Err(InterpError::ArrayKindMismatch {
        expected: expected_array_name(expected),
        found,
    })
}

fn expected_array_name(kind: ArrayKind) -> &'static str {
    if kind == ArrayKind::Byte {
        "boolean or byte"
    } else {
        kind.name()
    }
}

/// Whether a value can be stored through an array store opcode of this element kind.
fn value_fits_array(kind: ArrayKind, value: Value) -> bool {
    matches!(
        (kind, value),
        (
            ArrayKind::Boolean
                | ArrayKind::Byte
                | ArrayKind::Char
                | ArrayKind::Short
                | ArrayKind::Int,
            Value::Int(_),
        ) | (ArrayKind::Long, Value::Long(_))
            | (ArrayKind::Float, Value::Float(_))
            | (ArrayKind::Double, Value::Double(_))
            | (ArrayKind::Reference, Value::Ref(_))
    )
}

/// Resolve an `anewarray` component through the current class's loader.
///
/// An array component (`[I`, `[[Ljava/lang/String;`) names a type this slice has no
/// class identity for, so it is described by the descriptor and not resolved.
fn resolve_array_component(
    shared: &Shared,
    current: ClassId,
    component: &str,
) -> Result<(), InterpError> {
    if component.starts_with('[') {
        return Ok(());
    }
    resolve_from(shared, current, component)?;
    Ok(())
}

/// Allocate a `multianewarray` array, one nesting level per count.
///
/// `descriptor` is the full array descriptor from the instruction and `counts` has one
/// length per allocated dimension, outermost first. When fewer dimensions are allocated
/// than the descriptor has, the innermost allocated level is a reference array whose
/// elements stay null, which is what `new int[2][]` produces.
fn allocate_multidim(
    shared: &Shared,
    descriptor: &str,
    counts: &[i32],
) -> Result<ObjRef, InterpError> {
    let element = descriptor.strip_prefix('[').ok_or_else(|| {
        InterpError::Decode(format!(
            "multianewarray descriptor {descriptor} is not an array"
        ))
    })?;
    let Some((&first, rest)) = counts.split_first() else {
        return Err(InterpError::Decode(
            "multianewarray allocated no dimensions".to_owned(),
        ));
    };
    let length = checked_length(first)?;
    if rest.is_empty() {
        let kind = array_element_kind(element)?;
        return Ok(shared.heap().allocate_array(kind, length));
    }
    let reference = shared.heap().allocate_array(ArrayKind::Reference, length);
    for position in 0..length {
        let child = allocate_multidim(shared, element, rest)?;
        shared.heap().array_mut(reference)?.elements[position] = Value::Ref(child);
    }
    Ok(reference)
}

/// The `ArrayKind` of one array descriptor's element type.
fn array_element_kind(element: &str) -> Result<ArrayKind, InterpError> {
    Ok(match element.as_bytes().first().copied() {
        Some(b'I') => ArrayKind::Int,
        Some(b'J') => ArrayKind::Long,
        Some(b'F') => ArrayKind::Float,
        Some(b'D') => ArrayKind::Double,
        Some(b'B') => ArrayKind::Byte,
        Some(b'C') => ArrayKind::Char,
        Some(b'S') => ArrayKind::Short,
        Some(b'Z') => ArrayKind::Boolean,
        Some(b'L' | b'[') => ArrayKind::Reference,
        _ => {
            return Err(InterpError::Decode(format!(
                "bad array element descriptor {element}"
            )));
        }
    })
}

/// Replace the frame's published roots with its locals/stack plus `values`, the
/// references a call has just popped from the operand stack.
fn publish_roots(guard: &FrameRootsGuard<'_>, frame: &Frame, values: &[Value]) {
    let mut roots = frame.roots();
    roots.extend(values.iter().filter_map(|value| match value {
        Value::Ref(reference) if !reference.is_null() => Some(*reference),
        _ => None,
    }));
    guard.publish(&roots);
}

fn resolve_from(shared: &Shared, current: ClassId, name: &str) -> Result<ClassId, InterpError> {
    let loader = shared
        .entry(current)
        .ok_or_else(|| InterpError::Decode("current class vanished".to_owned()))?
        .loader;
    Ok(shared.load_class(loader, name)?)
}

/// The class a virtual call starts resolution from.
///
/// An instance dispatches from its defining class. A string, a class object, a box, a
/// `StringBuilder`, an `ArrayList` or a `HashMap` has no `ClassId` of its own in the heap,
/// so it dispatches from `java/lang/String`, `java/lang/Class`, its boxed class
/// (`java/lang/Integer`), `java/lang/StringBuilder`, `java/util/ArrayList` or
/// `java/util/HashMap`, resolved through the caller's loader. An array has no class
/// identity yet; `Object` methods (including `getClass`, which reports a typed error) still
/// dispatch, and any other method fails resolution normally.
fn dispatch_class(
    shared: &Shared,
    current: ClassId,
    reference: ObjRef,
) -> Result<ClassId, InterpError> {
    enum ReceiverKind {
        Instance(ClassId),
        Str,
        Class,
        Reflect(MemberKind),
        Boxed(BoxedPrimitive),
        Annotation(ClassId),
        Enum(ClassId),
        StringBuilder,
        List,
        Map,
        Array,
    }

    let kind = {
        let heap = shared.heap();
        match heap.get(reference).map_err(|_| InterpError::WrongValue)? {
            HeapObject::Instance(instance) => ReceiverKind::Instance(instance.class),
            HeapObject::Str(_) => ReceiverKind::Str,
            HeapObject::Class(_) => ReceiverKind::Class,
            HeapObject::Reflect(member) => ReceiverKind::Reflect(member.kind),
            HeapObject::Boxed(boxed) => ReceiverKind::Boxed(*boxed),
            HeapObject::Annotation(annotation) => ReceiverKind::Annotation(annotation.class),
            HeapObject::Enum(constant) => ReceiverKind::Enum(constant.class),
            HeapObject::StringBuilder(_) => ReceiverKind::StringBuilder,
            HeapObject::List(_) => ReceiverKind::List,
            HeapObject::Map(_) => ReceiverKind::Map,
            HeapObject::Array(_) => ReceiverKind::Array,
        }
    };

    match kind {
        ReceiverKind::Instance(class) => Ok(class),
        ReceiverKind::Str => resolve_from(shared, current, "java/lang/String"),
        ReceiverKind::Class => resolve_from(shared, current, "java/lang/Class"),
        ReceiverKind::Reflect(kind) => resolve_from(shared, current, kind.class_name()),
        ReceiverKind::Boxed(boxed) => resolve_from(shared, current, boxed.class_name()),
        ReceiverKind::Annotation(class) | ReceiverKind::Enum(class) => Ok(class),
        ReceiverKind::StringBuilder => {
            resolve_from(shared, current, rava_vm::collections::STRING_BUILDER_CLASS)
        }
        ReceiverKind::List => resolve_from(shared, current, rava_vm::collections::ARRAY_LIST_CLASS),
        ReceiverKind::Map => resolve_from(shared, current, rava_vm::maps::HASH_MAP_CLASS),
        ReceiverKind::Array => resolve_from(shared, current, "java/lang/Object"),
    }
}

/// Resolve a virtual method from `start` up the superclass chain, the way JVMS §5.4.5
/// resolves an instance method. Returns the declaring class and whether the declaration
/// is `ACC_NATIVE`; the caller consults the native registry before the flag, because the
/// C1 stubs declare no `Code` and do not set `ACC_NATIVE`.
fn resolve_virtual(
    shared: &Shared,
    start: ClassId,
    name: &str,
    descriptor: &str,
) -> Result<(Arc<ClassEntry>, bool), InterpError> {
    let mut current = start;
    loop {
        let entry = shared
            .entry(current)
            .ok_or_else(|| InterpError::Decode("receiver class vanished".to_owned()))?;
        if let Some(method) = entry.file.find_method(name, descriptor)? {
            return Ok((Arc::clone(&entry), method.is_native()));
        }
        let Some(super_class) = shared.resolution_superclass(current)? else {
            return Err(InterpError::MethodNotFound {
                class: entry.name.clone(),
                name: name.to_owned(),
                descriptor: descriptor.to_owned(),
            });
        };
        current = super_class;
    }
}

fn ensure_initialized(
    shared: &Shared,
    thread: &ThreadState,
    class: ClassId,
) -> Result<(), InterpError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| InterpError::Decode(format!("class {} is not loaded", class.0)))?;
    {
        let mut state = entry.state.lock().expect("class state lock poisoned");
        match *state {
            ClassState::Initialized => return Ok(()),
            ClassState::Initializing => return Ok(()),
            ClassState::Erroneous => {
                return Err(InterpError::Decode(format!(
                    "class {} failed initialization",
                    entry.name
                )));
            }
            ClassState::Uninitialized => *state = ClassState::Initializing,
        }
    }

    let result = (|| -> Result<(), InterpError> {
        if entry.file.find_method("<clinit>", "()V")?.is_some() {
            run_static_method(shared, thread, class, "<clinit>", "()V", Vec::new())?;
        }
        Ok(())
    })();

    let mut state = entry.state.lock().expect("class state lock poisoned");
    *state = if result.is_ok() {
        ClassState::Initialized
    } else {
        ClassState::Erroneous
    };
    result
}

fn argument_count(descriptor: &str) -> Result<usize, InterpError> {
    let Some((args, _)) = descriptor
        .strip_prefix('(')
        .and_then(|rest| rest.split_once(')'))
    else {
        return Err(InterpError::BadDescriptor(descriptor.to_owned()));
    };
    let mut count = 0;
    let mut chars = args.chars();
    while let Some(ch) = chars.next() {
        match ch {
            'I' | 'J' | 'F' | 'D' | 'Z' | 'B' | 'C' | 'S' => count += 1,
            'L' | '[' => {
                // Skip a reference descriptor; one value either way.
                if ch == 'L' {
                    for next in chars.by_ref() {
                        if next == ';' {
                            break;
                        }
                    }
                }
                count += 1;
            }
            other => {
                return Err(InterpError::BadDescriptor(format!(
                    "{descriptor} contains {other}"
                )));
            }
        }
    }
    Ok(count)
}
