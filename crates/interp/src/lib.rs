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
//! `rava-vm`'s `maps` module. M2.14 adds the named lookup natives to `rava-vm`'s
//! `reflection` module; this crate's descriptor parser now skips an array parameter's
//! element descriptor, so `Class.getDeclaredMethod(String, Class[])` is callable from
//! bytecode. M2.17 adds the class literal: `ldc`/`ldc_w` of a `CONSTANT_Class` pushes the
//! cached class object (a loaded class, a primitive or an array class), resolved through
//! the executing frame's loader. M2.18 adds `invokedynamic` execution for the
//! `StringConcatFactory` call sites javac emits for `+`, dispatching to
//! [`rava_vm::string_concat`] with the decoded bootstrap entry and the call site's
//! arguments. M2.19 adds `invokeinterface` and the lambda path: a `LambdaMetafactory`
//! bootstrap creates a [`rava_vm::LambdaObject`] (the interface, target and captured
//! values), and an interface call on that object runs the target through this crate's
//! existing static/instance machinery. M3.4 adds the `MethodHandle.invoke*` natives in
//! [`method_handles`]: a handle records its target by `(owner, name, descriptor)` and
//! invokes it through the same static/instance machinery, with a virtual handle dispatching
//! on the receiver's runtime class. The decoded form and the frame are the contracts a
//! later JIT consumes; there is no second frame model.

#![forbid(unsafe_code)]

pub mod callbacks;
mod error;
mod frame;
pub mod futures;
mod instr;
mod lambda;
pub mod method_handles;
pub mod object_methods;
pub mod proxy;
pub mod reflect_invoke;
pub mod streams;
pub mod unsafe_natives;

pub use error::InterpError;
pub use frame::Frame;
pub use instr::{DecodedMethod, ExceptionHandler, Instr, IntCondition, decode};

use std::collections::HashSet;
use std::sync::Arc;

