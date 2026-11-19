//! The bytecode dispatch loop.
//!
//! Two invariants govern this crate and are asserted by `scripts/check-invariants.sh`:
//!
//! - It depends on `rava-vm` only, never on `rava-classfile` (ADR-0002): the loop must
//!   not be able to inspect class-file structures or version data (R10).
//! - It matches on the decoded [`rava_vm::Instr`] form, never on raw bytes (ADR-0003).
//!
//! The loop is recursive: a Java call is a Rust call. That keeps the first version small
//! at the cost of using the native stack for deep recursion, which is fine until a
//! program overflows it — a real VM would keep an explicit frame stack.

use std::sync::Arc;

use rava_vm::native::NativeCtx;
use rava_vm::resolve;
use rava_vm::{
    Array, ArrayKind, BinOp, ClassId, ClassPath, Compare, Convert, Frame, HeapObject,
    Initialization, Instr, ObjRef, Shared, ThreadState, UnOp, Value, VmError,
};

/// What one instruction did.
enum Step {
    Continue,
    Return(Value),
}

/// Loads `class_name`, runs its `main([Ljava/lang/String;)V`, and leaves its output in
/// [`Shared::out`].
pub fn run_main(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    class_name: &str,
    program_args: &[String],
) -> Result<(), VmError> {
    // Natives reach Java through this function pointer (ADR-0002's `JavaCall` seam).
    shared.invoker = Some(invoke_from_native);

    let class = shared.load_class(path, class_name)?;
    ensure_initialized(shared, thread, path, class)?;

    let missing = || VmError::MethodNotFound {
        class: class_name.to_owned(),
        name: "main".to_owned(),
        descriptor: "([Ljava/lang/String;)V".to_owned(),
    };
    let (owner, index) = shared
        .resolve_method(class, "main", "([Ljava/lang/String;)V")
        .ok_or_else(missing)?;

    // `main`'s parameter is never null (JLS §10.9): an empty array is what Java passes when the
    // program has no arguments.
    let args = shared.string_array(program_args);
    invoke(shared, thread, path, owner, index, vec![Value::Ref(args)])?;
    Ok(())
}

/// Runs `<clinit>` once, on first active use (JVMS §5.5, single-threaded form).
fn ensure_initialized(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    class: ClassId,
) -> Result<(), VmError> {
    match shared.begin_initialization(class) {
        // `Initializing` means a `<clinit>` is already running on this thread: skip it.
        Initialization::AlreadyDone => Ok(()),
        Initialization::Erroneous => {
            let name = shared.class(class).name.clone();
            Err(new_exception(
                shared,
                thread,
                path,
                "java/lang/NoClassDefFoundError",
                Some(&format!("Could not initialize class {name}")),
            ))
        }
        Initialization::Run(index) => {
            match invoke(shared, thread, path, class, index, Vec::new()) {
                Ok(_) => {
                    shared.finish_initialization(class);
                    Ok(())
                }
                Err(error) => {
                    // JVMS §5.5: a class whose `<clinit>` threw is erroneous from then on.
                    shared.fail_initialization(class);
                    Err(error)
                }
            }
        }
    }
}

fn invoke(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    class: ClassId,
    method_index: usize,
    args: Vec<Value>,
) -> Result<Value, VmError> {
    shared.classes[class.index()].invocations[method_index] += 1;
    // One refcount bump, not a `format!` and two `String` clones: the ceiling experiment (S37)
    // found the diagnostic work was most of the per-call cost.
    let (is_native, access_flags, code, qualified) = {
        let method = &shared.class(class).methods[method_index];
        (
            method.is_native,
            method.access_flags,
            method.code.clone(),
            Arc::clone(&method.qualified),
        )
    };

    if is_native {
        // The implementation was resolved at load time, so a call allocates nothing. A native the
        // registry does not know is reported by name, which is the only path that needs strings.
        if let Some(native) = shared.class(class).methods[method_index].native {
            let outcome = {
                let mut ctx = NativeCtx {
                    shared,
                    thread,
                    path,
                    args: &args,
                };
                native(&mut ctx)
            };
            return match outcome {
                Err(VmError::JavaException { class, message }) => Err(new_exception(
                    shared,
                    thread,
                    path,
                    &class,
                    message.as_deref(),
                )),
                other => other,
            };
        }
        let (name, descriptor) = {
            let method = &shared.class(class).methods[method_index];
            (method.name.clone(), method.descriptor.clone())
        };
        let owner = shared.class(class).name.clone();
        return match call_native(shared, thread, path, &owner, &name, &descriptor, &args) {
            Err(VmError::JavaException { class, message }) => Err(new_exception(
                shared,
                thread,
                path,
                &class,
                message.as_deref(),
            )),
            other => other,
        };
    }

    let code = code
        .ok_or_else(|| VmError::BadConstantPool(format!("{} has no body", qualified.as_ref())))?;
    let mut frame = thread.take_frame(class, method_index, code.max_locals);
    for (slot, value) in args.iter().enumerate() {
        if slot < frame.locals.len() {
            frame.locals[slot] = *value;
        }
    }

    // `synchronized` on the method holds the monitor for the whole body (JVMS §2.11.10).
    // A static synchronized method would lock its Class object, which this VM does not have.
    let monitor = if access_flags & rava_vm::class::ACC_SYNCHRONIZED != 0 {
        match args.first().copied().and_then(Value::as_ref) {
            Some(reference) if !reference.is_null() => {
                shared.monitor_enter(reference, thread.id)?;
                Some(reference)
            }
            _ => {
                return Err(VmError::Unsupported {
                    feature: "synchronized static method",
                    detail: format!("{qualified} would lock its Class object"),
                });
            }
        }
    } else {
        None
    };

    // Publish the frame so a collection can see its locals and operand stack as roots.
    thread.frames.push(frame);
    let frame_index = thread.frames.len() - 1;
    let result = run_frame(shared, thread, path, frame_index, class, &code, &qualified);
    if let Some(frame) = thread.frames.pop() {
        thread.recycle_frame(frame);
    }
    if let Some(reference) = monitor {
        shared.monitor_exit(reference, thread.id);
    }
    result
}

