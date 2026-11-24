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

use rava_vm::{ClassId, Shared, ThreadState, Value};

/// Run a static method with `args` and return its result (`None` for `void`).
pub fn run_static_method(
    shared: &Shared,
    thread: &ThreadState,
    class: ClassId,
    name: &str,
    descriptor: &str,
    args: Vec<Value>,
) -> Result<Option<Value>, InterpError> {
    let entry = shared
        .entry(class)
        .ok_or_else(|| InterpError::Decode(format!("class {} is not loaded", class.0)))?;
    let file = entry.file.clone();
    let method =
        file.find_method(name, descriptor)?
            .ok_or_else(|| InterpError::MethodNotFound {
                class: entry.name.clone(),
                name: name.to_owned(),
                descriptor: descriptor.to_owned(),
            })?;
    if method.is_native() || method.is_abstract() {
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
    execute(shared, thread, &mut frame)
}

fn execute(
    shared: &Shared,
    thread: &ThreadState,
    frame: &mut Frame,
) -> Result<Option<Value>, InterpError> {
    loop {
        // The first safepoint poll point in the runtime. There is no stop-the-world
        // yet; the state machine is in place.
        let _ = thread.poll_safepoint();
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
                let target = shared.load_class(
                    // Static calls resolve from the current class's loader.
                    shared
                        .entry(frame.class)
                        .ok_or_else(|| InterpError::Decode("current class vanished".to_owned()))?
                        .loader,
                    &class,
                )?;
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
