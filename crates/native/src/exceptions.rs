//! The throwable natives.
//!
//! `Throwable` is the first class library type whose *behaviour* the VM has to supply rather
//! than only its storage: constructing any throwable calls `fillInStackTrace`, a `synchronized`
//! Java method that calls the private native `fillInStackTrace(int)` and then resets
//! `stackTrace` to `UNASSIGNED_STACK`. The trace the VM records is therefore taken here, while
//! the constructor's frames still exist.
//!
//! Two representations share that capture:
//!
//! - [`Shared::traces`] keeps the frames for the VM's own report ([`Shared::describe_throwable`])
//!   and for the interpreter's `athrow`;
//! - the receiver's `stackTrace`/`depth` fields get real `StackTraceElement` objects, which is
//!   what the host's `getStackTrace()`/`printStackTrace()` read. The JDK's private `backtrace`
//!   field is never written: it encodes a format only HotSpot knows, and the Java view does not
//!   need it once `stackTrace` is filled.
//!
//! `Throwable.fillInStackTrace()` itself is registered as an [`intrinsic`](Shared::intrinsics):
//! the host's Java body ends with `stackTrace = UNASSIGNED_STACK` unconditionally after calling
//! the private native, so filling the field from the native alone would leave the array
//! unreachable — measured as an empty `getStackTrace()` (see the S19 note).
//!
//! The `contains_key` check makes the two paths agree: the first capture wins, so the trace is
//! the place the throwable was created, not the place it was last rethrown or re-filled. That
//! is a deliberate simplification for this slice: the host's own `fillInStackTrace()` re-fills
//! the native `backtrace` on every explicit call (measured: `java Fill.java` reports
//! `first=again` after a second call), which the VM does not model yet — see the S12 note's
//! open threads.

use rava_vm::native::expect_args;
use rava_vm::{
    Array, ArrayKind, ClassId, HeapObject, Instr, NativeCtx, ObjRef, Shared, TraceFrame, Value,
    VmError,
};

/// The descriptor of the array `Throwable.stackTrace` holds.
const STACK_TRACE_ARRAY: &str = "[Ljava/lang/StackTraceElement;";

/// Installs the throwable natives.
pub fn register(shared: &mut Shared) {
    shared.register_native(
        "java/lang/Throwable",
        "fillInStackTrace",
        "(I)Ljava/lang/Throwable;",
        fill_in_stack_trace,
    );
    shared.register_intrinsic(
        "java/lang/Throwable",
        "fillInStackTrace",
        "()Ljava/lang/Throwable;",
        fill_in_stack_trace_intrinsic,
    );
}

/// `Throwable.fillInStackTrace(int)`: capture this thread's stack once for the receiver and
/// fill the receiver's Java view.
///
/// The `int` is a dummy argument in the host's declaration; the VM ignores its value but still
/// checks that the call site passed an `int`, which turns a wrong registration into a type error
/// instead of a silent one. The trace's innermost frame is the caller
/// (`Throwable.fillInStackTrace()`), which is still on the stack while this native runs.
fn fill_in_stack_trace(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, depth] = expect_args::<2>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a throwable reference",
        found: *this,
    })?;
    depth.as_int().ok_or(VmError::TypeMismatch {
        expected: "int",
        found: *depth,
    })?;
    capture_once(ctx, reference, false)?;
    Ok(Value::Ref(reference))
}

/// `Throwable.fillInStackTrace()`: the same capture, run in place of the host's Java body.
///
/// The host body calls the private native, then overwrites `stackTrace` with
/// `UNASSIGNED_STACK`. Replacing the whole method keeps the array the native wrote. The one
/// thing the Java body contributed to the trace was its own frame (it was still on the stack
/// when the native ran), so it is synthesised here with the line the body would have reported.
fn fill_in_stack_trace_intrinsic(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a throwable reference",
        found: *this,
    })?;
    capture_once(ctx, reference, true)?;
    Ok(Value::Ref(reference))
}

/// Records `reference`'s trace once, then fills its `stackTrace` and `depth` fields.
///
/// `own_frame` says whether the Java method's frame is missing from `thread.frames` (the
/// intrinsic replaced it) and has to be synthesised. A re-capture keeps the first trace: the
/// JDK re-records on an explicit `fillInStackTrace()`, which this slice deliberately does not.
fn capture_once(
    ctx: &mut NativeCtx<'_>,
    reference: ObjRef,
    own_frame: bool,
) -> Result<(), VmError> {
    if ctx.shared.traces.contains_key(&reference) {
        return Ok(());
    }
    let mut trace = if own_frame {
        Vec::with_capacity(ctx.thread.frames.len() + 1)
    } else {
        Vec::with_capacity(ctx.thread.frames.len())
    };
    if own_frame {
        trace.push(TraceFrame {
            class: "java/lang/Throwable".to_owned(),
            method: "fillInStackTrace".to_owned(),
            line: fill_in_stack_trace_line(ctx.shared),
        });
    }
    trace.extend(ctx.shared.capture_trace(ctx.thread));
    fill_java_view(ctx, reference, &trace)?;
    ctx.shared.traces.insert(reference, trace);
    Ok(())
}