fn run_frame(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    frame_index: usize,
    class: ClassId,
    code: &rava_vm::Code,
    qualified: &str,
) -> Result<Value, VmError> {
    loop {
        let index = thread.frames[frame_index].pc;
        shared.steps += 1;
        let instruction = *code
            .instrs
            .get(index)
            .ok_or_else(|| VmError::TruncatedCode {
                pc: code.pc(index),
                method: qualified.to_owned(),
            })?;
        thread.frames[frame_index].pc += 1;

        let step = (|| -> Result<Step, VmError> {
            match instruction {
                Instr::Nop => {}
                Instr::AconstNull => thread.frames[frame_index].stack.push(Value::NULL),
                Instr::Iconst(value) => thread.frames[frame_index].stack.push(Value::Int(value)),
                Instr::Lconst(value) => thread.frames[frame_index].stack.push(Value::Long(value)),
                Instr::Fconst(value) => thread.frames[frame_index].stack.push(Value::Float(value)),
                Instr::Dconst(value) => thread.frames[frame_index].stack.push(Value::Double(value)),
                Instr::Bipush(value) => thread.frames[frame_index]
                    .stack
                    .push(Value::Int(i32::from(value))),
                Instr::Sipush(value) => thread.frames[frame_index]
                    .stack
                    .push(Value::Int(i32::from(value))),

                Instr::Ldc(index) => {
                    let value = shared.resolve_constant(path, class, index)?;
                    thread.frames[frame_index].stack.push(value);
                }
                Instr::Ldc2(index) => {
                    let value = shared.resolve_constant(path, class, index)?;
                    thread.frames[frame_index].stack.push(value);
                }

                Instr::Iload(slot)
                | Instr::Lload(slot)
                | Instr::Fload(slot)
                | Instr::Dload(slot)
                | Instr::Aload(slot) => {
                    let value = *thread.frames[frame_index]
                        .locals
                        .get(usize::from(slot))
                        .ok_or_else(|| VmError::StackUnderflow {
                            method: qualified.to_owned(),
                            pc: code.pc(index),
                            needed: usize::from(slot) + 1,
                        })?;
                    thread.frames[frame_index].stack.push(value);
                }
                Instr::Istore(slot)
                | Instr::Lstore(slot)
                | Instr::Fstore(slot)
                | Instr::Dstore(slot)
                | Instr::Astore(slot) => {
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    let target = thread.frames[frame_index]
                        .locals
                        .get_mut(usize::from(slot))
                        .ok_or_else(|| {
                            VmError::BadConstantPool(format!(
                                "{qualified}: local slot {slot} is out of range"
                            ))
                        })?;
                    *target = value;
                }

                Instr::Pop => {
                    pop(&mut thread.frames[frame_index], qualified, code)?;
                }
                Instr::Pop2 => {
                    // Our stack holds one value per Java value, so a long or double is one
                    // entry and `pop2` must not pop it twice.
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    if !matches!(value, Value::Long(_) | Value::Double(_)) {
                        pop(&mut thread.frames[frame_index], qualified, code)?;
                    }
                }
                Instr::Dup => {
                    let value = *thread.frames[frame_index].stack.last().ok_or_else(|| {
                        VmError::StackUnderflow {
                            method: qualified.to_owned(),
                            pc: code.pc(index),
                            needed: 1,
                        }
                    })?;
                    thread.frames[frame_index].stack.push(value);
                }

                Instr::BinOp(op) => {
                    let rhs = pop(&mut thread.frames[frame_index], qualified, code)?;
                    let lhs = pop(&mut thread.frames[frame_index], qualified, code)?;
                    let value =
                        binary(op, lhs, rhs).map_err(|error| fault(shared, thread, path, error))?;
                    thread.frames[frame_index].stack.push(value);
                }
                Instr::UnOp(op) => {
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    thread.frames[frame_index].stack.push(unary(op, value)?);
                }
                Instr::Convert(op) => {
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    thread.frames[frame_index].stack.push(convert(op, value)?);
                }
                Instr::Compare(op) => {
                    let rhs = pop(&mut thread.frames[frame_index], qualified, code)?;
                    let lhs = pop(&mut thread.frames[frame_index], qualified, code)?;
                    thread.frames[frame_index]
                        .stack
                        .push(compare(op, lhs, rhs)?);
                }

                Instr::Iinc { slot, increment } => {
                    let cell = thread.frames[frame_index]
                        .locals
                        .get_mut(usize::from(slot))
                        .ok_or_else(|| {
                            VmError::BadConstantPool(format!(
                                "{qualified}: local slot {slot} is out of range"
                            ))
                        })?;
                    let value = cell.as_int().ok_or(VmError::TypeMismatch {
                        expected: "int",
                        found: *cell,
                    })?;
                    *cell = Value::Int(value.wrapping_add(i32::from(increment)));
                }

                Instr::IfEq(target) => branch_eq(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    true,
                )?,
                Instr::IfNe(target) => branch_eq(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    false,
                )?,
                Instr::IfLt(target) => branch_cmp_zero(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |v| v < 0,
                )?,
                Instr::IfGe(target) => branch_cmp_zero(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |v| v >= 0,
                )?,
                Instr::IfGt(target) => branch_cmp_zero(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |v| v > 0,
                )?,
                Instr::IfLe(target) => branch_cmp_zero(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |v| v <= 0,
                )?,
                Instr::IfICmpEq(target) => branch_icmp(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |a, b| a == b,
                )?,
                Instr::IfICmpNe(target) => branch_icmp(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |a, b| a != b,
                )?,
                Instr::IfICmpLt(target) => branch_icmp(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |a, b| a < b,
                )?,
                Instr::IfICmpGe(target) => branch_icmp(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |a, b| a >= b,
                )?,
                Instr::IfICmpGt(target) => branch_icmp(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |a, b| a > b,
                )?,
                Instr::IfICmpLe(target) => branch_icmp(
                    &mut thread.frames[frame_index],
                    qualified,
                    code,
                    target,
                    |a, b| a <= b,
                )?,
                Instr::IfACmpEq(target) => {
                    let rhs = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    let lhs = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    if lhs == rhs {
                        thread.frames[frame_index].pc = target as usize;
                    }
                }
                Instr::IfACmpNe(target) => {
                    let rhs = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    let lhs = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    if lhs != rhs {
                        thread.frames[frame_index].pc = target as usize;
                    }
                }
                Instr::IfNull(target) => {
                    let reference = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    if reference.is_null() {
                        thread.frames[frame_index].pc = target as usize;
                    }
                }
                Instr::IfNonNull(target) => {
                    let reference = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    if !reference.is_null() {
                        thread.frames[frame_index].pc = target as usize;
                    }
                }
                Instr::Goto(target) => thread.frames[frame_index].pc = target as usize,

                Instr::TableSwitch(table) => {
                    let key = pop_int(&mut thread.frames[frame_index], qualified, code)?;
                    let table = &code.switches[usize::from(table)];
                    let target = table
                        .cases
                        .iter()
                        .find(|(case, _)| *case == key)
                        .map(|(_, target)| *target)
                        .unwrap_or(table.default);
                    thread.frames[frame_index].pc = target as usize;
                }
                Instr::LookupSwitch(table) => {
                    let key = pop_int(&mut thread.frames[frame_index], qualified, code)?;
                    let table = &code.switches[usize::from(table)];
                    let target = table
                        .cases
                        .binary_search_by_key(&key, |(case, _)| *case)
                        .map(|position| table.cases[position].1)
                        .unwrap_or(table.default);
                    thread.frames[frame_index].pc = target as usize;
                }

                Instr::ArrayLength => {
                    let reference = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    null_check(shared, thread, path, reference)?;
                    let length = shared.array(reference)?.elements.len();
                    thread.frames[frame_index]
                        .stack
                        .push(Value::Int(length as i32));
                }
                Instr::ArrayLoad(kind) => {
                    let index = pop_int(&mut thread.frames[frame_index], qualified, code)?;
                    let reference = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    let array = shared.array(reference)?;
                    if !array.kind.accepts(kind) {
                        return Err(VmError::TypeMismatch {
                            expected: "an array whose element type matches the instruction",
                            found: Value::Ref(reference),
                        });
                    }
                    let element =
                        *array
                            .elements
                            .get(index as usize)
                            .ok_or(VmError::IndexOutOfBounds {
                                index,
                                length: array.elements.len(),
                            })?;
                    thread.frames[frame_index].stack.push(element);
                }
                Instr::ArrayStore(kind) => {
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    let index = pop_int(&mut thread.frames[frame_index], qualified, code)?;
                    let reference = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    null_check(shared, thread, path, reference)?;
                    let length = {
                        let array = shared.array_mut(reference)?;
                        if !array.kind.accepts(kind) {
                            return Err(VmError::TypeMismatch {
                                expected: "an array whose element type matches the instruction",
                                found: Value::Ref(reference),
                            });
                        }
                        array.elements.len()
                    };
                    let slot = shared
                        .array_mut(reference)?
                        .elements
                        .get_mut(index as usize);
                    match slot {
                        Some(cell) => *cell = value,
                        None => {
                            return Err(fault(
                                shared,
                                thread,
                                path,
                                VmError::IndexOutOfBounds { index, length },
                            ));
                        }
                    }
                }
                Instr::NewArray(atype) => {
                    let length = pop_int(&mut thread.frames[frame_index], qualified, code)?;
                    let kind = newarray_kind(atype)?;
                    let reference = allocate_array_checked(shared, thread, path, kind, length)?;
                    thread.frames[frame_index].stack.push(Value::Ref(reference));
                }
                Instr::ANewArray(_constant) => {
                    // The element class is not needed until `instanceof` and `checkcast` exist;
                    // the array itself only has to be a reference array of the right length.
                    let length = pop_int(&mut thread.frames[frame_index], qualified, code)?;
                    let reference =
                        allocate_array_checked(shared, thread, path, ArrayKind::Reference, length)?;
                    thread.frames[frame_index].stack.push(Value::Ref(reference));
                }
                Instr::MultiANewArray {
                    class: constant,
                    dimensions,
                } => {
                    let mut dims = Vec::with_capacity(usize::from(dimensions));
                    for _ in 0..dimensions {
                        dims.push(pop_int(&mut thread.frames[frame_index], qualified, code)?);
                    }
                    dims.reverse();
                    // Any negative dimension is the same exception, raised before anything is
                    // allocated (JVMS §6.5 `multianewarray`).
                    if let Some(&length) = dims.iter().find(|length| **length < 0) {
                        return Err(new_exception(
                            shared,
                            thread,
                            path,
                            "java/lang/NegativeArraySizeException",
                            Some(&length.to_string()),
                        ));
                    }
                    let descriptor = resolve::class_constant(shared.class(class), constant)?;
                    let base = element_kind(&descriptor)?;
                    let reference = allocate_multi(shared, base, &dims)?;
                    thread.frames[frame_index].stack.push(Value::Ref(reference));
                }

                Instr::GetStatic(constant) => {
                    let field = resolve::field_ref(shared.class(class), constant)?;
                    let owner = shared.load_class(path, &field.class_name)?;
                    ensure_initialized(shared, thread, path, owner)?;
                    let value = *shared
                        .class(owner)
                        .statics
                        .get(&field.name)
                        .ok_or_else(|| VmError::FieldNotFound {
                            class: field.class_name.clone(),
                            name: field.name.clone(),
                        })?;
                    thread.frames[frame_index].stack.push(value);
                }
                Instr::PutStatic(constant) => {
                    let field = resolve::field_ref(shared.class(class), constant)?;
                    let owner = shared.load_class(path, &field.class_name)?;
                    ensure_initialized(shared, thread, path, owner)?;
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    shared.classes[owner.index()]
                        .statics
                        .insert(field.name, value);
                }

                Instr::GetField(constant) => {
                    let field = resolve::field_ref(shared.class(class), constant)?;
                    let declaring = shared.load_class(path, &field.class_name)?;
                    let slot = shared
                        .class(declaring)
                        .field_slot(&field.name, &field.descriptor)
                        .ok_or_else(|| VmError::FieldNotFound {
                            class: field.class_name.clone(),
                            name: field.name.clone(),
                        })?;
                    let receiver = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    null_check(shared, thread, path, receiver)?;
                    let value = shared.instance_field(receiver, slot)?;
                    thread.frames[frame_index].stack.push(value);
                }
                Instr::PutField(constant) => {
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    let field = resolve::field_ref(shared.class(class), constant)?;
                    let declaring = shared.load_class(path, &field.class_name)?;
                    let slot = shared
                        .class(declaring)
                        .field_slot(&field.name, &field.descriptor)
                        .ok_or_else(|| VmError::FieldNotFound {
                            class: field.class_name.clone(),
                            name: field.name.clone(),
                        })?;
                    let receiver = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    null_check(shared, thread, path, receiver)?;
                    shared.set_instance_field(receiver, slot, value)?;
                }

                Instr::New(constant) => {
                    let class_name = resolve::class_constant(shared.class(class), constant)?;
                    let owner = shared.load_class(path, &class_name)?;
                    ensure_initialized(shared, thread, path, owner)?;
                    // The class library may give its instances storage the VM does not
                    // model (a StringBuilder is a Rust String); anything else is a plain
                    // instance with a field vector.
                    let factory = shared.object_factory;
                    let reference = match factory.and_then(|build| build(&class_name, owner)) {
                        Some(object) => shared.allocate(object),
                        None => shared.new_instance(owner),
                    };
                    thread.frames[frame_index].stack.push(Value::Ref(reference));
                }

                Instr::InvokeVirtual(constant)
                | Instr::InvokeSpecial(constant)
                | Instr::InvokeStatic(constant)
                | Instr::InvokeInterface(constant) => {
                    // A static or special call site resolves to the same method every time — no
                    // receiver, no override — so a resolved one is reused; the first call does the
                    // full path and fills the cache.
                    if matches!(
                        instruction,
                        Instr::InvokeStatic(_) | Instr::InvokeSpecial(_)
                    ) && let Some(call) =
                        shared.class(class).invoke_cache.get(&constant).copied()
                    {
                        let args = drain(
                            &mut thread.frames[frame_index],
                            call.params,
                            qualified,
                            code,
                        )?;
                        let result = invoke(shared, thread, path, call.class, call.method, args)?;
                        if call.returns_value {
                            thread.frames[frame_index].stack.push(result);
                        }
                        return Ok(Step::Continue);
                    }

                    // A virtual or interface call resolves by receiver class, which is why the
                    // cache is keyed by it: one entry per (call site, class seen so far).
                    if matches!(
                        instruction,
                        Instr::InvokeVirtual(_) | Instr::InvokeInterface(_)
                    ) && let Some(site) = shared.class(class).virtual_cache.get(&constant)
                    {
                        let params = site.params;
                        let returns_value = site.returns_value;
                        let receiver = thread.frames[frame_index]
                            .stack
                            .len()
                            .checked_sub(params)
                            .map(|index| thread.frames[frame_index].stack[index]);
                        let target = receiver
                            .and_then(Value::as_ref)
                            .and_then(|reference| shared.class_of(reference))
                            .and_then(|receiver_class| site.targets.get(&receiver_class).copied());
                        if let Some((target_class, target_method)) = target {
                            let args =
                                drain(&mut thread.frames[frame_index], params, qualified, code)?;
                            let result =
                                invoke(shared, thread, path, target_class, target_method, args)?;
                            if returns_value {
                                thread.frames[frame_index].stack.push(result);
                            }
                            return Ok(Step::Continue);
                        }
                    }

                    let target = resolve::method_ref(shared.class(class), constant)?;
                    let named = shared.load_class(path, &target.class_name)?;
                    ensure_initialized(shared, thread, path, named)?;

                    let is_static = matches!(instruction, Instr::InvokeStatic(_));
                    let mut count = resolve::parameter_count(&target.descriptor);
                    if !is_static {
                        count += 1; // the receiver
                    }
                    let args = drain(&mut thread.frames[frame_index], count, qualified, code)?;
                    if !is_static && let Some(Value::Ref(receiver)) = args.first().copied() {
                        null_check(shared, thread, path, receiver)?;
                    }

                    // A lambda has no class of its own: calling its functional interface method
                    // runs the captured implementation with the call's arguments appended.
                    if !is_static && let Some(Value::Ref(receiver)) = args.first().copied() {
                        let lambda = match shared.heap.get(receiver.index()) {
                            Some(HeapObject::Lambda(lambda)) => Some((
                                lambda.target_class,
                                lambda.target_method,
                                lambda.captured.clone(),
                            )),
                            _ => None,
                        };
                        if let Some((target_class, target_method, mut full)) = lambda {
                            full.extend_from_slice(&args[1..]);
                            let result =
                                invoke(shared, thread, path, target_class, target_method, full)?;
                            if !target.descriptor.ends_with('V') {
                                thread.frames[frame_index].stack.push(result);
                            }
                            return Ok(Step::Continue);
                        }
                    }

                    // `invokevirtual` and `invokeinterface` start at the receiver's runtime
                    // class — that is what makes a `Shape` reference reach `Rect.area` or
                    // `Square.area`. `invokespecial` and `invokestatic` start at the class
                    // named in the constant pool, which is how `super.area()` reaches the
                    // superclass implementation instead of recursing.
                    let dispatch = match instruction {
                        Instr::InvokeVirtual(_) | Instr::InvokeInterface(_) => args
                            .first()
                            .copied()
                            .and_then(Value::as_ref)
                            .and_then(|receiver| shared.class_of(receiver))
                            .unwrap_or(named),
                        _ => named,
                    };

                    let (resolved_class, resolved_index) = shared
                        .resolve_method(dispatch, &target.name, &target.descriptor)
                        .ok_or_else(|| VmError::MethodNotFound {
                            class: target.class_name.clone(),
                            name: target.name.clone(),
                            descriptor: target.descriptor.clone(),
                        })?;

                    // Remember the receiver's class before `args` is consumed, so the call site's
                    // target can be cached per receiver class.
                    let receiver_class = if is_static {
                        None
                    } else {
                        args.first()
                            .copied()
                            .and_then(Value::as_ref)
                            .and_then(|receiver| shared.class_of(receiver))
                    };
                    let result =
                        invoke(shared, thread, path, resolved_class, resolved_index, args)?;
                    let returns_value = !target.descriptor.ends_with('V');
                    if returns_value {
                        thread.frames[frame_index].stack.push(result);
                    }
                    if is_static || matches!(instruction, Instr::InvokeSpecial(_)) {
                        shared.classes[class.index()].invoke_cache.insert(
                            constant,
                            rava_vm::class::StaticCall {
                                class: resolved_class,
                                method: resolved_index,
                                params: count,
                                returns_value,
                            },
                        );
                    } else if matches!(
                        instruction,
                        Instr::InvokeVirtual(_) | Instr::InvokeInterface(_)
                    ) {
                        // Remember the target for this receiver class, so the next call from the
                        // same class takes the fast path above.
                        if let Some(receiver_class) = receiver_class {
                            let site = shared.classes[class.index()]
                                .virtual_cache
                                .entry(constant)
                                .or_insert_with(|| rava_vm::class::VirtualCall {
                                    params: count,
                                    returns_value,
                                    targets: std::collections::HashMap::new(),
                                });
                            site.targets
                                .insert(receiver_class, (resolved_class, resolved_index));
                        }
                    }
                }

                Instr::MonitorEnter => {
                    let reference = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    if reference.is_null() {
                        return Err(new_exception(
                            shared,
                            thread,
                            path,
                            "java/lang/NullPointerException",
                            None,
                        ));
                    }
                    shared.monitor_enter(reference, thread.id)?;
                }
                Instr::MonitorExit => {
                    let reference = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    shared.monitor_exit(reference, thread.id);
                }
                Instr::InstanceOf(constant) => {
                    let target = resolve::class_constant(shared.class(class), constant)?;
                    let target_id = shared.load_class(path, &target)?;
                    let result = match pop(&mut thread.frames[frame_index], qualified, code)? {
                        Value::Ref(reference) if !reference.is_null() => {
                            match shared.class_of(reference) {
                                Some(actual) => is_instance_of(shared, actual, target_id),
                                None => false,
                            }
                        }
                        // `null instanceof T` is false.
                        _ => false,
                    };
                    thread.frames[frame_index]
                        .stack
                        .push(Value::Int(i32::from(result)));
                }
                Instr::CheckCast(constant) => {
                    let target = resolve::class_constant(shared.class(class), constant)?;
                    let target_id = shared.load_class(path, &target)?;
                    let value = pop(&mut thread.frames[frame_index], qualified, code)?;
                    let allowed = match value {
                        Value::Ref(reference) if !reference.is_null() => {
                            match shared.class_of(reference) {
                                Some(actual) => is_instance_of(shared, actual, target_id),
                                None => false,
                            }
                        }
                        // A null reference passes any cast.
                        _ => true,
                    };
                    if !allowed {
                        return Err(new_exception(
                            shared,
                            thread,
                            path,
                            "java/lang/ClassCastException",
                            Some(&target),
                        ));
                    }
                    thread.frames[frame_index].stack.push(value);
                }
                Instr::InvokeDynamic(constant) => {
                    let call_site = shared.indy_site(path, class, constant)?;
                    match call_site.site {
                        rava_vm::CallSite::Concat { recipe } => {
                            let count = resolve::parameter_count(&call_site.descriptor);
                            let mut args = Vec::with_capacity(count);
                            for _ in 0..count {
                                args.push(pop(&mut thread.frames[frame_index], qualified, code)?);
                            }
                            args.reverse();
                            let text = render_concat(shared, &recipe, &args)?;
                            let reference = shared.new_string(text);
                            thread.frames[frame_index].stack.push(Value::Ref(reference));
                        }
                        rava_vm::CallSite::Lambda {
                            target_class,
                            target_method,
                        } => {
                            let count = resolve::parameter_count(&call_site.descriptor);
                            let mut captured = Vec::with_capacity(count);
                            for _ in 0..count {
                                captured.push(pop(
                                    &mut thread.frames[frame_index],
                                    qualified,
                                    code,
                                )?);
                            }
                            captured.reverse();
                            let reference = shared.allocate(HeapObject::Lambda(rava_vm::Lambda {
                                target_class,
                                target_method,
                                captured,
                            }));
                            thread.frames[frame_index].stack.push(Value::Ref(reference));
                        }
                        rava_vm::CallSite::RecordMethod {
                            kind,
                            class_name,
                            components,
                        } => {
                            let value = execute_record_method(
                                shared,
                                &mut thread.frames[frame_index],
                                qualified,
                                code,
                                kind,
                                &class_name,
                                &components,
                            )?;
                            thread.frames[frame_index].stack.push(value);
                        }
                        rava_vm::CallSite::TypeSwitch { cases } => {
                            // Stack order is `value, start` (JVMS `typeSwitch`'s method type).
                            let start = pop(&mut thread.frames[frame_index], qualified, code)?;
                            let selector = pop(&mut thread.frames[frame_index], qualified, code)?;
                            let start = match start {
                                Value::Int(value) => value.max(0) as usize,
                                other => {
                                    return Err(VmError::TypeMismatch {
                                        expected: "int (the switch's start index)",
                                        found: other,
                                    });
                                }
                            };
                            let index = match selector {
                                // `null` has no class, and `-1` is the index javac's switch
                                // maps to a `case null` arm.
                                Value::Ref(reference) if reference.is_null() => -1,
                                Value::Ref(reference) => {
                                    match shared.class_of(reference).and_then(|actual| {
                                        cases
                                            .iter()
                                            .enumerate()
                                            .skip(start)
                                            .find(|(_, case)| {
                                                is_instance_of(shared, actual, **case)
                                            })
                                            .map(|(index, _)| index as i32)
                                    }) {
                                        Some(index) => index,
                                        // No case matched: the length is not a valid label, so
                                        // the switch's default arm runs.
                                        None => cases.len() as i32,
                                    }
                                }
                                other => {
                                    return Err(VmError::TypeMismatch {
                                        expected: "a reference (the switch's selector)",
                                        found: other,
                                    });
                                }
                            };
                            thread.frames[frame_index].stack.push(Value::Int(index));
                        }
                        rava_vm::CallSite::Unsupported { bootstrap } => {
                            return Err(VmError::Unsupported {
                                feature: "bootstrap method",
                                detail: format!(
                                    "{bootstrap} in {qualified}; only StringConcatFactory is implemented"
                                ),
                            });
                        }
                    }
                }
                Instr::Ireturn => {
                    return pop(&mut thread.frames[frame_index], qualified, code).map(Step::Return);
                }
                Instr::Lreturn | Instr::Freturn | Instr::Dreturn | Instr::Areturn => {
                    return pop(&mut thread.frames[frame_index], qualified, code).map(Step::Return);
                }
                Instr::Return => return Ok(Step::Return(Value::NULL)),
                Instr::Athrow => {
                    let object = pop_ref(&mut thread.frames[frame_index], qualified, code)?;
                    if object.is_null() {
                        return Err(new_exception(
                            shared,
                            thread,
                            path,
                            "java/lang/NullPointerException",
                            None,
                        ));
                    }
                    // A Java-level `throw` carries an object that was created earlier; the trace
                    // must be taken here, while the frames still exist, and only once — a
                    // rethrow keeps the original trace, as Java does.
                    if !shared.traces.contains_key(&object) {
                        let trace = shared.capture_trace(thread);
                        shared.traces.insert(object, trace);
                    }
                    return Err(VmError::Thrown { object });
                }
            }
            Ok(Step::Continue)
        })();

        match step {
            Ok(Step::Continue) => {}
            Ok(Step::Return(value)) => return Ok(value),
            Err(VmError::Thrown { object }) => {
                match find_handler(shared, path, code, &thread.frames[frame_index], object)? {
                    Some(target) => {
                        // JVMS §4.10.1.6: the operand stack is cleared and only the
                        // exception remains, then execution resumes at the handler.
                        thread.frames[frame_index].stack.clear();
                        thread.frames[frame_index].stack.push(Value::Ref(object));
                        thread.frames[frame_index].pc = target;
                    }
                    None => return Err(VmError::Thrown { object }),
                }
            }
            Err(error) => return Err(error),
        }

        // The allocation budget's safe point: every temporary of the instruction just
        // executed is gone, so the frame stack is the complete root set.
        if shared.pending_gc {
            shared.collect(thread);
        }
    }
}

