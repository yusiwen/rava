//! The class-object natives.
//!
//! `Object.getClass` hands out the mirror; the `Class` methods answer from the VM's `class_targets` map, and `initClassName` caches its answer in the object's `name` field.

use rava_vm::native::expect_args;
use rava_vm::{ClassTarget, NativeCtx, Shared, Value, VmError};

use crate::no_op;

/// Installs the class-object natives.
pub fn register(shared: &mut Shared) {
    shared.register_native(
        "java/lang/Object",
        "getClass",
        "()Ljava/lang/Class;",
        get_class,
    );
    shared.register_native("java/lang/Class", "registerNatives", "()V", no_op);
    shared.register_native(
        "java/lang/Class",
        "initClassName",
        "()Ljava/lang/String;",
        init_class_name,
    );
    shared.register_native(
        "java/lang/Class",
        "desiredAssertionStatus0",
        "(Ljava/lang/Class;)Z",
        assertions_disabled,
    );
    shared.register_native("java/lang/Class", "isPrimitive", "()Z", is_primitive);
    shared.register_native("java/lang/Class", "isArray", "()Z", is_array);
    shared.register_native("java/lang/Class", "isInterface", "()Z", is_interface);
    shared.register_native(
        "java/lang/Class",
        "getPrimitiveClass",
        "(Ljava/lang/String;)Ljava/lang/Class;",
        get_primitive_class,
    );
    shared.register_native(
        "jdk/internal/reflect/Reflection",
        "getCallerClass",
        "()Ljava/lang/Class;",
        get_caller_class,
    );
}

/// `Object.getClass()`: the class object of the receiver's runtime class.
///
/// An array has no [`ClassId`](rava_vm::ClassId), so its runtime class is the type class object
/// for its descriptor; [`Shared::type_class`] caches it, which is what makes
/// `new int[3].getClass() == int[].class` hold.
fn get_class(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "an object reference",
        found: *this,
    })?;
    let path = ctx.path;
    if let Some(class) = ctx.shared.class_of(reference) {
        return Ok(Value::Ref(ctx.shared.class_object(path, class)?));
    }
    if let Some(descriptor) = ctx.shared.array_class(reference).map(str::to_owned) {
        return Ok(Value::Ref(ctx.shared.type_class(path, &descriptor)?));
    }
    Err(VmError::Native(format!(
        "getClass on a value with no runtime class ({reference:?})"
    )))
}

/// `Class.initClassName()`: the name `Class.getName` would give, cached in the object's `name`
/// field so the next call is a plain field read (the host's `getName` is exactly
/// `name != null ? name : initClassName()`).
fn init_class_name(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let name = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Loaded(class)) => ctx.shared.class(*class).name.replace('/', "."),
        Some(ClassTarget::Type(descriptor)) => rava_vm::type_name(descriptor),
        None => {
            return Err(VmError::Native(format!(
                "initClassName on {reference:?}, which is not a class object"
            )));
        }
    };
    let string = ctx.shared.intern(ctx.path, &name)?;
    let class_class = ctx.shared.class_of(reference);
    if let Some(class_class) = class_class
        && let Some(slot) = ctx
            .shared
            .class(class_class)
            .field_slot("name", "Ljava/lang/String;")
    {
        ctx.shared
            .set_instance_field(reference, slot, Value::Ref(string))?;
    }
    Ok(Value::Ref(string))
}

/// Assertions are disabled: the VM has no `-ea`, so every class reports the default.
fn assertions_disabled(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [_class] = expect_args::<1>(ctx)?;
    Ok(Value::Int(0))
}

fn is_primitive(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let primitive = matches!(
        ctx.shared.class_targets.get(&reference),
        Some(ClassTarget::Type(descriptor)) if !descriptor.starts_with('[')
    );
    Ok(Value::Int(i32::from(primitive)))
}

fn is_array(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let array = matches!(
        ctx.shared.class_targets.get(&reference),
        Some(ClassTarget::Type(descriptor)) if descriptor.starts_with('[')
    );
    Ok(Value::Int(i32::from(array)))
}

fn is_interface(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let interface = match ctx.shared.class_targets.get(&reference) {
        Some(ClassTarget::Loaded(class)) => ctx
            .shared
            .class(*class)
            .file
            .as_ref()
            .is_some_and(|file| file.access_flags & 0x0200 != 0),
        _ => false,
    };
    Ok(Value::Int(i32::from(interface)))
}

/// `Class.getPrimitiveClass(String)`: the class object for a primitive wrapper's `TYPE`.
fn get_primitive_class(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [name] = expect_args::<1>(ctx)?;
    let reference = name.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a primitive name",
        found: *name,
    })?;
    let text = ctx
        .shared
        .string_text(reference)
        .map_err(|error| VmError::Native(format!("getPrimitiveClass needs a string: {error}")))?;
    let descriptor = match text.as_str() {
        "int" => "I",
        "long" => "J",
        "float" => "F",
        "double" => "D",
        "boolean" => "Z",
        "byte" => "B",
        "char" => "C",
        "short" => "S",
        "void" => "V",
        other => {
            return Err(VmError::Native(format!(
                "getPrimitiveClass: {other} is not a primitive"
            )));
        }
    };
    let path = ctx.path;
    Ok(Value::Ref(ctx.shared.type_class(path, descriptor)?))
}

/// `Reflection.getCallerClass()`: the class that called the caller-sensitive method. The
/// native has no frame of its own, so the top frame called the method and the one below it is
/// the caller whose class is wanted.
fn get_caller_class(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let caller = ctx
        .thread
        .frames
        .iter()
        .rev()
        .nth(1)
        .ok_or_else(|| VmError::Native("getCallerClass: no caller frame".to_owned()))?;
    let class = caller.class;
    let path = ctx.path;
    Ok(Value::Ref(ctx.shared.class_object(path, class)?))
}