/// The source line the host's `fillInStackTrace()` body is on when it calls its private native.
///
/// The Java body is `fillInStackTrace(0); stackTrace = UNASSIGNED_STACK;`, and the trace of that
/// frame is the line of the `fillInStackTrace(I)` call. Reading it from the loaded class keeps
/// the VM's report identical to what the interpreter would have recorded, without hard-coding a
/// JDK line number.
fn fill_in_stack_trace_line(shared: &Shared) -> u16 {
    let Some(class_id) = shared.class_ids.get("java/lang/Throwable").copied() else {
        return 0;
    };
    let Some(method_index) = shared
        .class(class_id)
        .method_index("fillInStackTrace", "()Ljava/lang/Throwable;")
    else {
        return 0;
    };
    let method = &shared.class(class_id).methods[method_index];
    let Some(code) = &method.code else {
        return 0;
    };
    for (index, instruction) in code.instrs.iter().enumerate() {
        if let Instr::InvokeVirtual(constant) = instruction
            && let Ok(target) = rava_vm::resolve::method_ref(shared.class(class_id), *constant)
            && target.name == "fillInStackTrace"
            && target.descriptor.starts_with("(I)")
        {
            return method.line_at(index as u32);
        }
    }
    0
}

/// Writes the receiver's `stackTrace` and `depth` fields from `trace`.
///
/// One `StackTraceElement` per frame. `declaringClass` is the binary name with `/` turned into
/// `.`; `fileName` comes from the class's `SourceFile` attribute and is `null` when the class
/// was compiled without one; `lineNumber` is the frame's line, or `-2` ("Unknown Source") when
/// the trace has none. `classLoaderName`, `moduleName` and `moduleVersion` stay `null`, and so
/// does `backtrace`.
fn fill_java_view(
    ctx: &mut NativeCtx<'_>,
    throwable: ObjRef,
    trace: &[TraceFrame],
) -> Result<(), VmError> {
    let element_class = ctx
        .shared
        .load_class(ctx.path, "java/lang/StackTraceElement")?;
    let mut elements = Vec::with_capacity(trace.len());
    for frame in trace {
        let element = ctx.shared.new_instance(element_class);
        let class_name = ctx
            .shared
            .intern(ctx.path, &frame.class.replace('/', "."))?;
        let method_name = ctx.shared.intern(ctx.path, &frame.method)?;
        let file_name = match ctx
            .shared
            .class_ids
            .get(&frame.class)
            .and_then(|class| ctx.shared.class(*class).source_file.clone())
        {
            Some(file) => Value::Ref(ctx.shared.intern(ctx.path, &file)?),
            None => Value::NULL,
        };
        let line_number = if frame.line == 0 {
            -2
        } else {
            i32::from(frame.line)
        };
        set_field(
            ctx,
            element_class,
            element,
            "declaringClass",
            "Ljava/lang/String;",
            Value::Ref(class_name),
        )?;
        set_field(
            ctx,
            element_class,
            element,
            "methodName",
            "Ljava/lang/String;",
            Value::Ref(method_name),
        )?;
        set_field(
            ctx,
            element_class,
            element,
            "fileName",
            "Ljava/lang/String;",
            file_name,
        )?;
        set_field(
            ctx,
            element_class,
            element,
            "lineNumber",
            "I",
            Value::Int(line_number),
        )?;
        elements.push(Value::Ref(element));
    }

    let array = ctx.shared.allocate(HeapObject::Array(Array {
        kind: ArrayKind::Reference,
        elements,
        class_name: Some(STACK_TRACE_ARRAY.to_owned()),
    }));

    let throwable_class = ctx.shared.class_of(throwable).ok_or_else(|| {
        VmError::Native(format!(
            "fillInStackTrace on {throwable:?}, which is not an instance"
        ))
    })?;
    set_field(
        ctx,
        throwable_class,
        throwable,
        "stackTrace",
        STACK_TRACE_ARRAY,
        Value::Ref(array),
    )?;
    set_field(
        ctx,
        throwable_class,
        throwable,
        "depth",
        "I",
        Value::Int(trace.len() as i32),
    )?;
    Ok(())
}

fn set_field(
    ctx: &mut NativeCtx<'_>,
    class: ClassId,
    object: ObjRef,
    name: &str,
    descriptor: &str,
    value: Value,
) -> Result<(), VmError> {
    let slot = ctx
        .shared
        .class(class)
        .field_slot(name, descriptor)
        .ok_or_else(|| {
            VmError::Native(format!(
                "{} has no field {name}:{descriptor}",
                ctx.shared.class(class).name
            ))
        })?;
    ctx.shared.set_instance_field(object, slot, value)
}