fn pop(frame: &mut Frame, method: &str, code: &rava_vm::Code) -> Result<Value, VmError> {
    frame.stack.pop().ok_or_else(|| VmError::StackUnderflow {
        method: method.to_owned(),
        pc: code.pc(frame.pc.saturating_sub(1)),
        needed: 1,
    })
}

fn pop_int(frame: &mut Frame, method: &str, code: &rava_vm::Code) -> Result<i32, VmError> {
    let value = pop(frame, method, code)?;
    value.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: value,
    })
}

fn pop_ref(
    frame: &mut Frame,
    method: &str,
    code: &rava_vm::Code,
) -> Result<rava_vm::ObjRef, VmError> {
    let value = pop(frame, method, code)?;
    value.as_ref().ok_or(VmError::TypeMismatch {
        expected: "reference",
        found: value,
    })
}

fn branch_eq(
    frame: &mut Frame,
    method: &str,
    code: &rava_vm::Code,
    target: u32,
    take_if_zero: bool,
) -> Result<(), VmError> {
    let value = pop_int(frame, method, code)?;
    if (value == 0) == take_if_zero {
        frame.pc = target as usize;
    }
    Ok(())
}

fn branch_cmp_zero(
    frame: &mut Frame,
    method: &str,
    code: &rava_vm::Code,
    target: u32,
    predicate: impl FnOnce(i32) -> bool,
) -> Result<(), VmError> {
    let value = pop_int(frame, method, code)?;
    if predicate(value) {
        frame.pc = target as usize;
    }
    Ok(())
}

