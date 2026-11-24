//! Bytecode decoding, frames and the interpreter.
//!
//! M0.5a implements the integer subset and `invokestatic`; the decoded form and the
//! frame are the contracts a later JIT consumes.

#![forbid(unsafe_code)]

mod error;
mod frame;
mod instr;

pub use error::InterpError;
pub use frame::Frame;
pub use instr::{DecodedMethod, Instr, decode};

use rava_vm::{ClassId, ClassState, FrameRootsGuard, HeapObject, Shared, ThreadState, Value};

/// Run a static method with `args` and return its result (`None` for `void`).
pub fn run_static_method(
    shared: &Shared,
    thread: &ThreadState,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    shared.check_no_locks_held()?;
    let entry = shared
        .entry(class)
        .ok_or_else(|| InterpError::Decode(format!("class {} is not loaded", class.0)))?;
    if name != "<clinit>" {
        ensure_initialized(shared, thread, class)?;
    }
    let file = entry.file.clone();
    let method =
        file.find_method(name, descriptor)?
            .ok_or_else(|| InterpError::MethodNotFound {
                class: entry.name.clone(),
                name: name.to_owned(),
                descriptor: descriptor.to_owned(),
            })?;
    if method.is_native() {
        let native = shared
            .native(&entry.name, name, descriptor)
            .ok_or_else(|| {
                InterpError::Decode(format!(
                    "no native implementation for {}.{name}{descriptor}",
                    entry.name
                ))
            })?;
        return native(shared, thread, entry.loader, &args).map_err(InterpError::Vm);
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

    let mut locals = vec![Value::Int(0); usize::from(decoded.max_locals)];
    if args.len() > locals.len() {
        return Err(InterpError::Decode(format!(
            "{} args do not fit in {} locals",
            args.len(),
            locals.len()
        )));
    }
    locals[..args.len()].copy_from_slice(&args);
    let mut frame = Frame::new(class, locals, decoded.instructions, decoded.max_stack);
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
            Instr::ILoad(index) => {
                let value = *frame.locals.get(usize::from(index)).ok_or_else(|| {
                    InterpError::Decode(format!("local {index} is outside the frame"))
                })?;
                frame.push(value)?;
            }
            Instr::IStore(index) => {
                let value = frame.pop()?;
                let slot = frame.locals.get_mut(usize::from(index)).ok_or_else(|| {
                    InterpError::Decode(format!("local {index} is outside the frame"))
                })?;
                *slot = value;
            }
            Instr::LdcString(text) => {
                let reference = shared.heap().allocate_string(text);
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
            Instr::InvokeVirtual {
                class: _,
                name,
                descriptor,
            } => {
                let count = argument_count(&descriptor)?;
                let mut args = Vec::with_capacity(count);
                for _ in 0..count {
                    args.push(frame.pop()?);
                }
                args.reverse();
                let receiver = frame.pop()?;
                let Value::Ref(reference) = receiver else {
                    return Err(InterpError::WrongValue);
                };
                let receiver_class = {
                    let heap = shared.heap();
                    match heap.get(reference).map_err(|_| InterpError::WrongValue)? {
                        HeapObject::Instance(instance) => instance.class,
                        _ => return Err(InterpError::WrongValue),
                    }
                };
                let entry = shared
                    .entry(receiver_class)
                    .ok_or_else(|| InterpError::Decode("receiver class vanished".to_owned()))?;
                let method = entry.file.find_method(&name, &descriptor)?.ok_or_else(|| {
                    InterpError::MethodNotFound {
                        class: entry.name.clone(),
                        name: name.clone(),
                        descriptor: descriptor.clone(),
                    }
                })?;
                if !method.is_native() {
                    return Err(InterpError::Decode(format!(
                        "non-native instance call {}.{name}{descriptor} is not implemented",
                        entry.name
                    )));
                }
                let native = shared
                    .native(&entry.name, &name, &descriptor)
                    .ok_or_else(|| {
                        InterpError::Decode(format!(
                            "no native implementation for {}.{name}{descriptor}",
                            entry.name
                        ))
                    })?;
                shared.check_no_locks_held()?;
                // The receiver and arguments have been popped from the frame; publish
                // them with the frame's roots so the native call cannot lose them.
                let mut call_roots = args.clone();
                call_roots.push(Value::Ref(reference));
                publish_roots(guard, frame, &call_roots);
                let result =
                    native(shared, thread, entry.loader, &args).map_err(InterpError::Vm)?;
                if !descriptor.ends_with(")V") {
                    frame.push(
                        result.ok_or_else(|| {
                            InterpError::Decode("missing return value".to_owned())
                        })?,
                    )?;
                }
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
                let result = run_static_method(shared, thread, target, &name, &descriptor, args)?;
                if !descriptor.ends_with(")V") {
                    let value = result
                        .ok_or_else(|| InterpError::Decode("missing return value".to_owned()))?;
                    frame.push(value)?;
                }
            }
            Instr::IReturn => return Ok(Some(frame.pop()?)),
            Instr::Return => return Ok(None),
        }
    }
}

fn int(value: Value) -> Result<i32, InterpError> {
    value.as_int().ok_or(InterpError::WrongValue)
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
