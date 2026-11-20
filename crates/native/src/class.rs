//! The class-object natives.
//!
//! `Object.getClass` hands out the mirror; the `Class` methods answer from the VM's `class_targets` map, and `initClassName` caches its answer in the object's `name` field.

use rava_vm::ClassId;
use rava_vm::native::expect_args;
use rava_vm::{Array, ClassTarget, HeapObject, NativeCtx, Shared, Value, VmError};

use crate::no_op;

/// Installs the class-object natives.
pub fn register(shared: &mut Shared) {
    shared.register_native(
        "java/lang/Object",
        "getClass",
        "()Ljava/lang/Class;",
        get_class,
    );
    shared.register_native(
        "java/lang/Object",
        "clone",
        "()Ljava/lang/Object;",
        clone_array,
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
    shared.register_native(
        "java/lang/Class",
        "getInterfaces0",
        "()[Ljava/lang/Class;",
        get_interfaces,
    );
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
    // `StringUTF16` stores a 16-bit char in two bytes and asks which comes first; this host is
    // little-endian (aarch64), so the low byte leads.
    shared.register_native("java/lang/StringUTF16", "isBigEndian", "()Z", is_big_endian);
    // `String.intern()` is the Java face of the VM's intern table; the same table `ldc` uses,
    // so an interned literal and `"literal".intern()` are one object.
    shared.register_native(
        "java/lang/String",
        "intern",
        "()Ljava/lang/String;",
        intern_string,
    );
    // `Class.forName` is how the library loads optional providers (`Locale`'s, for one); the
    // bootstrap loader is the only loader this VM has, and `initialize` runs `<clinit>` through
    // the interpreter's call seam.
    shared.register_native(
        "java/lang/Class",
        "forName0",
        "(Ljava/lang/String;ZLjava/lang/ClassLoader;Ljava/lang/Class;)Ljava/lang/Class;",
        for_name,
    );
}

/// `StringUTF16.isBigEndian()`: false on this little-endian host.
fn is_big_endian(_ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    Ok(Value::Int(0))
}

/// `Class.forName0(String, boolean, ClassLoader, Class)`: load a class by binary name from the
/// host image, running `<clinit>` when asked. Loaders are not modelled, so the loader and caller
/// arguments are ignored.
fn for_name(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let args = expect_args::<4>(ctx)?;
    let (name, initialize) = (args[0], args[1]);
    let name_reference = name.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class name String",
        found: name,
    })?;
    let name = ctx.shared.string_text(name_reference)?;
    let binary = name.replace('.', "/");
    let class = if binary.starts_with('[') {
        return Ok(Value::Ref(ctx.shared.type_class(ctx.path, &binary)?));
    } else {
        ctx.shared.load_class(ctx.path, &binary)?
    };
    if initialize.as_int().unwrap_or(0) != 0 {
        initialize_class(ctx, class)?;
    }
    Ok(Value::Ref(ctx.shared.class_object(ctx.path, class)?))
}

/// Runs `<clinit>` (superclass first, JVMS §5.5) through the interpreter's call seam, which is
/// what `Class.forName(name, true, ...)` promises.
fn initialize_class(ctx: &mut NativeCtx<'_>, class: ClassId) -> Result<(), VmError> {
    if let Some(parent) = ctx.shared.class(class).super_class {
        initialize_class(ctx, parent)?;
    }
    match ctx.shared.begin_initialization(class) {
        rava_vm::Initialization::AlreadyDone => Ok(()),
        rava_vm::Initialization::Erroneous => Err(VmError::JavaException {
            class: "java/lang/NoClassDefFoundError".to_owned(),
            message: Some(format!(
                "Could not initialize class {}",
                ctx.shared.class(class).name
            )),
        }),
        rava_vm::Initialization::Run(index) => {
            let invoker = ctx.shared.invoker.ok_or_else(|| {
                VmError::Native("Class.forName needs the interpreter's call seam".to_owned())
            })?;
            invoker(ctx.shared, ctx.thread, ctx.path, class, index, Vec::new())?;
            ctx.shared.finish_initialization(class);
            Ok(())
        }
    }
}

/// `String.intern()`: the canonical reference for the receiver's text.
fn intern_string(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a String",
        found: *this,
    })?;
    let text = ctx.shared.string_text(reference)?;
    let interned = ctx.shared.intern(ctx.path, &text)?;
    Ok(Value::Ref(interned))
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

/// `Class.getInterfaces0()`: the direct superinterfaces as class objects. The host's
/// `getInterfaces()` wraps this and clones the array; a primitive or array class object has no
/// loaded class behind it and reports none.
fn get_interfaces(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "a class object",
        found: *this,
    })?;
    let elements: Vec<Value> = match ctx.shared.class_targets.get(&reference) {
        Some(rava_vm::ClassTarget::Loaded(class)) => {
            let interfaces = ctx.shared.class(*class).interfaces.clone();
            let mut elements = Vec::with_capacity(interfaces.len());
            for interface in interfaces {
                elements.push(Value::Ref(ctx.shared.class_object(ctx.path, interface)?));
            }
            elements
        }
        Some(rava_vm::ClassTarget::Type(_)) => Vec::new(),
        None => {
            return Err(VmError::Native(format!(
                "{reference:?} is not a class object"
            )));
        }
    };
    let reference = ctx
        .shared
        .allocate(rava_vm::HeapObject::Array(rava_vm::Array {
            kind: rava_vm::ArrayKind::Reference,
            elements,
            class_name: Some("[Ljava/lang/Class;".to_owned()),
        }));
    Ok(Value::Ref(reference))
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

/// `Object.clone()` for arrays (JVMS §5.4.3.3, §6.5 `invokevirtual`).
///
/// An array's class is a descriptor, not a loaded class, and calling a method on it resolves in
/// `java.lang.Object` — `clone` is the only method this matters for before `Object`'s other
/// methods are reached. This slice implements the array half only: the elements are copied
/// value by value (a primitive copies its value, a reference copies the handle) into a fresh
/// array of the same element kind and class descriptor. An instance clone would need a shallow
/// copy of the field vector, which no slice needs yet, so it is reported as unsupported rather
/// than approximated.
fn clone_array(ctx: &mut NativeCtx<'_>) -> Result<Value, VmError> {
    let [this] = expect_args::<1>(ctx)?;
    let reference = this.as_ref().ok_or(VmError::TypeMismatch {
        expected: "an array reference",
        found: *this,
    })?;
    let copy = match ctx.shared.heap.get(reference.index()) {
        Some(HeapObject::Array(array)) => Array {
            kind: array.kind,
            elements: array.elements.clone(),
            class_name: array.class_name.clone(),
        },
        Some(HeapObject::Null) => {
            return Err(VmError::TypeMismatch {
                expected: "an array reference",
                found: Value::NULL,
            });
        }
        Some(_) => {
            return Err(VmError::Unsupported {
                feature: "Object.clone on an instance",
                detail: "this slice implements array clone only; an instance clone needs a \
                         shallow field copy"
                    .to_owned(),
            });
        }
        None => {
            return Err(VmError::Native(format!(
                "clone on dangling reference {reference:?}"
            )));
        }
    };
    Ok(Value::Ref(ctx.shared.allocate(HeapObject::Array(copy))))
}