fn branch_icmp(
    frame: &mut Frame,
    method: &str,
    code: &rava_vm::Code,
    target: u32,
    predicate: impl FnOnce(i32, i32) -> bool,
) -> Result<(), VmError> {
    let rhs = pop_int(frame, method, code)?;
    let lhs = pop_int(frame, method, code)?;
    if predicate(lhs, rhs) {
        frame.pc = target as usize;
    }
    Ok(())
}

fn drain(
    frame: &mut Frame,
    count: usize,
    method: &str,
    code: &rava_vm::Code,
) -> Result<Vec<Value>, VmError> {
    if frame.stack.len() < count {
        return Err(VmError::StackUnderflow {
            method: method.to_owned(),
            pc: code.pc(frame.pc.saturating_sub(1)),
            needed: count,
        });
    }
    let start = frame.stack.len() - count;
    Ok(frame.stack.split_off(start))
}

/// Entry point for natives: `Thread.start()` and anything else that must run Java code.
pub fn invoke_from_native(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    class: ClassId,
    method_index: usize,
    args: Vec<Value>,
) -> Result<Value, VmError> {
    // A native call has no caller frame, so the lambda interception that lives in the
    // `invoke*` instruction arm cannot run: do it here as well.
    if let Some(Value::Ref(receiver)) = args.first().copied() {
        if let Some(lambda) = lambda_target(shared, receiver) {
            let mut full = lambda.2;
            full.extend_from_slice(&args[1..]);
            return invoke(shared, thread, path, lambda.0, lambda.1, full);
        }
    }
    invoke(shared, thread, path, class, method_index, args)
}

