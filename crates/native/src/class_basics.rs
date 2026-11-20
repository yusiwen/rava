//! The `Class` query natives: instance and assignability tests, superclass and modifiers.
//!
//! Every answer comes from the VM's own model, never from reflection over the host library:
//! a class object's target is in [`Shared::class_targets`], subtype checks go through
//! [`Shared::is_assignable`], and the superclass is the loader's resolved link. In
//! particular, a primitive or array class object (`ClassTarget::Type`) answers these queries
//! from its descriptor, because it has no loaded class behind it.
//!
//! [`Shared::class_targets`]: rava_vm::Shared::class_targets
//! [`Shared::is_assignable`]: rava_vm::Shared::is_assignable

use rava_vm::native::expect_args;
use rava_vm::{ClassTarget, NativeCtx, ObjRef, Shared, Value, VmError};

/// The reflection modifier bits a class file may carry: `PUBLIC | FINAL | INTERFACE |
/// ABSTRACT | SYNTHETIC | ANNOTATION | ENUM`. `ACC_SUPER` (0x0020) and the unused bits are
/// dropped, as `java.lang.reflect.Modifier` does.
const MODIFIER_MASK: u16 = 0x7611;

/// Installs the `Class` query natives.
pub fn register(shared: &mut Shared) {
    shared.register_native(
        "java/lang/Class",
        "isInstance",
        "(Ljava/lang/Object;)Z",
        is_instance,
    );
    shared.register_native(
        "java/lang/Class",
        "isAssignableFrom",
        "(Ljava/lang/Class;)Z",
        is_assignable_from,
    );
    shared.register_native(
        "java/lang/Class",
        "getSuperclass",
        "()Ljava/lang/Class;",
        get_superclass,
    );
    shared.register_native("java/lang/Class", "getModifiers", "()I", get_modifiers);
}

/// The receiver, which every one of these natives has in `args[0]`. It is never null in
/// Java code, but the value stack does not type-check (ADR-0011), so report it if it is.
fn receiver(this: Value) -> Result<ObjRef, VmError> {
    this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: this,
    })
}

/// What the receiver class object names.
fn class_target<'a>(ctx: &'a NativeCtx<'_>, reference: ObjRef) -> Result<&'a ClassTarget, VmError> {
    ctx.shared.class_targets.get(&reference).ok_or_else(|| {
        VmError::Native(format!(
            "Class query on {reference:?}, which is not a class object"
        ))
    })
}

/// `Class.isInstance(Object)`: `null` is an instance of nothing, and a primitive or array
/// class object names no ordinary object whose class could be tested.
fn is_instance(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, object] = expect_args::<2>(ctx)?;
    let target = class_target(ctx, receiver(*this)?)?;
    // `null` is an instance of no type, including `Object` (JVMS §2.4).
    let argument = match object.as_ref() {
        Some(reference) if !reference.is_null() => reference,
        _ => return Ok(Value::Int(0)),
    };
    let instance = match target {
        ClassTarget::Loaded(target) => ctx
            .shared
            .class_of(argument)
            .is_some_and(|actual| ctx.shared.is_assignable(actual, *target)),
        ClassTarget::Type(_) => false,
    };
    Ok(Value::Int(i32::from(instance)))
}

/// `Class.isAssignableFrom(Class)`: `A.isAssignableFrom(B)` is "B is a subtype of A".
///
/// A primitive or array type is assignable only from the identical descriptor: this VM does
/// not model array covariance or boxing conversions, and the host library does not ask for
/// them here yet. A `null` argument violates the method's contract and is reported as such
/// rather than answered `false`.
fn is_assignable_from(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, other] = expect_args::<2>(ctx)?;
    let target = class_target(ctx, receiver(*this)?)?;
    let argument = other.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *other,
    })?;
    if argument.is_null() {
        return Err(VmError::Native("Class.isAssignableFrom(null)".to_owned()));
    }
    let actual = match ctx.shared.class_targets.get(&argument) {
        Some(target) => target,
        // A `java.lang.Class` instance the VM did not allocate is not a target it can name.
        None => return Ok(Value::Int(0)),
    };
    let assignable = match (actual, target) {
        (ClassTarget::Loaded(actual), ClassTarget::Loaded(target)) => {
            ctx.shared.is_assignable(*actual, *target)
        }
        (ClassTarget::Type(actual), ClassTarget::Type(target)) => actual == target,
        (ClassTarget::Loaded(_), ClassTarget::Type(_))
        | (ClassTarget::Type(_), ClassTarget::Loaded(_)) => false,
    };
    Ok(Value::Int(i32::from(assignable)))
}

/// `Class.getSuperclass()`: the loader's resolved superclass, as its cached class object.
/// Primitive and array class objects have no superclass in this VM (`Type`), and neither do
/// `Object` or interfaces (`super_class` is `None` for them).
fn get_superclass(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let class = match class_target(ctx, receiver(*this)?)? {
        ClassTarget::Loaded(class) => *class,
        ClassTarget::Type(_) => return Ok(Value::NULL),
    };
    let Some(super_class) = ctx.shared.class(class).super_class else {
        return Ok(Value::NULL);
    };
    let path = ctx.path;
    Ok(Value::Ref(ctx.shared.class_object(path, super_class)?))
}

/// `Class.getModifiers()`: the class file's access flags, masked to the reflection bits.
/// A primitive or array type reports 0 rather than an invented value.
fn get_modifiers(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let modifiers = match class_target(ctx, receiver(*this)?)? {
        ClassTarget::Loaded(class) => ctx
            .shared
            .class(*class)
            .file
            .as_ref()
            .map_or(0, |file| file.access_flags & MODIFIER_MASK),
        ClassTarget::Type(_) => 0,
    };
    Ok(Value::Int(i32::from(modifiers)))
}