use rava_classfile::model::ACC_STATIC;
use rava_vm::{
    ArrayKind, BoxedPrimitive, ClassEntry, ClassId, ClassRef, ClassState, FrameRootsGuard,
    HeapObject, MemberKind, ObjRef, PrimitiveKind, Shared, ThreadState, Value, VmError,
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
            class: entry.name.clone(),
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        });
    }
    let code = file
        .code(method)?
        .ok_or_else(|| InterpError::NativeOrAbstract {
            class: entry.name.clone(),
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        })?;
    let decoded = decode(&file, &code.data).map_err(|error| match error {
        InterpError::Decode(message) => {
            InterpError::Decode(format!("{message} (in {}.{name}{descriptor})", entry.name))
        }
        other => other,
    })?;

    // A bytecode method's receiver must be an instance of the declaring class (or a
    // subclass). Natives keep their own receiver rules, so this runs only on the `Code`
    // path.
    if let Some(reference) = receiver {
        let actual = shared.heap().object_class(reference)?;
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
            Instr::LConst(value) => frame.push(Value::Long(value))?,
            Instr::FConst(value) => frame.push(Value::Float(value))?,
            Instr::DConst(value) => frame.push(Value::Double(value))?,
            Instr::AconstNull => frame.push(Value::Ref(ObjRef::NULL))?,
            Instr::Pop => {
                frame.pop()?;
            }
            Instr::Pop2 => {
                let value = frame.pop()?;
                if !is_category_two(value) {
                    frame.pop()?;
                }
            }
            Instr::Dup => {
                let value = *frame.stack.last().ok_or(InterpError::StackUnderflow)?;
                frame.push(value)?;
            }
            Instr::DupX1 => stack_dup_x1(frame)?,
            Instr::DupX2 => stack_dup_x2(frame)?,
            Instr::Dup2 => stack_dup2(frame)?,
            Instr::Dup2X1 => stack_dup2_x1(frame)?,
            Instr::Dup2X2 => stack_dup2_x2(frame)?,
            Instr::Swap => stack_swap(frame)?,
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
                *slot = Value::Int(value.wrapping_add(increment));
            }
            Instr::ILoad(index)
            | Instr::LLoad(index)
            | Instr::FLoad(index)
            | Instr::DLoad(index)
            | Instr::ALoad(index) => {
                let value = *frame.locals.get(usize::from(index)).ok_or_else(|| {
                    InterpError::Decode(format!("local {index} is outside the frame"))
                })?;
                frame.push(value)?;
            }
            Instr::IStore(index)
            | Instr::LStore(index)
            | Instr::FStore(index)
            | Instr::DStore(index)
            | Instr::AStore(index) => {
                let value = frame.pop()?;
                let slot = frame.locals.get_mut(usize::from(index)).ok_or_else(|| {
                    InterpError::Decode(format!("local {index} is outside the frame"))
                })?;
                *slot = value;
            }
            Instr::NewArray(kind) => {
                let length = checked_length(int(frame.pop()?)?)?;
                let component =
                    kind.primitive_kind()
                        .map(ClassRef::Primitive)
                        .ok_or_else(|| {
                            InterpError::Decode("newarray has a reference element kind".to_owned())
                        })?;
                let reference = shared.heap().allocate_array(component, length);
                frame.push(Value::Ref(reference))?;
            }
            Instr::ANewArray { component } => {
                let length = checked_length(int(frame.pop()?)?)?;
                // `anewarray` resolves its component type (JVMS §6.5.anewarray), which
                // may itself be an array class (`[[I` is allocated with `anewarray` of
                // `[I`).
                let component = resolve_class_ref(shared, frame.class, &component)?;
                let reference = shared.heap().allocate_array(component, length);
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
                let reference = allocate_multidim(shared, frame.class, &component, &counts)?;
                frame.push(Value::Ref(reference))?;
            }
            Instr::ArrayLength => {
                let reference = pop_reference(frame)?;
                let length = shared.heap().array(reference)?.elements.len();
                let length = i32::try_from(length).map_err(|_| {
                    InterpError::Decode("array is longer than an int can hold".to_owned())
                })?;
                frame.push(Value::Int(length))?;
            }
            Instr::ArrayLoad(kind) => {
                let index = int(frame.pop()?)?;
                let reference = pop_reference(frame)?;
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
                let reference = pop_reference(frame)?;
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
            Instr::IfACmpEq(target) => {
                let right = pop_reference(frame)?;
                let left = pop_reference(frame)?;
                if left == right {
                    frame.pc = target;
                }
            }
            Instr::IfACmpNe(target) => {
                let right = pop_reference(frame)?;
                let left = pop_reference(frame)?;
                if left != right {
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
            Instr::TableSwitch {
                default,
                low,
                targets,
            } => {
                let key = int(frame.pop()?)?;
                let index = i64::from(key) - i64::from(low);
                let target = usize::try_from(index)
                    .ok()
                    .and_then(|index| targets.get(index).copied())
                    .unwrap_or(default);
                frame.pc = target;
            }
            Instr::LookupSwitch {
                default,
                keys,
                targets,
            } => {
                let key = int(frame.pop()?)?;
                let target = keys
                    .binary_search(&key)
                    .ok()
                    .and_then(|index| targets.get(index).copied())
                    .unwrap_or(default);
                frame.pc = target;
            }
            Instr::InstanceOf { class } => {
                // JVMS §6.5.instanceof: a null reference is an instance of no type, so the
                // instruction pushes 0 without resolving the operand. A primitive operand
                // names a non-reference type, so no object is ever its instance either.
                let reference = pop_reference(frame)?;
                let result = if reference.is_null() {
                    false
                } else {
                    let target = resolve_class_operand(shared, frame.class, &class)?;
                    let actual = runtime_class_ref(shared, reference)?;
                    class_ref_assignable(shared, &actual, &target)?
                };
                frame.push(Value::Int(i32::from(result)))?;
            }
            Instr::CheckCast { class } => {
                let reference = pop_reference(frame)?;
                if reference.is_null() {
                    // A null passes every reference cast. A primitive operand names no
                    // reference type, so even null is not assignable to it.
                    if let Some(kind) = primitive_operand(&class) {
                        return Err(InterpError::ClassCast {
                            expected: kind.name().to_owned(),
                            found: "null".to_owned(),
                        });
                    }
                } else {
                    let target = resolve_class_operand(shared, frame.class, &class)?;
                    let actual = runtime_class_ref(shared, reference)?;
                    if !class_ref_assignable(shared, &actual, &target)? {
                        return Err(InterpError::ClassCast {
                            expected: class_ref_label(shared, &target)?,
                            found: class_ref_label(shared, &actual)?,
                        });
                    }
                }
                frame.push(Value::Ref(reference))?;
            }
            Instr::LdcString(text) => {
                // JVMS §5.1: a string constant is interned. Using the intern table here
                // makes `ldc` of the same constant reference-identical and keeps a loop
                // from allocating one fresh string per execution.
                let reference = shared.intern_string(&text);
                frame.push(Value::Ref(reference))?;
            }
            Instr::LdcClass(name) => {
                // JVMS §5.4.3: a class constant is resolved through the defining loader
                // of the class that executes the instruction, and only resolved — not
                // initialized. The cache owns the object, so repeated execution of the
                // same constant pushes the same `ObjRef`.
                let reference = class_literal(shared, frame.class, &name)?;
                frame.push(Value::Ref(reference))?;
            }
            Instr::LdcInt(value) => frame.push(Value::Int(value))?,
            Instr::LdcFloat(value) => frame.push(Value::Float(value))?,
            Instr::LdcLong(value) => frame.push(Value::Long(value))?,
            Instr::LdcDouble(value) => frame.push(Value::Double(value))?,
            Instr::GetStatic {
                class,
                name,
                descriptor,
            } => {
                let target = static_field(shared, frame.class, &class, &name, &descriptor)?;
                ensure_initialized(shared, thread, target)?;
                let entry = shared
                    .entry(target)
                    .ok_or_else(|| InterpError::Decode("static class vanished".to_owned()))?;
                // The statics table is the normal home of the value; the primitive
                // wrapper `TYPE` fields are resolved virtually by the VM because the C1
                // stubs carry no initializer `Code` to write them.
                let value = shared
                    .static_field_value(target, &name, &descriptor)?
                    .ok_or_else(|| {
                        InterpError::Decode(format!(
                            "no static field {}.{name}:{descriptor}",
                            entry.name
                        ))
                    })?;
                frame.push(value)?;
            }
            Instr::PutStatic {
                class,
                name,
                descriptor,
            } => {
                // JVMS §6.5.putstatic: the value is on top of the operand stack. The
                // owner is resolved through the executing class's loader, then the
                // declaring class is initialized before the store.
                let target = static_field(shared, frame.class, &class, &name, &descriptor)?;
                ensure_initialized(shared, thread, target)?;
                let value = frame.pop()?;
                if !value_matches_descriptor(&descriptor, value) {
                    return Err(InterpError::FieldValueMismatch {
                        class: class_name(shared, target),
                        name,
                        descriptor,
                    });
                }
                let entry = shared
                    .entry(target)
                    .ok_or_else(|| InterpError::Decode("static class vanished".to_owned()))?;
                entry
                    .statics
                    .lock()
                    .expect("static field lock poisoned")
                    .insert((name, descriptor), value);
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
                let value = shared.heap().object_fields(receiver)?[slot];
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
                shared.heap().object_fields_mut(receiver)?[slot] = value;
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
            Instr::InvokeInterface {
                class,
                name,
                descriptor,
            } => {
                let caller_pc = frame.pc - 1;
                let args = pop_arguments(frame, &descriptor)?;
                let receiver = pop_receiver(frame)?;
                let interface = resolve_from(shared, frame.class, &class)?;
                let result = invoke_interface(
                    shared,
                    thread,
                    frame,
                    guard,
                    receiver,
                    args,
                    interface,
                    &name,
                    &descriptor,
                );
                deliver_call(shared, frame, caller_pc, &descriptor, result)?;
            }
            Instr::IAdd
            | Instr::ISub
            | Instr::IMul
            | Instr::IDiv
            | Instr::IRem
            | Instr::INeg
            | Instr::IAnd
            | Instr::IOr
            | Instr::IXor
            | Instr::IShl
            | Instr::IShr
            | Instr::IUShr
            | Instr::LAdd
            | Instr::LSub
            | Instr::LMul
            | Instr::LDiv
            | Instr::LRem
            | Instr::LNeg
            | Instr::LAnd
            | Instr::LOr
            | Instr::LXor
            | Instr::LShl
            | Instr::LShr
            | Instr::LUShr
            | Instr::FAdd
            | Instr::FSub
            | Instr::FMul
            | Instr::FDiv
            | Instr::FRem
            | Instr::FNeg
            | Instr::DAdd
            | Instr::DSub
            | Instr::DMul
            | Instr::DDiv
            | Instr::DRem
            | Instr::DNeg
            | Instr::I2L
            | Instr::I2F
            | Instr::I2D
            | Instr::L2I
            | Instr::L2F
            | Instr::L2D
            | Instr::F2I
            | Instr::F2L
            | Instr::F2D
            | Instr::D2I
            | Instr::D2L
            | Instr::D2F
            | Instr::I2B
            | Instr::I2C
            | Instr::I2S => execute_numeric(frame, &instruction)?,
            Instr::LCmp => {
                let right = long(frame.pop()?)?;
                let left = long(frame.pop()?)?;
                frame.push(Value::Int(ordering(left.cmp(&right))))?;
            }
            Instr::FCmpL => compare_floats(frame, -1)?,
            Instr::FCmpG => compare_floats(frame, 1)?,
            Instr::DCmpL => compare_doubles(frame, -1)?,
            Instr::DCmpG => compare_doubles(frame, 1)?,
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
            Instr::InvokeDynamic {
                name,
                descriptor,
                bootstrap,
            } => {
                // M2.18 dispatches the `StringConcatFactory` bootstrap, M2.19 adds the
                // two `LambdaMetafactory` factory methods, and M3.6 adds the
                // `java.lang.runtime.ObjectMethods` record `hashCode` call site. The call
                // site's arguments are popped exactly as for any other call. A lambda's
                // captured values are stored in the new heap object (and enumerated by
                // `Shared::roots`).
                let args = pop_arguments(frame, &descriptor)?;
                let value = if bootstrap.handle.owner == lambda::LAMBDA_METAFACTORY {
                    Value::Ref(lambda::invoke_call_site(
                        shared,
                        frame.class,
                        &bootstrap,
                        &name,
                        &descriptor,
                        &args,
                    )?)
                } else if bootstrap.handle.owner == object_methods::OBJECT_METHODS {
                    // A record's `hashCode` computes an int in the one frame model; its
                    // `equals`/`toString` call sites are not synthesized.
                    object_methods::invoke_call_site(
                        shared,
                        thread,
                        frame.class,
                        &bootstrap,
                        &name,
                        &descriptor,
                        &args,
                    )?
                } else {
                    Value::Ref(rava_vm::string_concat::invoke_call_site(
                        shared,
                        &bootstrap,
                        &name,
                        &descriptor,
                        &args,
                    )?)
                };
                frame.push(value)?;
            }
            Instr::MonitorEnter => {
                let reference = pop_reference(frame)?;
                thread.monitor_enter(reference)?;
            }
            Instr::MonitorExit => {
                let reference = pop_reference(frame)?;
                thread.monitor_exit(reference)?;
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
            Instr::IReturn | Instr::LReturn | Instr::FReturn | Instr::DReturn | Instr::AReturn => {
                return Ok(Some(frame.pop()?));
            }
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

/// Execute one decoded `invokeinterface`.
///
/// A lambda receiver is answered by its metafactory target after the instruction's
/// interface and the recorded SAM name/descriptor are checked. Any other receiver is
/// resolved on its runtime class first, then on a default method in the instruction's
/// interface hierarchy, and the ordinary native-registry-before-`Code` path runs it.
#[allow(clippy::too_many_arguments)]
fn invoke_interface(
    shared: &Shared,
    thread: &ThreadState,
    frame: &Frame,
    guard: &FrameRootsGuard<'_>,
    receiver: ObjRef,
    args: Vec<Value>,
    interface: ClassId,
    name: &str,
    descriptor: &str,
) -> Result<Option<Value>, InterpError> {
    // A lambda is not an `Instance` and has no class file of its own: its heap
    // representation records the interface and the target, so it is handled before any
    // class resolution.
    let lambda = match shared.heap().get(receiver) {
        Ok(HeapObject::Lambda(lambda)) => Some(lambda.clone()),
        Ok(_) => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(lambda) = lambda {
        if lambda.sam_name == name && lambda.sam_descriptor == descriptor {
            if lambda.interface != interface {
                return Err(InterpError::NotAssignable {
                    expected: class_name(shared, interface),
                    found: class_name(shared, lambda.interface),
                });
            }
            // The receiver and the call arguments have been popped from the frame; publish
            // them as call roots while the target runs.
            let mut call_roots = args.clone();
            call_roots.push(Value::Ref(receiver));
            publish_roots(guard, frame, &call_roots);
            return lambda::invoke_sam(shared, thread, &lambda, args);
        }
        // A call that is not the SAM is still an ordinary interface call when the interface
        // declares a concrete method: `Comparator.reversed` on a `Comparator` lambda. A
        // declaration the lambda does not implement (an abstract method of its functional
        // interface) keeps the typed SAM-mismatch error.
        let concrete = match find_interface_method(shared, interface, name, descriptor)? {
            Some(declaring) => {
                let entry = shared
                    .entry(declaring)
                    .ok_or_else(|| InterpError::Decode("interface vanished".to_owned()))?;
                entry
                    .file
                    .find_method(name, descriptor)?
                    .map(|method| !method.is_abstract())
                    .unwrap_or(false)
            }
            None => false,
        };
        if !concrete {
            return Err(InterpError::Lambda(format!(
                "invokeinterface {name}{descriptor} does not match the lambda's SAM {}:{}",
                lambda.sam_name, lambda.sam_descriptor
            )));
        }
    }

    let start = dispatch_class(shared, frame.class, receiver)?;
    if !is_assignable(shared, start, interface)? {
        return Err(InterpError::NotAssignable {
            expected: class_name(shared, interface),
            found: class_name(shared, start),
        });
    }

    let declaring = match resolve_virtual(shared, start, name, descriptor) {
        Ok((entry, _)) => entry.id,
        Err(InterpError::MethodNotFound { .. }) => {
            // A class that inherits a default method has no declaration on its superclass
            // chain; the interface hierarchy supplies it.
            find_interface_method(shared, interface, name, descriptor)?.ok_or_else(|| {
                InterpError::MethodNotFound {
                    class: class_name(shared, start),
                    name: name.to_owned(),
                    descriptor: descriptor.to_owned(),
                }
            })?
        }
        Err(error) => return Err(error),
    };
    invoke_instance(
        shared, thread, frame, guard, declaring, receiver, args, name, descriptor,
    )
}

/// The interface (at or below `interface`) that declares `name:descriptor`, if any.
///
/// The search is a depth-first walk of the interface hierarchy; the first declaration in
/// that order wins. Only the interface's own method list is inspected, because an
/// interface's class file names `Object` as its superclass but `Object` members are not
/// inherited interface members.
pub(crate) fn find_interface_method(
    shared: &Shared,
    interface: ClassId,
    name: &str,
    descriptor: &str,
) -> Result<Option<ClassId>, InterpError> {
    let mut stack = vec![interface];
    let mut seen = HashSet::new();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let entry = shared
            .entry(current)
            .ok_or_else(|| InterpError::Decode("interface vanished".to_owned()))?;
        if entry.file.find_method(name, descriptor)?.is_some() {
            return Ok(Some(current));
        }
        for interface_name in entry.file.interface_names()? {
            match shared.load_class(entry.loader, interface_name) {
                Ok(id) => stack.push(id),
                Err(VmError::ClassNotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(None)
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
    let (entry, is_native) = match resolve_virtual(shared, start, name, descriptor) {
        Ok(found) => found,
        Err(InterpError::MethodNotFound { .. }) => {
            // A default method declared on a superinterface is part of method resolution
            // for a class and is the whole resolution for an interface receiver (the
            // `invokespecial AnnotationMetadata.getAnnotationAttributes` form Spring uses).
            let Some(interface) = find_interface_method(shared, start, name, descriptor)? else {
                return Err(InterpError::MethodNotFound {
                    class: class_name(shared, start),
                    name: name.to_owned(),
                    descriptor: descriptor.to_owned(),
                });
            };
            let entry = shared
                .entry(interface)
                .ok_or_else(|| InterpError::Decode("interface vanished".to_owned()))?;
            let method = entry.file.find_method(name, descriptor)?.ok_or_else(|| {
                InterpError::MethodNotFound {
                    class: entry.name.clone(),
                    name: name.to_owned(),
                    descriptor: descriptor.to_owned(),
                }
            })?;
            (Arc::clone(&entry), method.is_native())
        }
        Err(error) => return Err(error),
    };
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

/// Resolve a `getstatic`/`putstatic` owner to the class that declares the field.
///
/// Both opcodes share the field-resolution walk; a field that resolves to an instance
/// field is a typed error, so a static opcode never reads or writes instance state.
fn static_field(
    shared: &Shared,
    current: ClassId,
    owner: &str,
    name: &str,
    descriptor: &str,
) -> Result<ClassId, InterpError> {
    let (declaring, is_static) = resolve_field(shared, current, owner, name, descriptor)?;
    if !is_static {
        return Err(InterpError::InstanceFieldAccess {
            class: owner.to_owned(),
            name: name.to_owned(),
            descriptor: descriptor.to_owned(),
        });
    }
    Ok(declaring)
}

/// Whether a value's kind matches a field descriptor.
///
/// The descriptor's first byte selects the value representation: `int`-family primitives
/// are `Value::Int`, `long`/`float`/`double` keep their own variants, and an object or
/// array descriptor needs a reference handle. `void` matches nothing.
fn value_matches_descriptor(descriptor: &str, value: Value) -> bool {
    match descriptor.as_bytes().first() {
        Some(b'Z' | b'B' | b'C' | b'S' | b'I') => matches!(value, Value::Int(_)),
        Some(b'J') => matches!(value, Value::Long(_)),
        Some(b'F') => matches!(value, Value::Float(_)),
        Some(b'D') => matches!(value, Value::Double(_)),
        Some(b'L' | b'[') => matches!(value, Value::Ref(_)),
        _ => false,
    }
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
    let actual = shared.heap().object_class(receiver)?;
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

/// Is `from` the same class as `to`, a subclass of it, or a class that implements it?
///
/// The walk covers the superclass chain and every interface list on it, which is the
/// assignability M2.19's `invokeinterface` needs for a receiver whose method lives on an
/// interface. It is still a name-level walk of loaded classes: no verifier type is
/// consulted, and an array's component is not considered.
fn is_assignable(shared: &Shared, from: ClassId, to: ClassId) -> Result<bool, InterpError> {
    let mut stack = vec![from];
    let mut seen = HashSet::new();
    let mut omitted_superclass: Option<String> = None;
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        if current == to {
            return Ok(true);
        }
        let entry = shared
            .entry(current)
            .ok_or_else(|| InterpError::Decode("class vanished during assignability".to_owned()))?;
        for interface_name in entry.file.interface_names()? {
            // The C1 tree is the class library this VM ships; a transitive interface it
            // omits (for example `java.lang.constant.Constable` on `String`) cannot be
            // part of the walk.
            match shared.load_class(entry.loader, interface_name) {
                Ok(id) => stack.push(id),
                Err(VmError::ClassNotFound { .. }) => {}
                Err(error) => return Err(error.into()),
            }
        }
        // The tree is not closed under superclasses either: `java.util.LinkedList`
        // extends `java.util.AbstractSequentialList`, which is not a manifest owner and
        // therefore is not projected. A walk over a loaded class stops where the shipped
        // tree stops, so the class's own interface declarations still decide the answer.
        match shared.resolution_superclass(current) {
            Ok(Some(super_class)) => stack.push(super_class),
            Ok(None) => {}
            Err(VmError::ClassNotFound { name, .. }) => {
                omitted_superclass.get_or_insert(name);
            }
            Err(error) => return Err(error.into()),
        }
    }
    // Nothing found, but the walk could not see the whole hierarchy. A "no" would be a
    // guess, so report the omission instead: the caller can distinguish an unknown answer
    // from a real one.
    if let Some(name) = omitted_superclass {
        return Err(InterpError::Decode(format!(
            "assignability to {} is unknown: the C1 tree omits the superclass {name}",
            class_name(shared, to)
        )));
    }
    Ok(false)
}

fn class_name(shared: &Shared, class: ClassId) -> String {
    shared
        .entry(class)
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| format!("class {}", class.0))
}

/// Does `value` occupy two JVM operand-stack slots?
///
/// This interpreter stores one [`Value`] per argument and local (the M2.18 deviation), but
/// the `dup2`/`pop2` family is defined in terms of JVM categories, so the value's kind is
/// still what selects the form.
fn is_category_two(value: Value) -> bool {
    matches!(value, Value::Long(_) | Value::Double(_))
}

fn stack_dup_x1(frame: &mut Frame) -> Result<(), InterpError> {
    let stack = &mut frame.stack;
    let length = stack.len();
    if length < 2 {
        return Err(InterpError::StackUnderflow);
    }
    let value1 = stack[length - 1];
    let value2 = stack[length - 2];
    stack.truncate(length - 2);
    stack.push(value1);
    stack.push(value2);
    stack.push(value1);
    Ok(())
}

fn stack_dup_x2(frame: &mut Frame) -> Result<(), InterpError> {
    let stack = &mut frame.stack;
    let length = stack.len();
    if length < 2 {
        return Err(InterpError::StackUnderflow);
    }
    let value1 = stack[length - 1];
    let value2 = stack[length - 2];
    if is_category_two(value2) {
        stack.truncate(length - 2);
        stack.push(value1);
        stack.push(value2);
        stack.push(value1);
        return Ok(());
    }
    if length < 3 {
        return Err(InterpError::StackUnderflow);
    }
    let value3 = stack[length - 3];
    stack.truncate(length - 3);
    stack.push(value1);
    stack.push(value3);
    stack.push(value2);
    stack.push(value1);
    Ok(())
}

fn stack_dup2(frame: &mut Frame) -> Result<(), InterpError> {
    let stack = &mut frame.stack;
    let length = stack.len();
    if length < 1 {
        return Err(InterpError::StackUnderflow);
    }
    let value1 = stack[length - 1];
    if is_category_two(value1) {
        stack.push(value1);
        return Ok(());
    }
    if length < 2 {
        return Err(InterpError::StackUnderflow);
    }
    let value2 = stack[length - 2];
    stack.push(value2);
    stack.push(value1);
    Ok(())
}

fn stack_dup2_x1(frame: &mut Frame) -> Result<(), InterpError> {
    let stack = &mut frame.stack;
    let length = stack.len();
    if length < 2 {
        return Err(InterpError::StackUnderflow);
    }
    let value1 = stack[length - 1];
    let value2 = stack[length - 2];
    if is_category_two(value2) {
        stack.truncate(length - 2);
        stack.push(value1);
        stack.push(value2);
        stack.push(value1);
        return Ok(());
    }
    if length < 3 {
        return Err(InterpError::StackUnderflow);
    }
    let value3 = stack[length - 3];
    stack.truncate(length - 3);
    stack.push(value2);
    stack.push(value1);
    stack.push(value3);
    stack.push(value2);
    stack.push(value1);
    Ok(())
}

fn stack_dup2_x2(frame: &mut Frame) -> Result<(), InterpError> {
    let stack = &mut frame.stack;
    let length = stack.len();
    if length < 2 {
        return Err(InterpError::StackUnderflow);
    }
    let value1 = stack[length - 1];
    let value2 = stack[length - 2];
    if is_category_two(value1) {
        if is_category_two(value2) {
            // Form 4: value2 (cat2), value1 (cat2).
            stack.truncate(length - 2);
            stack.push(value1);
            stack.push(value2);
            stack.push(value1);
            return Ok(());
        }
        if length < 3 {
            return Err(InterpError::StackUnderflow);
        }
        // Form 3: value3, value2, value1 (cat2).
        let value3 = stack[length - 3];
        stack.truncate(length - 3);
        stack.push(value1);
        stack.push(value3);
        stack.push(value2);
        stack.push(value1);
        return Ok(());
    }
    if is_category_two(value2) {
        if length < 3 {
            return Err(InterpError::StackUnderflow);
        }
        // Form 2: value3, value2 (cat2), value1.
        let value3 = stack[length - 3];
        stack.truncate(length - 3);
        stack.push(value1);
        stack.push(value3);
        stack.push(value2);
        stack.push(value1);
        return Ok(());
    }
    if length < 4 {
        return Err(InterpError::StackUnderflow);
    }
    // Form 1: value4, value3, value2, value1.
    let value3 = stack[length - 3];
    let value4 = stack[length - 4];
    stack.truncate(length - 4);
    stack.push(value2);
    stack.push(value1);
    stack.push(value4);
    stack.push(value3);
    stack.push(value2);
    stack.push(value1);
    Ok(())
}

fn stack_swap(frame: &mut Frame) -> Result<(), InterpError> {
    let stack = &mut frame.stack;
    let length = stack.len();
    if length < 2 {
        return Err(InterpError::StackUnderflow);
    }
    if is_category_two(stack[length - 1]) || is_category_two(stack[length - 2]) {
        return Err(InterpError::Decode(
            "swap requires two category-1 values".to_owned(),
        ));
    }
    stack.swap(length - 1, length - 2);
    Ok(())
}

/// Execute one arithmetic, bitwise or conversion instruction.
fn execute_numeric(frame: &mut Frame, instruction: &Instr) -> Result<(), InterpError> {
    use Instr::*;
    match instruction {
        IAdd | ISub | IMul | IDiv | IRem | IAnd | IOr | IXor | IShl | IShr | IUShr => {
            let right = int(frame.pop()?)?;
            let left = int(frame.pop()?)?;
            let result = match instruction {
                IAdd => left.wrapping_add(right),
                ISub => left.wrapping_sub(right),
                IMul => left.wrapping_mul(right),
                IDiv => left
                    .checked_div(right)
                    .ok_or_else(|| InterpError::Decode("integer division by zero".to_owned()))?,
                IRem => left
                    .checked_rem(right)
                    .ok_or_else(|| InterpError::Decode("integer division by zero".to_owned()))?,
                IAnd => left & right,
                IOr => left | right,
                IXor => left ^ right,
                IShl => left.wrapping_shl((right as u32) & 0x1f),
                IShr => left.wrapping_shr((right as u32) & 0x1f),
                IUShr => ((left as u32) >> ((right as u32) & 0x1f)) as i32,
                _ => unreachable!(),
            };
            frame.push(Value::Int(result))?;
        }
        INeg => {
            let value = int(frame.pop()?)?;
            frame.push(Value::Int(value.wrapping_neg()))?;
        }
        LAdd | LSub | LMul | LDiv | LRem | LAnd | LOr | LXor | LShl | LShr | LUShr => {
            let right = long(frame.pop()?)?;
            let left = long(frame.pop()?)?;
            let result = match instruction {
                LAdd => left.wrapping_add(right),
                LSub => left.wrapping_sub(right),
                LMul => left.wrapping_mul(right),
                LDiv => left
                    .checked_div(right)
                    .ok_or_else(|| InterpError::Decode("long division by zero".to_owned()))?,
                LRem => left
                    .checked_rem(right)
                    .ok_or_else(|| InterpError::Decode("long division by zero".to_owned()))?,
                LAnd => left & right,
                LOr => left | right,
                LXor => left ^ right,
                LShl => left.wrapping_shl((right as u32) & 0x3f),
                LShr => left.wrapping_shr((right as u32) & 0x3f),
                LUShr => ((left as u64) >> ((right as u32) & 0x3f)) as i64,
                _ => unreachable!(),
            };
            frame.push(Value::Long(result))?;
        }
        LNeg => {
            let value = long(frame.pop()?)?;
            frame.push(Value::Long(value.wrapping_neg()))?;
        }
        FAdd | FSub | FMul | FDiv | FRem => {
            let right = float(frame.pop()?)?;
            let left = float(frame.pop()?)?;
            let result = match instruction {
                FAdd => left + right,
                FSub => left - right,
                FMul => left * right,
                FDiv => left / right,
                FRem => left % right,
                _ => unreachable!(),
            };
            frame.push(Value::Float(result))?;
        }
        FNeg => {
            let value = float(frame.pop()?)?;
            frame.push(Value::Float(-value))?;
        }
        DAdd | DSub | DMul | DDiv | DRem => {
            let right = double(frame.pop()?)?;
            let left = double(frame.pop()?)?;
            let result = match instruction {
                DAdd => left + right,
                DSub => left - right,
                DMul => left * right,
                DDiv => left / right,
                DRem => left % right,
                _ => unreachable!(),
            };
            frame.push(Value::Double(result))?;
        }
        DNeg => {
            let value = double(frame.pop()?)?;
            frame.push(Value::Double(-value))?;
        }
        I2L => {
            let value = int(frame.pop()?)?;
            frame.push(Value::Long(i64::from(value)))?;
        }
        I2F => {
            let value = int(frame.pop()?)?;
            frame.push(Value::Float(value as f32))?;
        }
        I2D => {
            let value = int(frame.pop()?)?;
            frame.push(Value::Double(f64::from(value)))?;
        }
        L2I => {
            let value = long(frame.pop()?)?;
            frame.push(Value::Int(value as i32))?;
        }
        L2F => {
            let value = long(frame.pop()?)?;
            frame.push(Value::Float(value as f32))?;
        }
        L2D => {
            let value = long(frame.pop()?)?;
            frame.push(Value::Double(value as f64))?;
        }
        F2I => {
            let value = float(frame.pop()?)?;
            frame.push(Value::Int(value as i32))?;
        }
        F2L => {
            let value = float(frame.pop()?)?;
            frame.push(Value::Long(value as i64))?;
        }
        F2D => {
            let value = float(frame.pop()?)?;
            frame.push(Value::Double(f64::from(value)))?;
        }
        D2I => {
            let value = double(frame.pop()?)?;
            frame.push(Value::Int(value as i32))?;
        }
        D2L => {
            let value = double(frame.pop()?)?;
            frame.push(Value::Long(value as i64))?;
        }
        D2F => {
            let value = double(frame.pop()?)?;
            frame.push(Value::Float(value as f32))?;
        }
        I2B => {
            let value = int(frame.pop()?)?;
            frame.push(Value::Int(i32::from(value as i8)))?;
        }
        I2C => {
            let value = int(frame.pop()?)?;
            frame.push(Value::Int(i32::from(value as u16)))?;
        }
        I2S => {
            let value = int(frame.pop()?)?;
            frame.push(Value::Int(i32::from(value as i16)))?;
        }
        _ => {
            return Err(InterpError::Decode(format!(
                "numeric dispatch does not handle {instruction:?}"
            )));
        }
    }
    Ok(())
}

fn int(value: Value) -> Result<i32, InterpError> {
    value.as_int().ok_or(InterpError::WrongValue)
}

fn long(value: Value) -> Result<i64, InterpError> {
    match value {
        Value::Long(value) => Ok(value),
        _ => Err(InterpError::WrongValue),
    }
}

fn float(value: Value) -> Result<f32, InterpError> {
    match value {
        Value::Float(value) => Ok(value),
        _ => Err(InterpError::WrongValue),
    }
}

fn double(value: Value) -> Result<f64, InterpError> {
    match value {
        Value::Double(value) => Ok(value),
        _ => Err(InterpError::WrongValue),
    }
}

/// `lcmp`'s result: -1, 0 or 1.
fn ordering(ordering: std::cmp::Ordering) -> i32 {
    match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// `fcmpl`/`fcmpg`: compare two floats, using `nan_result` when either is NaN.
///
/// The two opcodes differ only in the NaN result, which is what makes `fcmpg` the
/// "greater" form the compiler picks when a NaN must sort high.
fn compare_floats(frame: &mut Frame, nan_result: i32) -> Result<(), InterpError> {
    let right = float(frame.pop()?)?;
    let left = float(frame.pop()?)?;
    let result = if left.is_nan() || right.is_nan() {
        nan_result
    } else if left < right {
        -1
    } else if left > right {
        1
    } else {
        0
    };
    frame.push(Value::Int(result))
}

/// `dcmpl`/`dcmpg`: compare two doubles, using `nan_result` when either is NaN.
fn compare_doubles(frame: &mut Frame, nan_result: i32) -> Result<(), InterpError> {
    let right = double(frame.pop()?)?;
    let left = double(frame.pop()?)?;
    let result = if left.is_nan() || right.is_nan() {
        nan_result
    } else if left < right {
        -1
    } else if left > right {
        1
    } else {
        0
    };
    frame.push(Value::Int(result))
}

/// Pop a reference for an array instruction or a reference comparison.
///
/// A null handle is not rejected here: an array instruction reports it as
/// `HeapError::NullReference` and `if_acmpeq`/`if_acmpne` compare it like any other
/// handle, which is what the JVM does.
fn pop_reference(frame: &mut Frame) -> Result<ObjRef, InterpError> {
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

/// Resolve an `anewarray` or `multianewarray` component through the current class's
/// loader.
///
/// A component is a class name (`java/lang/String`), a primitive descriptor (`I`, from
/// the innermost `multianewarray` level) or an array descriptor (`[I`,
/// `[[Ljava/lang/String;`). Each becomes a [`ClassRef`], so the allocated array carries a
/// component class and `getClass()`/`getComponentType()` can name it.
fn resolve_class_ref(
    shared: &Shared,
    current: ClassId,
    component: &str,
) -> Result<ClassRef, InterpError> {
    if component.starts_with('[') || PrimitiveKind::from_descriptor(component).is_some() {
        let loader = resolve_loader(shared, current)?;
        return Ok(shared.class_ref_for_descriptor(loader, component)?);
    }
    Ok(ClassRef::Named(resolve_from(shared, current, component)?))
}

/// The primitive a `checkcast`/`instanceof` operand names, if it names one.
///
/// A real class file never uses a primitive as a cast target, but the operand is still
/// decoded as a name: accepting the Java name (`int`) as well as the one-character
/// descriptor (`I`) lets the VM reject the operand explicitly instead of treating it as
/// a missing class.
fn primitive_operand(name: &str) -> Option<PrimitiveKind> {
    PrimitiveKind::from_name(name).or_else(|| PrimitiveKind::from_descriptor(name))
}

/// Resolve a class constant or cast operand through `current`'s loader.
///
/// The three shapes the `Class` model already has are the same for `ldc`, `checkcast`
/// and `instanceof`: an array descriptor (`[I`, `[[I`, `[Ljava/lang/String;`) becomes a
/// [`ClassRef::Array`] resolved through `current`'s loader, a primitive name or
/// descriptor becomes [`ClassRef::Primitive`], and every other name is a loaded class.
fn resolve_class_operand(
    shared: &Shared,
    current: ClassId,
    name: &str,
) -> Result<ClassRef, InterpError> {
    if name.starts_with('[') {
        let loader = resolve_loader(shared, current)?;
        return Ok(shared.class_ref_for_descriptor(loader, name)?);
    }
    if let Some(kind) = primitive_operand(name) {
        return Ok(ClassRef::Primitive(kind));
    }
    Ok(ClassRef::Named(resolve_from(shared, current, name)?))
}

/// Resolve one `ldc`/`ldc_w` class constant through `current`'s loader.
///
/// The constant names one of the three shapes the `Class` model already has: an array
/// descriptor (`[I`, `[[I`, `[Ljava/lang/String;`) becomes the cached array class object
/// whose components are resolved through `current`'s loader; a primitive name (`int`,
/// `void`) becomes the cached primitive class object; every other name is a loaded class
/// resolved through `current`'s loader. A loaded class contributes its defining loader's
/// identity, so a child loader's class stays distinct from a bootstrap class of the same
/// name. The class is loaded but not initialized, exactly as JVMS §5.4.3 requires.
fn class_literal(shared: &Shared, current: ClassId, name: &str) -> Result<ObjRef, InterpError> {
    let class_ref = resolve_class_operand(shared, current, name)?;
    Ok(shared.class_object_for_ref(&class_ref)?)
}

/// The runtime type of a heap object, for `checkcast` and `instanceof`.
///
/// The representation decides the type: an instance reports its defining class, a
/// string `java.lang.String`, a class object `java.lang.Class`, a box its boxed class, a
/// container its VM class, an annotation or enum its declared class, a lambda its
/// functional interface and an array its array class (the component the allocation
/// named). Each VM-level class is resolved through the bootstrap library, which is the
/// identity [`Shared::get_class`] reports for the same object.
fn runtime_class_ref(shared: &Shared, reference: ObjRef) -> Result<ClassRef, InterpError> {
    enum RuntimeKind {
        Named(ClassId),
        Library(&'static str),
        Class,
        Array(ClassRef),
        Type(&'static str),
    }

    let kind = {
        let heap = shared.heap();
        match heap.get(reference)? {
            HeapObject::Instance(instance) => RuntimeKind::Named(instance.class),
            HeapObject::Str(_) => RuntimeKind::Library("java/lang/String"),
            HeapObject::Class(_) | HeapObject::PrimitiveClass(_) | HeapObject::ArrayClass(_) => {
                RuntimeKind::Class
            }
            HeapObject::Reflect(member) => RuntimeKind::Library(member.kind.class_name()),
            HeapObject::Boxed(boxed) => RuntimeKind::Library(boxed.class_name()),
            HeapObject::Annotation(annotation) => RuntimeKind::Named(annotation.class),
            HeapObject::Enum(constant) => RuntimeKind::Named(constant.class),
            HeapObject::StringBuilder(_) => {
                RuntimeKind::Library(rava_vm::collections::STRING_BUILDER_CLASS)
            }
            HeapObject::StringBuffer(_) => {
                RuntimeKind::Library(rava_vm::format::STRING_BUFFER_CLASS)
            }
            HeapObject::MessageFormat(_) => {
                RuntimeKind::Library(rava_vm::format::MESSAGE_FORMAT_CLASS)
            }
            HeapObject::List(list) => RuntimeKind::Named(list.class),
            HeapObject::Set(set) => RuntimeKind::Named(set.class),
            HeapObject::Map(map) => RuntimeKind::Named(map.class),
            HeapObject::Iterator(iterator) => RuntimeKind::Named(iterator.class),
            HeapObject::MapEntry(_) => RuntimeKind::Library(rava_vm::maps::MAP_ENTRY_CLASS),
            HeapObject::LogLevel(_) => RuntimeKind::Library("java/util/logging/Level"),
            HeapObject::Atomic(cell) => RuntimeKind::Library(cell.class_name()),
            HeapObject::Lock(_) => RuntimeKind::Library(rava_vm::locks::REENTRANT_LOCK_CLASS),
            HeapObject::FutureTask(_) => RuntimeKind::Library(rava_vm::futures::FUTURE_TASK_CLASS),
            HeapObject::BitSet(_) => RuntimeKind::Library(rava_vm::bitset::BIT_SET_CLASS),
            HeapObject::Condition(_) => RuntimeKind::Library(rava_vm::locks::CONDITION_CLASS),
            HeapObject::Pattern(_) => RuntimeKind::Library("java/util/regex/Pattern"),
            HeapObject::Matcher(_) => RuntimeKind::Library("java/util/regex/Matcher"),
            HeapObject::Enumeration(_) => RuntimeKind::Library("java/util/Enumeration"),
            HeapObject::Stream(_) => RuntimeKind::Library("java/util/stream/Stream"),
            HeapObject::Spliterator(_) => RuntimeKind::Library("java/util/Spliterator"),
            HeapObject::Collector(_) => RuntimeKind::Library("java/util/stream/Collector"),
            HeapObject::Optional(_) => RuntimeKind::Library("java/util/Optional"),
            HeapObject::Sink(_) => RuntimeKind::Library("java/util/function/Consumer"),
            HeapObject::Array(array) => {
                RuntimeKind::Array(ClassRef::array(array.component.clone()))
            }
            HeapObject::Lambda(lambda) => RuntimeKind::Named(lambda.interface),
            HeapObject::Type(ty) => RuntimeKind::Type(ty.class_name()),
            HeapObject::Lookup(_) => RuntimeKind::Library("java/lang/invoke/MethodHandles$Lookup"),
            HeapObject::MethodType(_) => RuntimeKind::Library("java/lang/invoke/MethodType"),
            HeapObject::MethodHandle(_) => RuntimeKind::Library("java/lang/invoke/MethodHandle"),
        }
    };

    Ok(match kind {
        RuntimeKind::Named(class) => ClassRef::Named(class),
        RuntimeKind::Class => ClassRef::Named(shared.load_library_class("java/lang/Class")?),
        RuntimeKind::Library(name) => ClassRef::Named(shared.load_library_class(name)?),
        RuntimeKind::Array(class_ref) => class_ref,
        RuntimeKind::Type(name) => ClassRef::Named(shared.load_library_class(name)?),
    })
}

/// The `Class.getName()`-style label of a class ref, for a typed cast error.
fn class_ref_label(shared: &Shared, class_ref: &ClassRef) -> Result<String, InterpError> {
    Ok(shared.class_ref_name(class_ref)?)
}

/// Is `from` assignable to `to` under the JVM's reference-type rules?
///
/// Named types use the superclass-and-interface walk of [`is_assignable`]. An array is
/// assignable to `Object`, `Cloneable` and `Serializable` and, covariantly, to another
/// array whose component type it is assignable to; a primitive component only matches
/// the same primitive. A primitive is never a reference type, so neither side of a cast
/// involving one can match.
fn class_ref_assignable(
    shared: &Shared,
    from: &ClassRef,
    to: &ClassRef,
) -> Result<bool, InterpError> {
    if matches!(from, ClassRef::Primitive(_)) || matches!(to, ClassRef::Primitive(_)) {
        return Ok(false);
    }
    match (from, to) {
        (ClassRef::Named(from), ClassRef::Named(to)) => is_assignable(shared, *from, *to),
        (ClassRef::Named(_), ClassRef::Array(_)) => Ok(false),
        (ClassRef::Array(_), ClassRef::Named(to)) => array_assignable_to(shared, *to),
        (ClassRef::Array(from), ClassRef::Array(to)) => {
            component_assignable(shared, &from.component, &to.component)
        }
        _ => Ok(false),
    }
}

/// Is one array component type assignable to another for array covariance?
///
/// Two primitive components must be the same primitive; every other pair is a reference
/// assignability question, including a nested array component (which is assignable to
/// `Object` through [`class_ref_assignable`]).
fn component_assignable(
    shared: &Shared,
    from: &ClassRef,
    to: &ClassRef,
) -> Result<bool, InterpError> {
    match (from, to) {
        (ClassRef::Primitive(from), ClassRef::Primitive(to)) => Ok(from == to),
        _ => class_ref_assignable(shared, from, to),
    }
}

/// Is an array assignable to the named class `target`?
///
/// JLS §4.10.3 gives every array type exactly three supertypes: `Object`, `Cloneable`
/// and `Serializable`. The comparison is by the loaded class's binary name, which is the
/// same name-level identity the rest of this VM's assignability walk uses.
fn array_assignable_to(shared: &Shared, target: ClassId) -> Result<bool, InterpError> {
    let entry = shared
        .entry(target)
        .ok_or_else(|| InterpError::Decode("cast target vanished".to_owned()))?;
    Ok(matches!(
        entry.name.as_str(),
        "java/lang/Object" | "java/lang/Cloneable" | "java/io/Serializable"
    ))
}

fn resolve_loader(shared: &Shared, current: ClassId) -> Result<rava_vm::LoaderId, InterpError> {
    Ok(shared
        .entry(current)
        .ok_or_else(|| InterpError::Decode("current class vanished".to_owned()))?
        .loader)
}

/// Allocate a `multianewarray` array, one nesting level per count.
///
/// `descriptor` is the full array descriptor from the instruction and `counts` has one
/// length per allocated dimension, outermost first. When fewer dimensions are allocated
/// than the descriptor has, the innermost allocated level is a reference array whose
/// elements stay null, which is what `new int[2][]` produces.
fn allocate_multidim(
    shared: &Shared,
    current: ClassId,
    descriptor: &str,
    counts: &[i32],
) -> Result<ObjRef, InterpError> {
    let component = descriptor.strip_prefix('[').ok_or_else(|| {
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
    let component = resolve_class_ref(shared, current, component)?;
    let reference = shared.heap().allocate_array(component, length);
    if !rest.is_empty() {
        let element = descriptor.strip_prefix('[').ok_or_else(|| {
            InterpError::Decode(format!(
                "multianewarray descriptor {descriptor} is not an array"
            ))
        })?;
        for position in 0..length {
            let child = allocate_multidim(shared, current, element, rest)?;
            shared.heap().array_mut(reference)?.elements[position] = Value::Ref(child);
        }
    }
    Ok(reference)
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
/// An instance dispatches from its defining class. A string, a class object (a loaded
/// class, a primitive or an array class), a box, a `StringBuilder`, an `ArrayList` or a
/// `HashMap` has no `ClassId` of its own in the heap, so it dispatches from
/// `java/lang/String`, `java/lang/Class`, its boxed class (`java/lang/Integer`),
/// `java/lang/StringBuilder`, `java/util/ArrayList` or `java/util/HashMap`, resolved
/// through the caller's loader. An array instance dispatches from `java/lang/Object` (its
/// `getClass` native reports the array class); any other method fails resolution
/// normally.
pub(crate) fn dispatch_class(
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
        Named(ClassId),
        Array,
        Lambda(ClassId),
        Type(&'static str),
    }

    let kind = {
        let heap = shared.heap();
        match heap.get(reference).map_err(|_| InterpError::WrongValue)? {
            HeapObject::Instance(instance) => ReceiverKind::Instance(instance.class),
            HeapObject::Str(_) => ReceiverKind::Str,
            HeapObject::Class(_) => ReceiverKind::Class,
            HeapObject::PrimitiveClass(_) => ReceiverKind::Class,
            HeapObject::ArrayClass(_) => ReceiverKind::Class,
            HeapObject::Reflect(member) => ReceiverKind::Reflect(member.kind),
            HeapObject::Boxed(boxed) => ReceiverKind::Boxed(*boxed),
            HeapObject::Annotation(annotation) => ReceiverKind::Annotation(annotation.class),
            HeapObject::Enum(constant) => ReceiverKind::Enum(constant.class),
            HeapObject::StringBuilder(_) => ReceiverKind::StringBuilder,
            HeapObject::StringBuffer(_) => ReceiverKind::Named(
                shared.load_library_class(rava_vm::format::STRING_BUFFER_CLASS)?,
            ),
            HeapObject::MessageFormat(_) => ReceiverKind::Named(
                shared.load_library_class(rava_vm::format::MESSAGE_FORMAT_CLASS)?,
            ),
            HeapObject::List(list) => ReceiverKind::Named(list.class),
            HeapObject::Set(set) => ReceiverKind::Named(set.class),
            HeapObject::Map(map) => ReceiverKind::Named(map.class),
            HeapObject::Iterator(iterator) => ReceiverKind::Named(iterator.class),
            HeapObject::MapEntry(_) => {
                ReceiverKind::Named(shared.load_library_class(rava_vm::maps::MAP_ENTRY_CLASS)?)
            }
            HeapObject::LogLevel(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/logging/Level")?)
            }
            HeapObject::Atomic(cell) => {
                ReceiverKind::Named(shared.load_library_class(cell.class_name())?)
            }
            HeapObject::Lock(_) => ReceiverKind::Named(
                shared.load_library_class(rava_vm::locks::REENTRANT_LOCK_CLASS)?,
            ),
            HeapObject::FutureTask(_) => {
                ReceiverKind::Named(shared.load_library_class(rava_vm::futures::FUTURE_TASK_CLASS)?)
            }
            HeapObject::BitSet(_) => {
                ReceiverKind::Named(shared.load_library_class(rava_vm::bitset::BIT_SET_CLASS)?)
            }
            HeapObject::Condition(_) => {
                ReceiverKind::Named(shared.load_library_class(rava_vm::locks::CONDITION_CLASS)?)
            }
            HeapObject::Pattern(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/regex/Pattern")?)
            }
            HeapObject::Matcher(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/regex/Matcher")?)
            }
            HeapObject::Enumeration(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/Enumeration")?)
            }
            HeapObject::Stream(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/stream/Stream")?)
            }
            HeapObject::Spliterator(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/Spliterator")?)
            }
            HeapObject::Collector(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/stream/Collector")?)
            }
            HeapObject::Optional(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/Optional")?)
            }
            HeapObject::Sink(_) => {
                ReceiverKind::Named(shared.load_library_class("java/util/function/Consumer")?)
            }
            HeapObject::Array(_) => ReceiverKind::Array,
            HeapObject::Lambda(lambda) => ReceiverKind::Lambda(lambda.interface),
            HeapObject::Type(ty) => ReceiverKind::Type(ty.class_name()),
            HeapObject::Lookup(_) => ReceiverKind::Named(
                shared.load_library_class("java/lang/invoke/MethodHandles$Lookup")?,
            ),
            HeapObject::MethodType(_) => {
                ReceiverKind::Named(shared.load_library_class("java/lang/invoke/MethodType")?)
            }
            HeapObject::MethodHandle(_) => {
                ReceiverKind::Named(shared.load_library_class("java/lang/invoke/MethodHandle")?)
            }
        }
    };

    match kind {
        ReceiverKind::Instance(class) => Ok(class),
        ReceiverKind::Named(class) => Ok(class),
        ReceiverKind::Lambda(interface) => Ok(interface),
        ReceiverKind::Str => resolve_from(shared, current, "java/lang/String"),
        ReceiverKind::Class => resolve_from(shared, current, "java/lang/Class"),
        ReceiverKind::Reflect(kind) => resolve_from(shared, current, kind.class_name()),
        ReceiverKind::Boxed(boxed) => resolve_from(shared, current, boxed.class_name()),
        ReceiverKind::Annotation(class) | ReceiverKind::Enum(class) => Ok(class),
        ReceiverKind::StringBuilder => {
            resolve_from(shared, current, rava_vm::collections::STRING_BUILDER_CLASS)
        }
        ReceiverKind::Array => resolve_from(shared, current, "java/lang/Object"),
        ReceiverKind::Type(name) => resolve_from(shared, current, name),
    }
}

/// Resolve a virtual method from `start` up the superclass chain, the way JVMS §5.4.5
/// resolves an instance method. Returns the declaring class and whether the declaration
/// is `ACC_NATIVE`; the caller consults the native registry before the flag, because the
/// C1 stubs declare no `Code` and do not set `ACC_NATIVE`.
pub(crate) fn resolve_virtual(
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
        // The C1 tree is not closed under superclasses: `LinkedList` extends
        // `AbstractSequentialList`, which is not a manifest owner. A walk that reaches
        // such a class ends here with `MethodNotFound`, so an `invokeinterface` can still
        // fall back to the interface's own declaration (for example `List.iterator`).
        match shared.resolution_superclass(current) {
            Ok(Some(super_class)) => current = super_class,
            Ok(None) => {
                return Err(InterpError::MethodNotFound {
                    class: entry.name.clone(),
                    name: name.to_owned(),
                    descriptor: descriptor.to_owned(),
                });
            }
            Err(VmError::ClassNotFound { .. }) => {
                return Err(InterpError::MethodNotFound {
                    class: entry.name.clone(),
                    name: name.to_owned(),
                    descriptor: descriptor.to_owned(),
                });
            }
            Err(error) => return Err(error.into()),
        }
    }
}

pub(crate) fn ensure_initialized(
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

pub(crate) fn argument_count(descriptor: &str) -> Result<usize, InterpError> {
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
            'L' => {
                // Skip the internal name of an object descriptor; one value.
                for next in chars.by_ref() {
                    if next == ';' {
                        break;
                    }
                }
                count += 1;
            }
            '[' => {
                // An array descriptor is any number of `[` followed by an element
                // descriptor. The whole thing is one value, but every character belongs
                // to the same parameter, so the element descriptor must be skipped as
                // well; otherwise `[Ljava/lang/Class;` would be counted twice.
                while chars.as_str().starts_with('[') {
                    chars.next();
                }
                match chars.next() {
                    Some('L') => {
                        for next in chars.by_ref() {
                            if next == ';' {
                                break;
                            }
                        }
                    }
                    Some('Z' | 'B' | 'C' | 'S' | 'I' | 'J' | 'F' | 'D') => {}
                    _ => {
                        return Err(InterpError::BadDescriptor(format!(
                            "{descriptor} has a malformed array parameter"
                        )));
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