/// `(target_class, target_method, captured)` if the reference is a lambda.
fn lambda_target(shared: &Shared, reference: ObjRef) -> Option<(ClassId, usize, Vec<Value>)> {
    match shared.heap.get(reference.index()) {
        Some(HeapObject::Lambda(lambda)) => Some((
            lambda.target_class,
            lambda.target_method,
            lambda.captured.clone(),
        )),
        _ => None,
    }
}

fn call_native(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    owner: &str,
    name: &str,
    descriptor: &str,
    args: &[Value],
) -> Result<Value, VmError> {
    let key = rava_vm::native::key(owner, name, descriptor);
    let function = *shared
        .natives
        .get(&key)
        .ok_or_else(|| VmError::MissingNative(key.clone()))?;
    let mut ctx = NativeCtx {
        shared,
        thread,
        path,
        args,
    };
    function(&mut ctx)
}

fn newarray_kind(atype: u8) -> Result<ArrayKind, VmError> {
    Ok(match atype {
        4 => ArrayKind::Boolean,
        5 => ArrayKind::Char,
        6 => ArrayKind::Float,
        7 => ArrayKind::Double,
        8 => ArrayKind::Byte,
        9 => ArrayKind::Short,
        10 => ArrayKind::Int,
        11 => ArrayKind::Long,
        other => {
            return Err(VmError::BadConstantPool(format!(
                "unknown newarray atype {other}"
            )));
        }
    })
}

