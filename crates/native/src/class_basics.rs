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
use rava_vm::{ClassId, ClassTarget, NativeCtx, ObjRef, Shared, Value, VmError};

/// The reflection modifier bits a class file may carry: `PUBLIC | FINAL | INTERFACE |
/// ABSTRACT | SYNTHETIC | ANNOTATION | ENUM`. `ACC_SUPER` (0x0020) and the unused bits are
/// dropped, as `java.lang.reflect.Modifier` does.
const MODIFIER_MASK: u16 = 0x7611;

/// `Class.getModifiers()` for an array class: `PUBLIC | FINAL | ABSTRACT` (0x0411 = 1041),
/// which is what the host JDK answers for `int[].class`.
const ARRAY_MODIFIERS: u16 = 0x0411;

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

/// `Class.isInstance(Object)`: `null` is an instance of nothing. A loaded target tests through
/// the class model; a type target (primitive or array) is answered from the argument's array
/// descriptor, and a primitive type has no instances at all.
fn is_instance(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, object] = expect_args::<2>(ctx)?;
    let target = class_target(ctx, receiver(*this)?)?.clone();
    // `null` is an instance of no type, including `Object` (JVMS §2.4).
    let argument = match object.as_ref() {
        Some(reference) if !reference.is_null() => reference,
        _ => return Ok(Value::Int(0)),
    };
    let instance = match target {
        ClassTarget::Loaded(target) => match ctx.shared.class_of(argument) {
            Some(actual) => ctx.shared.is_assignable(actual, target),
            // An array has no ClassId; every array is an Object, Cloneable and Serializable.
            None => array_instance_of_class(ctx, argument, target)?,
        },
        ClassTarget::Type(target) => match ctx.shared.array_class(argument).map(str::to_owned) {
            Some(actual) => ctx.shared.is_array_assignable(ctx.path, &actual, &target)?,
            None => false,
        },
    };
    Ok(Value::Int(i32::from(instance)))
}

/// Whether an argument that has no runtime [`ClassId`] — an array — is an instance of the
/// loaded class `target`.
fn array_instance_of_class(
    ctx: &mut NativeCtx<'_>,
    argument: ObjRef,
    target: ClassId,
) -> Result<bool, VmError> {
    let Some(actual) = ctx.shared.array_class(argument).map(str::to_owned) else {
        return Ok(false);
    };
    let target_name = ctx.shared.class(target).name.clone();
    ctx.shared
        .is_array_assignable(ctx.path, &actual, &target_name)
}

/// `Class.isAssignableFrom(Class)`: `A.isAssignableFrom(B)` is "B is a subtype of A".
///
/// A primitive type is assignable only from its identical descriptor. Arrays follow JVMS
/// §4.10.1.2: exact for primitive components, covariant for reference components, and every
/// array is assignable to `Object`, `Cloneable` and `Serializable`. A `null` argument violates
/// the method's contract and is reported as such rather than answered `false`.
fn is_assignable_from(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this, other] = expect_args::<2>(ctx)?;
    let target = class_target(ctx, receiver(*this)?)?.clone();
    let argument = other.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *other,
    })?;
    if argument.is_null() {
        return Err(VmError::Native("Class.isAssignableFrom(null)".to_owned()));
    }
    let actual = match ctx.shared.class_targets.get(&argument).cloned() {
        Some(target) => target,
        // A `java.lang.Class` instance the VM did not allocate is not a target it can name.
        None => return Ok(Value::Int(0)),
    };
    let assignable = match (actual, target) {
        (ClassTarget::Loaded(actual), ClassTarget::Loaded(target)) => {
            ctx.shared.is_assignable(actual, target)
        }
        (ClassTarget::Type(actual), ClassTarget::Type(target)) => {
            if actual.starts_with('[') || target.starts_with('[') {
                ctx.shared.is_array_assignable(ctx.path, &actual, &target)?
            } else {
                actual == target
            }
        }
        (ClassTarget::Type(actual), ClassTarget::Loaded(target)) => {
            if actual.starts_with('[') {
                let target_name = ctx.shared.class(target).name.clone();
                ctx.shared
                    .is_array_assignable(ctx.path, &actual, &target_name)?
            } else {
                false
            }
        }
        (ClassTarget::Loaded(_), ClassTarget::Type(_)) => false,
    };
    Ok(Value::Int(i32::from(assignable)))
}

/// `Class.getSuperclass()`: the loader's resolved superclass, as its cached class object.
///
/// A primitive class object has no superclass. An array's direct superclass is `java.lang.Object`
/// (JLS §10.8), so its class object reports `Object.class` rather than `null`.
fn get_superclass(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let target = class_target(ctx, receiver(*this)?)?.clone();
    let class = match target {
        ClassTarget::Loaded(class) => class,
        ClassTarget::Type(descriptor) if descriptor.starts_with('[') => {
            let path = ctx.path;
            let object = ctx.shared.load_class(path, "java/lang/Object")?;
            return Ok(Value::Ref(ctx.shared.class_object(path, object)?));
        }
        ClassTarget::Type(_) => return Ok(Value::NULL),
    };
    let Some(super_class) = ctx.shared.class(class).super_class else {
        return Ok(Value::NULL);
    };
    let path = ctx.path;
    Ok(Value::Ref(ctx.shared.class_object(path, super_class)?))
}

/// `Class.getModifiers()`: the class file's access flags, masked to the reflection bits.
/// A primitive type reports 0 rather than an invented value; an array class reports the host
/// JDK's `PUBLIC | FINAL | ABSTRACT`.
fn get_modifiers(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let modifiers = match class_target(ctx, receiver(*this)?)?.clone() {
        ClassTarget::Loaded(class) => ctx
            .shared
            .class(class)
            .file
            .as_ref()
            .map_or(0, |file| file.access_flags & MODIFIER_MASK),
        ClassTarget::Type(descriptor) if descriptor.starts_with('[') => ARRAY_MODIFIERS,
        ClassTarget::Type(_) => 0,
    };
    Ok(Value::Int(i32::from(modifiers)))
}