/// The innermost element kind of an array class descriptor such as `[[I`.
fn element_kind(descriptor: &str) -> Result<ArrayKind, VmError> {
    let base = descriptor
        .trim_start_matches('[')
        .as_bytes()
        .first()
        .copied();
    Ok(match base {
        Some(b'I') => ArrayKind::Int,
        Some(b'J') => ArrayKind::Long,
        Some(b'F') => ArrayKind::Float,
        Some(b'D') => ArrayKind::Double,
        Some(b'Z') => ArrayKind::Boolean,
        Some(b'B') => ArrayKind::Byte,
        Some(b'C') => ArrayKind::Char,
        Some(b'S') => ArrayKind::Short,
        Some(b'L') => ArrayKind::Reference,
        _ => {
            return Err(VmError::BadConstantPool(format!(
                "not an array type: {descriptor}"
            )));
        }
    })
}

/// Allocates an array, turning a negative length into the exception Java raises rather than a VM
/// error: `new int[-1]` is a program fault, not a broken class file (JVMS §6.5 `newarray`).
fn allocate_array_checked(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    kind: ArrayKind,
    length: i32,
) -> Result<ObjRef, VmError> {
    if length < 0 {
        return Err(new_exception(
            shared,
            thread,
            path,
            "java/lang/NegativeArraySizeException",
            Some(&length.to_string()),
        ));
    }
    allocate_array(shared, kind, length)
}

fn allocate_array(shared: &mut Shared, kind: ArrayKind, length: i32) -> Result<ObjRef, VmError> {
    if length < 0 {
        return Err(VmError::NegativeArraySize { length });
    }
    let elements = vec![kind.default_value(); length as usize];
    Ok(shared.allocate(HeapObject::Array(Array { kind, elements })))
}

/// Allocates `multianewarray` dimensions: every level but the last holds arrays.
fn allocate_multi(shared: &mut Shared, base: ArrayKind, dims: &[i32]) -> Result<ObjRef, VmError> {
    let kind = if dims.len() == 1 {
        base
    } else {
        ArrayKind::Reference
    };
    let length = dims[0];
    let reference = allocate_array(shared, kind, length)?;
    if dims.len() > 1 {
        for index in 0..length {
            let inner = allocate_multi(shared, base, &dims[1..])?;
            shared.array_mut(reference)?.elements[index as usize] = Value::Ref(inner);
        }
    }
    Ok(reference)
}

fn int(value: Value) -> Result<i32, VmError> {
    value.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: value,
    })
}

fn long(value: Value) -> Result<i64, VmError> {
    value.as_long().ok_or(VmError::TypeMismatch {
        expected: "long",
        found: value,
    })
}

fn float(value: Value) -> Result<f32, VmError> {
    value.as_float().ok_or(VmError::TypeMismatch {
        expected: "float",
        found: value,
    })
}

fn double(value: Value) -> Result<f64, VmError> {
    value.as_double().ok_or(VmError::TypeMismatch {
        expected: "double",
        found: value,
    })
}

fn binary(op: BinOp, lhs: Value, rhs: Value) -> Result<Value, VmError> {
    let value = match op {
        BinOp::IAdd => Value::Int(int(lhs)?.wrapping_add(int(rhs)?)),
        BinOp::LAdd => Value::Long(long(lhs)?.wrapping_add(long(rhs)?)),
        BinOp::FAdd => Value::Float(float(lhs)? + float(rhs)?),
        BinOp::DAdd => Value::Double(double(lhs)? + double(rhs)?),
        BinOp::ISub => Value::Int(int(lhs)?.wrapping_sub(int(rhs)?)),
        BinOp::LSub => Value::Long(long(lhs)?.wrapping_sub(long(rhs)?)),
        BinOp::FSub => Value::Float(float(lhs)? - float(rhs)?),
        BinOp::DSub => Value::Double(double(lhs)? - double(rhs)?),
        BinOp::IMul => Value::Int(int(lhs)?.wrapping_mul(int(rhs)?)),
        BinOp::LMul => Value::Long(long(lhs)?.wrapping_mul(long(rhs)?)),
        BinOp::FMul => Value::Float(float(lhs)? * float(rhs)?),
        BinOp::DMul => Value::Double(double(lhs)? * double(rhs)?),
        BinOp::IDiv => {
            let divisor = int(rhs)?;
            if divisor == 0 {
                return Err(VmError::Arithmetic("integer division by zero".to_owned()));
            }
            Value::Int(int(lhs)?.wrapping_div(divisor))
        }
        BinOp::LDiv => {
            let divisor = long(rhs)?;
            if divisor == 0 {
                return Err(VmError::Arithmetic("long division by zero".to_owned()));
            }
            Value::Long(long(lhs)?.wrapping_div(divisor))
        }
        BinOp::FDiv => Value::Float(float(lhs)? / float(rhs)?),
        BinOp::DDiv => Value::Double(double(lhs)? / double(rhs)?),
        BinOp::IRem => {
            let divisor = int(rhs)?;
            if divisor == 0 {
                return Err(VmError::Arithmetic("integer remainder by zero".to_owned()));
            }
            Value::Int(int(lhs)?.wrapping_rem(divisor))
        }
        BinOp::LRem => {
            let divisor = long(rhs)?;
            if divisor == 0 {
                return Err(VmError::Arithmetic("long remainder by zero".to_owned()));
            }
            Value::Long(long(lhs)?.wrapping_rem(divisor))
        }
        BinOp::FRem => Value::Float(float(lhs)? % float(rhs)?),
        BinOp::DRem => Value::Double(double(lhs)? % double(rhs)?),
        BinOp::IShl => Value::Int(int(lhs)? << (int(rhs)? & 31)),
        BinOp::LShl => Value::Long(long(lhs)? << (int(rhs)? & 63)),
        BinOp::IShr => Value::Int(int(lhs)? >> (int(rhs)? & 31)),
        BinOp::LShr => Value::Long(long(lhs)? >> (int(rhs)? & 63)),
        BinOp::IUShr => Value::Int(((int(lhs)? as u32) >> (int(rhs)? & 31)) as i32),
        BinOp::LUShr => Value::Long(((long(lhs)? as u64) >> (int(rhs)? & 63)) as i64),
        BinOp::IAnd => Value::Int(int(lhs)? & int(rhs)?),
        BinOp::LAnd => Value::Long(long(lhs)? & long(rhs)?),
        BinOp::IOr => Value::Int(int(lhs)? | int(rhs)?),
        BinOp::LOr => Value::Long(long(lhs)? | long(rhs)?),
        BinOp::IXor => Value::Int(int(lhs)? ^ int(rhs)?),
        BinOp::LXor => Value::Long(long(lhs)? ^ long(rhs)?),
    };
    Ok(value)
}

fn unary(op: UnOp, value: Value) -> Result<Value, VmError> {
    let result = match op {
        UnOp::INeg => Value::Int(int(value)?.wrapping_neg()),
        UnOp::LNeg => Value::Long(long(value)?.wrapping_neg()),
        UnOp::FNeg => Value::Float(-float(value)?),
        UnOp::DNeg => Value::Double(-double(value)?),
    };
    Ok(result)
}

fn convert(op: Convert, value: Value) -> Result<Value, VmError> {
    let result = match op {
        Convert::I2L => Value::Long(i64::from(int(value)?)),
        Convert::I2F => Value::Float(int(value)? as f32),
        Convert::I2D => Value::Double(f64::from(int(value)?)),
        Convert::L2I => Value::Int(long(value)? as i32),
        Convert::L2F => Value::Float(long(value)? as f32),
        Convert::L2D => Value::Double(long(value)? as f64),
        Convert::F2I => Value::Int(float(value)? as i32),
        Convert::F2L => Value::Long(float(value)? as i64),
        Convert::F2D => Value::Double(f64::from(float(value)?)),
        Convert::D2I => Value::Int(double(value)? as i32),
        Convert::D2L => Value::Long(double(value)? as i64),
        Convert::D2F => Value::Float(double(value)? as f32),
        Convert::I2B => Value::Int(i32::from(int(value)? as i8)),
        Convert::I2C => Value::Int(i32::from(int(value)? as u16)),
        Convert::I2S => Value::Int(i32::from(int(value)? as i16)),
    };
    Ok(result)
}

fn compare(op: Compare, lhs: Value, rhs: Value) -> Result<Value, VmError> {
    let result = match op {
        Compare::LCmp => {
            let left = long(lhs)?;
            let right = long(rhs)?;
            if left < right {
                -1
            } else if left > right {
                1
            } else {
                0
            }
        }
        // `fcmpl`/`dcmpl` return -1 for NaN, `fcmpg`/`dcmpg` return 1 (JVMS §6.fcmpl).
        Compare::FCmpL => float_compare(float(lhs)?, float(rhs)?, -1),
        Compare::FCmpG => float_compare(float(lhs)?, float(rhs)?, 1),
        Compare::DCmpL => double_compare(double(lhs)?, double(rhs)?, -1),
        Compare::DCmpG => double_compare(double(lhs)?, double(rhs)?, 1),
    };
    Ok(Value::Int(result))
}

fn float_compare(left: f32, right: f32, nan: i32) -> i32 {
    if left.is_nan() || right.is_nan() {
        nan
    } else if left < right {
        -1
    } else if left > right {
        1
    } else {
        0
    }
}

fn double_compare(left: f64, right: f64, nan: i32) -> i32 {
    if left.is_nan() || right.is_nan() {
        nan
    } else if left < right {
        -1
    } else if left > right {
        1
    } else {
        0
    }
}

/// Builds an exception object and wraps it as a throw, or reports why it could not be built.
fn new_exception(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    class_name: &str,
    message: Option<&str>,
) -> VmError {
    match allocate_exception(shared, thread, path, class_name, message) {
        Ok(object) => {
            let trace = shared.capture_trace(thread);
            shared.traces.insert(object, trace);
            VmError::Thrown { object }
        }
        Err(error) => error,
    }
}

/// Allocates an instance and runs its constructor — the same path a Java-level `throw` takes,
/// which is why the message can live in an ordinary Java field.
fn allocate_exception(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    class_name: &str,
    message: Option<&str>,
) -> Result<ObjRef, VmError> {
    let class = shared.load_class(path, class_name)?;
    ensure_initialized(shared, thread, path, class)?;
    let object = shared.new_instance(class);

    let descriptor = if message.is_some() {
        "(Ljava/lang/String;)V"
    } else {
        "()V"
    };
    let (owner, index) = shared
        .resolve_method(class, "<init>", descriptor)
        .ok_or_else(|| VmError::MethodNotFound {
            class: class_name.to_owned(),
            name: "<init>".to_owned(),
            descriptor: descriptor.to_owned(),
        })?;

    let mut args = vec![Value::Ref(object)];
    if let Some(text) = message {
        let text = shared.new_string(text.to_owned());
        args.push(Value::Ref(text));
    }
    invoke(shared, thread, path, owner, index, args)?;
    Ok(object)
}

/// Turns a VM-level fault into the Java exception the JVM would raise (M1). A fault that has
/// no Java counterpart passes through unchanged.
fn fault(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    error: VmError,
) -> VmError {
    match error {
        VmError::Arithmetic(detail) => {
            // The JDK's message for division by zero is exactly "/ by zero".
            let message = if detail.contains("by zero") {
                "/ by zero".to_owned()
            } else {
                detail
            };
            new_exception(
                shared,
                thread,
                path,
                "java/lang/ArithmeticException",
                Some(&message),
            )
        }
        VmError::NegativeArraySize { length } => new_exception(
            shared,
            thread,
            path,
            "java/lang/NegativeArraySizeException",
            Some(&length.to_string()),
        ),
        VmError::VerifyError { detail } => {
            new_exception(shared, thread, path, "java/lang/VerifyError", Some(&detail))
        }
        VmError::IndexOutOfBounds { index, .. } => new_exception(
            shared,
            thread,
            path,
            "java/lang/ArrayIndexOutOfBoundsException",
            Some(&index.to_string()),
        ),
        other => other,
    }
}

/// The instruction index of a handler covering `frame.pc`, if any.
fn find_handler(
    shared: &mut Shared,
    path: &ClassPath,
    code: &rava_vm::Code,
    frame: &Frame,
    object: ObjRef,
) -> Result<Option<usize>, VmError> {
    if code.handlers.is_empty() {
        return Ok(None);
    }
    let Some(thrown) = shared.class_of(object) else {
        return Ok(None);
    };
    let pc = frame.pc.saturating_sub(1);

    for handler in &code.handlers {
        if pc < handler.start || pc >= handler.end {
            continue;
        }
        if handler.catch_type == 0 {
            return Ok(Some(handler.handler));
        }
        let catch_name = resolve::class_constant(shared.class(frame.class), handler.catch_type)?;
        let catch_class = shared.load_class(path, &catch_name)?;
        if is_subclass(shared, thrown, catch_class) {
            return Ok(Some(handler.handler));
        }
    }
    Ok(None)
}

fn is_subclass(shared: &Shared, mut class: ClassId, target: ClassId) -> bool {
    loop {
        if class == target {
            return true;
        }
        match shared.class(class).super_class {
            Some(super_id) => class = super_id,
            None => return false,
        }
    }
}

/// A null receiver is a Java `NullPointerException`, not a VM failure — a `try` block around
/// the call must be able to catch it.
fn null_check(
    shared: &mut Shared,
    thread: &mut ThreadState,
    path: &ClassPath,
    reference: ObjRef,
) -> Result<(), VmError> {
    if reference.is_null() {
        Err(new_exception(
            shared,
            thread,
            path,
            "java/lang/NullPointerException",
            None,
        ))
    } else {
        Ok(())
    }
}

/// Renders a `StringConcatFactory` recipe against the call site's arguments.
///
/// `\u0001` takes the next dynamic argument and `\u0002` the next bootstrap constant, which is
/// the whole format: no separators, no locale, no padding.
fn render_concat(
    shared: &Shared,
    recipe: &[rava_vm::RecipePart],
    args: &[Value],
) -> Result<String, VmError> {
    let mut out = String::new();
    let mut next = args.iter();

    for part in recipe {
        match part {
            rava_vm::RecipePart::Text(text) => out.push_str(text),
            rava_vm::RecipePart::Constant(text) => out.push_str(text),
            rava_vm::RecipePart::Argument => {
                let value = *next.next().ok_or_else(|| {
                    VmError::BadConstantPool(
                        "the concat recipe has more arguments than the call site".to_owned(),
                    )
                })?;
                out.push_str(&render_value(shared, value)?);
            }
        }
    }
    Ok(out)
}

/// The `String.valueOf`-ish rendering of one argument.
///
/// Floating point is Rust's shortest representation, which differs from Java's for whole
/// numbers (`1.0` versus `1`); that gap is recorded in the milestone note rather than hidden.
fn render_value(shared: &Shared, value: Value) -> Result<String, VmError> {
    // A boxed `Integer` is the one non-String reference a concat can render: its `toString` is
    // just its value. General `toString` dispatch needs the JavaCall seam (issue #26).
    if let Value::Ref(reference) = value {
        if !reference.is_null()
            && let Some(HeapObject::Instance(_)) = shared.heap.get(reference.index())
            && let Some(class) = shared.class_of(reference)
            && shared.class(class).name == "java/lang/Integer"
            && let Some(slot) = shared.class(class).field_slot("value", "I")
            && let Ok(Value::Int(number)) = shared.instance_field(reference, slot)
        {
            return Ok(number.to_string());
        }
    }
    Ok(match value {
        Value::Int(number) => number.to_string(),
        Value::Long(number) => number.to_string(),
        Value::Float(number) => rava_vm::format::float_to_string(number),
        Value::Double(number) => rava_vm::format::double_to_string(number),
        Value::Ref(reference) if reference.is_null() => "null".to_owned(),
        Value::Ref(reference) => match shared.heap.get(reference.index()) {
            Some(HeapObject::Str(text)) => text.clone(),
            Some(other) => {
                return Err(VmError::Unsupported {
                    feature: "toString",
                    detail: format!("cannot render {other:?} in a concat recipe"),
                });
            }
            None => return Err(VmError::Native(format!("dangling reference {reference:?}"))),
        },
    })
}

/// `instanceof`, including interfaces: the class, its superclasses, and every interface
/// reachable from any of them.
fn is_instance_of(shared: &Shared, actual: ClassId, target: ClassId) -> bool {
    let mut visited_interfaces = Vec::new();
    let mut current = Some(actual);
    while let Some(class) = current {
        if class == target {
            return true;
        }
        for interface in &shared.class(class).interfaces {
            if is_interface_of(shared, *interface, target, &mut visited_interfaces) {
                return true;
            }
        }
        current = shared.class(class).super_class;
    }
    false
}

fn is_interface_of(
    shared: &Shared,
    interface: ClassId,
    target: ClassId,
    visited: &mut Vec<ClassId>,
) -> bool {
    if interface == target {
        return true;
    }
    if visited.contains(&interface) {
        return false;
    }
    visited.push(interface);
    shared
        .class(interface)
        .interfaces
        .iter()
        .any(|parent| is_interface_of(shared, *parent, target, visited))
}

/// Runs one of a record's generated methods over its component slots.
fn execute_record_method(
    shared: &mut Shared,
    frame: &mut Frame,
    qualified: &str,
    code: &rava_vm::Code,
    kind: rava_vm::RecordMethodKind,
    class_name: &str,
    components: &[rava_vm::Component],
) -> Result<Value, VmError> {
    use rava_vm::RecordMethodKind;

    match kind {
        RecordMethodKind::ToString => {
            let receiver = pop(frame, qualified, code)?;
            let reference = receiver.as_ref().ok_or(VmError::TypeMismatch {
                expected: "the record instance",
                found: receiver,
            })?;
            let mut text = format!("{class_name}[");
            for (index, component) in components.iter().enumerate() {
                if index > 0 {
                    text.push_str(", ");
                }
                let value = shared.instance_field(reference, component.slot)?;
                text.push_str(&component.name);
                text.push('=');
                text.push_str(&render_value(shared, value)?);
            }
            text.push(']');
            Ok(Value::Ref(shared.new_string(text)))
        }
        RecordMethodKind::Equals => {
            let other = pop(frame, qualified, code)?;
            let receiver = pop(frame, qualified, code)?;
            let equal = match (receiver.as_ref(), other.as_ref()) {
                (Some(left), Some(right)) if !right.is_null() => {
                    match (shared.class_of(left), shared.class_of(right)) {
                        (Some(left_class), Some(right_class)) if left_class == right_class => {
                            let mut same = true;
                            for component in components {
                                let a = shared.instance_field(left, component.slot)?;
                                let b = shared.instance_field(right, component.slot)?;
                                if !record_values_equal(shared, a, b) {
                                    same = false;
                                    break;
                                }
                            }
                            same
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
            Ok(Value::Int(i32::from(equal)))
        }
        RecordMethodKind::HashCode => {
            let receiver = pop(frame, qualified, code)?;
            let reference = receiver.as_ref().ok_or(VmError::TypeMismatch {
                expected: "the record instance",
                found: receiver,
            })?;
            // JLS §8.10.3 / the JDK's ObjectMethods: result = 31 * result + hash(component),
            // starting at zero (checked against java for Point(3, 4) == 97).
            let mut hash: i32 = 0;
            for component in components {
                let value = shared.instance_field(reference, component.slot)?;
                hash = hash
                    .wrapping_mul(31)
                    .wrapping_add(record_value_hash(shared, value));
            }
            Ok(Value::Int(hash))
        }
    }
}

/// Component equality: primitives by value, references null-safe with string content compared
/// by text (there is no `Object.equals` yet).
fn record_values_equal(shared: &Shared, left: Value, right: Value) -> bool {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Long(a), Value::Long(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
        (Value::Double(a), Value::Double(b)) => a.to_bits() == b.to_bits(),
        (Value::Ref(a), Value::Ref(b)) => {
            if a.is_null() || b.is_null() {
                return a == b;
            }
            match (shared.heap.get(a.index()), shared.heap.get(b.index())) {
                (Some(HeapObject::Str(x)), Some(HeapObject::Str(y))) => x == y,
                _ => a == b,
            }
        }
        _ => false,
    }
}

/// The component hash the JDK's generated method uses.
fn record_value_hash(shared: &Shared, value: Value) -> i32 {
    match value {
        Value::Int(number) => number,
        Value::Long(number) => (number ^ (number >> 32)) as i32,
        Value::Float(number) => number.to_bits() as i32,
        Value::Double(number) => {
            let bits = number.to_bits();
            (bits ^ (bits >> 32)) as i32
        }
        Value::Ref(reference) => {
            if reference.is_null() {
                return 0;
            }
            match shared.heap.get(reference.index()) {
                Some(HeapObject::Str(text)) => {
                    let mut hash: i32 = 0;
                    for unit in text.encode_utf16() {
                        hash = hash.wrapping_mul(31).wrapping_add(i32::from(unit));
                    }
                    hash
                }
                _ => reference.0 as i32,
            }
        }
    }
}
